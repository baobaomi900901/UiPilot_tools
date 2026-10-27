use std::{
    cell::Cell,
    cmp::Ordering,
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering as AtomicOrdering},
        Arc,
    },
};

use super::{
    core::{CoreAuthority, HostInstanceId, ProcessError, ProcessIntent, ProcessTarget},
    operation::{MonotonicClock, ProcessOperationAdapter, ProcessOperationError},
};

#[cfg(windows)]
pub(super) mod windows;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RestartPathInput {
    executable: PathBuf,
    working_directory: PathBuf,
}

impl RestartPathInput {
    pub(super) fn new(executable: PathBuf, working_directory: PathBuf) -> Self {
        Self {
            executable,
            working_directory,
        }
    }

    pub(super) fn executable(&self) -> &Path {
        &self.executable
    }

    pub(super) fn working_directory(&self) -> &Path {
        &self.working_directory
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NativeLaunchInput {
    command_line: Vec<u16>,
    environment: Vec<u16>,
}

impl NativeLaunchInput {
    pub(super) fn new(command_line: Vec<u16>, environment: Vec<u16>) -> Self {
        Self {
            command_line,
            environment,
        }
    }

    pub(super) fn command_line(&self) -> &[u16] {
        &self.command_line
    }

    pub(super) fn environment(&self) -> &[u16] {
        &self.environment
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CreatedProcessIdentity {
    pid: u32,
    creation_time: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LeasedPathMetadata {
    executable: PathBuf,
    working_directory: PathBuf,
    executable_volume_serial: u32,
    executable_file_index: u64,
}

impl LeasedPathMetadata {
    pub(super) fn new(
        executable: PathBuf,
        working_directory: PathBuf,
        executable_volume_serial: u32,
        executable_file_index: u64,
    ) -> Self {
        Self {
            executable,
            working_directory,
            executable_volume_serial,
            executable_file_index,
        }
    }

    pub(super) fn executable(&self) -> &Path {
        &self.executable
    }
    pub(super) fn working_directory(&self) -> &Path {
        &self.working_directory
    }
    pub(super) fn executable_file_identity(&self) -> (u32, u64) {
        (self.executable_volume_serial, self.executable_file_index)
    }
}

impl CreatedProcessIdentity {
    pub(super) fn new(pid: u32, creation_time: u64) -> Result<Self, RestartAdapterError> {
        if pid == 0 || creation_time == 0 {
            return Err(RestartAdapterError::IdentityUnavailable);
        }
        Ok(Self { pid, creation_time })
    }

    pub(super) fn pid(self) -> u32 {
        self.pid
    }

    pub(super) fn creation_time(self) -> u64 {
        self.creation_time
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestartAdapterError {
    InvalidInput,
    UnsupportedPath,
    PathUnavailable,
    PathChanged,
    CreationFailed,
    IdentityUnavailable,
}

impl fmt::Display for RestartAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "InvalidInput",
            Self::UnsupportedPath => "UnsupportedPath",
            Self::PathUnavailable => "PathUnavailable",
            Self::PathChanged => "PathChanged",
            Self::CreationFailed => "CreationFailed",
            Self::IdentityUnavailable => "IdentityUnavailable",
        })
    }
}

impl Error for RestartAdapterError {}

pub(super) trait ProcessRestartAdapter: ProcessOperationAdapter {
    type PathLeases;
    type CreatedProcess;

    fn acquire_path_leases(
        &self,
        input: &RestartPathInput,
    ) -> Result<Self::PathLeases, RestartAdapterError>;

    fn revalidate_path_leases(&self, leases: &Self::PathLeases) -> Result<(), RestartAdapterError>;

    fn path_lease_metadata<'a>(&self, leases: &'a Self::PathLeases) -> &'a LeasedPathMetadata;

    fn create_process(
        &self,
        leases: &Self::PathLeases,
        input: &NativeLaunchInput,
    ) -> Result<Self::CreatedProcess, RestartAdapterError>;

    fn created_process_identity(
        &self,
        process: &Self::CreatedProcess,
    ) -> Result<CreatedProcessIdentity, RestartAdapterError>;
}

const MAX_COMMAND_LINE_UNITS: usize = 32_767;

pub(crate) struct ManagedLaunchInput {
    executable: Option<PathBuf>,
    command_line: Option<Vec<u16>>,
    working_directory: Option<PathBuf>,
    environment: Option<Vec<(Vec<u16>, Vec<u16>)>>,
}

impl ManagedLaunchInput {
    pub(crate) fn new(
        executable: Option<PathBuf>,
        command_line: Option<Vec<u16>>,
        working_directory: Option<PathBuf>,
        environment: Option<Vec<(Vec<u16>, Vec<u16>)>>,
    ) -> Self {
        Self {
            executable,
            command_line,
            working_directory,
            environment,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ManagedLaunchError {
    LaunchContextUnavailable,
    InvalidLaunchInput,
    UnsupportedPath,
    PathUnavailable,
    PathChanged,
    CreationFailed,
    PlatformUnsupported,
}

impl fmt::Display for ManagedLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LaunchContextUnavailable => "LaunchContextUnavailable",
            Self::InvalidLaunchInput => "InvalidLaunchInput",
            Self::UnsupportedPath => "UnsupportedPath",
            Self::PathUnavailable => "PathUnavailable",
            Self::PathChanged => "PathChanged",
            Self::CreationFailed => "CreationFailed",
            Self::PlatformUnsupported => "PlatformUnsupported",
        })
    }
}

impl Error for ManagedLaunchError {}

const ATTEMPT_READY: u8 = 0;
const ATTEMPT_EXECUTING: u8 = 1;
const ATTEMPT_CONSUMED: u8 = 2;

pub(super) struct AttemptGuard {
    state: Arc<AtomicU8>,
    consumed: bool,
}

impl AttemptGuard {
    pub(super) fn acquire(state: &Arc<AtomicU8>) -> Result<Self, ProcessRestartError> {
        match state.compare_exchange(
            ATTEMPT_READY,
            ATTEMPT_EXECUTING,
            AtomicOrdering::AcqRel,
            AtomicOrdering::Acquire,
        ) {
            Ok(_) => Ok(Self {
                state: Arc::clone(state),
                consumed: false,
            }),
            Err(ATTEMPT_EXECUTING) => Err(ProcessRestartError::RestartInProgress),
            Err(_) => Err(ProcessRestartError::ReceiptConsumed),
        }
    }
    pub(super) fn consume(mut self) {
        self.state.store(ATTEMPT_CONSUMED, AtomicOrdering::Release);
        self.consumed = true;
    }
}

impl Drop for AttemptGuard {
    fn drop(&mut self) {
        if !self.consumed {
            let _ = self.state.compare_exchange(
                ATTEMPT_EXECUTING,
                ATTEMPT_READY,
                AtomicOrdering::AcqRel,
                AtomicOrdering::Acquire,
            );
        }
    }
}

struct FrozenLaunchInput {
    executable: PathBuf,
    command_line: Vec<u16>,
    working_directory: PathBuf,
    environment: Vec<u16>,
}

pub(crate) struct ProcessLaunchReceipt {
    host_instance_id: HostInstanceId,
    target: ProcessTarget,
    input: FrozenLaunchInput,
    executable_file_identity: (u32, u64),
    attempt: Arc<AtomicU8>,
}
impl fmt::Debug for ProcessLaunchReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProcessLaunchReceipt { .. }")
    }
}

