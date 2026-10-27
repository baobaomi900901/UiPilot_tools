use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use super::core::{
    CoreAdmission, CoreAuthority, Liveness, ObservationFailure, ProcessIntent,
    ProcessPlatformAdapter,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessOperationError {
    IntentMismatch,
    TargetExited,
    TargetChanged,
    InspectionDenied,
    ProtectedTarget,
    OperationAccessDenied,
    GracefulCloseUnavailable,
    GracefulCloseTimedOut,
    OperationUnavailable,
    TerminationPending,
    CompletionUnknown,
    PlatformUnsupported,
}

impl fmt::Display for ProcessOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::IntentMismatch => "IntentMismatch",
            Self::TargetExited => "TargetExited",
            Self::TargetChanged => "TargetChanged",
            Self::InspectionDenied => "InspectionDenied",
            Self::ProtectedTarget => "ProtectedTarget",
            Self::OperationAccessDenied => "OperationAccessDenied",
            Self::GracefulCloseUnavailable => "GracefulCloseUnavailable",
            Self::GracefulCloseTimedOut => "GracefulCloseTimedOut",
            Self::OperationUnavailable => "OperationUnavailable",
            Self::TerminationPending => "TerminationPending",
            Self::CompletionUnknown => "CompletionUnknown",
            Self::PlatformUnsupported => "PlatformUnsupported",
        })
    }
}

impl Error for ProcessOperationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OperationAccess {
    query_limited_information: bool,
    synchronize: bool,
    terminate: bool,
}

impl OperationAccess {
    pub(super) const GRACEFUL: Self = Self {
        query_limited_information: true,
        synchronize: true,
        terminate: false,
    };

    pub(super) const FORCE: Self = Self {
        query_limited_information: true,
        synchronize: true,
        terminate: true,
    };

    pub(super) fn query_limited_information(self) -> bool {
        self.query_limited_information
    }

    pub(super) fn synchronize(self) -> bool {
        self.synchronize
    }

