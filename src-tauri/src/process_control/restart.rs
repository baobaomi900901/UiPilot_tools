use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use super::operation::ProcessOperationAdapter;

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