impl ProcessLaunchReceipt {
    pub(crate) fn target(&self) -> &ProcessTarget {
        &self.target
    }
}

pub(crate) enum ManagedLaunchOutcome {
    Created(ProcessLaunchReceipt),
    CreatedIdentityUnknown,
}
impl fmt::Debug for ManagedLaunchOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Created(_) => "Created(..)",
            Self::CreatedIdentityUnknown => "CreatedIdentityUnknown",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessRestartMode {
    Graceful,
    Force,
}

pub(crate) struct ProcessRestartPreview {
    target: ProcessTarget,
    executable: PathBuf,
    command_line: Vec<u16>,
    working_directory: PathBuf,
    mode: ProcessRestartMode,
}
impl fmt::Debug for ProcessRestartPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProcessRestartPreview { .. }")
    }
}

impl ProcessRestartPreview {
    pub(crate) fn target(&self) -> &ProcessTarget {
        &self.target
    }
    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }
    pub(crate) fn command_line(&self) -> &[u16] {
        &self.command_line
    }
    pub(crate) fn working_directory(&self) -> &Path {
        &self.working_directory
    }
    pub(crate) fn mode(&self) -> ProcessRestartMode {
        self.mode
    }
}

pub(crate) struct ProcessRestartPlan {
    preview: ProcessRestartPreview,
    environment: Vec<u16>,
    executable_file_identity: (u32, u64),
    attempt: Arc<AtomicU8>,
}
impl fmt::Debug for ProcessRestartPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProcessRestartPlan { .. }")
    }
}

