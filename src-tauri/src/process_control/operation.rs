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

impl<A: ProcessOperationAdapter> CoreAuthority<A> {
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
