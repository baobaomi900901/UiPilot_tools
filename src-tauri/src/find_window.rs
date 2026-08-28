use std::{
    mem,
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::result_registry::{
    ExecutionTicket, PreparedApplicationQueryRetirement, ResultRegistries, ResultRegistry,
    WindowScope,
};

pub(crate) const PREPARATION_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const QUEUED_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerError {
    CounterExhausted,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FindReadyStatus {
    Prepared,
    Ready,
    Superseded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedInitialization {
    pub(crate) token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReadyCommit {
    pub(crate) outcome: FindReadyStatus,
    pub(crate) snapshot_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindForwardPayload {
    pub(crate) invocation_id: String,
    pub(crate) forward_sequence: String,
    pub(crate) query: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenFindCompletion {
    Forwarded,
    Superseded,
    Unavailable,
}

#[derive(Debug)]
pub(crate) struct OpenSubmission {
    pub(crate) completion: Receiver<OpenFindCompletion>,
    pub(crate) snapshot_required: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowLabel {
    Main,
    Find,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ForegroundWindow {
    Main,
    Find,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeFocusSnapshot {
    pub(crate) main_focused: bool,
    pub(crate) find_focused: bool,
    pub(crate) foreground: ForegroundWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeTransferPlan {
    pub(crate) transfer_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusEffect {
    None,
    RecheckNativeSnapshot(u64),
    ExpectedHideConsumed,
    HideFind,
    RestoreMainTopmost,
    ClearAndHideMain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferFocusResult {
    AwaitingEvidence,
    CommitFindScope,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ForwardFinish {
    Visible { snapshot_required: bool },
    Hidden { snapshot_required: bool },
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionHideAdmission {
    Started,
    Stale,
    Pinned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HideFinish {
    Hidden { snapshot_required: bool },
    Visible { snapshot_required: bool },
    Stale,
}

struct OpenTransaction {
    payload: FindForwardPayload,
    transfer_id: u64,
    retirement: PreparedApplicationQueryRetirement,
    deadline: Instant,
    waiter: Sender<OpenFindCompletion>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferPhase {
    AwaitingFocus,
    CommitScope,
    Emitting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TransferOrigin {
    Hidden,
    Visible { invocation_id: String },
}

struct TransferState {
    transaction: OpenTransaction,
    phase: TransferPhase,
    origin: TransferOrigin,
    deadline: Instant,
}

enum AdmissionState {
    NotReady,
    PreparedNotReady {
        token: String,
        deadline: Instant,
    },
    Hidden,
    Transferring(TransferState),
    VisibleReady {
        invocation_id: String,
    },
    HidingForExecution {
        ticket: ExecutionTicket,
        invocation_id: String,
        expected_focus_loss: bool,
    },
}

#[derive(Default)]
struct Counters {
    initialization: u64,
    invocation: u64,
    forward: u64,
    transfer: u64,
}

impl Counters {
    fn next_initialization(&mut self) -> Result<String, ControllerError> {
        self.initialization = self
            .initialization
            .checked_add(1)
            .ok_or(ControllerError::CounterExhausted)?;
        Ok(format!("find-initialization-{}", self.initialization))
    }

    fn reserve_open(&mut self) -> Result<(String, u64, u64), ControllerError> {
        let invocation = self
            .invocation
            .checked_add(1)
            .ok_or(ControllerError::CounterExhausted)?;
        let forward = self
            .forward
            .checked_add(1)
            .ok_or(ControllerError::CounterExhausted)?;
        let transfer = self
            .transfer
            .checked_add(1)
            .ok_or(ControllerError::CounterExhausted)?;
        self.invocation = invocation;
        self.forward = forward;
        self.transfer = transfer;
        Ok((format!("find-invocation-{invocation}"), forward, transfer))
    }
}

struct ControllerCore {
    state: AdmissionState,
    committed_token: Option<String>,
    queued: Option<OpenTransaction>,
    confirmed_main_focus: Option<bool>,
    confirmed_find_focus: Option<bool>,
    pinned: bool,
    native_move_active: bool,
    shutdown: bool,
    counters: Counters,
}

impl Default for ControllerCore {
    fn default() -> Self {
        Self {
            state: AdmissionState::NotReady,
            committed_token: None,
            queued: None,
            confirmed_main_focus: None,
            confirmed_find_focus: None,
            pinned: false,
            native_move_active: false,
            shutdown: false,
            counters: Counters::default(),
        }
    }
}

#[derive(Default)]
pub(crate) struct FindWindowController {
    core: Mutex<ControllerCore>,
}

impl FindWindowController {
    pub(crate) fn prepare_initialization(
        &self,
        now: Instant,
    ) -> Result<PreparedInitialization, ControllerError> {
        let mut core = self.lock();
        Self::expire_locked(&mut core, now);
        if core.shutdown {
            return Err(ControllerError::Unavailable);
        }
        if !matches!(
            core.state,
            AdmissionState::NotReady | AdmissionState::PreparedNotReady { .. }
        ) {
            return Err(ControllerError::Unavailable);
        }
        let token = core.counters.next_initialization()?;
        let deadline = now
            .checked_add(PREPARATION_TIMEOUT)
            .ok_or(ControllerError::CounterExhausted)?;
        core.committed_token = None;
        core.state = AdmissionState::PreparedNotReady {
            token: token.clone(),
            deadline,
        };
        Ok(PreparedInitialization { token })
    }

    pub(crate) fn ready_status(&self, token: &str, now: Instant) -> FindReadyStatus {
        let mut core = self.lock();
        Self::expire_locked(&mut core, now);
        if core.committed_token.as_deref() == Some(token) {
            return FindReadyStatus::Ready;
        }
        match &core.state {
            AdmissionState::PreparedNotReady { token: current, .. } if current == token => {
                FindReadyStatus::Prepared
            }
            _ => FindReadyStatus::Superseded,
        }
    }

    pub(crate) fn commit_ready(&self, token: &str, now: Instant) -> ReadyCommit {
        let mut core = self.lock();
        Self::expire_locked(&mut core, now);
        if core.committed_token.as_deref() == Some(token) {
            return ReadyCommit {
                outcome: FindReadyStatus::Ready,
                snapshot_required: core.queued.is_some()
                    && matches!(core.state, AdmissionState::Hidden),
            };
        }
        let current = matches!(
            &core.state,
            AdmissionState::PreparedNotReady { token: current, .. } if current == token
        );
        if !current {
            return ReadyCommit {
                outcome: FindReadyStatus::Superseded,
                snapshot_required: false,
            };
        }
        core.committed_token = Some(token.to_owned());
        core.state = AdmissionState::Hidden;
        ReadyCommit {
            outcome: FindReadyStatus::Ready,
            snapshot_required: core.queued.is_some(),
        }
    }

    pub(crate) fn submit_open(
        &self,
        query: String,
        retirement: PreparedApplicationQueryRetirement,
        now: Instant,
    ) -> Result<OpenSubmission, ControllerError> {
        let mut core = self.lock();
        Self::expire_locked(&mut core, now);
        if core.shutdown {
            return Err(ControllerError::Unavailable);
        }
        let (invocation_id, forward_sequence, transfer_id) = core.counters.reserve_open()?;
        let deadline = now
            .checked_add(QUEUED_OPEN_TIMEOUT)
            .ok_or(ControllerError::CounterExhausted)?;
        let (waiter, completion) = mpsc::channel();
        let transaction = OpenTransaction {
            payload: FindForwardPayload {
                invocation_id,
                forward_sequence: forward_sequence.to_string(),
                query,
            },
            transfer_id,
            retirement,
            deadline,
            waiter,
        };
        if let Some(replaced) = core.queued.replace(transaction) {
            Self::complete(replaced, OpenFindCompletion::Superseded);
        }
        let snapshot_required = matches!(
            core.state,
            AdmissionState::Hidden | AdmissionState::VisibleReady { .. }
        );
        Ok(OpenSubmission {
            completion,
            snapshot_required,
        })
    }

    pub(crate) fn admit_queued_transfer(
        &self,
        snapshot: NativeFocusSnapshot,
        now: Instant,
    ) -> Option<NativeTransferPlan> {
        let mut core = self.lock();
        Self::expire_locked(&mut core, now);
        let pinned = core.pinned;
        let origin = match &core.state {
            AdmissionState::Hidden
                if snapshot.main_focused
                    && !snapshot.find_focused
                    && snapshot.foreground == ForegroundWindow::Main =>
            {
                TransferOrigin::Hidden
            }
            AdmissionState::VisibleReady { invocation_id }
                if (!snapshot.main_focused
                    && snapshot.find_focused
                    && snapshot.foreground == ForegroundWindow::Find)
                    || (pinned
                        && snapshot.main_focused
                        && !snapshot.find_focused
                        && snapshot.foreground == ForegroundWindow::Main) =>
            {
                TransferOrigin::Visible {
                    invocation_id: invocation_id.clone(),
                }
            }
            _ => return None,
        };
        let transaction = core.queued.take()?;
        let transfer_id = transaction.transfer_id;
        let deadline = now.checked_add(TRANSFER_TIMEOUT)?;
        core.confirmed_main_focus = Some(snapshot.main_focused);
        core.confirmed_find_focus = Some(snapshot.find_focused);
        core.state = AdmissionState::Transferring(TransferState {
            transaction,
            phase: TransferPhase::AwaitingFocus,
            origin,
            deadline,
        });
        Some(NativeTransferPlan { transfer_id })
    }

    pub(crate) fn observe_focus(&self, label: WindowLabel, focused: bool) -> FocusEffect {
        let mut core = self.lock();
        let confirmed = match label {
            WindowLabel::Main => &mut core.confirmed_main_focus,
            WindowLabel::Find => &mut core.confirmed_find_focus,
        };
        if *confirmed == Some(focused) {
            return FocusEffect::None;
        }
        *confirmed = Some(focused);

        if let AdmissionState::HidingForExecution {
            expected_focus_loss,
            ..
        } = &mut core.state
        {
            if label == WindowLabel::Find && !focused && *expected_focus_loss {
                *expected_focus_loss = false;
                return FocusEffect::ExpectedHideConsumed;
            }
        }
        if let AdmissionState::Transferring(transfer) = &core.state {
            return FocusEffect::RecheckNativeSnapshot(transfer.transaction.transfer_id);
        }
        if label == WindowLabel::Find && !focused && core.native_move_active {
            return FocusEffect::None;
        }
        match (&core.state, label, focused) {
            (AdmissionState::VisibleReady { .. }, WindowLabel::Find, false) if core.pinned => {
                FocusEffect::None
            }
            (AdmissionState::VisibleReady { .. }, WindowLabel::Find, false) => {
                FocusEffect::HideFind
            }
            (_, WindowLabel::Main, true) => FocusEffect::RestoreMainTopmost,
            (_, WindowLabel::Main, false) => FocusEffect::ClearAndHideMain,
            _ => FocusEffect::None,
        }
    }

    pub(crate) fn begin_native_move(&self) {
        self.lock().native_move_active = true;
    }

    pub(crate) fn finish_native_move(&self, focused: bool) -> FocusEffect {
        let mut core = self.lock();
        core.native_move_active = false;
        core.confirmed_find_focus = Some(focused);
        match (&core.state, focused, core.pinned) {
            (AdmissionState::VisibleReady { .. }, false, false) => FocusEffect::HideFind,
            _ => FocusEffect::None,
        }
    }

    pub(crate) fn confirm_transfer_focus(
        &self,
        transfer_id: u64,
        snapshot: NativeFocusSnapshot,
    ) -> TransferFocusResult {
        let mut core = self.lock();
        let current = matches!(
            &core.state,
            AdmissionState::Transferring(transfer)
                if transfer.transaction.transfer_id == transfer_id
                    && transfer.phase == TransferPhase::AwaitingFocus
        );
        if !current {
            return TransferFocusResult::Stale;
        }
        core.confirmed_main_focus = Some(snapshot.main_focused);
        core.confirmed_find_focus = Some(snapshot.find_focused);
        if snapshot.main_focused
            || !snapshot.find_focused
            || snapshot.foreground != ForegroundWindow::Find
        {
            return TransferFocusResult::AwaitingEvidence;
        }
        let AdmissionState::Transferring(transfer) = &mut core.state else {
            unreachable!("validated transfer state changed while locked")
        };
        transfer.phase = TransferPhase::CommitScope;
        TransferFocusResult::CommitFindScope
    }

    pub(crate) fn commit_find_scope(
        &self,
        transfer_id: u64,
        registries: &ResultRegistries,
    ) -> Result<FindForwardPayload, ControllerError> {
        let mut core = self.lock();
        let AdmissionState::Transferring(transfer) = &mut core.state else {
            return Err(ControllerError::Unavailable);
        };
        if transfer.transaction.transfer_id != transfer_id
            || transfer.phase != TransferPhase::CommitScope
        {
            return Err(ControllerError::Unavailable);
        }
        if registries
            .find()
            .try_on_show(transfer.transaction.payload.invocation_id.clone())
            .is_err()
        {
            let state = mem::replace(&mut core.state, AdmissionState::Hidden);
            if let AdmissionState::Transferring(transfer) = state {
                core.state = Self::restore_origin(&transfer.origin);
                Self::complete(transfer.transaction, OpenFindCompletion::Unavailable);
            }
            return Err(ControllerError::CounterExhausted);
        }
        transfer.phase = TransferPhase::Emitting;
        Ok(transfer.transaction.payload.clone())
    }

    pub(crate) fn finish_forward_emit(
        &self,
        transfer_id: u64,
        emit_succeeded: bool,
        registries: &ResultRegistries,
    ) -> ForwardFinish {
        let mut core = self.lock();
        let state = mem::replace(&mut core.state, AdmissionState::Hidden);
        let AdmissionState::Transferring(transfer) = state else {
            core.state = state;
            return ForwardFinish::Stale;
        };
        if transfer.transaction.transfer_id != transfer_id
            || transfer.phase != TransferPhase::Emitting
        {
            core.state = AdmissionState::Transferring(transfer);
            return ForwardFinish::Stale;
        }
        if emit_succeeded {
            let invocation_id = transfer.transaction.payload.invocation_id.clone();
            let _ = registries
                .main()
                .retire_application_query_if_current(transfer.transaction.retirement.clone());
            Self::complete(transfer.transaction, OpenFindCompletion::Forwarded);
            core.state = AdmissionState::VisibleReady { invocation_id };
            ForwardFinish::Visible {
                snapshot_required: core.queued.is_some(),
            }
        } else {
            registries.find().hide_and_clear();
            Self::complete(transfer.transaction, OpenFindCompletion::Unavailable);
            core.state = AdmissionState::Hidden;
            ForwardFinish::Hidden {
                snapshot_required: core.queued.is_some(),
            }
        }
    }

    pub(crate) fn admit_search(&self, invocation_id: &str) -> bool {
        let core = self.lock();
        core.queued.is_none()
            && matches!(
                &core.state,
                AdmissionState::VisibleReady { invocation_id: active }
                    if active == invocation_id
            )
    }

    pub(crate) fn with_visible_admission<T, F>(
        &self,
        invocation_id: &str,
        operation: F,
    ) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        let core = self.lock();
        if core.queued.is_some()
            || !matches!(&core.state, AdmissionState::VisibleReady { invocation_id: active } if active == invocation_id)
        {
            return None;
        }
        Some(operation())
    }

    pub(crate) fn set_pin(&self, invocation_id: &str, pinned: bool) -> bool {
        let mut core = self.lock();
        if core.queued.is_some()
            || !matches!(
                &core.state,
                AdmissionState::VisibleReady { invocation_id: active }
                    if active == invocation_id
            )
        {
            return false;
        }
        core.pinned = pinned;
        true
    }

    pub(crate) fn request_explicit_hide(&self, invocation_id: &str, force: bool) -> bool {
        let core = self.lock();
        matches!(
            &core.state,
            AdmissionState::VisibleReady { invocation_id: active }
                if active == invocation_id && core.queued.is_none() && (force || !core.pinned)
        )
    }

    pub(crate) fn finish_explicit_hide(
        &self,
        invocation_id: &str,
        hide_succeeded: bool,
        registries: &ResultRegistries,
    ) -> bool {
        let mut core = self.lock();
        if !matches!(
            &core.state,
            AdmissionState::VisibleReady { invocation_id: active } if active == invocation_id
        ) {
            return false;
        }
        if hide_succeeded {
            registries.find().hide_and_clear();
            core.pinned = false;
            core.state = AdmissionState::Hidden;
        }
        true
    }

    pub(crate) fn pinned(&self) -> bool {
        self.lock().pinned
    }

    pub(crate) fn current_invocation(&self) -> Option<String> {
        let core = self.lock();
        match &core.state {
            AdmissionState::VisibleReady { invocation_id } => Some(invocation_id.clone()),
            AdmissionState::HidingForExecution { invocation_id, .. } => Some(invocation_id.clone()),
            _ => None,
        }
    }

    pub(crate) fn begin_execution_hide(
        &self,
        ticket: &ExecutionTicket,
        find_registry: &ResultRegistry,
    ) -> ExecutionHideAdmission {
        let mut core = self.lock();
        let invocation_id = match &core.state {
            AdmissionState::VisibleReady { invocation_id } if core.queued.is_none() => {
                invocation_id.clone()
            }
            _ => return ExecutionHideAdmission::Stale,
        };
        if ticket.scope() != WindowScope::Find || !find_registry.is_execution_ticket_current(ticket)
        {
            return ExecutionHideAdmission::Stale;
        }
        if core.pinned {
            return ExecutionHideAdmission::Pinned;
        }
        if !find_registry.retire_result_set_if_current(ticket) {
            return ExecutionHideAdmission::Stale;
        }
        core.state = AdmissionState::HidingForExecution {
            ticket: ticket.clone(),
            invocation_id,
            expected_focus_loss: true,
        };
        ExecutionHideAdmission::Started
    }

    pub(crate) fn finish_execution_hide(
        &self,
        hide_succeeded: bool,
        registries: &ResultRegistries,
    ) -> HideFinish {
        let mut core = self.lock();
        let state = mem::replace(&mut core.state, AdmissionState::Hidden);
        let AdmissionState::HidingForExecution {
            ticket,
            invocation_id,
            ..
        } = state
        else {
            core.state = state;
            return HideFinish::Stale;
        };
        let _ = ticket;
        if hide_succeeded {
            registries.find().hide_and_clear();
            core.state = AdmissionState::Hidden;
            HideFinish::Hidden {
                snapshot_required: core.queued.is_some(),
            }
        } else {
            core.state = AdmissionState::VisibleReady { invocation_id };
            HideFinish::Visible {
                snapshot_required: core.queued.is_some(),
            }
        }
    }

    pub(crate) fn expire(&self, now: Instant) {
        Self::expire_locked(&mut self.lock(), now);
    }

    pub(crate) fn fail_transfer_before_ownership(&self, transfer_id: u64) -> bool {
        let mut core = self.lock();
        let state = mem::replace(&mut core.state, AdmissionState::Hidden);
        let AdmissionState::Transferring(transfer) = state else {
            core.state = state;
            return false;
        };
        if transfer.transaction.transfer_id != transfer_id
            || transfer.phase == TransferPhase::Emitting
        {
            core.state = AdmissionState::Transferring(transfer);
            return false;
        }
        core.state = Self::restore_origin(&transfer.origin);
        Self::complete(transfer.transaction, OpenFindCompletion::Unavailable);
        true
    }

    pub(crate) fn shutdown(&self) {
        let mut core = self.lock();
        if core.shutdown {
            return;
        }
        core.shutdown = true;
        core.committed_token = None;
        if let Some(queued) = core.queued.take() {
            Self::complete(queued, OpenFindCompletion::Unavailable);
        }
        let state = mem::replace(&mut core.state, AdmissionState::NotReady);
        if let AdmissionState::Transferring(transfer) = state {
            Self::complete(transfer.transaction, OpenFindCompletion::Unavailable);
        }
    }

    pub(crate) fn queued_query(&self) -> Option<String> {
        self.lock()
            .queued
            .as_ref()
            .map(|queued| queued.payload.query.clone())
    }

    #[cfg(test)]
    fn exhaust_counters_for_test(&self) {
        let mut core = self.lock();
        core.counters.invocation = u64::MAX;
        core.counters.forward = u64::MAX;
        core.counters.transfer = u64::MAX;
    }

    #[cfg(test)]
    pub(crate) fn set_visible_for_test(&self, invocation_id: &str) {
        let mut core = self.lock();
        core.committed_token = Some("test-ready".into());
        core.state = AdmissionState::VisibleReady {
            invocation_id: invocation_id.into(),
        };
    }

    fn expire_locked(core: &mut ControllerCore, now: Instant) {
        if matches!(
            &core.state,
            AdmissionState::PreparedNotReady { deadline, .. } if *deadline <= now
        ) {
            core.state = AdmissionState::NotReady;
            core.committed_token = None;
        }
        if core
            .queued
            .as_ref()
            .is_some_and(|queued| queued.deadline <= now)
        {
            if let Some(expired) = core.queued.take() {
                Self::complete(expired, OpenFindCompletion::Unavailable);
            }
        }
        let transfer_expired = matches!(
            &core.state,
            AdmissionState::Transferring(transfer) if transfer.deadline <= now
        );
        if transfer_expired {
            let state = mem::replace(&mut core.state, AdmissionState::Hidden);
            if let AdmissionState::Transferring(transfer) = state {
                core.state = Self::restore_origin(&transfer.origin);
                Self::complete(transfer.transaction, OpenFindCompletion::Unavailable);
            }
        }
    }

    fn restore_origin(origin: &TransferOrigin) -> AdmissionState {
        match origin {
            TransferOrigin::Hidden => AdmissionState::Hidden,
            TransferOrigin::Visible { invocation_id } => AdmissionState::VisibleReady {
                invocation_id: invocation_id.clone(),
            },
        }
    }

    fn complete(transaction: OpenTransaction, outcome: OpenFindCompletion) {
        let _ = transaction.waiter.send(outcome);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ControllerCore> {
        self.core.lock().expect("find controller lock poisoned")
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{
        file_index::{IndexedKind, OpenIndexedPath, VolumeIdentity},
        file_search::FileExecutionAction,
        result_registry::{ExecutionTicket, QueryDomain, ResultAction, ResultRegistries},
    };

    fn snapshot(main: bool, find: bool, foreground: ForegroundWindow) -> NativeFocusSnapshot {
        NativeFocusSnapshot {
            main_focused: main,
            find_focused: find,
            foreground,
        }
    }

    fn prepared_open(
        controller: &FindWindowController,
        registries: &ResultRegistries,
        query_sequence: u64,
        query: &str,
        now: Instant,
    ) -> OpenSubmission {
        let main = registries.main();
        main.on_show("main-invocation".into());
        let token = main
            .begin_query(QueryDomain::Application, "main-invocation", query_sequence)
            .unwrap();
        main.publish_if_latest(
            token,
            vec![(
                (),
                ResultAction::CopyText {
                    plugin_id: "test".into(),
                    generation: 1,
                    text: "value".into(),
                },
            )],
            || true,
            |_, _| (),
        )
        .unwrap();
        let lease = main
            .prepare_application_query_retirement("main-invocation", query_sequence)
            .unwrap()
            .unwrap();
        controller.submit_open(query.into(), lease, now).unwrap()
    }

    fn current_find_ticket(registries: &ResultRegistries, invocation_id: &str) -> ExecutionTicket {
        let find = registries.find();
        find.on_show(invocation_id.into());
        let token = find
            .begin_query(QueryDomain::File, invocation_id, 1)
            .unwrap();
        let action =
            ResultAction::OpenFile(FileExecutionAction::Indexed(OpenIndexedPath::for_test(
                0,
                1,
                VolumeIdentity::for_test(r"\\?\Volume{FIND}\", 1, "ntfs"),
                "file.txt",
                IndexedKind::File,
            )));
        let (request_id, result_id) = find
            .publish_if_latest(
                token,
                vec![((), action)],
                || true,
                |request_id, items| (request_id, items[0].0.clone()),
            )
            .unwrap();
        find.resolve_with_ticket(&request_id, &result_id).unwrap().1
    }

    fn make_ready(
        controller: &FindWindowController,
        registries: &ResultRegistries,
        now: Instant,
    ) -> String {
        let token = controller.prepare_initialization(now).unwrap().token;
        assert_eq!(
            controller.commit_ready(&token, now),
            ReadyCommit {
                outcome: FindReadyStatus::Ready,
                snapshot_required: false,
            }
        );
        let submission = prepared_open(controller, registries, 1, "alpha", now);
        assert!(submission.snapshot_required);
        let plan = controller
            .admit_queued_transfer(snapshot(true, false, ForegroundWindow::Main), now)
            .unwrap();
        assert_eq!(
            controller.observe_focus(WindowLabel::Main, false),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, true),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
        assert_eq!(
            controller.confirm_transfer_focus(
                plan.transfer_id,
                snapshot(false, true, ForegroundWindow::Find),
            ),
            TransferFocusResult::CommitFindScope
        );
        let payload = controller
            .commit_find_scope(plan.transfer_id, registries)
            .unwrap();
        let invocation_id = payload.invocation_id.clone();
        assert_eq!(
            controller.finish_forward_emit(plan.transfer_id, true, registries),
            ForwardFinish::Visible {
                snapshot_required: false,
            }
        );
        assert_eq!(
            submission.completion.recv().unwrap(),
            OpenFindCompletion::Forwarded
        );
        invocation_id
    }

    #[test]
    fn explicit_close_resets_pin_only_after_hide_succeeds() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let invocation_id = make_ready(&controller, &registries, now);

        assert!(controller.set_pin(&invocation_id, true));
        assert!(!controller.request_explicit_hide(&invocation_id, false));
        assert!(controller.request_explicit_hide(&invocation_id, true));
        assert!(controller.finish_explicit_hide(&invocation_id, false, &registries));
        assert!(controller.pinned());

        assert!(controller.request_explicit_hide(&invocation_id, true));
        assert!(controller.finish_explicit_hide(&invocation_id, true, &registries));
        assert!(!controller.pinned());
    }

    #[test]
    fn readiness_transition_table_is_listener_first_and_idempotent() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let first = controller.prepare_initialization(now).unwrap();
        let second = controller.prepare_initialization(now).unwrap();

        assert_ne!(first.token, second.token);
        assert_eq!(
            controller.ready_status(&first.token, now),
            FindReadyStatus::Superseded
        );
        assert_eq!(
            controller.ready_status(&second.token, now),
            FindReadyStatus::Prepared
        );
        assert_eq!(
            controller.commit_ready(&first.token, now).outcome,
            FindReadyStatus::Superseded
        );
        assert_eq!(
            controller.commit_ready(&second.token, now).outcome,
            FindReadyStatus::Ready
        );
        assert_eq!(
            controller.commit_ready(&second.token, now).outcome,
            FindReadyStatus::Ready
        );
        assert_eq!(controller.current_invocation(), None);
    }

    #[test]
    fn prepare_without_commit_does_not_emit_or_wake_the_queue() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let queued = prepared_open(&controller, &registries, 1, "queued", now);
        let prepared = controller.prepare_initialization(now).unwrap();

        assert!(!queued.snapshot_required);
        assert!(queued.completion.try_recv().is_err());
        assert!(
            controller
                .commit_ready(&prepared.token, now)
                .snapshot_required
        );
        assert!(queued.completion.try_recv().is_err());
    }

    #[test]
    fn queue_keeps_latest_c_and_completes_replaced_b_once() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let b = prepared_open(&controller, &registries, 1, "b", now);
        let c = prepared_open(&controller, &registries, 2, "c", now);

        assert_eq!(b.completion.recv().unwrap(), OpenFindCompletion::Superseded);
        assert!(b.completion.try_recv().is_err());
        assert!(c.completion.try_recv().is_err());
        assert_eq!(controller.queued_query(), Some("c".into()));
    }

    #[test]
    fn preparation_queue_and_transfer_expiry_are_independent() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let prepared = controller.prepare_initialization(now).unwrap();
        let queued = prepared_open(
            &controller,
            &registries,
            1,
            "queued",
            now + Duration::from_secs(1),
        );

        controller.expire(now + PREPARATION_TIMEOUT);
        assert_eq!(
            controller.ready_status(&prepared.token, now),
            FindReadyStatus::Superseded
        );
        assert!(queued.completion.try_recv().is_err());
        controller.expire(now + Duration::from_secs(1) + QUEUED_OPEN_TIMEOUT);
        assert_eq!(
            queued.completion.recv().unwrap(),
            OpenFindCompletion::Unavailable
        );
    }

    #[test]
    fn shutdown_terminates_queue_once_and_drops_readiness() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let queued = prepared_open(&controller, &registries, 1, "queued", now);
        let prepared = controller.prepare_initialization(now).unwrap();

        controller.shutdown();
        controller.shutdown();
        assert_eq!(
            queued.completion.recv().unwrap(),
            OpenFindCompletion::Unavailable
        );
        assert!(queued.completion.try_recv().is_err());
        assert_eq!(
            controller.ready_status(&prepared.token, now),
            FindReadyStatus::Superseded
        );
        assert_eq!(
            controller.prepare_initialization(now),
            Err(ControllerError::Unavailable)
        );
    }

    #[test]
    fn duplicate_and_contradictory_focus_events_need_confirming_snapshot() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let token = controller.prepare_initialization(now).unwrap().token;
        controller.commit_ready(&token, now);
        let open = prepared_open(&controller, &registries, 1, "focus", now);
        let plan = controller
            .admit_queued_transfer(snapshot(true, false, ForegroundWindow::Main), now)
            .unwrap();

        assert_eq!(
            controller.observe_focus(WindowLabel::Main, true),
            FocusEffect::None
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Main, false),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Main, false),
            FocusEffect::None
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, true),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
        assert_eq!(
            controller.confirm_transfer_focus(
                plan.transfer_id,
                snapshot(false, false, ForegroundWindow::Main),
            ),
            TransferFocusResult::AwaitingEvidence
        );
        assert!(open.completion.try_recv().is_err());
    }

    #[test]
    fn transfer_timeout_is_two_seconds_and_fails_waiter_closed() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let token = controller.prepare_initialization(now).unwrap().token;
        controller.commit_ready(&token, now);
        let open = prepared_open(&controller, &registries, 1, "timeout", now);
        controller
            .admit_queued_transfer(snapshot(true, false, ForegroundWindow::Main), now)
            .unwrap();

        controller.expire(now + TRANSFER_TIMEOUT);
        assert_eq!(
            open.completion.recv().unwrap(),
            OpenFindCompletion::Unavailable
        );
        assert_eq!(controller.current_invocation(), None);
    }

    #[test]
    fn stale_snapshot_does_not_change_current_transfer_edges() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let token = controller.prepare_initialization(now).unwrap().token;
        controller.commit_ready(&token, now);
        let _open = prepared_open(&controller, &registries, 1, "focus", now);
        let plan = controller
            .admit_queued_transfer(snapshot(true, false, ForegroundWindow::Main), now)
            .unwrap();
        assert_eq!(
            controller.confirm_transfer_focus(
                plan.transfer_id - 1,
                snapshot(false, true, ForegroundWindow::Other),
            ),
            TransferFocusResult::Stale
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Main, false),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
    }

    #[test]
    fn replacement_timeout_restores_prior_visible_invocation() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let invocation = make_ready(&controller, &registries, now);
        let replacement = prepared_open(&controller, &registries, 2, "replacement", now);
        controller
            .admit_queued_transfer(snapshot(false, true, ForegroundWindow::Find), now)
            .unwrap();
        controller.expire(now + TRANSFER_TIMEOUT);
        assert_eq!(
            replacement.completion.recv().unwrap(),
            OpenFindCompletion::Unavailable
        );
        assert!(controller.admit_search(&invocation));
    }

    #[test]
    fn pinned_visible_find_accepts_transfer_from_refocused_main() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let prior_invocation = make_ready(&controller, &registries, now);
        assert!(controller.set_pin(&prior_invocation, true));

        let replacement = prepared_open(&controller, &registries, 2, "replacement", now);
        assert!(replacement.snapshot_required);
        let plan = controller
            .admit_queued_transfer(snapshot(true, false, ForegroundWindow::Main), now)
            .expect("pinned find must accept focus back from the refocused main window");

        assert_eq!(
            controller.observe_focus(WindowLabel::Main, false),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, true),
            FocusEffect::RecheckNativeSnapshot(plan.transfer_id)
        );
        assert_eq!(
            controller.confirm_transfer_focus(
                plan.transfer_id,
                snapshot(false, true, ForegroundWindow::Find),
            ),
            TransferFocusResult::CommitFindScope
        );
        let payload = controller
            .commit_find_scope(plan.transfer_id, &registries)
            .unwrap();
        assert_ne!(payload.invocation_id, prior_invocation);
        assert_eq!(payload.query, "replacement");
        assert_eq!(
            controller.finish_forward_emit(plan.transfer_id, true, &registries),
            ForwardFinish::Visible {
                snapshot_required: false,
            }
        );
        assert_eq!(
            replacement.completion.recv().unwrap(),
            OpenFindCompletion::Forwarded
        );
        assert!(controller.pinned());
    }

    #[test]
    fn stale_and_pinned_execution_tickets_do_not_mutate_lifecycle() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let invocation = make_ready(&controller, &registries, now);
        let stale = current_find_ticket(&registries, &invocation);
        registries.find().hide_and_clear();
        assert_eq!(
            controller.begin_execution_hide(&stale, registries.find()),
            ExecutionHideAdmission::Stale
        );

        let current = current_find_ticket(&registries, &invocation);
        assert!(controller.set_pin(&invocation, true));
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, false),
            FocusEffect::None
        );
        assert_eq!(
            controller.begin_execution_hide(&current, registries.find()),
            ExecutionHideAdmission::Pinned
        );
        assert_eq!(controller.current_invocation(), Some(invocation.clone()));
    }

    #[test]
    fn execution_hide_closes_all_ordinary_admission() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let invocation = make_ready(&controller, &registries, now);
        let ticket = current_find_ticket(&registries, &invocation);
        assert_eq!(
            controller.begin_execution_hide(&ticket, registries.find()),
            ExecutionHideAdmission::Started
        );

        assert!(!controller.admit_search(&invocation));
        assert!(!controller.set_pin(&invocation, true));
        assert_eq!(
            controller.begin_execution_hide(&ticket, registries.find()),
            ExecutionHideAdmission::Stale
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, false),
            FocusEffect::ExpectedHideConsumed
        );
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, false),
            FocusEffect::None
        );
    }

    #[test]
    fn native_move_suppresses_focus_loss_until_drag_finishes() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        make_ready(&controller, &registries, now);

        controller.begin_native_move();
        assert_eq!(
            controller.observe_focus(WindowLabel::Find, false),
            FocusEffect::None
        );
        assert_eq!(controller.finish_native_move(false), FocusEffect::HideFind);
    }

    #[test]
    fn hide_success_clears_scope_before_queued_forward_starts() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let invocation = make_ready(&controller, &registries, now);
        let ticket = current_find_ticket(&registries, &invocation);
        assert_eq!(
            controller.begin_execution_hide(&ticket, registries.find()),
            ExecutionHideAdmission::Started
        );
        let queued = prepared_open(&controller, &registries, 2, "next", now);

        assert_eq!(
            controller.finish_execution_hide(true, &registries),
            HideFinish::Hidden {
                snapshot_required: true,
            }
        );
        assert!(!registries.find().is_execution_ticket_current(&ticket));
        assert_eq!(controller.current_invocation(), None);
        assert!(queued.completion.try_recv().is_err());
    }

    #[test]
    fn hide_failure_processes_queued_forward_before_reopening_admission() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        let invocation = make_ready(&controller, &registries, now);
        let ticket = current_find_ticket(&registries, &invocation);
        assert_eq!(
            controller.begin_execution_hide(&ticket, registries.find()),
            ExecutionHideAdmission::Started
        );
        let queued = prepared_open(&controller, &registries, 2, "next", now);

        assert_eq!(
            controller.finish_execution_hide(false, &registries),
            HideFinish::Visible {
                snapshot_required: true,
            }
        );
        assert_eq!(controller.current_invocation(), Some(invocation.clone()));
        assert!(!controller.admit_search(&invocation));
        assert!(queued.completion.try_recv().is_err());
    }

    #[test]
    fn checked_counters_fail_before_queue_or_native_work() {
        let now = Instant::now();
        let controller = FindWindowController::default();
        let registries = ResultRegistries::default();
        controller.exhaust_counters_for_test();
        let main = registries.main();
        main.on_show("main-invocation".into());
        let token = main
            .begin_query(QueryDomain::Application, "main-invocation", 1)
            .unwrap();
        main.publish_if_latest(
            token,
            vec![(
                (),
                ResultAction::CopyText {
                    plugin_id: "p".into(),
                    generation: 1,
                    text: "x".into(),
                },
            )],
            || true,
            |_, _| (),
        )
        .unwrap();
        let lease = main
            .prepare_application_query_retirement("main-invocation", 1)
            .unwrap()
            .unwrap();

        assert_eq!(
            controller.submit_open("x".into(), lease, now).unwrap_err(),
            ControllerError::CounterExhausted
        );
        assert_eq!(controller.queued_query(), None);
    }
}