impl ProcessRestartPlan {
    pub(crate) fn preview(&self) -> &ProcessRestartPreview {
        &self.preview
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ProcessRestartError {
    LaunchContextUnavailable,
    ExpiredReceipt,
    TargetMismatch,
    RestartInProgress,
    ReceiptConsumed,
    Target(ProcessError),
    UnsupportedPath,
    PathUnavailable,
    PathChanged,
    PlatformUnsupported,
}

impl fmt::Display for ProcessRestartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LaunchContextUnavailable => "LaunchContextUnavailable",
            Self::ExpiredReceipt => "ExpiredReceipt",
            Self::TargetMismatch => "TargetMismatch",
            Self::RestartInProgress => "RestartInProgress",
            Self::ReceiptConsumed => "ReceiptConsumed",
            Self::Target(_) => "TargetRejected",
            Self::UnsupportedPath => "UnsupportedPath",
            Self::PathUnavailable => "PathUnavailable",
            Self::PathChanged => "PathChanged",
            Self::PlatformUnsupported => "PlatformUnsupported",
        })
    }
}

impl Error for ProcessRestartError {}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ProcessRestartStopError {
    Admission(ProcessError),
    Operation(ProcessOperationError),
}

pub(crate) enum ProcessRestartCompletion {
    StopFailedLaunchNotAttempted(ProcessRestartStopError),
    OldProcessExitedLaunchFailed,
    OldProcessExitedNewProcessCreated(ProcessLaunchReceipt),
    OldProcessExitedLaunchIdentityUnknown,
}

impl fmt::Debug for ProcessRestartCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StopFailedLaunchNotAttempted(error) => formatter
                .debug_tuple("StopFailedLaunchNotAttempted")
                .field(error)
                .finish(),
            Self::OldProcessExitedLaunchFailed => {
                formatter.write_str("OldProcessExitedLaunchFailed")
            }
            Self::OldProcessExitedNewProcessCreated(_) => {
                formatter.write_str("OldProcessExitedNewProcessCreated(..)")
            }
            Self::OldProcessExitedLaunchIdentityUnknown => {
                formatter.write_str("OldProcessExitedLaunchIdentityUnknown")
            }
        }
    }
}

pub(crate) struct ProcessRestartReport {
    completion: ProcessRestartCompletion,
}

impl ProcessRestartReport {
    pub(crate) fn completion(&self) -> &ProcessRestartCompletion {
        &self.completion
    }
}

impl fmt::Debug for ProcessRestartReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessRestartReport")
            .field("completion", &self.completion)
            .finish()
    }
}

fn valid_utf16(value: &[u16]) -> bool {
    char::decode_utf16(value.iter().copied()).all(|item| item.is_ok())
}

