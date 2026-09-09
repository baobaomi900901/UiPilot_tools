#[cfg(windows)]
mod windows;

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};

use super::{FakeHandle, FakeProcessAdapter};

use super::super::{
    core::{
        EnumerationFailure, ObservationAccess, ObservationFailure, ProcessCandidate,
        ProcessObservation, ProcessPlatformAdapter, ProcessProtection,
    },
    operation::{
        ExitWait, OperationAccess, OperationFailure, ProcessOperationAdapter, WindowCandidate,
        WindowFacts, WindowFailure,
    },
    restart::{
        AttemptGuard, CreatedProcessIdentity, LeasedPathMetadata, ManagedLaunchError,
        ManagedLaunchInput, ManagedLaunchOutcome, NativeLaunchInput, ProcessRestartAdapter,
        ProcessRestartError, ProcessRestartMode, RestartAdapterError, RestartPathInput,
    },
};

#[derive(Clone)]
struct DropResource {
    drops: Rc<Cell<usize>>,
    metadata: Option<LeasedPathMetadata>,
}

impl Drop for DropResource {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

struct FakeRestartAdapter {
    process: FakeProcessAdapter,
    lease_drops: Rc<Cell<usize>>,
    process_drops: Rc<Cell<usize>>,
    failure: Option<RestartAdapterError>,
    identity_failure: bool,
    created_inputs: Rc<RefCell<Vec<NativeLaunchInput>>>,
    metadata: LeasedPathMetadata,
}

impl ProcessPlatformAdapter for FakeRestartAdapter {
    type Handle = FakeHandle;

    fn enumerate_candidates(
        &self,
    ) -> Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure> {
        self.process.enumerate_candidates()
    }
    fn open_observation(
        &self,
        _pid: u32,
        _access: ObservationAccess,
    ) -> Result<Self::Handle, ObservationFailure> {
        self.process.open_observation(_pid, _access)
    }
    fn creation_time(&self, _handle: &Self::Handle) -> Result<u64, ObservationFailure> {
        self.process.creation_time(_handle)
    }
    fn liveness(
        &self,
        _handle: &Self::Handle,
    ) -> Result<super::super::core::Liveness, ObservationFailure> {
        self.process.liveness(_handle)
    }
    fn observation(
        &self,
        _handle: &Self::Handle,
    ) -> Result<ProcessObservation, ObservationFailure> {
        self.process.observation(_handle)
    }
    fn reenumerate_name(&self, _pid: u32) -> Result<Option<String>, ObservationFailure> {
        self.process.reenumerate_name(_pid)
    }
    fn process_names_equivalent(
        &self,
        _left: &std::ffi::OsStr,
        _right: &std::ffi::OsStr,
    ) -> Result<bool, ObservationFailure> {
        self.process.process_names_equivalent(_left, _right)
    }
    fn protection(&self, _handle: &Self::Handle) -> Result<ProcessProtection, ObservationFailure> {
        self.process.protection(_handle)
    }
}

impl ProcessOperationAdapter for FakeRestartAdapter {
    type WindowToken = u64;
    fn open_operation(
        &self,
        _pid: u32,
        _access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure> {
        self.process.open_operation(_pid, _access)
    }
    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure> {
        self.process.enumerate_top_level_windows()
    }
    fn window_facts(&self, _window: Self::WindowToken) -> Result<WindowFacts, WindowFailure> {
        self.process.window_facts(_window)
    }
    fn post_close(&self, _window: Self::WindowToken) -> Result<(), WindowFailure> {
        self.process.post_close(_window)
    }
    fn terminate_process(
        &self,
        _handle: &Self::Handle,
        _exit_code: u32,
    ) -> Result<(), OperationFailure> {
        self.process.terminate_process(_handle, _exit_code)
    }
    fn wait_for_exit(
        &self,
        _handle: &Self::Handle,
        _timeout: std::time::Duration,
    ) -> Result<ExitWait, OperationFailure> {
        self.process.wait_for_exit(_handle, _timeout)
    }
}

impl ProcessRestartAdapter for FakeRestartAdapter {
    type PathLeases = DropResource;
    type CreatedProcess = DropResource;

