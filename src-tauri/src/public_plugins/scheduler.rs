use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::manifest::{valid_plugin_id, PublicActivationMode};

const LIVE_TIMEOUT: Duration = Duration::from_secs(5);
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETAINED_CONTEXTS: usize = 4_096;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginRequestContext {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) request_id: String,
}

impl PluginRequestContext {
    fn valid_shape(&self) -> bool {
        valid_plugin_id(&self.plugin_id)
            && self.plugin_generation != 0
            && self
                .request_id
                .strip_prefix("public-request-")
                .is_some_and(|suffix| {
                    suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginSubmissionOwner {
    pub(crate) ui_intent_epoch: u64,
    pub(crate) control_value: String,
    pub(crate) submission_token: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PluginRequestOwner {
    Command(PluginSubmissionOwner),
    SharePrepare(super::share_prepare::SharePrepareOwner),
    ResultAction(super::result_action::ResultActionOrigin),
    WindowAction {
        identity: crate::plugin_window::WindowActionIdentity,
        request: super::window_action::WindowActionRequest,
    },
    PanelAction {
        content_label: String,
        session_epoch: u64,
        request: super::panel_action::PanelActionRequest,
    },
}
impl PluginRequestOwner {
    pub(crate) fn live(&self) -> bool {
        match self {
            Self::SharePrepare(owner) => owner.lease.live(),
            Self::ResultAction(origin) => {
                origin.lease.live()
                    && origin
                        .route
                        .result_publication
                        .as_ref()
                        .is_none_or(|token| token.live())
            }
            _ => true,
        }
    }
    pub(crate) fn command(&self) -> Option<&PluginSubmissionOwner> {
        match self {
            Self::Command(owner) => Some(owner),
            _ => None,
        }
    }
    pub(crate) fn token(&self) -> Option<&str> {
        self.command().map(|owner| owner.submission_token.as_str())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginRequestCandidate {
    pub(crate) command_id: String,
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) input: String,
    pub(crate) owner: PluginRequestOwner,
}

impl PluginRequestCandidate {
    fn valid(&self) -> bool {
        valid_plugin_id(&self.plugin_id)
            && super::manifest::valid_command_name(&self.command_id)
            && self.plugin_generation != 0
            && self.activation_id != 0
            && self.admission_epoch != 0
            && self.input.len() <= 64 * 1024
            && !self.input.contains('\0')
            && match &self.owner {
                PluginRequestOwner::SharePrepare(owner) => owner.valid_for(self),
                PluginRequestOwner::Command(owner) => {
                    owner.ui_intent_epoch != 0 && !owner.submission_token.is_empty()
                }
                PluginRequestOwner::ResultAction(origin) => {
                    origin.lease.live()
                        && origin
                            .route
                            .result_publication
                            .as_ref()
                            .is_none_or(|token| token.live())
                        && origin.route.plugin_id == self.plugin_id
                        && origin.route.command_id == self.command_id
                        && origin.route.generation == self.plugin_generation
                        && origin.route.activation_id == self.activation_id
                        && origin.route.admission_epoch == self.admission_epoch
                        && self.activation_mode == PublicActivationMode::Submit
                }
                PluginRequestOwner::WindowAction { identity, request } => {
                    identity.session_generation != 0
                        && identity.document_generation != 0
                        && identity.owner.plugin_id == self.plugin_id
                        && identity.owner.plugin_generation == self.plugin_generation
                        && identity.owner.activation_id == self.activation_id
                        && identity.owner.admission_epoch == self.admission_epoch
                        && !identity.content_label.is_empty()
                        && request.validate().is_ok()
                        && self.activation_mode == PublicActivationMode::Submit
                }
                PluginRequestOwner::PanelAction {
                    content_label,
                    session_epoch,
                    request,
                } => {
                    *session_epoch != 0
                        && !content_label.is_empty()
                        && request.validate().is_ok()
                        && self.activation_mode == PublicActivationMode::Submit
                }
            }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScheduledPluginRequest {
    pub(crate) context: PluginRequestContext,
    pub(crate) candidate: PluginRequestCandidate,
    pub(crate) dispatched_at: Instant,
    pub(crate) deadline: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReservedPluginRequest {
    pub(crate) context: PluginRequestContext,
    pub(crate) candidate: PluginRequestCandidate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginScheduleError {
    InvalidCandidate,
    Busy,
    GenerationMismatch,
    GenerationExhausted,
    RequestExhausted,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PluginScheduleOutcome {
    Dispatched(ScheduledPluginRequest),
    Waiting {
        expired: PluginRequestContext,
        request_context: PluginRequestContext,
        replaced_submission_token: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginCompletionOutcome {
    pub(crate) completed_owner: PluginRequestOwner,
    pub(crate) credential_publication: Option<(u64, super::result_action::ResultActionLease)>,
    pub(crate) accepted: bool,
    pub(crate) next: Option<ScheduledPluginRequest>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginRuntimeReplacement {
    pub(crate) plugin_id: String,
    pub(crate) previous_activation_id: u64,
    pub(crate) previous_admission_epoch: u64,
    pub(crate) expired: PluginRequestContext,
    pub(crate) previous_generation: u64,
    pub(crate) new_generation: u64,
    pub(crate) has_waiting: bool,
    pub(crate) counts_as_fault: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginContextStatus {
    Current,
    Expired,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginContextAccessError {
    Expired,
    Invalid,
    Unavailable,
}

struct RunningRequest {
    request: ScheduledPluginRequest,
    current: bool,
    // Preserve logical retirement across physical completion and controller settlement.
    action_retirement: Option<super::content_action::ContentActionError>,
    notification_published: bool,
    // Claimed under the scheduler lock immediately before the native emit.
    share_prepare_emission_started: bool,
}

pub(crate) struct PluginCurrentRequest<'a> {
    panel_action: bool,
    effects_allowed: bool,
    plugin_generation: u64,
    activation_id: u64,
    admission_epoch: u64,
    notification_published: &'a mut bool,
}

impl PluginCurrentRequest<'_> {
    pub(crate) fn effects_allowed(&self) -> bool {
        self.effects_allowed
    }

    pub(crate) fn is_panel_action(&self) -> bool {
        self.panel_action
    }

    pub(crate) fn activation_id(&self) -> u64 {
        self.activation_id
    }

    pub(crate) fn plugin_generation(&self) -> u64 {
        self.plugin_generation
    }

    pub(crate) fn admission_epoch(&self) -> u64 {
        self.admission_epoch
    }

    pub(crate) fn notification_published(&self) -> bool {
        *self.notification_published
    }

    pub(crate) fn mark_notification_published(&mut self) {
        *self.notification_published = true;
    }
}

struct PluginQueue {
    generation: u64,
    activation_id: u64,
    admission_epoch: u64,
    result_publication: super::result_action::ResultActionLease,
    running: Option<RunningRequest>,
    waiting: Option<ReservedPluginRequest>,
    rebuilding: bool,
    action_completion: Option<(
        PluginRequestContext,
        Option<super::content_action::ContentActionError>,
    )>,
    issued: HashSet<PluginRequestContext>,
    issued_order: VecDeque<PluginRequestContext>,
}

impl PluginQueue {
    fn new(generation: u64, activation_id: u64, admission_epoch: u64) -> Self {
        Self {
            generation,
            activation_id,
            admission_epoch,
            result_publication: super::result_action::ResultActionLease::default(),
            running: None,
            waiting: None,
            rebuilding: false,
            action_completion: None,
            issued: HashSet::new(),
            issued_order: VecDeque::new(),
        }
    }

    fn remember(&mut self, context: PluginRequestContext) {
        self.issued.insert(context.clone());
        self.issued_order.push_back(context);
        while self.issued_order.len() > MAX_RETAINED_CONTEXTS {
            if let Some(expired) = self.issued_order.pop_front() {
                self.issued.remove(&expired);
            }
        }
    }

    fn rebind_waiting_generation(&mut self, generation: u64) {
        let Some(waiting) = self.waiting.as_mut() else {
            return;
        };
        let previous = waiting.context.clone();
        waiting.candidate.plugin_generation = generation;
        waiting.context.plugin_generation = generation;
        let current = waiting.context.clone();
        self.issued.remove(&previous);
        self.issued_order.retain(|context| context != &previous);
        self.remember(current);
    }
}

struct SchedulerState {
    next_request: u64,
    result_publications_closed: bool,
    by_plugin: HashMap<String, PluginQueue>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            next_request: 1,
            result_publications_closed: false,
            by_plugin: HashMap::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct PluginRequestScheduler {
    state: Mutex<SchedulerState>,
}

impl PluginRequestScheduler {
    pub(crate) fn capture_result_publication(
        &self,
        plugin_id: &str,
        generation: u64,
        activation: u64,
        admission: u64,
    ) -> Result<super::result_action::ResultActionLease, PluginScheduleError> {
        let state = self.lock()?;
        if state.result_publications_closed {
            return Err(PluginScheduleError::Unavailable);
        }
        let queue = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginScheduleError::GenerationMismatch)?;
        if queue.rebuilding
            || queue.generation != generation
            || queue.activation_id != activation
            || queue.admission_epoch != admission
            || !queue.result_publication.live()
        {
            return Err(PluginScheduleError::GenerationMismatch);
        }
        Ok(queue.result_publication.clone())
    }
    pub(crate) fn result_publication_current(
        &self,
        plugin_id: &str,
        token: &super::result_action::ResultActionLease,
    ) -> bool {
        self.state.lock().is_ok_and(|state| {
            !state.result_publications_closed
                && token.live()
                && state
                    .by_plugin
                    .get(plugin_id)
                    .is_some_and(|q| q.result_publication == *token && !q.rebuilding)
        })
    }
    pub(crate) fn bound_command_publication(
        &self,
        context: &PluginRequestContext,
        submission_token: &str,
        command_id: &str,
        input: &str,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<super::result_action::ResultActionLease, PluginScheduleError> {
        let state = self.lock()?;
        let queue = state
            .by_plugin
            .get(&context.plugin_id)
            .ok_or(PluginScheduleError::GenerationMismatch)?;
        let running = queue
            .running
            .as_ref()
            .ok_or(PluginScheduleError::GenerationMismatch)?;
        if state.result_publications_closed
            || queue.rebuilding
            || !queue.result_publication.live()
            || !running.current
            || running.request.context != *context
            || running.request.candidate.owner.token() != Some(submission_token)
            || running.request.candidate.command_id != command_id
            || running.request.candidate.input != input
            || running.request.candidate.activation_id != activation_id
            || running.request.candidate.admission_epoch != admission_epoch
            || queue.generation != context.plugin_generation
        {
            return Err(PluginScheduleError::GenerationMismatch);
        }
        Ok(queue.result_publication.clone())
    }
    pub(crate) fn with_result_publication<T>(
        &self,
        plugin_id: &str,
        token: &super::result_action::ResultActionLease,
        publish: impl FnOnce() -> T,
    ) -> Result<T, super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        let state = self.lock().map_err(|_| E::ActionUnavailable)?;
        if state.result_publications_closed
            || !token.live()
            || !state
                .by_plugin
                .get(plugin_id)
                .is_some_and(|q| q.result_publication == *token && !q.rebuilding)
        {
            return Err(E::StaleRequest);
        }
        Ok(publish())
    }
    pub(crate) fn close_result_publications(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.result_publications_closed = true;
        for queue in state.by_plugin.values() {
            queue.result_publication.revoke();
            if let Some(running) = queue.running.as_ref() {
                if let PluginRequestOwner::ResultAction(origin) = &running.request.candidate.owner {
                    origin.lease.revoke();
                }
            }
        }
    }

    pub(crate) fn reserve(
        &self,
        candidate: PluginRequestCandidate,
    ) -> Result<ReservedPluginRequest, PluginScheduleError> {
        if !candidate.valid() {
            return Err(PluginScheduleError::InvalidCandidate);
        }
        let mut state = self.lock()?;
        let request_number = state.next_request;
        state.next_request = state
            .next_request
            .checked_add(1)
            .ok_or(PluginScheduleError::RequestExhausted)?;
        Ok(ReservedPluginRequest {
            context: PluginRequestContext {
                plugin_id: candidate.plugin_id.clone(),
                plugin_generation: candidate.plugin_generation,
                request_id: format!("public-request-{request_number:016x}"),
            },
            candidate,
        })
    }

    #[cfg(test)]
    pub(crate) fn enqueue(
        &self,
        candidate: PluginRequestCandidate,
        now: Instant,
    ) -> Result<PluginScheduleOutcome, PluginScheduleError> {
        let reserved = self.reserve(candidate)?;
        self.enqueue_reserved(reserved, now)
    }

    pub(crate) fn enqueue_reserved(
        &self,
        reserved: ReservedPluginRequest,
        now: Instant,
    ) -> Result<PluginScheduleOutcome, PluginScheduleError> {
        if matches!(
            reserved.candidate.owner,
            PluginRequestOwner::ResultAction(_)
        ) || !reserved.candidate.valid()
            || reserved.context.plugin_id != reserved.candidate.plugin_id
            || reserved.context.plugin_generation != reserved.candidate.plugin_generation
            || !reserved.context.valid_shape()
        {
            return Err(PluginScheduleError::InvalidCandidate);
        }
        let mut state = self.lock()?;
        let plugin_id = reserved.candidate.plugin_id.clone();
        let generation = reserved.candidate.plugin_generation;
        let activation_id = reserved.candidate.activation_id;
        let queue = state.by_plugin.entry(plugin_id.clone()).or_insert_with(|| {
            PluginQueue::new(
                generation,
                activation_id,
                reserved.candidate.admission_epoch,
            )
        });
        if queue.generation != generation
            || queue.activation_id != activation_id
            || queue.admission_epoch != reserved.candidate.admission_epoch
        {
            return Err(PluginScheduleError::GenerationMismatch);
        }
        // A never-emitted prepare has no physical handler to wait for. A
        // command supersedes it atomically; the service settles its channel.
        // Once emission is claimed, retain the existing waiting/deadline path.
        if matches!(reserved.candidate.owner, PluginRequestOwner::Command(_))
            && queue.running.as_ref().is_some_and(|running| {
                !running.share_prepare_emission_started
                    && matches!(running.request.candidate.owner, PluginRequestOwner::SharePrepare(_))
            })
        {
            if let Some(running) = queue.running.take() {
                if let PluginRequestOwner::SharePrepare(owner) = &running.request.candidate.owner {
                    owner.lease.cancel();
                }
            }
        }
        if matches!(
            reserved.candidate.owner,
            PluginRequestOwner::SharePrepare(_)
                | PluginRequestOwner::PanelAction { .. }
                | PluginRequestOwner::WindowAction { .. }
                | PluginRequestOwner::ResultAction(_)
        ) && (queue.rebuilding || queue.running.is_some() || queue.waiting.is_some())
        {
            return Err(PluginScheduleError::Busy);
        }
        match &reserved.candidate.owner {
            PluginRequestOwner::Command(_) => {
                if let Some((_, reason)) = queue
                    .action_completion
                    .as_mut()
                    .filter(|(_, reason)| reason.is_none())
                {
                    *reason = Some(super::content_action::ContentActionError::Cancelled);
                }
            }
            PluginRequestOwner::SharePrepare(_)
            | PluginRequestOwner::PanelAction { .. }
            | PluginRequestOwner::WindowAction { .. }
            | PluginRequestOwner::ResultAction(_) => {
                if queue
                    .action_completion
                    .as_ref()
                    .is_some_and(|(_, reason)| reason.is_none())
                {
                    return Err(PluginScheduleError::Busy);
                }
                queue.action_completion = None;
            }
        }
        queue.remember(reserved.context.clone());
        if queue.rebuilding {
            let replaced_submission_token =
                queue.waiting.replace(reserved.clone()).map(|request| {
                    request
                        .candidate
                        .owner
                        .command()
                        .expect("waiting request is a command")
                        .submission_token
                        .clone()
                });
            let expired = queue
                .running
                .as_ref()
                .map(|running| running.request.context.clone())
                .ok_or(PluginScheduleError::Unavailable)?;
            return Ok(PluginScheduleOutcome::Waiting {
                expired,
                request_context: reserved.context,
                replaced_submission_token,
            });
        }
        if let Some(running) = queue.running.as_mut() {
            if running.current
                && matches!(
                    running.request.candidate.owner,
                    PluginRequestOwner::SharePrepare(_)
                        | PluginRequestOwner::PanelAction { .. }
                        | PluginRequestOwner::WindowAction { .. }
                        | PluginRequestOwner::ResultAction(_)
                )
            {
                running.action_retirement =
                    Some(super::content_action::ContentActionError::Cancelled);
            }
            running.current = false;
            if let PluginRequestOwner::SharePrepare(owner) = &running.request.candidate.owner {
                owner.lease.cancel();
            }
            let expired = running.request.context.clone();
            let replaced_submission_token =
                queue.waiting.replace(reserved.clone()).map(|request| {
                    request
                        .candidate
                        .owner
                        .command()
                        .expect("waiting request is a command")
                        .submission_token
                        .clone()
                });
            return Ok(PluginScheduleOutcome::Waiting {
                expired,
                request_context: reserved.context,
                replaced_submission_token,
            });
        }
        let request = dispatch(&mut state, reserved, now)?;
        Ok(PluginScheduleOutcome::Dispatched(request))
    }

    // Pending -> admission -> mutation -> authority -> scheduler -> registry.
    // Everything fallible is checked before claim; dispatch cannot queue this owner.
    pub(crate) fn admit_result_action(
        &self,
        reserved: ReservedPluginRequest,
        now: Instant,
        claim: impl FnOnce() -> Result<(), super::result_action::ResultActionErrorCode>,
    ) -> Result<ScheduledPluginRequest, super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        if !reserved.candidate.valid() {
            return Err(E::StaleRequest);
        }
        let deadline = now
            .checked_add(SUBMIT_TIMEOUT)
            .ok_or(E::ActionUnavailable)?;
        let mut state = self.lock().map_err(|_| E::ActionUnavailable)?;
        let queue = state
            .by_plugin
            .get_mut(&reserved.context.plugin_id)
            .ok_or(E::StaleRequest)?;
        if queue.generation != reserved.candidate.plugin_generation
            || queue.activation_id != reserved.candidate.activation_id
            || queue.admission_epoch != reserved.candidate.admission_epoch
        {
            return Err(E::StaleRequest);
        }
        if let Some(running) = queue.running.as_ref() {
            if let (
                PluginRequestOwner::ResultAction(active),
                PluginRequestOwner::ResultAction(next),
            ) = (&running.request.candidate.owner, &reserved.candidate.owner)
            {
                if active.lease == next.lease {
                    return Err(E::ActionInProgress);
                }
            }
        }
        if queue.rebuilding
            || queue.running.is_some()
            || queue.waiting.is_some()
            || queue
                .action_completion
                .as_ref()
                .is_some_and(|(_, reason)| reason.is_none())
        {
            return Err(E::ActionBusy);
        }
        claim()?;
        let request = ScheduledPluginRequest {
            context: reserved.context,
            candidate: reserved.candidate,
            dispatched_at: now,
            deadline,
        };
        queue.remember(request.context.clone());
        queue.action_completion = None;
        queue.running = Some(RunningRequest {
            request: request.clone(),
            current: true,
            action_retirement: None,
            notification_published: false,
            share_prepare_emission_started: false,
        });
        Ok(request)
    }
    pub(crate) fn finish_result_action<T>(
        &self,
        context: &PluginRequestContext,
        lease: &super::result_action::ResultActionLease,
        publish: impl FnOnce() -> Result<T, super::result_action::ResultActionErrorCode>,
    ) -> Result<T, super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        let mut state = self.lock().map_err(|_| E::ActionUnavailable)?;
        let queue = state
            .by_plugin
            .get_mut(&context.plugin_id)
            .ok_or(E::ActionCancelled)?;
        if !queue
            .action_completion
            .as_ref()
            .is_some_and(|(c, _)| c == context)
        {
            return Err(E::ActionCancelled);
        }
        let (_, reason) = queue.action_completion.take().ok_or(E::ActionCancelled)?;
        if reason.is_some() || !lease.live() {
            return Err(E::ActionCancelled);
        }
        publish()
    }

    pub(crate) fn complete(
        &self,
        context: &PluginRequestContext,
        now: Instant,
    ) -> Result<PluginCompletionOutcome, PluginScheduleError> {
        let mut state = self.lock()?;
        let queue = state
            .by_plugin
            .get_mut(&context.plugin_id)
            .ok_or(PluginScheduleError::InvalidCandidate)?;
        let Some(running) = queue.running.take() else {
            return Err(if queue.issued.contains(context) {
                PluginScheduleError::GenerationMismatch
            } else {
                PluginScheduleError::InvalidCandidate
            });
        };
        if running.request.context != *context {
            queue.running = Some(running);
            return Err(if queue.issued.contains(context) {
                PluginScheduleError::GenerationMismatch
            } else {
                PluginScheduleError::InvalidCandidate
            });
        }
        let accepted = running.current && running.request.candidate.owner.live();
        if matches!(
            running.request.candidate.owner,
            PluginRequestOwner::SharePrepare(_)
                | PluginRequestOwner::PanelAction { .. }
                | PluginRequestOwner::WindowAction { .. }
                | PluginRequestOwner::ResultAction(_)
        ) {
            queue.action_completion = Some((
                context.clone(),
                if !running.request.candidate.owner.live() {
                    Some(super::content_action::ContentActionError::Expired)
                } else {
                    running.action_retirement
                },
            ));
        }
        let next = if queue.rebuilding {
            None
        } else if let Some(candidate) = queue.waiting.take() {
            Some(dispatch(&mut state, candidate, now)?)
        } else {
            None
        };
        Ok(PluginCompletionOutcome {
            completed_owner: running.request.candidate.owner.clone(),
            credential_publication: None,
            accepted,
            next,
        })
    }

    pub(crate) fn context_status(&self, context: &PluginRequestContext) -> PluginContextStatus {
        if !context.valid_shape() {
            return PluginContextStatus::Invalid;
        }
        let Ok(state) = self.state.lock() else {
            return PluginContextStatus::Invalid;
        };
        let Some(queue) = state.by_plugin.get(&context.plugin_id) else {
            return PluginContextStatus::Invalid;
        };
        if queue.running.as_ref().is_some_and(|running| {
            running.current
                && running.request.candidate.owner.live()
                && running.request.context == *context
                && queue.generation == context.plugin_generation
        }) {
            PluginContextStatus::Current
        } else if queue.issued.contains(context) {
            PluginContextStatus::Expired
        } else {
            PluginContextStatus::Invalid
        }
    }

    // The physical handler is finished, but its response still needs a live-owner claim.
    pub(crate) fn discard_panel_completion(&self, context: &PluginRequestContext) {
        self.discard_action_completion(context);
    }
    pub(crate) fn claim_panel_completion(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), super::panel_action::PanelActionError> {
        self.claim_action_completion(context).map_err(Into::into)
    }
    pub(crate) fn discard_action_completion(&self, context: &PluginRequestContext) {
        if let Ok(mut state) = self.lock() {
            if let Some(queue) = state.by_plugin.get_mut(&context.plugin_id) {
                if queue
                    .action_completion
                    .as_ref()
                    .is_some_and(|(c, _)| c == context)
                {
                    queue.action_completion = None;
                }
            }
        }
    }

    pub(crate) fn claim_action_completion(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), super::content_action::ContentActionError> {
        use super::content_action::ContentActionError as E;
        let mut state = self.lock().map_err(|_| E::Unavailable)?;
        let queue = state
            .by_plugin
            .get_mut(&context.plugin_id)
            .ok_or(E::Expired)?;
        if !queue
            .action_completion
            .as_ref()
            .is_some_and(|(c, _)| c == context)
        {
            return Err(E::Expired);
        }
        let (_, reason) = queue.action_completion.take().ok_or(E::Expired)?;
        reason.map_or(Ok(()), Err)
    }

    /// Serialize emission against abort and command replacement. After this
    /// claim, cancellation must preserve physical occupancy even if emit fails.
    pub(crate) fn claim_share_prepare_emission(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), PluginScheduleError> {
        let mut state = self.lock()?;
        let running = state.by_plugin.get_mut(&context.plugin_id)
            .and_then(|queue| queue.running.as_mut())
            .filter(|running| {
                running.request.context == *context
                    && running.current
                    && running.request.candidate.owner.live()
                    && matches!(running.request.candidate.owner, PluginRequestOwner::SharePrepare(_))
                    && !running.share_prepare_emission_started
            })
            .ok_or(PluginScheduleError::GenerationMismatch)?;
        running.share_prepare_emission_started = true;
        Ok(())
    }

    /// Called after logical retirement. Both this abort and emission claim use
    /// the scheduler lock, so a released slot can never subsequently emit.
    pub(crate) fn abort_unemitted_share_prepare(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), PluginScheduleError> {
        let mut state = self.lock()?;
        if let Some(queue) = state.by_plugin.get_mut(&context.plugin_id) {
            if queue.running.as_ref().is_some_and(|running| {
                running.request.context == *context
                    && !running.current
                    && !running.share_prepare_emission_started
                    && matches!(running.request.candidate.owner, PluginRequestOwner::SharePrepare(_))
            }) {
                // enqueue_reserved never queues behind an unclaimed prepare.
                debug_assert!(queue.waiting.is_none());
                queue.running = None;
            }
        }
        Ok(())
    }

    pub(crate) fn retire_context(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), PluginScheduleError> {
        let mut state = self.lock()?;
        if let Some(queue) = state.by_plugin.get_mut(&context.plugin_id) {
            if let Some((_, reason)) = queue
                .action_completion
                .as_mut()
                .filter(|(c, _)| c == context)
            {
                *reason = Some(super::content_action::ContentActionError::Expired);
            }
            if let Some(running) = queue
                .running
                .as_mut()
                .filter(|r| r.request.context == *context)
            {
                running.current = false;
                if let PluginRequestOwner::SharePrepare(owner) = &running.request.candidate.owner {
                    owner.lease.cancel();
                }
                running.action_retirement =
                    Some(super::content_action::ContentActionError::Expired);
            }
        }
        Ok(())
    }

    pub(crate) fn cancel(
        &self,
        context: &PluginRequestContext,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PluginScheduleError> {
        if !context.valid_shape() {
            return Ok(None);
        }
        let mut state = self.lock()?;
        let Some(queue) = state.by_plugin.get_mut(&context.plugin_id) else {
            return Ok(None);
        };
        if let Some(running) = queue.running.as_mut().filter(|running| {
            running.request.context == *context
                && matches!(
                    running.request.candidate.owner,
                    PluginRequestOwner::ResultAction(_) | PluginRequestOwner::SharePrepare(_)
                )
        }) {
            running.current = false;
            if let PluginRequestOwner::SharePrepare(owner) = &running.request.candidate.owner {
                owner.lease.cancel();
            }
            running.action_retirement = Some(super::content_action::ContentActionError::Cancelled);
            return Ok(None);
        }
        if queue
            .running
            .as_ref()
            .is_some_and(|running| running.request.context == *context)
        {
            queue.running = None;
            let next = if queue.rebuilding {
                None
            } else {
                queue.waiting.take()
            };
            return next
                .map(|candidate| dispatch(&mut state, candidate, now))
                .transpose();
        }
        if queue
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.context == *context)
        {
            queue.waiting = None;
            return Ok(None);
        }
        Ok(None)
    }

    pub(crate) fn with_current<T>(
        &self,
        context: &PluginRequestContext,
        operation: impl FnOnce(&mut PluginCurrentRequest<'_>) -> T,
    ) -> Result<T, PluginContextAccessError> {
        if !context.valid_shape() {
            return Err(PluginContextAccessError::Invalid);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| PluginContextAccessError::Unavailable)?;
        let queue = state
            .by_plugin
            .get_mut(&context.plugin_id)
            .ok_or(PluginContextAccessError::Invalid)?;
        if queue.running.as_ref().is_some_and(|running| {
            running.current
                && running.request.candidate.owner.live()
                && running.request.context == *context
                && queue.generation == context.plugin_generation
        }) {
            let running = queue
                .running
                .as_mut()
                .ok_or(PluginContextAccessError::Unavailable)?;
            Ok(operation(&mut PluginCurrentRequest {
                effects_allowed: !matches!(running.request.candidate.owner, PluginRequestOwner::SharePrepare(_)),
                panel_action: matches!(
                    running.request.candidate.owner,
                    PluginRequestOwner::PanelAction { .. }
                ),
                plugin_generation: running.request.candidate.plugin_generation,
                activation_id: running.request.candidate.activation_id,
                admission_epoch: running.request.candidate.admission_epoch,
                notification_published: &mut running.notification_published,
            }))
        } else if queue.issued.contains(context) {
            Err(PluginContextAccessError::Expired)
        } else {
            Err(PluginContextAccessError::Invalid)
        }
    }

    pub(crate) fn expire_timeouts(
        &self,
        now: Instant,
    ) -> Result<Vec<PluginRuntimeReplacement>, PluginScheduleError> {
        let mut state = self.lock()?;
        let mut replacements = Vec::new();
        for (plugin_id, queue) in &mut state.by_plugin {
            let timed_out = queue
                .running
                .as_ref()
                .is_some_and(|running| running.request.deadline <= now);
            if !timed_out {
                continue;
            }
            let running = queue
                .running
                .take()
                .ok_or(PluginScheduleError::Unavailable)?;
            let previous_generation = queue.generation;
            let new_generation = previous_generation
                .checked_add(1)
                .ok_or(PluginScheduleError::GenerationExhausted)?;
            let counts_as_fault = running.current && running.request.candidate.owner.live();
            queue.result_publication.revoke();
            queue.rebind_waiting_generation(new_generation);
            queue.generation = new_generation;
            queue.rebuilding = true;
            replacements.push(PluginRuntimeReplacement {
                plugin_id: plugin_id.clone(),
                previous_activation_id: queue.activation_id,
                previous_admission_epoch: queue.admission_epoch,
                expired: running.request.context,
                previous_generation,
                new_generation,
                has_waiting: queue.waiting.is_some(),
                counts_as_fault,
            });
        }
        Ok(replacements)
    }

    pub(crate) fn runtime_replaced(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PluginScheduleError> {
        let mut state = self.lock()?;
        let queue = state
            .by_plugin
            .get_mut(plugin_id)
            .ok_or(PluginScheduleError::InvalidCandidate)?;
        if activation_id == 0
            || queue.generation != generation
            || admission_epoch == 0
            || !queue.rebuilding
            || queue.running.is_some()
        {
            return Err(PluginScheduleError::GenerationMismatch);
        }
        queue.activation_id = activation_id;
        queue.admission_epoch = admission_epoch;
        if let Some(waiting) = queue.waiting.as_mut() {
            waiting.candidate.activation_id = activation_id;
            waiting.candidate.admission_epoch = admission_epoch;
        }
        queue.result_publication.revoke();
        queue.result_publication = super::result_action::ResultActionLease::default();
        queue.rebuilding = false;
        let candidate = queue.waiting.take();
        candidate
            .map(|candidate| dispatch(&mut state, candidate, now))
            .transpose()
    }

    // A host credential lease belongs only to the request that used the service.
    // Keep its physical execution slot until completion, as with permission revocation.
    pub(crate) fn retire_request_context(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), PluginScheduleError> {
        let mut state = self.lock()?;
        let Some(queue) = state.by_plugin.get_mut(&context.plugin_id) else {
            return Ok(());
        };
        let mut changed = false;
        if let Some(running) = queue
            .running
            .as_mut()
            .filter(|r| r.request.context == *context)
        {
            running.current = false;
            if let PluginRequestOwner::SharePrepare(owner) = &running.request.candidate.owner {
                owner.lease.cancel();
            }
            running.action_retirement = Some(super::content_action::ContentActionError::Expired);
            changed = true;
        }
        if let Some((_, reason)) = queue
            .action_completion
            .as_mut()
            .filter(|(current, _)| current == context)
        {
            *reason = Some(super::content_action::ContentActionError::Expired);
            changed = true;
        }
        if changed {
            queue.result_publication.revoke();
            queue.result_publication = super::result_action::ResultActionLease::default();
        }
        Ok(())
    }
    /// Service results can outlive the physical request slot. Restore admission after
    /// revoking their publication, without reviving the old result or an old generation.
    pub(crate) fn retire_service_request_context(
        &self,
        context: &PluginRequestContext,
    ) -> Result<(), PluginScheduleError> {
        self.retire_request_context(context)?;
        let mut state = self.lock()?;
        if let Some(queue) = state.by_plugin.get_mut(&context.plugin_id) {
            if queue.generation == context.plugin_generation && !queue.result_publication.live() {
                queue.result_publication = super::result_action::ResultActionLease::default();
            }
        }
        Ok(())
    }

    // Retire logical ownership while the Runtime still owns its execution slot.
    pub(crate) fn retire_plugin_requests(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<(PluginRequestContext, Option<String>)>, PluginScheduleError> {
        let mut state = self.lock()?;
        let Some(queue) = state.by_plugin.get_mut(plugin_id) else {
            return Ok(Vec::new());
        };
        queue.result_publication.revoke();
        queue.result_publication = super::result_action::ResultActionLease::default();
        let mut retired = Vec::new();
        if let Some((context, reason)) = queue.action_completion.as_mut() {
            *reason = Some(super::content_action::ContentActionError::Expired);
            retired.push((context.clone(), None));
        }
        if let Some(running) = queue.running.as_mut() {
            running.current = false;
            if let PluginRequestOwner::SharePrepare(owner) = &running.request.candidate.owner {
                owner.lease.cancel();
            }
            running.action_retirement = Some(super::content_action::ContentActionError::Expired);
            retired.push((
                running.request.context.clone(),
                running.request.candidate.owner.token().map(str::to_owned),
            ));
        }
        if let Some(waiting) = queue.waiting.take() {
            retired.push((
                waiting.context,
                waiting.candidate.owner.token().map(str::to_owned),
            ));
        }
        Ok(retired)
    }

    pub(crate) fn invalidate_plugin(
        &self,
        plugin_id: &str,
        next_activation: Option<(u64, u64, u64)>,
    ) -> Result<Option<PluginRequestContext>, PluginScheduleError> {
        let mut state = self.lock()?;
        let Some(queue) = state.by_plugin.get_mut(plugin_id) else {
            if let Some((generation, activation_id, admission_epoch)) = next_activation {
                if generation == 0 || activation_id == 0 || admission_epoch == 0 {
                    return Err(PluginScheduleError::GenerationMismatch);
                }
                state.by_plugin.insert(
                    plugin_id.into(),
                    PluginQueue::new(generation, activation_id, admission_epoch),
                );
            }
            return Ok(None);
        };
        if let Some((generation, activation_id, admission_epoch)) = next_activation {
            if generation == 0
                || activation_id == 0
                || generation < queue.generation
                || activation_id == queue.activation_id
                || admission_epoch == 0
                || admission_epoch == queue.admission_epoch
            {
                return Err(PluginScheduleError::GenerationMismatch);
            }
            queue.generation = generation;
            queue.activation_id = activation_id;
            queue.admission_epoch = admission_epoch;
        }
        queue.result_publication.revoke();
        queue.result_publication = super::result_action::ResultActionLease::default();
        let expired = queue.running.take().map(|running| running.request.context);
        queue.waiting = None;
        queue.rebuilding = false;
        Ok(expired)
    }

    pub(crate) fn forget_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginRequestContext>, PluginScheduleError> {
        let mut state = self.lock()?;
        Ok(state.by_plugin.remove(plugin_id).and_then(|queue| {
            queue.result_publication.revoke();
            queue.running.map(|running| running.request.context)
        }))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, SchedulerState>, PluginScheduleError> {
        self.state
            .lock()
            .map_err(|_| PluginScheduleError::Unavailable)
    }
}

fn dispatch(
    state: &mut SchedulerState,
    reserved: ReservedPluginRequest,
    now: Instant,
) -> Result<ScheduledPluginRequest, PluginScheduleError> {
    let timeout = match reserved.candidate.activation_mode {
        PublicActivationMode::Live => LIVE_TIMEOUT,
        PublicActivationMode::Submit => SUBMIT_TIMEOUT,
    };
    let request = ScheduledPluginRequest {
        context: reserved.context,
        candidate: reserved.candidate,
        dispatched_at: now,
        deadline: now
            .checked_add(timeout)
            .ok_or(PluginScheduleError::Unavailable)?,
    };
    let queue = state
        .by_plugin
        .get_mut(&request.context.plugin_id)
        .ok_or(PluginScheduleError::Unavailable)?;
    if queue.running.is_some()
        || queue.rebuilding
        || queue.generation != request.context.plugin_generation
        || queue.activation_id != request.candidate.activation_id
        || queue.admission_epoch != request.candidate.admission_epoch
    {
        return Err(PluginScheduleError::Unavailable);
    }
    queue.running = Some(RunningRequest {
        request: request.clone(),
        current: true,
        action_retirement: None,
        notification_published: false,
        share_prepare_emission_started: false,
    });
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(value: &str) -> PluginSubmissionOwner {
        PluginSubmissionOwner {
            ui_intent_epoch: 1,
            control_value: value.into(),
            submission_token: format!("submission-{value}"),
        }
    }

    pub(super) fn candidate(
        value: &str,
        generation: u64,
        mode: PublicActivationMode,
    ) -> PluginRequestCandidate {
        PluginRequestCandidate {
            command_id: "default".into(),
            plugin_id: "com.example.scheduler".into(),
            plugin_generation: generation,
            activation_id: 17,
            admission_epoch: 23,
            activation_mode: mode,
            input: value.into(),
            owner: crate::public_plugins::PluginRequestOwner::Command(owner(value)),
        }
    }

    fn command_candidate(
        command_id: &str,
        input: &str,
        generation: u64,
        mode: PublicActivationMode,
    ) -> PluginRequestCandidate {
        let mut candidate = candidate(input, generation, mode);
        candidate.command_id = command_id.into();
        candidate
    }

    #[test]
    fn panel_action_retirement_reason_survives_completion_and_lifecycle_still_expires() {
        use super::super::panel_action::{PanelActionError as E, PanelActionRequest};
        for lifecycle in 0..3 {
            let scheduler = PluginRequestScheduler::default();
            let now = Instant::now();
            let mut panel = candidate("", 1, PublicActivationMode::Submit);
            panel.owner = PluginRequestOwner::PanelAction {
                content_label: "panel".into(),
                session_epoch: 1,
                request: PanelActionRequest {
                    image_id: None,
                    action: "save".into(),
                    payload: serde_json::Value::Null,
                },
            };
            let PluginScheduleOutcome::Dispatched(action) = scheduler.enqueue(panel, now).unwrap()
            else {
                panic!()
            };
            assert!(matches!(
                scheduler
                    .enqueue(candidate("next", 1, PublicActivationMode::Submit), now)
                    .unwrap(),
                PluginScheduleOutcome::Waiting { .. }
            ));
            if lifecycle == 1 {
                scheduler.retire_context(&action.context).unwrap();
            } else if lifecycle == 2 {
                scheduler
                    .retire_plugin_requests(&action.context.plugin_id)
                    .unwrap();
            }
            let before = scheduler.lock().unwrap().by_plugin[&action.context.plugin_id]
                .running
                .as_ref()
                .unwrap()
                .request
                .clone();
            assert_eq!(
                before, action,
                "retirement keeps the physical request and deadline"
            );
            assert_eq!(
                scheduler.context_status(&action.context),
                PluginContextStatus::Expired
            );
            let outcome = scheduler.complete(&action.context, now).unwrap();
            assert!(!outcome.accepted);
            // Further Host commands cannot turn lifecycle expiry back into cancellation.
            scheduler
                .enqueue(candidate("latest", 1, PublicActivationMode::Submit), now)
                .unwrap();
            assert_eq!(
                scheduler.claim_panel_completion(&action.context),
                Err(if lifecycle == 0 {
                    E::PanelRequestCancelledError
                } else {
                    E::ExpiredPanelSession
                })
            );
            assert_eq!(
                scheduler.claim_panel_completion(&action.context),
                Err(E::ExpiredPanelSession)
            );
            assert!(
                scheduler.lock().unwrap().by_plugin[&action.context.plugin_id]
                    .action_completion
                    .is_none()
            );
        }
    }

    #[test]
    fn command_candidates_reject_invalid_identity_before_reserving() {
        for id in ["", "Upper", "with space", "constructor.invalid"] {
            let scheduler = PluginRequestScheduler::default();
            assert_eq!(
                scheduler.reserve(command_candidate(
                    id,
                    "input",
                    1,
                    PublicActivationMode::Live
                )),
                Err(PluginScheduleError::InvalidCandidate)
            );
        }
        assert!(PluginRequestScheduler::default()
            .reserve(command_candidate(
                "constructor",
                "input",
                1,
                PublicActivationMode::Live
            ))
            .is_ok());
    }

    #[test]
    fn recovery_accepts_same_generation_only_with_both_new_identity_parts() {
        let scheduler = PluginRequestScheduler::default();
        let now = Instant::now();
        let PluginScheduleOutcome::Dispatched(old) = scheduler
            .enqueue(candidate("old", 2, PublicActivationMode::Submit), now)
            .unwrap()
        else {
            panic!()
        };
        scheduler
            .invalidate_plugin("com.example.scheduler", Some((2, 18, 24)))
            .unwrap();
        assert_eq!(
            scheduler.context_status(&old.context),
            PluginContextStatus::Expired
        );
        let mut new = candidate("new", 2, PublicActivationMode::Submit);
        new.activation_id = 18;
        new.admission_epoch = 24;
        assert!(matches!(
            scheduler.enqueue(new, now).unwrap(),
            PluginScheduleOutcome::Dispatched(_)
        ));
    }

    #[test]
    fn recovery_rejects_old_identity_or_lower_generation_without_losing_running() {
        for identity in [(1, 18, 24), (2, 17, 24), (2, 18, 23)] {
            let scheduler = PluginRequestScheduler::default();
            let now = Instant::now();
            let PluginScheduleOutcome::Dispatched(old) = scheduler
                .enqueue(candidate("old", 2, PublicActivationMode::Submit), now)
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(
                scheduler.invalidate_plugin("com.example.scheduler", Some(identity)),
                Err(PluginScheduleError::GenerationMismatch)
            );
            assert_eq!(
                scheduler.context_status(&old.context),
                PluginContextStatus::Current
            );
        }
    }

    #[test]
    fn retired_running_keeps_deadline_until_timeout_and_then_dispatches_waiter() {
        let scheduler = PluginRequestScheduler::default();
        let now = Instant::now();
        let PluginScheduleOutcome::Dispatched(old) = scheduler
            .enqueue(candidate("old", 1, PublicActivationMode::Submit), now)
            .unwrap()
        else {
            panic!()
        };
        scheduler
            .enqueue(candidate("discarded", 1, PublicActivationMode::Submit), now)
            .unwrap();
        let retired = scheduler
            .retire_plugin_requests("com.example.scheduler")
            .unwrap();
        assert_eq!(retired.len(), 2);
        assert_eq!(retired[0].1.as_deref(), Some("submission-old"));
        assert_eq!(retired[1].1.as_deref(), Some("submission-discarded"));
        assert!(matches!(
            scheduler
                .enqueue(candidate("next", 1, PublicActivationMode::Submit), now)
                .unwrap(),
            PluginScheduleOutcome::Waiting { .. }
        ));
        assert!(scheduler
            .expire_timeouts(old.deadline - Duration::from_millis(1))
            .unwrap()
            .is_empty());
        let replacement = scheduler
            .expire_timeouts(old.deadline)
            .unwrap()
            .pop()
            .unwrap();
        assert!(!replacement.counts_as_fault);
        let next = scheduler
            .runtime_replaced(
                "com.example.scheduler",
                replacement.new_generation,
                18,
                24,
                old.deadline,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            next.candidate
                .owner
                .command()
                .expect("waiting request is a command")
                .submission_token,
            "submission-next"
        );
        assert_eq!(next.context.plugin_generation, 2);
    }

    #[test]
    fn reserved_request_identity_is_stable_until_dispatch() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let reserved = scheduler
            .reserve(candidate("A", 1, PublicActivationMode::Submit))
            .unwrap();
        let request_id = reserved.context.request_id.clone();

        let dispatched = match scheduler.enqueue_reserved(reserved, start).unwrap() {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };

        assert_eq!(dispatched.context.request_id, request_id);
    }

    #[test]
    fn waiting_request_keeps_reserved_identity_when_promoted() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let first = match scheduler
            .enqueue(candidate("A", 1, PublicActivationMode::Submit), start)
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };
        let reserved = scheduler
            .reserve(candidate("B", 1, PublicActivationMode::Submit))
            .unwrap();
        let request_id = reserved.context.request_id.clone();

        assert!(matches!(
            scheduler
                .enqueue_reserved(reserved, start + Duration::from_secs(1))
                .unwrap(),
            PluginScheduleOutcome::Waiting {
                request_context,
                ..
            } if request_context.request_id == request_id
        ));
        let promoted = scheduler
            .complete(&first.context, start + Duration::from_secs(2))
            .unwrap()
            .next
            .unwrap();

        assert_eq!(promoted.context.request_id, request_id);
    }

    #[test]
    fn cancelling_one_current_request_leaves_scheduler_ready_for_a_new_request() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let first = match scheduler
            .enqueue(candidate("A", 1, PublicActivationMode::Submit), start)
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };
        assert!(scheduler.cancel(&first.context, start).unwrap().is_none());
        assert_eq!(
            scheduler.context_status(&first.context),
            PluginContextStatus::Expired
        );
        assert!(matches!(
            scheduler
                .enqueue(
                    candidate("B", 1, PublicActivationMode::Submit),
                    start + Duration::from_secs(1),
                )
                .unwrap(),
            PluginScheduleOutcome::Dispatched(request) if request.candidate.input == "B"
        ));
    }

    #[test]
    fn cancelling_running_request_promotes_existing_waiter_atomically() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let first = match scheduler
            .enqueue(candidate("A", 1, PublicActivationMode::Submit), start)
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };
        assert!(matches!(
            scheduler
                .enqueue(
                    candidate("C", 1, PublicActivationMode::Submit),
                    start + Duration::from_secs(1),
                )
                .unwrap(),
            PluginScheduleOutcome::Waiting { .. }
        ));

        let promoted = scheduler
            .cancel(&first.context, start + Duration::from_secs(2))
            .unwrap()
            .expect("C must be promoted while cancelling A");

        assert_eq!(promoted.candidate.input, "C");
        assert_eq!(
            scheduler.context_status(&promoted.context),
            PluginContextStatus::Current
        );
        assert_eq!(
            scheduler.context_status(&first.context),
            PluginContextStatus::Expired
        );
    }

    #[test]
    fn running_a_expires_while_waiting_b_is_replaced_by_c() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let a = match scheduler
            .enqueue(
                command_candidate("upper", "A", 1, PublicActivationMode::Live),
                start,
            )
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };
        assert_eq!(a.deadline.duration_since(start), LIVE_TIMEOUT);
        assert_eq!(
            scheduler.context_status(&a.context),
            PluginContextStatus::Current
        );

        assert_eq!(
            scheduler
                .enqueue(
                    command_candidate("constructor", "B", 1, PublicActivationMode::Submit),
                    start + Duration::from_secs(1)
                )
                .unwrap(),
            PluginScheduleOutcome::Waiting {
                expired: a.context.clone(),
                request_context: PluginRequestContext {
                    plugin_id: "com.example.scheduler".into(),
                    plugin_generation: 1,
                    request_id: "public-request-0000000000000002".into(),
                },
                replaced_submission_token: None,
            }
        );
        assert_eq!(
            scheduler.context_status(&a.context),
            PluginContextStatus::Expired
        );
        assert!(matches!(
            scheduler
                .enqueue(
                    command_candidate("lower", "C", 1, PublicActivationMode::Submit),
                    start + Duration::from_secs(2)
                )
                .unwrap(),
            PluginScheduleOutcome::Waiting {
                replaced_submission_token: Some(token),
                ..
            } if token == "submission-B"
        ));

        let completed = scheduler
            .complete(&a.context, start + Duration::from_secs(3))
            .unwrap();
        assert!(!completed.accepted);
        let c = completed.next.unwrap();
        assert_eq!(c.candidate.input, "C");
        assert_eq!(c.candidate.command_id, "lower");
        assert_eq!(c.deadline.duration_since(c.dispatched_at), SUBMIT_TIMEOUT);
        assert_eq!(
            scheduler.context_status(&c.context),
            PluginContextStatus::Current
        );
    }

    #[test]
    fn timeout_starts_at_dispatch_and_rebuilds_before_waiting_dispatch() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let a = match scheduler
            .enqueue(
                command_candidate("upper", "A", 7, PublicActivationMode::Live),
                start,
            )
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };
        scheduler
            .enqueue(
                command_candidate("constructor", "B", 7, PublicActivationMode::Live),
                start + Duration::from_secs(4),
            )
            .unwrap();
        assert!(scheduler
            .expire_timeouts(start + Duration::from_secs(4))
            .unwrap()
            .is_empty());

        let replacements = scheduler.expire_timeouts(start + LIVE_TIMEOUT).unwrap();
        assert_eq!(
            replacements,
            vec![PluginRuntimeReplacement {
                plugin_id: "com.example.scheduler".into(),
                previous_activation_id: 17,
                previous_admission_epoch: 23,
                expired: a.context.clone(),
                previous_generation: 7,
                new_generation: 8,
                has_waiting: true,
                counts_as_fault: false,
            }]
        );
        assert_eq!(
            scheduler.context_status(&a.context),
            PluginContextStatus::Expired
        );
        let b = scheduler
            .runtime_replaced(
                "com.example.scheduler",
                8,
                18,
                24,
                start + Duration::from_secs(6),
            )
            .unwrap()
            .unwrap();
        assert_eq!(b.candidate.input, "B");
        assert_eq!(b.candidate.command_id, "constructor");
        assert_eq!(b.candidate.activation_id, 18);
        assert_eq!(b.candidate.admission_epoch, 24);
        assert_eq!(b.context.plugin_generation, 8);
        assert_eq!(b.dispatched_at, start + Duration::from_secs(6));
    }

    #[test]
    fn one_plugin_timeout_rebuild_does_not_block_another_plugin() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let candidate = |plugin_id: &str, value: &str, generation: u64| PluginRequestCandidate {
            command_id: "default".into(),
            plugin_id: plugin_id.into(),
            plugin_generation: generation,
            activation_id: generation + 10,
            admission_epoch: generation + 20,
            activation_mode: PublicActivationMode::Live,
            input: value.into(),
            owner: crate::public_plugins::PluginRequestOwner::Command(owner(value)),
        };
        let failed = match scheduler
            .enqueue(candidate("com.example.failed", "A", 1), start)
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("failed plugin must dispatch"),
        };
        scheduler
            .enqueue(candidate("com.example.failed", "latest", 1), start)
            .unwrap();
        let healthy = match scheduler
            .enqueue(
                candidate("com.example.healthy", "ready", 4),
                start + Duration::from_secs(1),
            )
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("healthy plugin must dispatch"),
        };

        let replacements = scheduler.expire_timeouts(start + LIVE_TIMEOUT).unwrap();
        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].plugin_id, "com.example.failed");
        assert_eq!(replacements[0].expired, failed.context);
        assert_eq!(
            scheduler.context_status(&healthy.context),
            PluginContextStatus::Current
        );
        assert!(
            scheduler
                .complete(&healthy.context, start + LIVE_TIMEOUT)
                .unwrap()
                .accepted
        );

        let latest = scheduler
            .runtime_replaced("com.example.failed", 2, 12, 22, start + LIVE_TIMEOUT)
            .unwrap()
            .unwrap();
        assert_eq!(latest.candidate.input, "latest");
        assert_eq!(latest.context.plugin_generation, 2);
    }