#[cfg(windows)]
fn compare_environment_names(left: &[u16], right: &[u16]) -> Result<Ordering, ()> {
    windows::compare_environment_names(left, right)
}
#[cfg(not(windows))]
fn compare_environment_names(left: &[u16], right: &[u16]) -> Result<Ordering, ()> {
    Ok(String::from_utf16_lossy(left)
        .to_uppercase()
        .cmp(&String::from_utf16_lossy(right).to_uppercase()))
}

fn freeze_input(
    input: ManagedLaunchInput,
) -> Result<(RestartPathInput, NativeLaunchInput), ManagedLaunchError> {
    let executable = input
        .executable
        .ok_or(ManagedLaunchError::LaunchContextUnavailable)?;
    let mut command_line = input
        .command_line
        .ok_or(ManagedLaunchError::LaunchContextUnavailable)?;
    let working_directory = input
        .working_directory
        .ok_or(ManagedLaunchError::LaunchContextUnavailable)?;
    let mut entries = input
        .environment
        .ok_or(ManagedLaunchError::LaunchContextUnavailable)?;
    if executable.as_os_str().is_empty()
        || working_directory.as_os_str().is_empty()
        || command_line.is_empty()
        || command_line.len() >= MAX_COMMAND_LINE_UNITS
        || command_line.contains(&0)
        || !valid_utf16(&command_line)
    {
        return Err(ManagedLaunchError::InvalidLaunchInput);
    }
    command_line.push(0);
    for (name, value) in &entries {
        let drive_name = matches!(name.as_slice(), [61, letter, 58] if (*letter as u8).is_ascii_alphabetic() && *letter <= 0x7f);
        let drive_value = !drive_name
            || matches!(value.as_slice(), [letter, 58, slash, ..]
            if *letter <= 0x7f && (*letter as u8).eq_ignore_ascii_case(&(name[1] as u8)) && (*slash == 92 || *slash == 47));
        if name.is_empty()
            || name.contains(&0)
            || value.contains(&0)
            || !valid_utf16(name)
            || !valid_utf16(value)
            || (!drive_name && name.contains(&(b'=' as u16)))
            || (!drive_name && name.first() == Some(&(b'=' as u16)))
            || !drive_value
        {
            return Err(ManagedLaunchError::InvalidLaunchInput);
        }
    }
    let comparison_failed = Cell::new(false);
    entries.sort_by(|left, right| {
        compare_environment_names(&left.0, &right.0).unwrap_or_else(|_| {
            comparison_failed.set(true);
            Ordering::Equal
        })
    });
    if comparison_failed.get()
        || entries
            .windows(2)
            .any(|pair| compare_environment_names(&pair[0].0, &pair[1].0) != Ok(Ordering::Less))
    {
        return Err(ManagedLaunchError::InvalidLaunchInput);
    }
    let mut environment = Vec::new();
    for (name, value) in entries {
        environment.extend(name);
        environment.push(b'=' as u16);
        environment.extend(value);
        environment.push(0);
    }
    environment.push(0);
    if environment.len() == 1 {
        environment.push(0);
    }
    if environment.len() > MAX_COMMAND_LINE_UNITS {
        return Err(ManagedLaunchError::InvalidLaunchInput);
    }
    Ok((
        RestartPathInput::new(executable, working_directory),
        NativeLaunchInput::new(command_line, environment),
    ))
}

fn map_adapter_error(error: RestartAdapterError) -> ManagedLaunchError {
    match error {
        RestartAdapterError::InvalidInput => ManagedLaunchError::InvalidLaunchInput,
        RestartAdapterError::UnsupportedPath => ManagedLaunchError::UnsupportedPath,
        RestartAdapterError::PathUnavailable => ManagedLaunchError::PathUnavailable,
        RestartAdapterError::PathChanged => ManagedLaunchError::PathChanged,
        RestartAdapterError::CreationFailed => ManagedLaunchError::CreationFailed,
        RestartAdapterError::IdentityUnavailable => ManagedLaunchError::CreationFailed,
    }
}