    fn acquire_path_leases(
        &self,
        _input: &RestartPathInput,
    ) -> Result<Self::PathLeases, RestartAdapterError> {
        self.failure.map_or_else(
            || {
                Ok(DropResource {
                    drops: self.lease_drops.clone(),
                    metadata: Some(self.metadata.clone()),
                })
            },
            Err,
        )
    }
    fn revalidate_path_leases(
        &self,
        _leases: &Self::PathLeases,
    ) -> Result<(), RestartAdapterError> {
        self.failure.map_or(Ok(()), Err)
    }
    fn path_lease_metadata<'a>(&self, leases: &'a Self::PathLeases) -> &'a LeasedPathMetadata {
        leases.metadata.as_ref().unwrap()
    }
    fn create_process(
        &self,
        _leases: &Self::PathLeases,
        _input: &NativeLaunchInput,
    ) -> Result<Self::CreatedProcess, RestartAdapterError> {
        self.created_inputs.borrow_mut().push(_input.clone());
        self.failure.map_or_else(
            || {
                Ok(DropResource {
                    drops: self.process_drops.clone(),
                    metadata: None,
                })
            },
            Err,
        )
    }
    fn created_process_identity(
        &self,
        _process: &Self::CreatedProcess,
    ) -> Result<CreatedProcessIdentity, RestartAdapterError> {
        if self.identity_failure {
            Err(RestartAdapterError::IdentityUnavailable)
        } else {
            self.failure
                .map_or_else(|| CreatedProcessIdentity::new(42, 99), Err)
        }
    }
}

fn paths() -> RestartPathInput {
    RestartPathInput::new(
        PathBuf::from(r"C:\owned\child.exe"),
        PathBuf::from(r"C:\owned\cwd"),
    )
}

#[test]
fn restart_adapter_resources_release_exactly_once_after_identity_failure() {
    let lease_drops = Rc::new(Cell::new(0));
    let process_drops = Rc::new(Cell::new(0));
    let mut adapter = FakeRestartAdapter {
        process: FakeProcessAdapter::default(),
        lease_drops: lease_drops.clone(),
        process_drops: process_drops.clone(),
        failure: None,
        identity_failure: false,
        created_inputs: Rc::new(RefCell::new(Vec::new())),
        metadata: LeasedPathMetadata::new(
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\owned\child.exe"),
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\owned\cwd"),
            7,
            11,
        ),
    };
    let leases = adapter.acquire_path_leases(&paths()).unwrap();
    adapter.revalidate_path_leases(&leases).unwrap();
    let process = adapter
        .create_process(&leases, &NativeLaunchInput::new(vec![0], vec![0, 0]))
        .unwrap();
    adapter.failure = Some(RestartAdapterError::IdentityUnavailable);
    assert_eq!(
        adapter.created_process_identity(&process),
        Err(RestartAdapterError::IdentityUnavailable)
    );
    assert_eq!((lease_drops.get(), process_drops.get()), (0, 0));
    drop(process);
    drop(leases);
    assert_eq!((lease_drops.get(), process_drops.get()), (1, 1));
}

#[test]
fn restart_adapter_error_display_is_stable_and_redacted() {
    assert_eq!(
        RestartAdapterError::UnsupportedPath.to_string(),
        "UnsupportedPath"
    );
    assert_eq!(
        RestartAdapterError::CreationFailed.to_string(),
        "CreationFailed"
    );
}

