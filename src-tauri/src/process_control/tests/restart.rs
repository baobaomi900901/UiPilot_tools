#[cfg(windows)]
mod windows;

use std::{cell::Cell, path::PathBuf, rc::Rc};

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
        CreatedProcessIdentity, LeasedPathMetadata, NativeLaunchInput, ProcessRestartAdapter,
        RestartAdapterError, RestartPathInput,
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
    lease_drops: Rc<Cell<usize>>,
    process_drops: Rc<Cell<usize>>,
    failure: Option<RestartAdapterError>,
    metadata: LeasedPathMetadata,
}

impl ProcessPlatformAdapter for FakeRestartAdapter {
    type Handle = ();

    fn enumerate_candidates(
        &self,
    ) -> Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure> {
        unimplemented!()
    }
    fn open_observation(
        &self,
        _pid: u32,
        _access: ObservationAccess,
    ) -> Result<Self::Handle, ObservationFailure> {
        unimplemented!()
    }
    fn creation_time(&self, _handle: &Self::Handle) -> Result<u64, ObservationFailure> {
        unimplemented!()
    }
    fn liveness(
        &self,
        _handle: &Self::Handle,
    ) -> Result<super::super::core::Liveness, ObservationFailure> {
        unimplemented!()
    }
    fn observation(
        &self,
        _handle: &Self::Handle,
    ) -> Result<ProcessObservation, ObservationFailure> {
        unimplemented!()
    }
    fn reenumerate_name(&self, _pid: u32) -> Result<Option<String>, ObservationFailure> {
        unimplemented!()
    }
    fn process_names_equivalent(
        &self,
        _left: &std::ffi::OsStr,
        _right: &std::ffi::OsStr,
    ) -> Result<bool, ObservationFailure> {
        unimplemented!()
    }
    fn protection(&self, _handle: &Self::Handle) -> Result<ProcessProtection, ObservationFailure> {
        unimplemented!()
    }
}

impl ProcessOperationAdapter for FakeRestartAdapter {
    type WindowToken = ();
    fn open_operation(
        &self,
        _pid: u32,
        _access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure> {
        unimplemented!()
    }
    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure> {
        unimplemented!()
    }
    fn window_facts(&self, _window: Self::WindowToken) -> Result<WindowFacts, WindowFailure> {
        unimplemented!()
    }
    fn post_close(&self, _window: Self::WindowToken) -> Result<(), WindowFailure> {
        unimplemented!()
    }
    fn terminate_process(
        &self,
        _handle: &Self::Handle,
        _exit_code: u32,
    ) -> Result<(), OperationFailure> {
        unimplemented!()
    }
    fn wait_for_exit(
        &self,
        _handle: &Self::Handle,
        _timeout: std::time::Duration,
    ) -> Result<ExitWait, OperationFailure> {
        unimplemented!()
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
        self.failure
            .map_or_else(|| CreatedProcessIdentity::new(42, 99), Err)
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
        lease_drops: lease_drops.clone(),
        process_drops: process_drops.clone(),
        failure: None,
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
        lease_drops: drops.clone(),
        process_drops: Rc::new(Cell::new(0)),
        failure: None,
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