fn map_restart_adapter_error(error: RestartAdapterError) -> ProcessRestartError {
    match error {
        RestartAdapterError::UnsupportedPath => ProcessRestartError::UnsupportedPath,
        RestartAdapterError::PathUnavailable => ProcessRestartError::PathUnavailable,
        RestartAdapterError::PathChanged => ProcessRestartError::PathChanged,
        RestartAdapterError::InvalidInput
        | RestartAdapterError::CreationFailed
        | RestartAdapterError::IdentityUnavailable => ProcessRestartError::PathUnavailable,
    }
}

impl<A: ProcessRestartAdapter> CoreAuthority<A> {
    pub(super) fn launch_managed(
        &self,
        input: ManagedLaunchInput,
    ) -> Result<ManagedLaunchOutcome, ManagedLaunchError> {
        let (paths, native) = freeze_input(input)?;
        let leases = self
            .adapter()
            .acquire_path_leases(&paths)
            .map_err(map_adapter_error)?;
        self.adapter()
            .revalidate_path_leases(&leases)
            .map_err(map_adapter_error)?;
        let metadata = self.adapter().path_lease_metadata(&leases).clone();
        let process = self
            .adapter()
            .create_process(&leases, &native)
            .map_err(map_adapter_error)?;
        let identity = match self.adapter().created_process_identity(&process) {
            Ok(identity) => identity,
            Err(RestartAdapterError::IdentityUnavailable) => {
                return Ok(ManagedLaunchOutcome::CreatedIdentityUnknown)
            }
            Err(_) => return Ok(ManagedLaunchOutcome::CreatedIdentityUnknown),
        };
        let target = match ProcessTarget::new(
            self.host_instance_id(),
            identity.pid(),
            identity.creation_time(),
        ) {
            Ok(target) => target,
            Err(_) => return Ok(ManagedLaunchOutcome::CreatedIdentityUnknown),
        };
        Ok(ManagedLaunchOutcome::Created(ProcessLaunchReceipt {
            host_instance_id: self.host_instance_id(),
            target,
            input: FrozenLaunchInput {
                executable: metadata.executable().to_owned(),
                command_line: native.command_line()[..native.command_line().len() - 1].to_vec(),
                working_directory: metadata.working_directory().to_owned(),
                environment: native.environment().to_vec(),
            },
            executable_file_identity: metadata.executable_file_identity(),
            attempt: Arc::new(AtomicU8::new(ATTEMPT_READY)),
        }))
    }

    pub(super) fn plan_restart(
        &self,
        target: &ProcessTarget,
        receipt: Option<&ProcessLaunchReceipt>,
        mode: ProcessRestartMode,
    ) -> Result<ProcessRestartPlan, ProcessRestartError> {
        let receipt = receipt.ok_or(ProcessRestartError::LaunchContextUnavailable)?;
        if receipt.host_instance_id != self.host_instance_id() {
            return Err(ProcessRestartError::ExpiredReceipt);
        }
        if receipt.target != *target {
            return Err(ProcessRestartError::TargetMismatch);
        }
        match receipt.attempt.load(AtomicOrdering::Acquire) {
            ATTEMPT_READY => {}
            ATTEMPT_EXECUTING => return Err(ProcessRestartError::RestartInProgress),
            _ => return Err(ProcessRestartError::ReceiptConsumed),
        }
        let admission = self
            .admit(target, ProcessIntent::Restart)
            .map_err(ProcessRestartError::Target)?;
        drop(admission);
        Ok(ProcessRestartPlan {
            preview: ProcessRestartPreview {
                target: *target,
                executable: receipt.input.executable.clone(),
                command_line: receipt.input.command_line.clone(),
                working_directory: receipt.input.working_directory.clone(),
                mode,
            },
            environment: receipt.input.environment.clone(),
            executable_file_identity: receipt.executable_file_identity,
            attempt: Arc::clone(&receipt.attempt),
        })
    }

