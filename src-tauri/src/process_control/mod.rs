mod core;

pub(crate) type HostInstanceId = core::HostInstanceId;
pub(crate) type ProcessAuthorityInitError = core::ProcessAuthorityInitError;
pub(crate) type ProcessError = core::ProcessError;
pub(crate) type ProcessOmissions = core::ProcessOmissions;
pub(crate) type ProcessRecord = core::ProcessRecord;
pub(crate) type ProcessSnapshot = core::ProcessSnapshot;
pub(crate) type ProcessTarget = core::ProcessTarget;

#[cfg(test)]
mod tests;
