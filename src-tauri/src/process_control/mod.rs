mod batch;
mod core;
mod operation;
mod restart;

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
pub(crate) type ProcessBatchError = batch::ProcessBatchError;
pub(crate) type ProcessBatchOmissions = batch::ProcessBatchOmissions;
pub(crate) type ProcessBatchOperation = batch::ProcessBatchOperation;
pub(crate) type ProcessBatchPlan = batch::ProcessBatchPlan;
pub(crate) type ProcessBatchPreview = batch::ProcessBatchPreview;
pub(crate) type ProcessBatchScope = batch::ProcessBatchScope;
pub(crate) type ProcessBatchSelectionCompleteness = batch::ProcessBatchSelectionCompleteness;
pub(crate) type ProcessBatchTargetError = batch::ProcessBatchTargetError;
pub(crate) type ProcessBatchTargetOutcome = batch::ProcessBatchTargetOutcome;
pub(crate) type ProcessBatchTargetResult = batch::ProcessBatchTargetResult;
pub(crate) type ProcessBatchReport = batch::ProcessBatchReport;
pub(crate) type ProcessBatchCompletion = batch::ProcessBatchCompletion;
pub(crate) type ManagedLaunchInput = restart::ManagedLaunchInput;
pub(crate) type ManagedLaunchOutcome = restart::ManagedLaunchOutcome;
pub(crate) type ManagedLaunchError = restart::ManagedLaunchError;
pub(crate) type ProcessLaunchReceipt = restart::ProcessLaunchReceipt;
pub(crate) type ProcessRestartMode = restart::ProcessRestartMode;
pub(crate) type ProcessRestartPlan = restart::ProcessRestartPlan;
pub(crate) type ProcessRestartPreview = restart::ProcessRestartPreview;
pub(crate) type ProcessRestartError = restart::ProcessRestartError;

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

    pub(crate) fn plan_batch(
        &self,
        root: &ProcessTarget,
        scope: ProcessBatchScope,
        operation: ProcessBatchOperation,
    ) -> Result<ProcessBatchPlan, ProcessBatchError> {
        self.inner.plan_batch(root, scope, operation)
    }

    pub(crate) fn execute_batch(
        &self,
        plan: ProcessBatchPlan,
    ) -> Result<ProcessBatchReport, ProcessBatchError> {
        self.inner
            .execute_batch_with_clock(plan, &operation::SystemMonotonicClock)
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

    pub(crate) fn terminate(
        &self,
        admission: ProcessAdmission,
    ) -> Result<(), ProcessOperationError> {
        self.inner
            .terminate_with_clock(admission._inner, &operation::SystemMonotonicClock)
    }

    pub(crate) fn force_terminate(
        &self,
        admission: ProcessAdmission,
    ) -> Result<(), ProcessOperationError> {
        self.inner
            .force_terminate_with_clock(admission._inner, &operation::SystemMonotonicClock)
    }

    pub(crate) fn launch_managed(
        &self,
        input: ManagedLaunchInput,
    ) -> Result<ManagedLaunchOutcome, ManagedLaunchError> {
        self.inner.launch_managed(input)
    }

    pub(crate) fn plan_restart(
        &self,
        target: &ProcessTarget,
        receipt: Option<&ProcessLaunchReceipt>,
        mode: ProcessRestartMode,
    ) -> Result<ProcessRestartPlan, ProcessRestartError> {
        self.inner.plan_restart(target, receipt, mode)
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

    pub(crate) fn plan_batch(
        &self,
        _root: &ProcessTarget,
        _scope: ProcessBatchScope,
        _operation: ProcessBatchOperation,
    ) -> Result<ProcessBatchPlan, ProcessBatchError> {
        Err(ProcessBatchError::PlatformUnsupported)
    }

    pub(crate) fn execute_batch(
        &self,
        _plan: ProcessBatchPlan,
    ) -> Result<ProcessBatchReport, ProcessBatchError> {
        Err(ProcessBatchError::PlatformUnsupported)
    }

    pub(crate) fn admit(
        &self,
        _target: &ProcessTarget,
        _intent: ProcessIntent,
    ) -> Result<ProcessAdmission, ProcessError> {
        Err(ProcessError::PlatformUnsupported)
    }

    pub(crate) fn terminate(
        &self,
        _admission: ProcessAdmission,
    ) -> Result<(), ProcessOperationError> {
        Err(ProcessOperationError::PlatformUnsupported)
    }

    pub(crate) fn force_terminate(
        &self,
        _admission: ProcessAdmission,
    ) -> Result<(), ProcessOperationError> {
        Err(ProcessOperationError::PlatformUnsupported)
    }

    pub(crate) fn launch_managed(
        &self,
        _input: ManagedLaunchInput,
    ) -> Result<ManagedLaunchOutcome, ManagedLaunchError> {
        Err(ManagedLaunchError::PlatformUnsupported)
    }

    pub(crate) fn plan_restart(
        &self,
        _target: &ProcessTarget,
        _receipt: Option<&ProcessLaunchReceipt>,
        _mode: ProcessRestartMode,
    ) -> Result<ProcessRestartPlan, ProcessRestartError> {
        Err(ProcessRestartError::PlatformUnsupported)
    }
}

#[cfg(not(windows))]
pub(crate) enum ProcessAdmission {}

#[cfg(test)]
mod tests;