#[test]
fn restart_adapter_metadata_and_revalidation_mismatch_use_the_opaque_lease() {
    let drops = Rc::new(Cell::new(0));
    let mut adapter = FakeRestartAdapter {
        process: FakeProcessAdapter::default(),
        lease_drops: drops.clone(),
        process_drops: Rc::new(Cell::new(0)),
        failure: None,
        identity_failure: false,
        created_inputs: Rc::new(RefCell::new(Vec::new())),
        metadata: LeasedPathMetadata::new(
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\owned\child.exe"),
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\owned\cwd"),
            17,
            23,
        ),
    };
    let leases = adapter.acquire_path_leases(&paths()).unwrap();
    assert_eq!(
        adapter
            .path_lease_metadata(&leases)
            .executable_file_identity(),
        (17, 23)
    );
    adapter.failure = Some(RestartAdapterError::PathChanged);
    assert_eq!(
        adapter.revalidate_path_leases(&leases),
        Err(RestartAdapterError::PathChanged)
    );
    drop(leases);
    assert_eq!(drops.get(), 1);
}

fn managed_adapter(identity_failure: bool) -> FakeRestartAdapter {
    let process = FakeProcessAdapter::default().with_process(
        42,
        super::FakeProcessScript {
            open: vec![Ok(()); 4].into(),
            creation_times: vec![Ok(99); 4].into(),
            liveness: vec![Ok(super::Liveness::Alive); 4].into(),
            protections: vec![Ok(super::ProcessProtection::new(false, false)); 4].into(),
            ..super::FakeProcessScript::default()
        },
    );
    FakeRestartAdapter {
        process,
        lease_drops: Rc::new(Cell::new(0)),
        process_drops: Rc::new(Cell::new(0)),
        failure: None,
        identity_failure,
        created_inputs: Rc::new(RefCell::new(Vec::new())),
        metadata: LeasedPathMetadata::new(
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\owned\child.exe"),
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\owned\cwd"),
            7,
            11,
        ),
    }
}

fn managed_input(command_line: Option<Vec<u16>>) -> ManagedLaunchInput {
    ManagedLaunchInput::new(
        Some(PathBuf::from(r"C:\owned\child.exe")),
        command_line,
        Some(PathBuf::from(r"C:\owned\cwd")),
        Some(vec![
            ("EMPTY".encode_utf16().collect(), vec![]),
            (
                "UNICODE".encode_utf16().collect(),
                "雪".encode_utf16().collect(),
            ),
        ]),
    )
}

fn managed_authority(adapter: FakeRestartAdapter) -> super::CoreAuthority<FakeRestartAdapter> {
    let host = super::HostInstanceId::new(super::HOST).unwrap();
    super::CoreAuthority::new(
        host,
        super::ProcessTarget::new(host, super::SELF_PID, super::SELF_CREATION_TIME).unwrap(),
        adapter,
    )
}

#[test]
fn restart_receipt_rejects_unknown_and_malformed_context_before_creation() {
    let authority = managed_authority(managed_adapter(false));
    assert_eq!(
        authority
            .launch_managed(managed_input(Some(vec![])))
            .unwrap_err(),
        ManagedLaunchError::InvalidLaunchInput
    );
    assert_eq!(
        authority.launch_managed(managed_input(None)).unwrap_err(),
        ManagedLaunchError::LaunchContextUnavailable
    );
    assert_eq!(
        authority
            .launch_managed(managed_input(Some(vec![b'a' as u16, 0, b'b' as u16])))
            .unwrap_err(),
        ManagedLaunchError::InvalidLaunchInput
    );
    assert_eq!(
        authority
            .launch_managed(managed_input(Some(vec![0xD800])))
            .unwrap_err(),
        ManagedLaunchError::InvalidLaunchInput
    );
}

#[test]
fn restart_receipt_empty_arguments_are_known_and_plan_freezes_exact_input() {
    let adapter = managed_adapter(false);
    let captured = Rc::clone(&adapter.created_inputs);
    let authority = managed_authority(adapter);
    let receipt = match authority
        .launch_managed(managed_input(Some("child.exe".encode_utf16().collect())))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(value) => value,
        _ => panic!("identity expected"),
    };
    let plan = authority
        .plan_restart(
            receipt.target(),
            Some(&receipt),
            ProcessRestartMode::Graceful,
        )
        .unwrap();
    assert_eq!(
        plan.preview().command_line(),
        "child.exe".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(plan.preview().mode(), ProcessRestartMode::Graceful);
    assert!(plan
        .preview()
        .executable()
        .to_string_lossy()
        .starts_with(r"\\?\Volume{"));
    let inputs = captured.borrow();
    assert_eq!(
        inputs[0].command_line(),
        "child.exe\0".encode_utf16().collect::<Vec<_>>()
    );
    let expected_environment = "EMPTY=\0UNICODE=雪\0\0".encode_utf16().collect::<Vec<_>>();
    assert_eq!(inputs[0].environment(), expected_environment);
}