    #[test]
    fn notification_publish_state_belongs_to_each_running_request() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let first = match scheduler
            .enqueue(candidate("first", 1, PublicActivationMode::Submit), start)
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("first request must dispatch"),
        };

        scheduler
            .with_current(&first.context, |current| {
                assert!(!current.notification_published());
                current.mark_notification_published();
                assert!(current.notification_published());
            })
            .unwrap();
        scheduler
            .enqueue(
                candidate("second", 1, PublicActivationMode::Submit),
                start + Duration::from_secs(1),
            )
            .unwrap();
        let second = scheduler
            .complete(&first.context, start + Duration::from_secs(2))
            .unwrap()
            .next
            .unwrap();

        scheduler
            .with_current(&second.context, |current| {
                assert!(!current.notification_published());
            })
            .unwrap();
    }

    #[test]
    fn forgotten_plugin_can_reuse_generation_but_not_the_old_activation_identity() {
        let scheduler = PluginRequestScheduler::default();
        let now = Instant::now();
        let old = scheduler
            .enqueue(candidate("old", 1, PublicActivationMode::Submit), now)
            .unwrap();
        let old_context = match old {
            PluginScheduleOutcome::Dispatched(request) => request.context,
            PluginScheduleOutcome::Waiting { .. } => panic!("old activation must dispatch"),
        };
        scheduler.forget_plugin("com.example.scheduler").unwrap();

        let mut current = candidate("current", 1, PublicActivationMode::Submit);
        current.activation_id = 18;
        current.admission_epoch = 24;
        assert!(matches!(
            scheduler.enqueue(current, now),
            Ok(PluginScheduleOutcome::Dispatched(_))
        ));
        assert_eq!(
            scheduler.context_status(&old_context),
            PluginContextStatus::Invalid
        );
        assert_eq!(
            scheduler.enqueue(candidate("stale", 1, PublicActivationMode::Submit), now),
            Err(PluginScheduleError::GenerationMismatch)
        );
    }
    #[test]
    fn result_action_publication_token_never_revives_after_queue_aba_rebuild_or_shutdown() {
        for case in 0..4 {
            let scheduler = PluginRequestScheduler::default();
            let now = Instant::now();
            let PluginScheduleOutcome::Dispatched(request) = scheduler
                .enqueue(candidate("first", 1, PublicActivationMode::Submit), now)
                .unwrap()
            else {
                panic!()
            };
            let old = scheduler
                .capture_result_publication(&request.context.plugin_id, 1, 17, 23)
                .unwrap();
            match case {
                0 => {
                    scheduler.forget_plugin(&request.context.plugin_id).unwrap();
                    scheduler
                        .enqueue(candidate("new", 1, PublicActivationMode::Submit), now)
                        .unwrap();
                }
                1 => {
                    let replacements = scheduler.expire_timeouts(request.deadline).unwrap();
                    assert_eq!(replacements.len(), 1);
                    assert!(scheduler
                        .capture_result_publication(&request.context.plugin_id, 2, 17, 23)
                        .is_err());
                    scheduler
                        .runtime_replaced(&request.context.plugin_id, 2, 12, 24, now)
                        .unwrap();
                }
                2 => {
                    scheduler
                        .invalidate_plugin(&request.context.plugin_id, Some((2, 12, 24)))
                        .unwrap();
                }
                _ => scheduler.close_result_publications(),
            }
            assert!(!old.live());
            assert!(scheduler
                .with_result_publication(&request.context.plugin_id, &old, || panic!(
                    "old token published"
                ))
                .is_err());
            if case < 3 {
                let generation = if case == 0 { 1 } else { 2 };
                let activation = if case == 0 { 17 } else { 12 };
                let admission = if case == 0 { 23 } else { 24 };
                let fresh = scheduler
                    .capture_result_publication(
                        &request.context.plugin_id,
                        generation,
                        activation,
                        admission,
                    )
                    .unwrap();
                assert_ne!(old, fresh);
                assert!(fresh.live());
            }
        }
    }
    #[test]
    fn result_action_publication_callback_excludes_timeout_until_atomic_registry_work_finishes() {
        use std::{
            sync::{mpsc, Arc},
            thread,
        };
        let scheduler = Arc::new(PluginRequestScheduler::default());
        let now = Instant::now();
        let PluginScheduleOutcome::Dispatched(request) = scheduler
            .enqueue(candidate("first", 1, PublicActivationMode::Submit), now)
            .unwrap()
        else {
            panic!()
        };
        let token = scheduler
            .capture_result_publication(&request.context.plugin_id, 1, 17, 23)
            .unwrap();
        let (entered, observed) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = {
            let scheduler = scheduler.clone();
            let token = token.clone();
            let plugin = request.context.plugin_id.clone();
            thread::spawn(move || {
                scheduler
                    .with_result_publication(&plugin, &token, || {
                        entered.send(()).unwrap();
                        wait.recv().unwrap();
                        assert!(token.live());
                    })
                    .unwrap()
            })
        };
        observed.recv().unwrap();
        let (done, finished) = mpsc::channel();
        let timeout = {
            let scheduler = scheduler.clone();
            thread::spawn(move || {
                done.send(scheduler.expire_timeouts(request.deadline).unwrap())
                    .unwrap()
            })
        };
        assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(2)).unwrap().len(),
            1
        );
        timeout.join().unwrap();
        assert!(!token.live());
    }
}

#[cfg(test)]
mod share_prepare_tests;
