use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Instant,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    commands::clear_and_hide,
    result_registry::ResultRegistry,
    validation_data::{ValidationEvent, ValidationStore},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShowTarget {
    Launcher,
    Settings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum LifecycleNotice {
    SettingsFailed,
    ValidationFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherShown {
    invocation_id: String,
    target: ShowTarget,
    notice: Option<LifecycleNotice>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleError {
    MainThreadDispatchFailed,
    WindowFailed,
    InvocationExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShowOutcome {
    Shown,
    Ignored,
}

#[derive(Debug, Default)]
struct Readiness {
    setup_ready: bool,
    frontend_ready: bool,
    pending_target: Option<ShowTarget>,
}

impl Readiness {
    fn request(&mut self, target: ShowTarget) -> Option<ShowTarget> {
        if self.setup_ready && self.frontend_ready {
            Some(target)
        } else {
            self.pending_target = Some(target);
            None
        }
    }

    fn mark_setup_ready(&mut self) -> Option<ShowTarget> {
        self.setup_ready = true;
        self.take_if_ready()
    }

    fn mark_frontend_ready(&mut self) -> Option<ShowTarget> {
        self.frontend_ready = true;
        self.take_if_ready()
    }

    fn take_if_ready(&mut self) -> Option<ShowTarget> {
        if self.setup_ready && self.frontend_ready {
            self.pending_target.take()
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModalState {
    Normal,
    Open,
    AwaitingFocusRestore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusDecision {
    Suppress,
    ClearAndHide,
    ReportWindowFailureAndHide,
}

impl ModalState {
    fn begin_export<F>(&mut self, query_focus: F) -> Result<(), FocusDecision>
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        match self {
            Self::Normal => {
                *self = Self::Open;
                Ok(())
            }
            Self::Open => Err(FocusDecision::Suppress),
            Self::AwaitingFocusRestore => {
                *self = Self::Normal;
                match query_focus() {
                    Ok(true) => {
                        *self = Self::Open;
                        Ok(())
                    }
                    Ok(false) => Err(FocusDecision::ClearAndHide),
                    Err(()) => Err(FocusDecision::ReportWindowFailureAndHide),
                }
            }
        }
    }

    fn finish_export(&mut self) {
        if *self == Self::Open {
            *self = Self::AwaitingFocusRestore;
        }
    }

    fn claim_export(&mut self) -> Result<bool, FocusDecision> {
        match self {
            Self::Normal => {
                *self = Self::Open;
                Ok(false)
            }
            Self::Open => Err(FocusDecision::Suppress),
            Self::AwaitingFocusRestore => {
                *self = Self::Open;
                Ok(true)
            }
        }
    }

    fn resolve_export_focus(&mut self, focused: Result<bool, ()>) -> Result<(), FocusDecision> {
        match focused {
            Ok(true) => Ok(()),
            Ok(false) => {
                *self = Self::Normal;
                Err(FocusDecision::ClearAndHide)
            }
            Err(()) => {
                *self = Self::Normal;
                Err(FocusDecision::ReportWindowFailureAndHide)
            }
        }
    }

    fn on_focus<F>(&mut self, focused: bool, query_focus: F) -> FocusDecision
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        match (*self, focused) {
            (Self::Open, _) => FocusDecision::Suppress,
            (Self::AwaitingFocusRestore, true) => {
                *self = Self::Normal;
                FocusDecision::Suppress
            }
            (Self::AwaitingFocusRestore, false) => {
                *self = Self::Normal;
                match query_focus() {
                    Ok(true) => FocusDecision::Suppress,
                    Ok(false) => FocusDecision::ClearAndHide,
                    Err(()) => FocusDecision::ReportWindowFailureAndHide,
                }
            }
            (Self::Normal, true) => FocusDecision::Suppress,
            (Self::Normal, false) => FocusDecision::ClearAndHide,
        }
    }

    fn on_successful_show(&mut self) {
        if *self == Self::AwaitingFocusRestore {
            *self = Self::Normal;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitState {
    Running,
    Cleaning,
    Clean,
    SystemEnding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanOwner {
    Tray,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanResult {
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanAttempt {
    Idle,
    Waiting {
        owner: CleanOwner,
        deadline: Instant,
    },
    Calling {
        owner: CleanOwner,
        deadline: Instant,
    },
    Finished(CleanResult),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanDecision {
    Wait { deadline: Instant },
    CallMarker,
    Exit,
    ReturnRunning,
    ObserveOnly,
}

#[derive(Debug)]
struct ExitGate {
    state: ExitState,
    in_flight_critical: usize,
    clean_attempt: CleanAttempt,
}

impl Default for ExitGate {
    fn default() -> Self {
        Self {
            state: ExitState::Running,
            in_flight_critical: 0,
            clean_attempt: CleanAttempt::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReservationError {
    NotRunning,
    Overflow,
}

#[derive(Debug)]
pub(crate) struct LifecycleCoordinator {
    next_invocation: AtomicU64,
    readiness: Mutex<Readiness>,
    modal: Mutex<ModalState>,
    exit_gate: Mutex<ExitGate>,
    critical_changed: Condvar,
    pending_notice: Mutex<Option<LifecycleNotice>>,
}

impl Default for LifecycleCoordinator {
    fn default() -> Self {
        Self {
            next_invocation: AtomicU64::new(0),
            readiness: Mutex::new(Readiness::default()),
            modal: Mutex::new(ModalState::Normal),
            exit_gate: Mutex::new(ExitGate::default()),
            critical_changed: Condvar::new(),
            pending_notice: Mutex::new(None),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ModalGuard {
    coordinator: Arc<LifecycleCoordinator>,
}

impl Drop for ModalGuard {
    fn drop(&mut self) {
        self.coordinator
            .modal
            .lock()
            .expect("modal lock poisoned")
            .finish_export();
    }
}

struct ShowMainClosures<'a> {
    center: &'a mut dyn FnMut() -> Result<(), ()>,
    always_on_top: &'a mut dyn FnMut() -> Result<(), ()>,
    show: &'a mut dyn FnMut() -> Result<(), ()>,
    focus: &'a mut dyn FnMut() -> Result<(), ()>,
    registry_on_show: &'a mut dyn FnMut(String),
    emit: &'a mut dyn FnMut(&LauncherShown) -> Result<(), ()>,
    clear_and_hide: &'a mut dyn FnMut(),
    record_launcher: &'a mut dyn FnMut(CriticalReservation),
}

#[derive(Debug)]
struct CriticalReservation {
    coordinator: Arc<LifecycleCoordinator>,
    released: bool,
}

impl Drop for CriticalReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }

        {
            let mut gate = self
                .coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned");
            gate.in_flight_critical = gate
                .in_flight_critical
                .checked_sub(1)
                .expect("critical reservation count underflow");
        }
        self.released = true;
        self.coordinator.critical_changed.notify_all();
    }
}

impl LifecycleCoordinator {
    pub(crate) fn request_show(
        self: &Arc<Self>,
        app: &AppHandle,
        target: ShowTarget,
    ) -> Result<(), LifecycleError> {
        let dispatcher = app.clone();
        let app_for_show = app.clone();
        self.request_show_with(target, move |coordinator, target| {
            dispatcher
                .run_on_main_thread(move || {
                    coordinator.handle_request_main(&app_for_show, target);
                })
                .map_err(|_| ())
        })
    }

    pub(crate) fn mark_setup_ready(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), LifecycleError> {
        self.mark_setup_ready_with(|target| self.show_main(app, target))
    }

    pub(crate) fn mark_frontend_ready(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), LifecycleError> {
        self.mark_frontend_ready_with(|target| self.show_main(app, target))
    }

    fn request_show_with<D>(
        self: &Arc<Self>,
        target: ShowTarget,
        dispatch: D,
    ) -> Result<(), LifecycleError>
    where
        D: FnOnce(Arc<Self>, ShowTarget) -> Result<(), ()>,
    {
        dispatch(Arc::clone(self), target).map_err(|_| LifecycleError::MainThreadDispatchFailed)
    }

    fn handle_request_main(self: &Arc<Self>, app: &AppHandle, target: ShowTarget) {
        let target = self
            .readiness
            .lock()
            .expect("readiness lock poisoned")
            .request(target);
        if let Some(target) = target {
            let _ = self.show_main(app, target);
        }
    }

    fn mark_setup_ready_with<S>(&self, show: S) -> Result<(), LifecycleError>
    where
        S: FnOnce(ShowTarget) -> Result<ShowOutcome, LifecycleError>,
    {
        let target = self
            .readiness
            .lock()
            .expect("readiness lock poisoned")
            .mark_setup_ready();
        if let Some(target) = target {
            show(target)?;
        }
        Ok(())
    }

    fn mark_frontend_ready_with<S>(&self, show: S) -> Result<(), LifecycleError>
    where
        S: FnOnce(ShowTarget) -> Result<ShowOutcome, LifecycleError>,
    {
        let target = self
            .readiness
            .lock()
            .expect("readiness lock poisoned")
            .mark_frontend_ready();
        if let Some(target) = target {
            show(target)?;
        }
        Ok(())
    }

    pub(crate) fn begin_modal_export<F>(
        self: &Arc<Self>,
        query_focus: F,
    ) -> Result<ModalGuard, FocusDecision>
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        let query_required = self
            .modal
            .lock()
            .expect("modal lock poisoned")
            .claim_export()?;
        if query_required {
            let focused = query_focus();
            self.modal
                .lock()
                .expect("modal lock poisoned")
                .resolve_export_focus(focused)?;
        }
        Ok(ModalGuard {
            coordinator: Arc::clone(self),
        })
    }

    pub(crate) fn on_successful_show(&self) {
        self.modal
            .lock()
            .expect("modal lock poisoned")
            .on_successful_show();
    }

    fn show_main(
        self: &Arc<Self>,
        app: &AppHandle,
        target: ShowTarget,
    ) -> Result<ShowOutcome, LifecycleError> {
        let Some((window, registry)) = self.show_main_with_resolver(|| {
            let window = app
                .get_webview_window("main")
                .ok_or(LifecycleError::WindowFailed)?;
            let registry = app.state::<ResultRegistry>();
            Ok((window, registry))
        })?
        else {
            return Ok(ShowOutcome::Ignored);
        };

        let mut center = || window.center().map_err(|_| ());
        let mut always_on_top = || window.set_always_on_top(true).map_err(|_| ());
        let mut show = || window.show().map_err(|_| ());
        let mut focus = || window.set_focus().map_err(|_| ());
        let mut registry_on_show = |invocation_id| registry.on_show(invocation_id);
        let mut emit =
            |payload: &LauncherShown| window.emit("launcher://shown", payload).map_err(|_| ());
        let mut clear_window = || {
            let _ = clear_and_hide(&registry, &window);
        };
        let app_for_record = app.clone();
        let coordinator_for_record = Arc::clone(self);
        let mut record_launcher = move |reservation| {
            let app = app_for_record.clone();
            let coordinator = Arc::clone(&coordinator_for_record);
            drop(tauri::async_runtime::spawn_blocking(move || {
                let _reservation = reservation;
                let result = app
                    .state::<ValidationStore>()
                    .record(ValidationEvent::LauncherInvoked)
                    .map_err(|_| ());
                coordinator.finish_launcher_record(result);
            }));
        };
        let mut operations = ShowMainClosures {
            center: &mut center,
            always_on_top: &mut always_on_top,
            show: &mut show,
            focus: &mut focus,
            registry_on_show: &mut registry_on_show,
            emit: &mut emit,
            clear_and_hide: &mut clear_window,
            record_launcher: &mut record_launcher,
        };
        self.show_main_core(target, &mut operations)
    }

    fn show_main_with_resolver<T, R>(&self, resolve: R) -> Result<Option<T>, LifecycleError>
    where
        R: FnOnce() -> Result<T, LifecycleError>,
    {
        if self.observe_exit() != ExitState::Running {
            return Ok(None);
        }
        resolve().map(Some)
    }

    fn show_main_core(
        self: &Arc<Self>,
        target: ShowTarget,
        operations: &mut ShowMainClosures<'_>,
    ) -> Result<ShowOutcome, LifecycleError> {
        if self.observe_exit() != ExitState::Running {
            return Ok(ShowOutcome::Ignored);
        }

        let previous = self
            .next_invocation
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| LifecycleError::InvocationExhausted)?;
        let invocation_id = format!("invocation-{}", previous + 1);

        let _ = (operations.center)();
        for operation in [
            &mut operations.always_on_top,
            &mut operations.show,
            &mut operations.focus,
        ] {
            if operation().is_err() {
                (operations.clear_and_hide)();
                return Err(LifecycleError::WindowFailed);
            }
        }

        (operations.registry_on_show)(invocation_id.clone());
        let notice = *self
            .pending_notice
            .lock()
            .expect("pending notice lock poisoned");
        let payload = LauncherShown {
            invocation_id,
            target,
            notice,
        };
        if (operations.emit)(&payload).is_err() {
            (operations.clear_and_hide)();
            return Err(LifecycleError::WindowFailed);
        }
        self.consume_notice(notice);
        self.on_successful_show();

        if target == ShowTarget::Launcher {
            match self.reserve_critical() {
                Ok(reservation) => (operations.record_launcher)(reservation),
                Err(_) => self.set_notice_once(LifecycleNotice::ValidationFailed),
            }
        }
        Ok(ShowOutcome::Shown)
    }

    fn finish_launcher_record(&self, result: Result<(), ()>) {
        if result.is_err() {
            self.set_notice_once(LifecycleNotice::ValidationFailed);
        }
    }

    fn set_notice_once(&self, notice: LifecycleNotice) {
        let mut pending = self
            .pending_notice
            .lock()
            .expect("pending notice lock poisoned");
        if pending.is_none() {
            *pending = Some(notice);
        }
    }

    fn consume_notice(&self, notice: Option<LifecycleNotice>) {
        let Some(notice) = notice else {
            return;
        };
        let mut pending = self
            .pending_notice
            .lock()
            .expect("pending notice lock poisoned");
        if *pending == Some(notice) {
            *pending = None;
        }
    }

    fn reserve_critical(self: &Arc<Self>) -> Result<CriticalReservation, ReservationError> {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        if gate.state != ExitState::Running {
            return Err(ReservationError::NotRunning);
        }
        gate.in_flight_critical = gate
            .in_flight_critical
            .checked_add(1)
            .ok_or(ReservationError::Overflow)?;

        Ok(CriticalReservation {
            coordinator: Arc::clone(self),
            released: false,
        })
    }

    fn begin_tray_clean(&self, deadline: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        if gate.state != ExitState::Running || !matches!(gate.clean_attempt, CleanAttempt::Idle) {
            return CleanDecision::ObserveOnly;
        }

        gate.state = ExitState::Cleaning;
        Self::start_waiting(&mut gate, CleanOwner::Tray, deadline)
    }

    fn begin_system_end(&self, deadline: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        gate.state = ExitState::SystemEnding;

        match gate.clean_attempt {
            CleanAttempt::Idle => Self::start_waiting(&mut gate, CleanOwner::System, deadline),
            CleanAttempt::Waiting { deadline, .. } | CleanAttempt::Calling { deadline, .. } => {
                CleanDecision::Wait { deadline }
            }
            CleanAttempt::Finished(_) => CleanDecision::ObserveOnly,
        }
    }

    fn advance_clean(&self, now: Instant) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        match gate.clean_attempt {
            CleanAttempt::Waiting { owner, deadline } if now >= deadline => {
                let decision = if owner == CleanOwner::Tray && gate.state == ExitState::Cleaning {
                    gate.state = ExitState::Running;
                    gate.clean_attempt = CleanAttempt::Idle;
                    CleanDecision::ReturnRunning
                } else {
                    gate.clean_attempt = CleanAttempt::Finished(CleanResult::TimedOut);
                    CleanDecision::ObserveOnly
                };
                drop(gate);
                self.critical_changed.notify_all();
                decision
            }
            CleanAttempt::Waiting { owner, deadline } if gate.in_flight_critical == 0 => {
                gate.clean_attempt = CleanAttempt::Calling { owner, deadline };
                CleanDecision::CallMarker
            }
            CleanAttempt::Waiting { deadline, .. } => CleanDecision::Wait { deadline },
            CleanAttempt::Calling { deadline, .. } if now < deadline => {
                CleanDecision::Wait { deadline }
            }
            CleanAttempt::Calling { .. } | CleanAttempt::Idle | CleanAttempt::Finished(_) => {
                CleanDecision::ObserveOnly
            }
        }
    }

    fn complete_clean(&self, result: CleanResult) -> CleanDecision {
        let mut gate = self.exit_gate.lock().expect("exit gate lock poisoned");
        let CleanAttempt::Calling { owner, .. } = gate.clean_attempt else {
            return CleanDecision::ObserveOnly;
        };

        let decision = if gate.state == ExitState::SystemEnding {
            gate.clean_attempt = CleanAttempt::Finished(result);
            CleanDecision::ObserveOnly
        } else if gate.state == ExitState::Cleaning && owner == CleanOwner::Tray {
            match result {
                CleanResult::Succeeded => {
                    gate.state = ExitState::Clean;
                    gate.clean_attempt = CleanAttempt::Finished(result);
                    CleanDecision::Exit
                }
                CleanResult::Failed | CleanResult::TimedOut => {
                    gate.state = ExitState::Running;
                    gate.clean_attempt = CleanAttempt::Idle;
                    CleanDecision::ReturnRunning
                }
            }
        } else {
            CleanDecision::ObserveOnly
        };

        drop(gate);
        self.critical_changed.notify_all();
        decision
    }

    fn observe_exit(&self) -> ExitState {
        self.exit_gate
            .lock()
            .expect("exit gate lock poisoned")
            .state
    }

    fn start_waiting(gate: &mut ExitGate, owner: CleanOwner, deadline: Instant) -> CleanDecision {
        if gate.in_flight_critical == 0 {
            gate.clean_attempt = CleanAttempt::Calling { owner, deadline };
            CleanDecision::CallMarker
        } else {
            gate.clean_attempt = CleanAttempt::Waiting { owner, deadline };
            CleanDecision::Wait { deadline }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        panic::{catch_unwind, AssertUnwindSafe},
        sync::{atomic::Ordering, Arc, Barrier},
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::result_registry::ResultRegistry;

    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle, ShowTarget) -> Result<(), LifecycleError> =
        LifecycleCoordinator::request_show;
    const _: fn(&Arc<LifecycleCoordinator>, &AppHandle) -> Result<(), LifecycleError> =
        LifecycleCoordinator::mark_setup_ready;

    #[derive(Clone, Copy)]
    enum ReadyOrder {
        SetupFirst,
        FrontendFirst,
    }

    fn apply_ready_order(state: &mut Readiness, order: ReadyOrder) -> Vec<ShowTarget> {
        let results = match order {
            ReadyOrder::SetupFirst => [state.mark_setup_ready(), state.mark_frontend_ready()],
            ReadyOrder::FrontendFirst => [state.mark_frontend_ready(), state.mark_setup_ready()],
        };
        results.into_iter().flatten().collect()
    }

    #[test]
    fn readiness_does_not_synthesize_a_startup_show() {
        for order in [ReadyOrder::SetupFirst, ReadyOrder::FrontendFirst] {
            let mut state = Readiness::default();
            assert_eq!(
                apply_ready_order(&mut state, order),
                Vec::<ShowTarget>::new()
            );
            assert_eq!(state.pending_target, None);
        }
    }

    #[test]
    fn readiness_drains_only_the_last_early_explicit_target_once() {
        for (target, order) in [
            (ShowTarget::Launcher, ReadyOrder::SetupFirst),
            (ShowTarget::Settings, ReadyOrder::FrontendFirst),
        ] {
            let mut state = Readiness::default();
            assert_eq!(state.request(target), None);
            assert_eq!(apply_ready_order(&mut state, order), vec![target]);
            assert_eq!(state.mark_setup_ready(), None);
            assert_eq!(state.mark_frontend_ready(), None);
        }

        let mut state = Readiness::default();
        assert_eq!(state.request(ShowTarget::Launcher), None);
        assert_eq!(state.request(ShowTarget::Settings), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_frontend_ready(), Some(ShowTarget::Settings));
        assert_eq!(state.mark_frontend_ready(), None);
    }

    #[test]
    fn readiness_returns_each_request_after_both_ready() {
        let mut state = Readiness::default();
        assert!(apply_ready_order(&mut state, ReadyOrder::SetupFirst).is_empty());

        for target in [ShowTarget::Launcher, ShowTarget::Settings] {
            assert_eq!(state.request(target), Some(target));
            assert_eq!(state.pending_target, None);
        }
    }

    #[test]
    fn readiness_marks_are_idempotent_and_state_has_only_approved_fields() {
        let mut state = Readiness {
            setup_ready: false,
            frontend_ready: false,
            pending_target: None,
        };
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.request(ShowTarget::Launcher), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_frontend_ready(), Some(ShowTarget::Launcher));
        assert_eq!(state.mark_frontend_ready(), None);
    }

    #[test]
    fn modal_normal_and_open_export_paths_allow_only_one_chooser() {
        let queries = Cell::new(0);
        let mut state = ModalState::Normal;
        assert_eq!(
            state.begin_export(|| {
                queries.set(queries.get() + 1);
                Ok(true)
            }),
            Ok(())
        );
        assert_eq!(state, ModalState::Open);
        assert_eq!(
            state.begin_export(|| {
                queries.set(queries.get() + 1);
                Ok(true)
            }),
            Err(FocusDecision::Suppress)
        );
        assert_eq!(state, ModalState::Open);
        assert_eq!(queries.get(), 0);
    }

    #[test]
    fn modal_guard_drop_and_focus_events_restore_normal() {
        let mut state = ModalState::Open;
        assert_eq!(
            state.on_focus(false, || panic!("Open must not query focus")),
            FocusDecision::Suppress
        );
        assert_eq!(state, ModalState::Open);

        state.finish_export();
        assert_eq!(state, ModalState::AwaitingFocusRestore);
        assert_eq!(
            state.on_focus(true, || panic!("Focused(true) must not query focus")),
            FocusDecision::Suppress
        );
        assert_eq!(state, ModalState::Normal);
    }

    #[test]
    fn modal_awaiting_false_classifies_after_leaving_awaiting() {
        for (focus_result, expected) in [
            (Ok(true), FocusDecision::Suppress),
            (Ok(false), FocusDecision::ClearAndHide),
            (Err(()), FocusDecision::ReportWindowFailureAndHide),
        ] {
            let mut state = ModalState::AwaitingFocusRestore;
            assert_eq!(state.on_focus(false, || focus_result), expected);
            assert_eq!(state, ModalState::Normal);
        }

        let mut state = ModalState::AwaitingFocusRestore;
        let query = catch_unwind(AssertUnwindSafe(|| {
            state.on_focus(false, || panic!("focus query sentinel"));
        }));
        assert!(query.is_err());
        assert_eq!(state, ModalState::Normal);
    }

    #[test]
    fn modal_retry_and_successful_show_cannot_leave_awaiting_stuck() {
        let mut focused = ModalState::AwaitingFocusRestore;
        assert_eq!(focused.begin_export(|| Ok(true)), Ok(()));
        assert_eq!(focused, ModalState::Open);

        for (focus_result, expected) in [
            (Ok(false), FocusDecision::ClearAndHide),
            (Err(()), FocusDecision::ReportWindowFailureAndHide),
        ] {
            let mut state = ModalState::AwaitingFocusRestore;
            assert_eq!(state.begin_export(|| focus_result), Err(expected));
            assert_eq!(state, ModalState::Normal);
            assert_eq!(
                state.begin_export(|| panic!("Normal must not query focus")),
                Ok(())
            );
            assert_eq!(state, ModalState::Open);
        }

        let mut state = ModalState::AwaitingFocusRestore;
        state.on_successful_show();
        assert_eq!(state, ModalState::Normal);
        state = ModalState::Open;
        state.on_successful_show();
        assert_eq!(state, ModalState::Open);

        let mut state = ModalState::AwaitingFocusRestore;
        let query = catch_unwind(AssertUnwindSafe(|| {
            let _ = state.begin_export(|| panic!("retry focus query sentinel"));
        }));
        assert!(query.is_err());
        assert_eq!(state, ModalState::Normal);

        let coordinator = coordinator_for_test();
        let first = coordinator.begin_modal_export(|| Ok(true)).unwrap();
        drop(first);
        let recovered = coordinator
            .begin_modal_export(|| {
                let lock = coordinator
                    .modal
                    .try_lock()
                    .expect("focus query must run without the modal lock");
                drop(lock);
                assert_eq!(
                    coordinator
                        .begin_modal_export(|| panic!("Open must not query focus"))
                        .unwrap_err(),
                    FocusDecision::Suppress
                );
                Ok(true)
            })
            .unwrap();
        drop(recovered);
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum ShowFailure {
        None,
        Center,
        AlwaysOnTop,
        Show,
        Focus,
        Emit,
        Record,
        PanicRecord,
    }

    #[derive(Default)]
    struct ShowProbe {
        trace: RefCell<Vec<String>>,
        payloads: RefCell<Vec<LauncherShown>>,
        clears: Cell<usize>,
        records: Cell<usize>,
    }

    fn run_show_case(
        coordinator: &Arc<LifecycleCoordinator>,
        target: ShowTarget,
        index: u64,
        failure: ShowFailure,
        probe: &ShowProbe,
    ) -> Result<ShowOutcome, LifecycleError> {
        let mut center = || {
            probe.trace.borrow_mut().push(format!("invocation-{index}"));
            probe.trace.borrow_mut().push(format!("center-{index}"));
            (failure != ShowFailure::Center).then_some(()).ok_or(())
        };
        let mut always_on_top = || {
            probe
                .trace
                .borrow_mut()
                .push(format!("always-on-top-{index}"));
            (failure != ShowFailure::AlwaysOnTop)
                .then_some(())
                .ok_or(())
        };
        let mut show = || {
            probe.trace.borrow_mut().push(format!("show-{index}"));
            (failure != ShowFailure::Show).then_some(()).ok_or(())
        };
        let mut focus = || {
            probe.trace.borrow_mut().push(format!("focus-{index}"));
            (failure != ShowFailure::Focus).then_some(()).ok_or(())
        };
        let mut registry_on_show = |invocation_id: String| {
            assert_eq!(invocation_id, format!("invocation-{index}"));
            probe
                .trace
                .borrow_mut()
                .push(format!("registry-on-show-{index}"));
        };
        let mut emit = |payload: &LauncherShown| {
            probe.trace.borrow_mut().push(format!("emit-{index}"));
            probe.payloads.borrow_mut().push(payload.clone());
            (failure != ShowFailure::Emit).then_some(()).ok_or(())
        };
        let mut clear_and_hide = || {
            probe.clears.set(probe.clears.get() + 1);
            probe.trace.borrow_mut().push(format!("clear-{index}"));
        };
        let coordinator_for_record = Arc::clone(coordinator);
        let mut record_launcher = move |_reservation: CriticalReservation| {
            probe.records.set(probe.records.get() + 1);
            probe
                .trace
                .borrow_mut()
                .push(format!("reserve-launcher-record-{index}"));
            match failure {
                ShowFailure::Record => coordinator_for_record.finish_launcher_record(Err(())),
                ShowFailure::PanicRecord => panic!("launcher record panic sentinel"),
                _ => coordinator_for_record.finish_launcher_record(Ok(())),
            }
        };
        let mut operations = ShowMainClosures {
            center: &mut center,
            always_on_top: &mut always_on_top,
            show: &mut show,
            focus: &mut focus,
            registry_on_show: &mut registry_on_show,
            emit: &mut emit,
            clear_and_hide: &mut clear_and_hide,
            record_launcher: &mut record_launcher,
        };
        coordinator.show_main_core(target, &mut operations)
    }

    #[test]
    fn show_serializes_two_ready_requests_in_registry_event_order() {
        let coordinator = coordinator_for_test();
        coordinator
            .mark_setup_ready_with(|_| panic!("setup ready must not synthesize show"))
            .unwrap();
        coordinator
            .mark_frontend_ready_with(|_| panic!("frontend ready must not synthesize show"))
            .unwrap();
        let probe = ShowProbe::default();

        for index in 1..=2 {
            coordinator
                .request_show_with(ShowTarget::Launcher, |coordinator, target| {
                    probe.trace.borrow_mut().push(format!("dispatch-{index}"));
                    let target = coordinator
                        .readiness
                        .lock()
                        .expect("readiness lock poisoned")
                        .request(target);
                    if let Some(target) = target {
                        run_show_case(&coordinator, target, index, ShowFailure::None, &probe)
                            .map_err(|_| ())?;
                    }
                    Ok(())
                })
                .unwrap();
        }

        assert_eq!(
            *probe.trace.borrow(),
            [
                "dispatch-1",
                "invocation-1",
                "center-1",
                "always-on-top-1",
                "show-1",
                "focus-1",
                "registry-on-show-1",
                "emit-1",
                "reserve-launcher-record-1",
                "dispatch-2",
                "invocation-2",
                "center-2",
                "always-on-top-2",
                "show-2",
                "focus-2",
                "registry-on-show-2",
                "emit-2",
                "reserve-launcher-record-2",
            ]
        );
        assert_eq!(probe.records.get(), 2);
        assert_eq!(probe.clears.get(), 0);
        assert_eq!(probe.payloads.borrow().len(), 2);
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn show_dispatch_failure_has_zero_side_effects() {
        let coordinator = coordinator_for_test();
        let dispatched = Cell::new(0);
        assert_eq!(
            coordinator.request_show_with(ShowTarget::Launcher, |_, _| {
                dispatched.set(dispatched.get() + 1);
                Err(())
            }),
            Err(LifecycleError::MainThreadDispatchFailed)
        );
        assert_eq!(dispatched.get(), 1);
        assert_eq!(coordinator.next_invocation.load(Ordering::Relaxed), 0);
        assert_eq!(
            coordinator
                .readiness
                .lock()
                .expect("readiness lock poisoned")
                .pending_target,
            None
        );
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn show_window_and_emit_failures_clear_once_while_center_continues() {
        for failure in [
            ShowFailure::AlwaysOnTop,
            ShowFailure::Show,
            ShowFailure::Focus,
            ShowFailure::Emit,
        ] {
            let coordinator = coordinator_for_test();
            let probe = ShowProbe::default();
            assert_eq!(
                run_show_case(&coordinator, ShowTarget::Launcher, 1, failure, &probe,),
                Err(LifecycleError::WindowFailed)
            );
            assert_eq!(probe.clears.get(), 1);
            assert_eq!(probe.records.get(), 0);
            assert_eq!(exit_snapshot(&coordinator).1, 0);
        }

        let coordinator = coordinator_for_test();
        let probe = ShowProbe::default();
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Launcher,
                1,
                ShowFailure::Center,
                &probe,
            ),
            Ok(ShowOutcome::Shown)
        );
        assert_eq!(probe.clears.get(), 0);
        assert_eq!(probe.records.get(), 1);

        let registry = ResultRegistry::default();
        registry.on_show("old".into());
        assert!(registry.begin_query("old", 1).is_some());
        let hides = Cell::new(0);
        let mut center = || Ok(());
        let mut always_on_top = || Ok(());
        let mut show = || Ok(());
        let mut focus = || Err(());
        let mut registry_on_show = |id| registry.on_show(id);
        let mut emit = |_: &LauncherShown| Ok(());
        let mut clear_and_hide = || {
            registry.hide_and_clear();
            hides.set(hides.get() + 1);
        };
        let mut record_launcher = |_: CriticalReservation| {};
        let mut operations = ShowMainClosures {
            center: &mut center,
            always_on_top: &mut always_on_top,
            show: &mut show,
            focus: &mut focus,
            registry_on_show: &mut registry_on_show,
            emit: &mut emit,
            clear_and_hide: &mut clear_and_hide,
            record_launcher: &mut record_launcher,
        };
        assert_eq!(
            coordinator_for_test().show_main_core(ShowTarget::Launcher, &mut operations),
            Err(LifecycleError::WindowFailed)
        );
        assert_eq!(hides.get(), 1);
        assert!(registry.begin_query("old", 2).is_none());
    }

    #[test]
    fn show_non_running_state_is_ignored_before_invocation() {
        for state in [
            ExitState::Cleaning,
            ExitState::Clean,
            ExitState::SystemEnding,
        ] {
            let coordinator = coordinator_for_test();
            coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned")
                .state = state;
            let probe = ShowProbe::default();
            let resolutions = Cell::new(0);
            assert_eq!(
                coordinator.show_main_with_resolver(|| {
                    resolutions.set(resolutions.get() + 1);
                    run_show_case(
                        &coordinator,
                        ShowTarget::Launcher,
                        1,
                        ShowFailure::None,
                        &probe,
                    )
                }),
                Ok(None)
            );
            assert_eq!(resolutions.get(), 0);
            assert!(probe.trace.borrow().is_empty());
            assert_eq!(coordinator.next_invocation.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn show_target_notice_and_record_failures_keep_first_pending_notice() {
        let coordinator = coordinator_for_test();
        coordinator.set_notice_once(LifecycleNotice::SettingsFailed);
        let probe = ShowProbe::default();
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Settings,
                1,
                ShowFailure::None,
                &probe,
            ),
            Ok(ShowOutcome::Shown)
        );
        assert_eq!(probe.records.get(), 0);
        assert_eq!(
            probe.payloads.borrow()[0].notice,
            Some(LifecycleNotice::SettingsFailed)
        );
        assert_eq!(*coordinator.pending_notice.lock().unwrap(), None);

        coordinator.set_notice_once(LifecycleNotice::SettingsFailed);
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Settings,
                2,
                ShowFailure::Emit,
                &probe,
            ),
            Err(LifecycleError::WindowFailed)
        );
        coordinator.set_notice_once(LifecycleNotice::ValidationFailed);
        assert_eq!(
            *coordinator.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::SettingsFailed)
        );

        *coordinator.pending_notice.lock().unwrap() = None;
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Launcher,
                3,
                ShowFailure::Record,
                &probe,
            ),
            Ok(ShowOutcome::Shown)
        );
        assert_eq!(
            *coordinator.pending_notice.lock().unwrap(),
            Some(LifecycleNotice::ValidationFailed)
        );

        let panicking = coordinator_for_test();
        let panic_probe = ShowProbe::default();
        let panic_result = catch_unwind(AssertUnwindSafe(|| {
            let _ = run_show_case(
                &panicking,
                ShowTarget::Launcher,
                1,
                ShowFailure::PanicRecord,
                &panic_probe,
            );
        }));
        assert!(panic_result.is_err());
        assert_eq!(exit_snapshot(&panicking).1, 0);
    }

    #[test]
    fn show_invocation_overflow_and_payload_serialization_are_fixed() {
        let coordinator = coordinator_for_test();
        coordinator
            .next_invocation
            .store(u64::MAX, Ordering::Relaxed);
        let probe = ShowProbe::default();
        assert_eq!(
            run_show_case(
                &coordinator,
                ShowTarget::Launcher,
                1,
                ShowFailure::None,
                &probe,
            ),
            Err(LifecycleError::InvocationExhausted)
        );
        assert!(probe.trace.borrow().is_empty());
        assert_eq!(exit_snapshot(&coordinator).1, 0);

        for (target, expected) in [
            (ShowTarget::Launcher, "launcher"),
            (ShowTarget::Settings, "settings"),
        ] {
            let value = serde_json::to_value(LauncherShown {
                invocation_id: "invocation-1".into(),
                target,
                notice: Some(LifecycleNotice::ValidationFailed),
            })
            .unwrap();
            assert_eq!(value["target"], expected);
            assert_eq!(value["notice"], "validationFailed");
        }
    }

    fn coordinator_for_test() -> Arc<LifecycleCoordinator> {
        Arc::new(LifecycleCoordinator::default())
    }

    fn exit_snapshot(coordinator: &LifecycleCoordinator) -> (ExitState, usize, CleanAttempt) {
        let gate = coordinator
            .exit_gate
            .lock()
            .expect("exit gate lock poisoned");
        (gate.state, gate.in_flight_critical, gate.clean_attempt)
    }

    fn complete_requested_clean<F>(
        coordinator: &LifecycleCoordinator,
        decision: CleanDecision,
        marker: F,
    ) -> CleanDecision
    where
        F: FnOnce() -> CleanResult,
    {
        assert_eq!(decision, CleanDecision::CallMarker);
        coordinator.complete_clean(marker())
    }

    #[derive(Default)]
    struct AttemptTrace {
        next: u64,
        active: Option<u64>,
    }

    impl AttemptTrace {
        fn begin<F>(&mut self, coordinator: &LifecycleCoordinator, begin: F) -> (u64, CleanDecision)
        where
            F: FnOnce() -> CleanDecision,
        {
            let was_idle = matches!(exit_snapshot(coordinator).2, CleanAttempt::Idle);
            let decision = begin();
            let is_idle = matches!(exit_snapshot(coordinator).2, CleanAttempt::Idle);
            if was_idle && !is_idle {
                self.next += 1;
                self.active = Some(self.next);
            }
            (self.active.expect("attempt trace must be active"), decision)
        }

        fn observe(&self) -> u64 {
            self.active.expect("attempt trace must be active")
        }

        fn clear_if_idle(&mut self, coordinator: &LifecycleCoordinator) {
            if matches!(exit_snapshot(coordinator).2, CleanAttempt::Idle) {
                self.active = None;
            }
        }
    }

    #[test]
    fn critical_reservation_releases_once_on_drop() {
        let coordinator = coordinator_for_test();
        let reservation = coordinator.reserve_critical().unwrap();
        assert_eq!(exit_snapshot(&coordinator).1, 1);

        let waiting = Arc::clone(&coordinator);
        let barrier = Arc::new(Barrier::new(2));
        let waiting_barrier = Arc::clone(&barrier);
        let waiter = thread::spawn(move || {
            let gate = waiting.exit_gate.lock().expect("exit gate lock poisoned");
            waiting_barrier.wait();
            let (gate, timeout) = waiting
                .critical_changed
                .wait_timeout_while(gate, Duration::from_secs(1), |gate| {
                    gate.in_flight_critical != 0
                })
                .expect("exit gate lock poisoned");
            assert!(!timeout.timed_out());
            assert_eq!(gate.in_flight_critical, 0);
        });

        barrier.wait();
        drop(reservation);
        waiter.join().unwrap();
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn critical_reservation_rejects_non_running_states_and_overflow() {
        let coordinator = coordinator_for_test();
        for state in [
            ExitState::Cleaning,
            ExitState::Clean,
            ExitState::SystemEnding,
        ] {
            coordinator
                .exit_gate
                .lock()
                .expect("exit gate lock poisoned")
                .state = state;
            assert_eq!(
                coordinator.reserve_critical().unwrap_err(),
                ReservationError::NotRunning
            );
        }

        let mut gate = coordinator
            .exit_gate
            .lock()
            .expect("exit gate lock poisoned");
        gate.state = ExitState::Running;
        gate.in_flight_critical = usize::MAX;
        drop(gate);
        assert_eq!(
            coordinator.reserve_critical().unwrap_err(),
            ReservationError::Overflow
        );
        assert_eq!(exit_snapshot(&coordinator).1, usize::MAX);
    }

    #[test]
    fn critical_reservation_releases_on_error_and_panic() {
        fn fail_after_reserving(
            coordinator: &Arc<LifecycleCoordinator>,
        ) -> Result<(), &'static str> {
            let _reservation = coordinator.reserve_critical().unwrap();
            Err("sentinel")
        }

        let coordinator = coordinator_for_test();
        assert_eq!(fail_after_reserving(&coordinator), Err("sentinel"));
        assert_eq!(exit_snapshot(&coordinator).1, 0);

        let panicking = Arc::clone(&coordinator);
        let result = catch_unwind(AssertUnwindSafe(move || {
            let _reservation = panicking.reserve_critical().unwrap();
            panic!("reservation panic sentinel");
        }));
        assert!(result.is_err());
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn critical_plain_show_state_does_not_reserve() {
        let coordinator = coordinator_for_test();
        {
            let mut readiness = coordinator
                .readiness
                .lock()
                .expect("readiness lock poisoned");
            assert_eq!(readiness.request(ShowTarget::Launcher), None);
            assert_eq!(readiness.mark_setup_ready(), None);
            assert_eq!(readiness.mark_frontend_ready(), Some(ShowTarget::Launcher));
        }
        {
            let mut modal = coordinator.modal.lock().expect("modal lock poisoned");
            assert_eq!(modal.begin_export(|| Ok(true)), Ok(()));
            modal.finish_export();
            modal.on_successful_show();
        }
        assert_eq!(exit_snapshot(&coordinator).1, 0);
    }

    #[test]
    fn clean_attempt_system_shares_waiting_and_calling_tray_attempt() {
        let coordinator = coordinator_for_test();
        let reservation = coordinator.reserve_critical().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let later_deadline = deadline + Duration::from_secs(5);
        let mut trace = AttemptTrace::default();

        let (tray_trace, tray) =
            trace.begin(&coordinator, || coordinator.begin_tray_clean(deadline));
        assert_eq!(tray, CleanDecision::Wait { deadline });
        let system = coordinator.begin_system_end(later_deadline);
        assert_eq!(system, CleanDecision::Wait { deadline });
        assert_eq!(trace.observe(), tray_trace);
        assert_eq!(exit_snapshot(&coordinator).0, ExitState::SystemEnding);

        drop(reservation);
        assert_eq!(
            coordinator.advance_clean(Instant::now()),
            CleanDecision::CallMarker
        );
        assert!(matches!(
            exit_snapshot(&coordinator).2,
            CleanAttempt::Calling {
                owner: CleanOwner::Tray,
                deadline: stored
            } if stored == deadline
        ));
        assert_eq!(
            coordinator.begin_system_end(later_deadline),
            CleanDecision::Wait { deadline }
        );
        assert_eq!(trace.observe(), tray_trace);

        let marker_calls = Cell::new(0);
        let completion = complete_requested_clean(&coordinator, CleanDecision::CallMarker, || {
            marker_calls.set(marker_calls.get() + 1);
            CleanResult::Succeeded
        });
        assert_eq!(completion, CleanDecision::ObserveOnly);
        assert_eq!(marker_calls.get(), 1);
        assert_eq!(exit_snapshot(&coordinator).0, ExitState::SystemEnding);
    }

    #[test]
    fn clean_attempt_tray_completion_cannot_revert_system_ending() {
        for result in [CleanResult::Succeeded, CleanResult::Failed] {
            let coordinator = coordinator_for_test();
            let deadline = Instant::now() + Duration::from_secs(5);
            assert_eq!(
                coordinator.begin_tray_clean(deadline),
                CleanDecision::CallMarker
            );
            assert_eq!(
                coordinator.begin_system_end(deadline + Duration::from_secs(1)),
                CleanDecision::Wait { deadline }
            );
            assert_eq!(
                coordinator.complete_clean(result),
                CleanDecision::ObserveOnly
            );
            assert_eq!(
                exit_snapshot(&coordinator),
                (ExitState::SystemEnding, 0, CleanAttempt::Finished(result))
            );
        }
    }

    #[test]
    fn clean_attempt_later_system_work_uses_a_fresh_trace_after_failure_or_timeout() {
        let marker_calls = Cell::new(0);

        let succeeded = coordinator_for_test();
        let success_deadline = Instant::now() + Duration::from_secs(5);
        let mut success_trace = AttemptTrace::default();
        let (tray_trace, tray) =
            success_trace.begin(&succeeded, || succeeded.begin_tray_clean(success_deadline));
        assert_eq!(
            complete_requested_clean(&succeeded, tray, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Succeeded
            }),
            CleanDecision::Exit
        );
        assert_eq!(
            succeeded.begin_system_end(success_deadline + Duration::from_secs(1)),
            CleanDecision::ObserveOnly
        );
        assert_eq!(success_trace.observe(), tray_trace);
        assert_eq!(marker_calls.get(), 1);

        marker_calls.set(0);
        let failed = coordinator_for_test();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut failed_trace = AttemptTrace::default();
        let (tray_trace, tray) = failed_trace.begin(&failed, || failed.begin_tray_clean(deadline));
        assert_eq!(
            complete_requested_clean(&failed, tray, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Failed
            }),
            CleanDecision::ReturnRunning
        );
        failed_trace.clear_if_idle(&failed);
        let (system_trace, system) = failed_trace.begin(&failed, || {
            failed.begin_system_end(deadline + Duration::from_secs(1))
        });
        assert_ne!(tray_trace, system_trace);
        assert_eq!(
            complete_requested_clean(&failed, system, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Succeeded
            }),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 2);

        marker_calls.set(0);
        let timed_out = coordinator_for_test();
        let reservation = timed_out.reserve_critical().unwrap();
        let timeout_deadline = Instant::now() + Duration::from_secs(1);
        let mut timeout_trace = AttemptTrace::default();
        let (tray_trace, tray) =
            timeout_trace.begin(&timed_out, || timed_out.begin_tray_clean(timeout_deadline));
        assert_eq!(
            tray,
            CleanDecision::Wait {
                deadline: timeout_deadline
            }
        );
        assert_eq!(
            timed_out.advance_clean(timeout_deadline),
            CleanDecision::ReturnRunning
        );
        timeout_trace.clear_if_idle(&timed_out);
        drop(reservation);
        let (system_trace, system) = timeout_trace.begin(&timed_out, || {
            timed_out.begin_system_end(timeout_deadline + Duration::from_secs(1))
        });
        assert_ne!(tray_trace, system_trace);
        assert_eq!(
            complete_requested_clean(&timed_out, system, || {
                marker_calls.set(marker_calls.get() + 1);
                CleanResult::Succeeded
            }),
            CleanDecision::ObserveOnly
        );
        assert_eq!(marker_calls.get(), 1);
    }

    #[test]
    fn clean_attempt_timeouts_and_exit_observer_are_fail_closed() {
        let tray = coordinator_for_test();
        let tray_reservation = tray.reserve_critical().unwrap();
        let tray_deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            tray.begin_tray_clean(tray_deadline),
            CleanDecision::Wait {
                deadline: tray_deadline
            }
        );
        assert_eq!(
            tray.advance_clean(tray_deadline),
            CleanDecision::ReturnRunning
        );
        assert_eq!(
            exit_snapshot(&tray),
            (ExitState::Running, 1, CleanAttempt::Idle)
        );
        drop(tray_reservation);

        let late_marker_calls = Cell::new(0);
        let released_tray = coordinator_for_test();
        let released_tray_reservation = released_tray.reserve_critical().unwrap();
        let released_tray_deadline = Instant::now();
        assert_eq!(
            released_tray.begin_tray_clean(released_tray_deadline),
            CleanDecision::Wait {
                deadline: released_tray_deadline
            }
        );
        drop(released_tray_reservation);
        let released_tray_decision = released_tray.advance_clean(released_tray_deadline);
        if released_tray_decision == CleanDecision::CallMarker {
            late_marker_calls.set(late_marker_calls.get() + 1);
        }
        assert_eq!(released_tray_decision, CleanDecision::ReturnRunning);
        assert_eq!(late_marker_calls.get(), 0);
        assert_eq!(
            exit_snapshot(&released_tray),
            (ExitState::Running, 0, CleanAttempt::Idle)
        );

        let shared = coordinator_for_test();
        let shared_reservation = shared.reserve_critical().unwrap();
        let shared_deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            shared.begin_tray_clean(shared_deadline),
            CleanDecision::Wait {
                deadline: shared_deadline
            }
        );
        assert_eq!(
            shared.begin_system_end(shared_deadline + Duration::from_secs(1)),
            CleanDecision::Wait {
                deadline: shared_deadline
            }
        );
        assert_eq!(
            shared.advance_clean(shared_deadline),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            exit_snapshot(&shared),
            (
                ExitState::SystemEnding,
                1,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );
        drop(shared_reservation);

        let released_shared = coordinator_for_test();
        let released_shared_reservation = released_shared.reserve_critical().unwrap();
        let released_shared_deadline = Instant::now();
        assert_eq!(
            released_shared.begin_tray_clean(released_shared_deadline),
            CleanDecision::Wait {
                deadline: released_shared_deadline
            }
        );
        assert_eq!(
            released_shared.begin_system_end(released_shared_deadline + Duration::from_secs(1)),
            CleanDecision::Wait {
                deadline: released_shared_deadline
            }
        );
        drop(released_shared_reservation);
        let released_shared_decision = released_shared.advance_clean(released_shared_deadline);
        if released_shared_decision == CleanDecision::CallMarker {
            late_marker_calls.set(late_marker_calls.get() + 1);
        }
        assert_eq!(released_shared_decision, CleanDecision::ObserveOnly);
        assert_eq!(late_marker_calls.get(), 0);
        assert_eq!(
            exit_snapshot(&released_shared),
            (
                ExitState::SystemEnding,
                0,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );

        let system = coordinator_for_test();
        let system_reservation = system.reserve_critical().unwrap();
        let system_deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            system.begin_system_end(system_deadline),
            CleanDecision::Wait {
                deadline: system_deadline
            }
        );
        assert_eq!(
            system.advance_clean(system_deadline),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            exit_snapshot(&system),
            (
                ExitState::SystemEnding,
                1,
                CleanAttempt::Finished(CleanResult::TimedOut)
            )
        );
        drop(system_reservation);
        assert_eq!(
            system.advance_clean(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(
            system.begin_tray_clean(Instant::now()),
            CleanDecision::ObserveOnly
        );
        assert_eq!(system.observe_exit(), ExitState::SystemEnding);
    }
}