#[test]
fn restart_receipt_environment_drive_entries_are_ordered_and_invalid_entries_reject() {
    let adapter = managed_adapter(false);
    let captured = Rc::clone(&adapter.created_inputs);
    let authority = managed_authority(adapter);
    let input = ManagedLaunchInput::new(
        Some(PathBuf::from(r"C:\owned\child.exe")),
        Some("child.exe \"a b\" 雪".encode_utf16().collect()),
        Some(PathBuf::from(r"C:\owned\cwd")),
        Some(vec![
            (
                "z".encode_utf16().collect(),
                "last".encode_utf16().collect(),
            ),
            (
                "=C:".encode_utf16().collect(),
                r"C:\owned\cwd".encode_utf16().collect(),
            ),
            ("A".encode_utf16().collect(), vec![]),
        ]),
    );
    assert!(matches!(
        authority.launch_managed(input),
        Ok(ManagedLaunchOutcome::Created(_))
    ));
    let environment = String::from_utf16(captured.borrow()[0].environment()).unwrap();
    assert_eq!(environment, "=C:=C:\\owned\\cwd\0A=\0z=last\0\0");
    let invalid = ManagedLaunchInput::new(
        Some(PathBuf::from(r"C:\owned\child.exe")),
        Some("child.exe".encode_utf16().collect()),
        Some(PathBuf::from(r"C:\owned\cwd")),
        Some(vec![("BAD=NAME".encode_utf16().collect(), vec![])]),
    );
    assert_eq!(
        authority.launch_managed(invalid).unwrap_err(),
        ManagedLaunchError::InvalidLaunchInput
    );
    let duplicate = ManagedLaunchInput::new(
        Some(PathBuf::from(r"C:\owned\child.exe")),
        Some("child.exe".encode_utf16().collect()),
        Some(PathBuf::from(r"C:\owned\cwd")),
        Some(vec![
            ("Ö".encode_utf16().collect(), vec![]),
            ("ö".encode_utf16().collect(), vec![]),
        ]),
    );
    assert_eq!(
        authority.launch_managed(duplicate).unwrap_err(),
        ManagedLaunchError::InvalidLaunchInput
    );
    let malformed_drive = ManagedLaunchInput::new(
        Some(PathBuf::from(r"C:\owned\child.exe")),
        Some("child.exe".encode_utf16().collect()),
        Some(PathBuf::from(r"C:\owned\cwd")),
        Some(vec![(
            "=C:".encode_utf16().collect(),
            vec![0x143, 58, 92, 120],
        )]),
    );
    assert_eq!(
        authority.launch_managed(malformed_drive).unwrap_err(),
        ManagedLaunchError::InvalidLaunchInput
    );
    let empty_environment = ManagedLaunchInput::new(
        Some(PathBuf::from(r"C:\owned\child.exe")),
        Some("child.exe".encode_utf16().collect()),
        Some(PathBuf::from(r"C:\owned\cwd")),
        Some(vec![]),
    );
    assert!(matches!(
        authority.launch_managed(empty_environment),
        Ok(ManagedLaunchOutcome::Created(_))
    ));
    assert_eq!(captured.borrow()[1].environment(), &[0, 0]);
}

