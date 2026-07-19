use std::{
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShowTarget {
    Launcher,
    Settings,
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
enum FocusDecision {
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
    readiness: Mutex<Readiness>,
    modal: Mutex<ModalState>,
    exit_gate: Mutex<ExitGate>,
    critical_changed: Condvar,
}

impl Default for LifecycleCoordinator {
    fn default() -> Self {
        Self {
            readiness: Mutex::new(Readiness::default()),
            modal: Mutex::new(ModalState::Normal),
            exit_gate: Mutex::new(ExitGate::default()),
            critical_changed: Condvar::new(),
        }
    }
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
            CleanAttempt::Waiting { owner, deadline } if gate.in_flight_critical == 0 => {
                gate.clean_attempt = CleanAttempt::Calling { owner, deadline };
                CleanDecision::CallMarker
            }
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
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
        sync::{Arc, Barrier},
        thread,
        time::{Duration, Instant},
    };

    use super::*;

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
