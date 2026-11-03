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
        ProcessRestartCompletion, ProcessRestartError, ProcessRestartMode, RestartAdapterError,
        RestartPathInput,
    },
};

#[derive(Clone)]
struct DropResource {
    drops: Rc<Cell<usize>>,
    metadata: Option<LeasedPathMetadata>,
}

type FakeCallback = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

impl Drop for DropResource {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

struct FakeRestartAdapter {
    process: FakeProcessAdapter,
    lease_drops: Rc<Cell<usize>>,
    process_drops: Rc<Cell<usize>>,
    failure: Rc<Cell<Option<RestartAdapterError>>>,
    create_failure: Rc<Cell<Option<RestartAdapterError>>>,
    identity_failure: Rc<Cell<bool>>,
    lease_acquisitions: Rc<Cell<usize>>,
    create_calls: Rc<Cell<usize>>,
    on_acquire: FakeCallback,
    on_create: FakeCallback,
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
        self.lease_acquisitions
            .set(self.lease_acquisitions.get() + 1);
        if let Some(callback) = self.on_acquire.borrow_mut().take() {
            callback();
        }
        self.failure.get().map_or_else(
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
        self.failure.get().map_or(Ok(()), Err)
    }
    fn path_lease_metadata<'a>(&self, leases: &'a Self::PathLeases) -> &'a LeasedPathMetadata {
        leases.metadata.as_ref().unwrap()
    }
    fn create_process(
        &self,
        _leases: &Self::PathLeases,
        _input: &NativeLaunchInput,
    ) -> Result<Self::CreatedProcess, RestartAdapterError> {
        self.create_calls.set(self.create_calls.get() + 1);
        if let Some(callback) = self.on_create.borrow_mut().take() {
            callback();
        }
        self.created_inputs.borrow_mut().push(_input.clone());
        self.create_failure
            .get()
            .or(self.failure.get())
            .map_or_else(
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
        if self.identity_failure.get() {
            Err(RestartAdapterError::IdentityUnavailable)
        } else {
            self.failure
                .get()
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
    let adapter = FakeRestartAdapter {
        process: FakeProcessAdapter::default(),
        lease_drops: lease_drops.clone(),
        process_drops: process_drops.clone(),
        failure: Rc::new(Cell::new(None)),
        create_failure: Rc::new(Cell::new(None)),
        identity_failure: Rc::new(Cell::new(false)),
        lease_acquisitions: Rc::new(Cell::new(0)),
        create_calls: Rc::new(Cell::new(0)),
        on_acquire: Rc::new(RefCell::new(None)),
        on_create: Rc::new(RefCell::new(None)),
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
    adapter
        .failure
        .set(Some(RestartAdapterError::IdentityUnavailable));
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
    let adapter = FakeRestartAdapter {
        process: FakeProcessAdapter::default(),
        lease_drops: drops.clone(),
        process_drops: Rc::new(Cell::new(0)),
        failure: Rc::new(Cell::new(None)),
        create_failure: Rc::new(Cell::new(None)),
        identity_failure: Rc::new(Cell::new(false)),
        lease_acquisitions: Rc::new(Cell::new(0)),
        create_calls: Rc::new(Cell::new(0)),
        on_acquire: Rc::new(RefCell::new(None)),
        on_create: Rc::new(RefCell::new(None)),
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
    adapter.failure.set(Some(RestartAdapterError::PathChanged));
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
        failure: Rc::new(Cell::new(None)),
        create_failure: Rc::new(Cell::new(None)),
        identity_failure: Rc::new(Cell::new(identity_failure)),
        lease_acquisitions: Rc::new(Cell::new(0)),
        create_calls: Rc::new(Cell::new(0)),
        on_acquire: Rc::new(RefCell::new(None)),
        on_create: Rc::new(RefCell::new(None)),
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
        let attributes = restart_declaration_attributes(source, declaration);
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

fn restart_declaration_attributes(source: &str, declaration: &str) -> String {
    let source = source.replace("\r\n", "\n");
    let (prefix, _) = source
        .split_once(declaration)
        .unwrap_or_else(|| panic!("missing {declaration}"));
    prefix.rsplit("\n\n").next().unwrap_or_default().to_owned()
}

#[test]
fn restart_receipt_attribute_guard_supports_lf_and_crlf() {
    let declaration = "pub(crate) struct ProcessLaunchReceipt";
    for ending in ["\n", "\r\n"] {
        let source = format!("#[derive(Clone)]\nstruct Unrelated;\n\n{declaration} {{}}")
            .replace('\n', ending);
        assert!(restart_declaration_attributes(&source, declaration).is_empty());
        for forbidden in ["Clone", "Copy", "Serialize", "Deserialize"] {
            let source =
                format!("#[derive({forbidden})]\n{declaration} {{}}").replace('\n', ending);
            assert!(restart_declaration_attributes(&source, declaration).contains(forbidden));
        }
    }
}

#[test]
fn restart_execution_graceful_stops_then_creates_one_replacement() {
    let mut adapter = managed_adapter(false);
    adapter.process = adapter
        .process
        .clone()
        .with_window_enumeration(Ok(vec![WindowCandidate::new(1, 42, true, false)]))
        .with_window_facts(1, vec![Ok(WindowFacts::new(42, true, false))])
        .with_post_close_outcomes(1, vec![Ok(())]);
    adapter
        .process
        .state
        .borrow_mut()
        .processes
        .get(&42)
        .unwrap()
        .borrow_mut()
        .script
        .operation_open = vec![Ok(())].into();
    adapter
        .process
        .state
        .borrow_mut()
        .processes
        .get(&42)
        .unwrap()
        .borrow_mut()
        .script
        .waits = vec![Ok(ExitWait::Exited)].into();
    let process = adapter.process.clone();
    let created = Rc::clone(&adapter.created_inputs);
    let authority = managed_authority(adapter);
    let receipt = match authority
        .launch_managed(managed_input(Some("child.exe".encode_utf16().collect())))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(receipt) => receipt,
        _ => unreachable!(),
    };
    let plan = authority
        .plan_restart(
            receipt.target(),
            Some(&receipt),
            ProcessRestartMode::Graceful,
        )
        .unwrap();

    let report = authority
        .execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        )
        .unwrap();

    if !matches!(
        report.completion(),
        ProcessRestartCompletion::OldProcessExitedNewProcessCreated(_)
    ) {
        panic!("unexpected winning completion: {:?}", report.completion());
    }
    assert_eq!(
        process.requested_operation_access(),
        vec![OperationAccess::GRACEFUL]
    );
    assert_eq!(process.terminate_calls(), 0);
    assert_eq!(created.borrow().len(), 2);
}

fn configure_force_success(adapter: &FakeRestartAdapter) {
    let state = adapter.process.state.borrow_mut();
    let mut process = state.processes.get(&42).unwrap().borrow_mut();
    process.script.operation_open = vec![Ok(())].into();
    process.script.terminations = vec![Ok(())].into();
    process.script.waits = vec![Ok(ExitWait::TimedOut), Ok(ExitWait::Exited)].into();
}

fn launched_receipt(
    authority: &super::CoreAuthority<FakeRestartAdapter>,
) -> super::super::restart::ProcessLaunchReceipt {
    match authority
        .launch_managed(managed_input(Some("child.exe".encode_utf16().collect())))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(receipt) => receipt,
        _ => unreachable!(),
    }
}

#[test]
fn restart_execution_force_routes_once_and_replacement_receipt_matches_new_identity() {
    let adapter = managed_adapter(false);
    configure_force_success(&adapter);
    let process = adapter.process.clone();
    let authority = managed_authority(adapter);
    let receipt = launched_receipt(&authority);
    let plan = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();

    let report = authority
        .execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        )
        .unwrap();

    let replacement = match report.completion() {
        ProcessRestartCompletion::OldProcessExitedNewProcessCreated(receipt) => receipt,
        completion => panic!("unexpected completion: {completion:?}"),
    };
    assert_eq!(
        (
            replacement.target().pid(),
            replacement.target().creation_time()
        ),
        (42, 99)
    );
    assert_eq!(
        process.requested_operation_access(),
        vec![OperationAccess::FORCE]
    );
    assert_eq!(process.terminate_calls(), 1);
}

#[test]
fn restart_execution_changed_frozen_executable_rejects_before_stop_and_releases_attempt() {
    let adapter = managed_adapter(false);
    let process = adapter.process.clone();
    let authority = managed_authority(adapter);
    let receipt = launched_receipt(&authority);
    let plan = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    // Planning freezes (7, 11); execution observes a different executable file.
    // Use the adapter's deterministic revalidation seam.
    authority
        .adapter()
        .failure
        .set(Some(RestartAdapterError::PathChanged));

    assert!(matches!(
        authority.execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now())
        ),
        Err(ProcessRestartError::PathChanged)
    ));
    assert_eq!(process.terminate_calls(), 0);
    assert!(authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .is_ok());
}