#[test]
fn restart_receipt_identity_failure_reports_created_unknown_without_stop_or_retry() {
    let adapter = managed_adapter(true);
    let process = adapter.process.clone();
    let authority = managed_authority(adapter);
    assert!(matches!(
        authority
            .launch_managed(managed_input(Some(
                "child.exe \"space value\" 雪".encode_utf16().collect()
            )))
            .unwrap(),
        ManagedLaunchOutcome::CreatedIdentityUnknown
    ));
    assert_eq!(process.terminate_calls(), 0);
}

#[test]
fn restart_receipt_plans_share_ready_state_and_cancellation_allows_replan() {
    let authority = managed_authority(managed_adapter(false));
    let receipt = match authority
        .launch_managed(managed_input(Some("child.exe".encode_utf16().collect())))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(value) => value,
        _ => unreachable!(),
    };
    let first = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    let second = authority
        .plan_restart(
            receipt.target(),
            Some(&receipt),
            ProcessRestartMode::Graceful,
        )
        .unwrap();
    drop(first);
    drop(second);
    assert!(authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .is_ok());
    assert_eq!(
        authority
            .plan_restart(receipt.target(), None, ProcessRestartMode::Force)
            .unwrap_err(),
        ProcessRestartError::LaunchContextUnavailable
    );
}

#[test]
fn restart_receipt_plan_rejects_exact_target_and_host_mismatch_before_admission() {
    let authority = managed_authority(managed_adapter(false));
    let receipt = match authority
        .launch_managed(managed_input(Some("child.exe".encode_utf16().collect())))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(value) => value,
        _ => unreachable!(),
    };
    let wrong_target = super::ProcessTarget::from_parts_for_test(super::HOST, 43, 99);
    assert_eq!(
        authority
            .plan_restart(&wrong_target, Some(&receipt), ProcessRestartMode::Force)
            .unwrap_err(),
        ProcessRestartError::TargetMismatch
    );
    let other_host = super::HostInstanceId::new([0x55; 16]).unwrap();
    let other = super::CoreAuthority::new(
        other_host,
        super::ProcessTarget::new(other_host, super::SELF_PID, super::SELF_CREATION_TIME).unwrap(),
        managed_adapter(false),
    );
    assert_eq!(
        other
            .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
            .unwrap_err(),
        ProcessRestartError::ExpiredReceipt
    );
}

#[test]
fn restart_receipt_attempt_guard_releases_on_drop_and_unwind_then_consumes_once() {
    let state = Arc::new(AtomicU8::new(0));
    {
        let _guard = AttemptGuard::acquire(&state).unwrap();
        assert_eq!(state.load(Ordering::Acquire), 1);
    }
    assert_eq!(state.load(Ordering::Acquire), 0);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe({
        let state = Arc::clone(&state);
        move || {
            let _guard = AttemptGuard::acquire(&state).unwrap();
            panic!("owned unwind");
        }
    }));
    assert!(unwind.is_err());
    assert_eq!(state.load(Ordering::Acquire), 0);
    AttemptGuard::acquire(&state).unwrap().consume();
    assert_eq!(state.load(Ordering::Acquire), 2);
    assert!(matches!(
        AttemptGuard::acquire(&state),
        Err(ProcessRestartError::ReceiptConsumed)
    ));
}

#[test]
fn restart_receipt_contract_is_opaque_nonclone_and_debug_redacted() {
    let source = include_str!("../restart.rs");
    for declaration in [
        "pub(crate) struct ProcessLaunchReceipt",
        "pub(crate) struct ProcessRestartPlan",
    ] {
        let prefix = source.split(declaration).next().unwrap();
        let attributes = prefix.rsplit("\n\n").next().unwrap_or_default();
        assert!(!attributes.contains("Clone"));
        assert!(!attributes.contains("Copy"));
        assert!(!attributes.contains("Serialize"));
        assert!(!attributes.contains("Deserialize"));
    }
    let authority = managed_authority(managed_adapter(false));
    let receipt = match authority
        .launch_managed(managed_input(Some(
            "child.exe secret".encode_utf16().collect(),
        )))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(value) => value,
        _ => unreachable!(),
    };
    assert_eq!(format!("{receipt:?}"), "ProcessLaunchReceipt { .. }");
}