    pub(super) fn execute_restart_with_clock<C: MonotonicClock>(
        &self,
        plan: ProcessRestartPlan,
        clock: &C,
    ) -> Result<ProcessRestartReport, ProcessRestartError> {
        let target = plan.preview.target;
        if target.host_instance_id() != self.host_instance_id() {
            return Err(ProcessRestartError::ExpiredReceipt);
        }
        let attempt = AttemptGuard::acquire(&plan.attempt)?;

        let paths = RestartPathInput::new(
            plan.preview.executable.clone(),
            plan.preview.working_directory.clone(),
        );
        let leases = self
            .adapter()
            .acquire_path_leases(&paths)
            .map_err(map_restart_adapter_error)?;
        self.adapter()
            .revalidate_path_leases(&leases)
            .map_err(map_restart_adapter_error)?;
        if self
            .adapter()
            .path_lease_metadata(&leases)
            .executable_file_identity()
            != plan.executable_file_identity
        {
            return Err(ProcessRestartError::PathChanged);
        }

        let policy = self
            .admit(&target, ProcessIntent::Restart)
            .map_err(ProcessRestartError::Target)?;
        let stop_intent = match plan.preview.mode {
            ProcessRestartMode::Graceful => ProcessIntent::Terminate,
            ProcessRestartMode::Force => ProcessIntent::ForceTerminate,
        };
        let stop_admission = match self.admit(&target, stop_intent) {
            Ok(admission) => admission,
            Err(error) => {
                drop(policy);
                return Ok(ProcessRestartReport {
                    completion: ProcessRestartCompletion::StopFailedLaunchNotAttempted(
                        ProcessRestartStopError::Admission(error),
                    ),
                });
            }
        };
        drop(policy);
        let stop = match plan.preview.mode {
            ProcessRestartMode::Graceful => self.terminate_with_clock(stop_admission, clock),
            ProcessRestartMode::Force => self.force_terminate_with_clock(stop_admission, clock),
        };
        if let Err(error) = stop {
            return Ok(ProcessRestartReport {
                completion: ProcessRestartCompletion::StopFailedLaunchNotAttempted(
                    ProcessRestartStopError::Operation(error),
                ),
            });
        }

        let native = NativeLaunchInput::new(
            {
                let mut command_line = plan.preview.command_line;
                command_line.push(0);
                command_line
            },
            plan.environment,
        );
        attempt.consume();
        let process = match self.adapter().create_process(&leases, &native) {
            Ok(process) => process,
            Err(_) => {
                return Ok(ProcessRestartReport {
                    completion: ProcessRestartCompletion::OldProcessExitedLaunchFailed,
                });
            }
        };
        let identity = match self.adapter().created_process_identity(&process) {
            Ok(identity) => identity,
            Err(_) => {
                return Ok(ProcessRestartReport {
                    completion: ProcessRestartCompletion::OldProcessExitedLaunchIdentityUnknown,
                });
            }
        };
        let replacement_target = match ProcessTarget::new(
            self.host_instance_id(),
            identity.pid(),
            identity.creation_time(),
        ) {
            Ok(target) => target,
            Err(_) => {
                return Ok(ProcessRestartReport {
                    completion: ProcessRestartCompletion::OldProcessExitedLaunchIdentityUnknown,
                });
            }
        };
        let metadata = self.adapter().path_lease_metadata(&leases);
        Ok(ProcessRestartReport {
            completion: ProcessRestartCompletion::OldProcessExitedNewProcessCreated(
                ProcessLaunchReceipt {
                    host_instance_id: self.host_instance_id(),
                    target: replacement_target,
                    input: FrozenLaunchInput {
                        executable: metadata.executable().to_owned(),
                        command_line: native.command_line()[..native.command_line().len() - 1]
                            .to_vec(),
                        working_directory: metadata.working_directory().to_owned(),
                        environment: native.environment().to_vec(),
                    },
                    executable_file_identity: metadata.executable_file_identity(),
                    attempt: Arc::new(AtomicU8::new(ATTEMPT_READY)),
                },
            ),
        })
    }
}