#[test]
fn restart_execution_stop_failure_never_launches_and_receipt_can_be_replanned() {
    let adapter = managed_adapter(false);
    adapter
        .process
        .state
        .borrow_mut()
        .processes
        .get(&42)
        .unwrap()
        .borrow_mut()
        .script
        .operation_open = vec![Err(OperationFailure::ExitedOrChanged)].into();
    let creates = Rc::clone(&adapter.create_calls);
    let authority = managed_authority(adapter);
    let receipt = launched_receipt(&authority);
    let plan = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();

    let report = authority
        .execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        )
        .unwrap();

    assert!(matches!(
        report.completion(),
        ProcessRestartCompletion::StopFailedLaunchNotAttempted(
            super::super::restart::ProcessRestartStopError::Operation(
                super::super::operation::ProcessOperationError::TargetExited
            )
        )
    ));
    assert_eq!(creates.get(), 1);
    assert!(authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .is_ok());
}

#[test]
fn restart_execution_stop_admission_failure_never_stops_or_launches_and_releases_resources() {
    for mode in [ProcessRestartMode::Graceful, ProcessRestartMode::Force] {
        let adapter = managed_adapter(false);
        let process = adapter.process.clone();
        let creates = Rc::clone(&adapter.create_calls);
        let lease_drops = Rc::clone(&adapter.lease_drops);
        let authority = managed_authority(adapter);
        let receipt = launched_receipt(&authority);
        let plan = authority
            .plan_restart(receipt.target(), Some(&receipt), mode)
            .unwrap();
        // Execution's Restart policy admission succeeds; only its fresh stop
        // admission fails. The final success permits a later explicit replan.
        process
            .state
            .borrow_mut()
            .processes
            .get(&42)
            .unwrap()
            .borrow_mut()
            .script
            .open = vec![Ok(()), Err(ObservationFailure::AccessDenied), Ok(())].into();
        let opened = process.opened();
        let dropped = process.dropped();
        let observations = process.requested_access().len();
        let leases_released = lease_drops.get();
        let initial_creates = creates.get();

        let report = authority
            .execute_restart_with_clock(
                plan,
                &super::FakeMonotonicClock::at(std::time::Instant::now()),
            )
            .unwrap();

        assert!(matches!(
            report.completion(),
            ProcessRestartCompletion::StopFailedLaunchNotAttempted(
                super::super::restart::ProcessRestartStopError::Admission(
                    super::super::core::ProcessError::InspectionDenied
                )
            )
        ));
        assert_eq!(process.requested_access().len(), observations + 2);
        assert_eq!(process.opened(), opened + 1);
        assert_eq!(process.dropped(), dropped + 1);
        assert_eq!(process.opened(), process.dropped());
        assert_eq!(lease_drops.get(), leases_released + 1);
        assert!(process.requested_operation_access().is_empty());
        assert_eq!(process.post_close_calls(), 0);
        assert_eq!(process.terminate_calls(), 0);
        assert_eq!(creates.get(), initial_creates);
        assert!(authority
            .plan_restart(receipt.target(), Some(&receipt), mode)
            .is_ok());
    }
}