    pub(super) fn terminate(self) -> bool {
        self.terminate
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OperationFailure {
    ExitedOrChanged,
    AccessDenied,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowFailure {
    Gone,
    AccessDenied,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WindowCandidate<W> {
    token: W,
    pid: u32,
    visible: bool,
    has_owner: bool,
}

impl<W: Copy> WindowCandidate<W> {
    pub(super) fn new(token: W, pid: u32, visible: bool, has_owner: bool) -> Self {
        Self {
            token,
            pid,
            visible,
            has_owner,
        }
    }

    pub(super) fn token(self) -> W {
        self.token
    }

    pub(super) fn pid(self) -> u32 {
        self.pid
    }

    pub(super) fn visible(self) -> bool {
        self.visible
    }

    pub(super) fn has_owner(self) -> bool {
        self.has_owner
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WindowFacts {
    pid: u32,
    visible: bool,
    has_owner: bool,
}

impl WindowFacts {
    pub(super) fn new(pid: u32, visible: bool, has_owner: bool) -> Self {
        Self {
            pid,
            visible,
            has_owner,
        }
    }

    pub(super) fn pid(self) -> u32 {
        self.pid
    }

    pub(super) fn visible(self) -> bool {
        self.visible
    }

    pub(super) fn has_owner(self) -> bool {
        self.has_owner
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExitWait {
    Exited,
    TimedOut,
}

pub(super) trait ProcessOperationAdapter: ProcessPlatformAdapter {
    type WindowToken: Copy;

    fn open_operation(
        &self,
        pid: u32,
        access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure>;

    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure>;

    fn window_facts(&self, window: Self::WindowToken) -> Result<WindowFacts, WindowFailure>;

    fn post_close(&self, window: Self::WindowToken) -> Result<(), WindowFailure>;

    fn terminate_process(
        &self,
        handle: &Self::Handle,
        exit_code: u32,
    ) -> Result<(), OperationFailure>;

    fn wait_for_exit(
        &self,
        handle: &Self::Handle,
        timeout: Duration,
    ) -> Result<ExitWait, OperationFailure>;
}

pub(super) trait MonotonicClock {
    fn now(&self) -> Instant;
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SystemMonotonicClock;

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

const GRACEFUL_CLOSE_DEADLINE: Duration = Duration::from_secs(5);
const FORCE_TERMINATION_DEADLINE: Duration = Duration::from_secs(5);

impl<A: ProcessOperationAdapter> CoreAuthority<A> {
    pub(super) fn terminate_with_clock<C: MonotonicClock>(
        &self,
        admission: CoreAdmission<A::Handle>,
        clock: &C,
    ) -> Result<(), ProcessOperationError> {
        let target_pid = admission.target().pid();
        self.with_final_operation_admission(
            admission,
            ProcessIntent::Terminate,
            |adapter, operation_handle| {
                let deadline = clock.now() + GRACEFUL_CLOSE_DEADLINE;
                terminate_gracefully(adapter, operation_handle, target_pid, clock, deadline)
            },
        )
    }

    pub(super) fn force_terminate_with_clock<C: MonotonicClock>(
        &self,
        admission: CoreAdmission<A::Handle>,
        clock: &C,
    ) -> Result<(), ProcessOperationError> {
        self.with_final_operation_admission(
            admission,
            ProcessIntent::ForceTerminate,
            |adapter, operation_handle| {
                let deadline = clock.now() + FORCE_TERMINATION_DEADLINE;
                force_terminate(adapter, operation_handle, clock, deadline)
            },
        )
    }

    pub(super) fn with_final_operation_admission<T>(
        &self,
        admission: CoreAdmission<A::Handle>,
        requested_intent: ProcessIntent,
        operation: impl FnOnce(&A, &A::Handle) -> Result<T, ProcessOperationError>,
    ) -> Result<T, ProcessOperationError> {
        let (target, bound_intent, admission_handle) = admission.into_parts();
        if bound_intent != requested_intent {
            return Err(ProcessOperationError::IntentMismatch);
        }
        let access = match requested_intent {
            ProcessIntent::Terminate => OperationAccess::GRACEFUL,
            ProcessIntent::ForceTerminate => OperationAccess::FORCE,
            ProcessIntent::Restart => return Err(ProcessOperationError::IntentMismatch),
        };

        let operation_handle = self
            .adapter()
            .open_operation(target.pid(), access)
            .map_err(operation_open_failure)?;
        let creation_time = self
            .adapter()
            .creation_time(&operation_handle)
            .map_err(final_identity_failure)?;
        if creation_time == 0 {
            return Err(ProcessOperationError::InspectionDenied);
        }
        if creation_time != target.creation_time() {
            return Err(ProcessOperationError::TargetChanged);
        }

        match self.adapter().liveness(&operation_handle) {
            Ok(Liveness::Alive) => {}
            Ok(Liveness::Exited) => return Err(ProcessOperationError::TargetExited),
            Err(_) => return Err(ProcessOperationError::InspectionDenied),
        }

        let protection = self
            .adapter()
            .protection(&operation_handle)
            .map_err(|_| ProcessOperationError::InspectionDenied)?;
        let (self_pid, self_creation_time) = self.self_identity();
        if target.pid() == self_pid && target.creation_time() == self_creation_time {
            return Err(ProcessOperationError::ProtectedTarget);
        }
        if protection.is_critical() || protection.is_protected() {
            return Err(ProcessOperationError::ProtectedTarget);
        }

        drop(admission_handle);
        operation(self.adapter(), &operation_handle)
    }
}

fn terminate_gracefully<A: ProcessOperationAdapter, C: MonotonicClock>(
    adapter: &A,
    operation_handle: &A::Handle,
    target_pid: u32,
    clock: &C,
    deadline: Instant,
) -> Result<(), ProcessOperationError> {
    let candidates = adapter
        .enumerate_top_level_windows()
        .map_err(side_effect_availability_failure)?
        .into_iter()
        .filter(|candidate| {
            candidate.pid() == target_pid && candidate.visible() && !candidate.has_owner()
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        let unavailable = if clock.now() >= deadline {
            ProcessOperationError::OperationUnavailable
        } else {
            ProcessOperationError::GracefulCloseUnavailable
        };
        return unavailable_unless_exited(adapter, operation_handle, unavailable);
    }

    let mut posted = false;
    let mut access_denied = false;
    let mut unavailable = false;
    let mut deadline_exhausted = false;
    let mut completion_observation_failed = false;

    for candidate in candidates {
        match adapter.wait_for_exit(operation_handle, Duration::ZERO) {
            Ok(ExitWait::Exited) => return Ok(()),
            Ok(ExitWait::TimedOut) => {}
            Err(_failure) if posted => {
                completion_observation_failed = true;
                break;
            }
            Err(failure) => return Err(side_effect_availability_failure(failure)),
        }
        if clock.now() >= deadline {
            deadline_exhausted = true;
            break;
        }

        let facts = adapter.window_facts(candidate.token());
        if clock.now() >= deadline {
            deadline_exhausted = true;
            break;
        }
        let facts = match facts {
            Ok(facts) if facts.pid() == target_pid && facts.visible() && !facts.has_owner() => {
                facts
            }
            Ok(_) | Err(WindowFailure::Gone) => continue,
            Err(WindowFailure::AccessDenied) => {
                access_denied = true;
                continue;
            }
            Err(WindowFailure::Unavailable) => {
                unavailable = true;
                continue;
            }
        };
        debug_assert_eq!(facts.pid(), target_pid);

        // This token is HWND-like, not HANDLE-backed identity. Even after the
        // zero-time process poll and immediate PID/visibility/owner revalidation,
        // a narrow target-exit plus HWND/PID-reuse race remains before post_close.
        // Never retarget a changed or gone token; graceful close cannot claim the
        // operation HANDLE's stronger identity guarantee.
        match adapter.post_close(candidate.token()) {
            Ok(()) => posted = true,
            Err(WindowFailure::Gone) => {}
            Err(WindowFailure::AccessDenied) => access_denied = true,
            Err(WindowFailure::Unavailable) => unavailable = true,
        }
    }

    match adapter.wait_for_exit(operation_handle, Duration::ZERO) {
        Ok(ExitWait::Exited) => return Ok(()),
        Ok(ExitWait::TimedOut) => {}
        Err(_failure) if posted => completion_observation_failed = true,
        Err(failure) => return Err(side_effect_availability_failure(failure)),
    }

    if deadline_exhausted && !posted {
        return Err(ProcessOperationError::OperationUnavailable);
    }
    if !posted {
        return Err(if unavailable {
            ProcessOperationError::OperationUnavailable
        } else if access_denied {
            ProcessOperationError::OperationAccessDenied
        } else {
            ProcessOperationError::GracefulCloseUnavailable
        });
    }

    let remaining = deadline.saturating_duration_since(clock.now());
    let wait = adapter.wait_for_exit(operation_handle, remaining);
    let final_poll = adapter.wait_for_exit(operation_handle, Duration::ZERO);
    if matches!(wait, Ok(ExitWait::Exited)) || matches!(final_poll, Ok(ExitWait::Exited)) {
        return Ok(());
    }
    if completion_observation_failed || wait.is_err() || final_poll.is_err() {
        return Err(ProcessOperationError::CompletionUnknown);
    }
    Err(ProcessOperationError::GracefulCloseTimedOut)
}

fn force_terminate<A: ProcessOperationAdapter, C: MonotonicClock>(
    adapter: &A,
    operation_handle: &A::Handle,
    clock: &C,
    deadline: Instant,
) -> Result<(), ProcessOperationError> {
    match adapter.wait_for_exit(operation_handle, Duration::ZERO) {
        Ok(ExitWait::Exited) => return Ok(()),
        Ok(ExitWait::TimedOut) => {}
        Err(failure) => return Err(side_effect_availability_failure(failure)),
    }

    match adapter.terminate_process(operation_handle, 1) {
        Ok(()) => {
            let remaining = deadline.saturating_duration_since(clock.now());
            let wait = adapter.wait_for_exit(operation_handle, remaining);
            let final_poll = adapter.wait_for_exit(operation_handle, Duration::ZERO);
            if matches!(wait, Ok(ExitWait::Exited)) || matches!(final_poll, Ok(ExitWait::Exited)) {
                return Ok(());
            }
            if wait.is_err() || final_poll.is_err() {
                return Err(ProcessOperationError::CompletionUnknown);
            }
            Err(ProcessOperationError::TerminationPending)
        }
        Err(failure) => match adapter.wait_for_exit(operation_handle, Duration::ZERO) {
            Ok(ExitWait::Exited) => Ok(()),
            Ok(ExitWait::TimedOut) => Err(side_effect_availability_failure(failure)),
            Err(_) => Err(ProcessOperationError::CompletionUnknown),
        },
    }
}

fn unavailable_unless_exited<A: ProcessOperationAdapter>(
    adapter: &A,
    operation_handle: &A::Handle,
    unavailable: ProcessOperationError,
) -> Result<(), ProcessOperationError> {
    match adapter.wait_for_exit(operation_handle, Duration::ZERO) {
        Ok(ExitWait::Exited) => Ok(()),
        Ok(ExitWait::TimedOut) => Err(unavailable),
        Err(failure) => Err(side_effect_availability_failure(failure)),
    }
}

fn side_effect_availability_failure(failure: OperationFailure) -> ProcessOperationError {
    match failure {
        OperationFailure::AccessDenied => ProcessOperationError::OperationAccessDenied,
        OperationFailure::ExitedOrChanged | OperationFailure::Unavailable => {
            ProcessOperationError::OperationUnavailable
        }
    }
}

fn operation_open_failure(failure: OperationFailure) -> ProcessOperationError {
    match failure {
        OperationFailure::ExitedOrChanged => ProcessOperationError::TargetExited,
        OperationFailure::AccessDenied => ProcessOperationError::OperationAccessDenied,
        OperationFailure::Unavailable => ProcessOperationError::OperationUnavailable,
    }
}

fn final_identity_failure(failure: ObservationFailure) -> ProcessOperationError {
    match failure {
        ObservationFailure::ExitedOrChanged => ProcessOperationError::TargetExited,
        ObservationFailure::AccessDenied | ObservationFailure::IdentityUnavailable => {
            ProcessOperationError::InspectionDenied
        }
    }
}
