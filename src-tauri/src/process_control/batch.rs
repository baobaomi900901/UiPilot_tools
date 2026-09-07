use std::{error::Error, fmt, path::Path};

use super::core::{
    CoreAuthority, ExpansionSnapshot, HostInstanceId, Liveness, ProcessError, ProcessIntent,
    ProcessPlatformAdapter, ProcessRecord, ProcessTarget,
};

#[allow(dead_code)]
const BATCH_TARGET_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchScope {
    Tree,
    SameName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchOperation {
    Terminate,
    ForceTerminate,
}

impl ProcessBatchOperation {
    fn intent(self) -> ProcessIntent {
        match self {
            Self::Terminate => ProcessIntent::Terminate,
            Self::ForceTerminate => ProcessIntent::ForceTerminate,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchSelectionCompleteness {
    CompleteSelection,
    IncompleteSelection,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ProcessBatchOmissions {
    exited_or_changed: u64,
    inspection_denied: u64,
    identity_unavailable: u64,
    ancestry_unverified: u64,
}

impl ProcessBatchOmissions {
    fn new() -> Self {
        Self {
            exited_or_changed: 0,
            inspection_denied: 0,
            identity_unavailable: 0,
            ancestry_unverified: 0,
        }
    }

    pub(crate) fn exited_or_changed(&self) -> u64 {
        self.exited_or_changed
    }

    pub(crate) fn inspection_denied(&self) -> u64 {
        self.inspection_denied
    }

    pub(crate) fn identity_unavailable(&self) -> u64 {
        self.identity_unavailable
    }

    pub(crate) fn ancestry_unverified(&self) -> u64 {
        self.ancestry_unverified
    }

    pub(crate) fn total(&self) -> u64 {
        self.exited_or_changed
            .checked_add(self.inspection_denied)
            .and_then(|total| total.checked_add(self.identity_unavailable))
            .and_then(|total| total.checked_add(self.ancestry_unverified))
            .expect("ProcessBatchOmissions is constructed through checked increments")
    }

    fn increment(&mut self, reason: BatchOmissionReason) -> Result<(), ProcessBatchError> {
        self.total()
            .checked_add(1)
            .ok_or(ProcessBatchError::CatalogUnavailable)?;
        let count = match reason {
            BatchOmissionReason::ExitedOrChanged => &mut self.exited_or_changed,
            BatchOmissionReason::InspectionDenied => &mut self.inspection_denied,
            BatchOmissionReason::IdentityUnavailable => &mut self.identity_unavailable,
            BatchOmissionReason::AncestryUnverified => &mut self.ancestry_unverified,
        };
        *count = count
            .checked_add(1)
            .ok_or(ProcessBatchError::CatalogUnavailable)?;
        Ok(())
    }
}

enum BatchOmissionReason {
    ExitedOrChanged,
    InspectionDenied,
    IdentityUnavailable,
    AncestryUnverified,
}

pub(crate) struct ProcessBatchPlan {
    authority_id: HostInstanceId,
    scope: ProcessBatchScope,
    operation: ProcessBatchOperation,
    targets: Vec<ProcessTarget>,
    previews: Vec<ProcessBatchPreview>,
    omissions: ProcessBatchOmissions,
}

impl ProcessBatchPlan {
    pub(crate) fn scope(&self) -> ProcessBatchScope {
        self.scope
    }

    pub(crate) fn operation(&self) -> ProcessBatchOperation {
        self.operation
    }

    pub(crate) fn previews(&self) -> &[ProcessBatchPreview] {
        &self.previews
    }

    pub(crate) fn omissions(&self) -> &ProcessBatchOmissions {
        &self.omissions
    }

    pub(crate) fn selection_completeness(&self) -> ProcessBatchSelectionCompleteness {
        if self.omissions.total() == 0 {
            ProcessBatchSelectionCompleteness::CompleteSelection
        } else {
            ProcessBatchSelectionCompleteness::IncompleteSelection
        }
    }
}

pub(crate) struct ProcessBatchPreview {
    ordinal: usize,
    pid: u32,
    name: String,
    executable_path: Option<std::path::PathBuf>,
    depth: Option<usize>,
}

impl ProcessBatchPreview {
    pub(crate) fn ordinal(&self) -> usize {
        self.ordinal
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn executable_path(&self) -> Option<&Path> {
        self.executable_path.as_deref()
    }

    pub(crate) fn depth(&self) -> Option<usize> {
        self.depth
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchError {
    InvalidTarget,
    ExpiredTarget,
    TargetExited,
    TargetChanged,
    InspectionDenied,
    ProtectedTarget,
    CatalogUnavailable,
    TargetSetTooLarge,
    ExpiredPlan,
    PlatformUnsupported,
}

impl fmt::Display for ProcessBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTarget => "InvalidTarget",
            Self::ExpiredTarget => "ExpiredTarget",
            Self::TargetExited => "TargetExited",
            Self::TargetChanged => "TargetChanged",
            Self::InspectionDenied => "InspectionDenied",
            Self::ProtectedTarget => "ProtectedTarget",
            Self::CatalogUnavailable => "CatalogUnavailable",
            Self::TargetSetTooLarge => "TargetSetTooLarge",
            Self::ExpiredPlan => "ExpiredPlan",
            Self::PlatformUnsupported => "PlatformUnsupported",
        })
    }
}

impl Error for ProcessBatchError {}

pub(super) struct PreparedBatchPlanning {
    authority_id: HostInstanceId,
    root: ProcessRecord,
    snapshot: ExpansionSnapshot,
    operation: ProcessBatchOperation,
}

impl PreparedBatchPlanning {
    pub(super) fn authority_id(&self) -> HostInstanceId {
        self.authority_id
    }

    pub(super) fn root(&self) -> &ProcessRecord {
        &self.root
    }

    pub(super) fn snapshot(&self) -> &ExpansionSnapshot {
        &self.snapshot
    }

    pub(super) fn operation(&self) -> ProcessBatchOperation {
        self.operation
    }
}

impl<A: ProcessPlatformAdapter> CoreAuthority<A> {
    pub(super) fn prepare_batch_planning(
        &self,
        root: &ProcessTarget,
        operation: ProcessBatchOperation,
    ) -> Result<PreparedBatchPlanning, ProcessBatchError> {
        let admission = self
            .admit(root, operation.intent())
            .map_err(ProcessBatchError::from)?;
        let snapshot = self.expansion_snapshot().map_err(ProcessBatchError::from)?;
        let root_record = correlate_root(self, root, &admission, &snapshot)?;
        drop(admission);
        Ok(PreparedBatchPlanning {
            authority_id: self.host_instance_id(),
            root: root_record,
            snapshot,
            operation,
        })
    }
}

fn correlate_root<A: ProcessPlatformAdapter>(
    authority: &CoreAuthority<A>,
    root: &ProcessTarget,
    admission: &super::core::CoreAdmission<A::Handle>,
    snapshot: &ExpansionSnapshot,
) -> Result<ProcessRecord, ProcessBatchError> {
    let matching = snapshot
        .entries()
        .iter()
        .filter(|entry| entry.pid_hint() == root.pid())
        .collect::<Vec<_>>();

    if let Some(record) = matching
        .iter()
        .filter_map(|entry| entry.record())
        .find(|record| record.target().creation_time() == root.creation_time())
    {
        return Ok(record.clone());
    }
    if matching.iter().any(|entry| entry.record().is_some()) {
        return Err(ProcessBatchError::TargetChanged);
    }
    if !matching.is_empty() {
        return Err(ProcessBatchError::InspectionDenied);
    }

    match authority.admission_liveness(admission) {
        Ok(Liveness::Exited) => Err(ProcessBatchError::TargetExited),
        Err(_) => Err(ProcessBatchError::InspectionDenied),
        Ok(Liveness::Alive) => Err(ProcessBatchError::CatalogUnavailable),
    }
}

impl From<ProcessError> for ProcessBatchError {
    fn from(error: ProcessError) -> Self {
        match error {
            ProcessError::InvalidTarget => Self::InvalidTarget,
            ProcessError::ExpiredTarget => Self::ExpiredTarget,
            ProcessError::TargetExited => Self::TargetExited,
            ProcessError::TargetChanged => Self::TargetChanged,
            ProcessError::InspectionDenied => Self::InspectionDenied,
            ProcessError::ProtectedTarget => Self::ProtectedTarget,
            ProcessError::CatalogUnavailable => Self::CatalogUnavailable,
            ProcessError::PlatformUnsupported => Self::PlatformUnsupported,
        }
    }
}