#[test]
fn restart_execution_creation_failure_is_irreversible_and_consumes_old_receipt() {
    let adapter = managed_adapter(false);
    configure_force_success(&adapter);
    let authority = managed_authority(adapter);
    let receipt = launched_receipt(&authority);
    // Initial creation must succeed; fail only the restart creation.
    authority
        .adapter()
        .create_failure
        .set(Some(RestartAdapterError::CreationFailed));
    let plan = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();

    let report = authority
        .execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        )
        .unwrap();

    assert!(matches!(
        report.completion(),
        ProcessRestartCompletion::OldProcessExitedLaunchFailed
    ));
    assert_eq!(
        authority
            .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
            .unwrap_err(),
        ProcessRestartError::ReceiptConsumed
    );
}

#[test]
fn restart_execution_foreign_plan_rejects_without_claim_or_adapter_calls() {
    let owner_adapter = managed_adapter(false);
    configure_force_success(&owner_adapter);
    let owner = managed_authority(owner_adapter);
    let receipt = launched_receipt(&owner);
    let plan = owner
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    let foreign_host = super::HostInstanceId::new([0x44; 16]).unwrap();
    let foreign_adapter = managed_adapter(false);
    let foreign_leases = Rc::clone(&foreign_adapter.lease_acquisitions);
    let foreign_creates = Rc::clone(&foreign_adapter.create_calls);
    let foreign = super::CoreAuthority::new(
        foreign_host,
        super::ProcessTarget::new(foreign_host, super::SELF_PID, super::SELF_CREATION_TIME)
            .unwrap(),
        foreign_adapter,
    );

    assert!(matches!(
        foreign.execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now())
        ),
        Err(ProcessRestartError::ExpiredReceipt)
    ));
    assert_eq!((foreign_leases.get(), foreign_creates.get()), (0, 0));

    let retry = owner
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    drop(retry);
}

