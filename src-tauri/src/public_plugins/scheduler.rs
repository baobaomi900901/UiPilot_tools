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
pub(crate) struct PluginRequestCandidate {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) input: String,
    pub(crate) owner: PluginSubmissionOwner,
}

impl PluginRequestCandidate {
    fn valid(&self) -> bool {
        valid_plugin_id(&self.plugin_id)
            && self.plugin_generation != 0
            && self.activation_id != 0
            && self.admission_epoch != 0
            && self.input.len() <= 64 * 1024
            && !self.input.contains('\0')
            && self.owner.ui_intent_epoch != 0
            && !self.owner.submission_token.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScheduledPluginRequest {
    pub(crate) context: PluginRequestContext,
    pub(crate) candidate: PluginRequestCandidate,
    pub(crate) dispatched_at: Instant,
    pub(crate) deadline: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginScheduleError {
    InvalidCandidate,
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
        replaced_submission_token: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginCompletionOutcome {
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
    notification_published: bool,
}

pub(crate) struct PluginCurrentRequest<'a> {
    activation_id: u64,
    notification_published: &'a mut bool,
}

impl PluginCurrentRequest<'_> {
    pub(crate) fn activation_id(&self) -> u64 {
        self.activation_id
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
    running: Option<RunningRequest>,
    waiting: Option<PluginRequestCandidate>,
    rebuilding: bool,
    issued: HashSet<PluginRequestContext>,
    issued_order: VecDeque<PluginRequestContext>,
}

impl PluginQueue {
    fn new(generation: u64, activation_id: u64, admission_epoch: u64) -> Self {
        Self {
            generation,
            activation_id,
            admission_epoch,
            running: None,
            waiting: None,
            rebuilding: false,
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
}

struct SchedulerState {
    next_request: u64,
    by_plugin: HashMap<String, PluginQueue>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            next_request: 1,
            by_plugin: HashMap::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct PluginRequestScheduler {
    state: Mutex<SchedulerState>,
}

impl PluginRequestScheduler {
    pub(crate) fn enqueue(
        &self,
        candidate: PluginRequestCandidate,
        now: Instant,
    ) -> Result<PluginScheduleOutcome, PluginScheduleError> {
        if !candidate.valid() {
            return Err(PluginScheduleError::InvalidCandidate);
        }
        let mut state = self.lock()?;
        let plugin_id = candidate.plugin_id.clone();
        let generation = candidate.plugin_generation;
        let activation_id = candidate.activation_id;
        let queue = state.by_plugin.entry(plugin_id.clone()).or_insert_with(|| {
            PluginQueue::new(generation, activation_id, candidate.admission_epoch)
        });
        if queue.generation != generation
            || queue.activation_id != activation_id
            || queue.admission_epoch != candidate.admission_epoch
        {
            return Err(PluginScheduleError::GenerationMismatch);
        }
        if queue.rebuilding {
            let replaced_submission_token = queue
                .waiting
                .replace(candidate)
                .map(|candidate| candidate.owner.submission_token);
            let expired = queue
                .running
                .as_ref()
                .map(|running| running.request.context.clone())
                .ok_or(PluginScheduleError::Unavailable)?;
            return Ok(PluginScheduleOutcome::Waiting {
                expired,
                replaced_submission_token,
            });
        }
        if let Some(running) = queue.running.as_mut() {
            running.current = false;
            let expired = running.request.context.clone();
            let replaced_submission_token = queue
                .waiting
                .replace(candidate)
                .map(|candidate| candidate.owner.submission_token);
            return Ok(PluginScheduleOutcome::Waiting {
                expired,
                replaced_submission_token,
            });
        }
        let request = dispatch(&mut state, candidate, now)?;
        Ok(PluginScheduleOutcome::Dispatched(request))
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
        let accepted = running.current;
        let next = if queue.rebuilding {
            None
        } else if let Some(candidate) = queue.waiting.take() {
            Some(dispatch(&mut state, candidate, now)?)
        } else {
            None
        };
        Ok(PluginCompletionOutcome { accepted, next })
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
                && running.request.context == *context
                && queue.generation == context.plugin_generation
        }) {
            let running = queue
                .running
                .as_mut()
                .ok_or(PluginContextAccessError::Unavailable)?;
            Ok(operation(&mut PluginCurrentRequest {
                activation_id: running.request.candidate.activation_id,
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
            if let Some(waiting) = queue.waiting.as_mut() {
                waiting.plugin_generation = new_generation;
            }
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
                counts_as_fault: running.current,
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
            waiting.activation_id = activation_id;
            waiting.admission_epoch = admission_epoch;
        }
        queue.rebuilding = false;
        let candidate = queue.waiting.take();
        candidate
            .map(|candidate| dispatch(&mut state, candidate, now))
            .transpose()
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
        let expired = queue.running.take().map(|running| running.request.context);
        queue.waiting = None;
        queue.rebuilding = false;
        if let Some((generation, activation_id, admission_epoch)) = next_activation {
            if generation == 0
                || activation_id == 0
                || generation <= queue.generation
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
        Ok(expired)
    }

    pub(crate) fn forget_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginRequestContext>, PluginScheduleError> {
        let mut state = self.lock()?;
        Ok(state
            .by_plugin
            .remove(plugin_id)
            .and_then(|queue| queue.running.map(|running| running.request.context)))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, SchedulerState>, PluginScheduleError> {
        self.state
            .lock()
            .map_err(|_| PluginScheduleError::Unavailable)
    }
}

fn dispatch(
    state: &mut SchedulerState,
    candidate: PluginRequestCandidate,
    now: Instant,
) -> Result<ScheduledPluginRequest, PluginScheduleError> {
    let request_number = state.next_request;
    state.next_request = state
        .next_request
        .checked_add(1)
        .ok_or(PluginScheduleError::RequestExhausted)?;
    let timeout = match candidate.activation_mode {
        PublicActivationMode::Live => LIVE_TIMEOUT,
        PublicActivationMode::Submit => SUBMIT_TIMEOUT,
    };
    let request = ScheduledPluginRequest {
        context: PluginRequestContext {
            plugin_id: candidate.plugin_id.clone(),
            plugin_generation: candidate.plugin_generation,
            request_id: format!("public-request-{request_number:016x}"),
        },
        candidate,
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
    queue.remember(request.context.clone());
    queue.running = Some(RunningRequest {
        request: request.clone(),
        current: true,
        notification_published: false,
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

    fn candidate(
        value: &str,
        generation: u64,
        mode: PublicActivationMode,
    ) -> PluginRequestCandidate {
        PluginRequestCandidate {
            plugin_id: "com.example.scheduler".into(),
            plugin_generation: generation,
            activation_id: 17,
            admission_epoch: 23,
            activation_mode: mode,
            input: value.into(),
            owner: owner(value),
        }
    }

    #[test]
    fn running_a_expires_while_waiting_b_is_replaced_by_c() {
        let scheduler = PluginRequestScheduler::default();
        let start = Instant::now();
        let a = match scheduler
            .enqueue(candidate("A", 1, PublicActivationMode::Live), start)
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
                    candidate("B", 1, PublicActivationMode::Submit),
                    start + Duration::from_secs(1)
                )
                .unwrap(),
            PluginScheduleOutcome::Waiting {
                expired: a.context.clone(),
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
                    candidate("C", 1, PublicActivationMode::Submit),
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
            .enqueue(candidate("A", 7, PublicActivationMode::Live), start)
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("A must dispatch"),
        };
        scheduler
            .enqueue(
                candidate("B", 7, PublicActivationMode::Live),
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
            plugin_id: plugin_id.into(),
            plugin_generation: generation,
            activation_id: generation + 10,
            admission_epoch: generation + 20,
            activation_mode: PublicActivationMode::Live,
            input: value.into(),
            owner: owner(value),
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
}
