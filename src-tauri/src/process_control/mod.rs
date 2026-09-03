mod core;
mod operation;

#[cfg(windows)]
mod windows;

pub(crate) type HostInstanceId = core::HostInstanceId;
pub(crate) type ProcessAuthorityInitError = core::ProcessAuthorityInitError;
pub(crate) type ProcessError = core::ProcessError;
pub(crate) type ProcessOmissions = core::ProcessOmissions;
pub(crate) type ProcessRecord = core::ProcessRecord;
pub(crate) type ProcessIntent = core::ProcessIntent;
pub(crate) type ProcessOperationError = operation::ProcessOperationError;
pub(crate) type ProcessSnapshot = core::ProcessSnapshot;
pub(crate) type ProcessTarget = core::ProcessTarget;

#[cfg(windows)]
pub(crate) struct ProcessAuthority {
    inner: core::CoreAuthority<windows::WindowsProcessAdapter>,
}

#[cfg(windows)]
impl ProcessAuthority {
    pub(crate) fn new() -> Result<Self, ProcessAuthorityInitError> {
        Ok(Self {
            inner: windows::create_authority()?,
        })
    }

    pub(crate) fn snapshot(&self) -> Result<ProcessSnapshot, ProcessError> {
        self.inner.snapshot()
    }

    pub(crate) fn admit(
        &self,
        target: &ProcessTarget,
        intent: ProcessIntent,
    ) -> Result<ProcessAdmission, ProcessError> {
        self.inner
            .admit(target, intent)
            .map(|inner| ProcessAdmission { _inner: inner })
    }
}

#[cfg(windows)]
pub(crate) struct ProcessAdmission {
    _inner: core::CoreAdmission<windows::OwnedProcessHandle>,
}

#[cfg(not(windows))]
pub(crate) struct ProcessAuthority;

#[cfg(not(windows))]
impl ProcessAuthority {
    pub(crate) fn new() -> Result<Self, ProcessAuthorityInitError> {
        Ok(Self)
    }

    pub(crate) fn snapshot(&self) -> Result<ProcessSnapshot, ProcessError> {
        Err(ProcessError::PlatformUnsupported)
    }

    pub(crate) fn admit(
        &self,
        _target: &ProcessTarget,
        _intent: ProcessIntent,
    ) -> Result<ProcessAdmission, ProcessError> {
        Err(ProcessError::PlatformUnsupported)
    }
}

#[cfg(not(windows))]
pub(crate) enum ProcessAdmission {}

#[cfg(test)]
mod tests;