#[test]
fn restart_execution_reentrant_overlap_has_one_winner_and_zero_secondary_effects() {
    let adapter = managed_adapter(false);
    configure_force_success(&adapter);
    {
        let state = adapter.process.state.borrow_mut();
        let mut process = state.processes.get(&42).unwrap().borrow_mut();
        process.script.creation_times.push_back(Ok(99));
        process
            .script
            .liveness
            .push_back(Ok(super::Liveness::Alive));
        process
            .script
            .protections
            .push_back(Ok(ProcessProtection::new(false, false)));
    }
    let leases = Rc::clone(&adapter.lease_acquisitions);
    let creates = Rc::clone(&adapter.create_calls);
    let hook = Rc::clone(&adapter.on_acquire);
    let authority = Rc::new(managed_authority(adapter));
    let receipt = launched_receipt(&authority);
    let winning = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    let overlapping = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    let nested_result = Rc::new(Cell::new(false));
    let nested_result_for_hook = Rc::clone(&nested_result);
    let authority_for_hook = Rc::clone(&authority);
    hook.borrow_mut().replace(Box::new(move || {
        let result = authority_for_hook.execute_restart_with_clock(
            overlapping,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        );
        nested_result_for_hook.set(match result {
            Err(ProcessRestartError::RestartInProgress) => true,
            _ => panic!("overlapping execution was not rejected"),
        });
    }));

    let report = authority
        .execute_restart_with_clock(
            winning,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        )
        .unwrap();

    assert!(nested_result.get());
    if !matches!(
        report.completion(),
        ProcessRestartCompletion::OldProcessExitedNewProcessCreated(_)
    ) {
        panic!("unexpected winning completion: {:?}", report.completion());
    }
    assert_eq!((leases.get(), creates.get()), (2, 2));
}

#[test]
fn restart_execution_identity_unknown_consumes_receipt_without_retry() {
    let adapter = managed_adapter(false);
    configure_force_success(&adapter);
    let authority = managed_authority(adapter);
    let receipt = launched_receipt(&authority);
    authority.adapter().identity_failure.set(true);
    let plan = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();

    let report = authority
        .execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        )
        .unwrap();

    assert!(matches!(
        report.completion(),
        ProcessRestartCompletion::OldProcessExitedLaunchIdentityUnknown
    ));
    assert_eq!(
        authority
            .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
            .unwrap_err(),
        ProcessRestartError::ReceiptConsumed
    );
}

#[test]
fn restart_execution_unwind_during_create_keeps_receipt_consumed() {
    let adapter = managed_adapter(false);
    configure_force_success(&adapter);
    let authority = managed_authority(adapter);
    let receipt = launched_receipt(&authority);
    authority
        .adapter()
        .on_create
        .borrow_mut()
        .replace(Box::new(|| panic!("owned post-consume unwind")));
    let plan = authority
        .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
        .unwrap();

    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = authority.execute_restart_with_clock(
            plan,
            &super::FakeMonotonicClock::at(std::time::Instant::now()),
        );
    }));

    assert!(unwind.is_err());
    assert_eq!(
        authority
            .plan_restart(receipt.target(), Some(&receipt), ProcessRestartMode::Force)
            .unwrap_err(),
        ProcessRestartError::ReceiptConsumed
    );
}
