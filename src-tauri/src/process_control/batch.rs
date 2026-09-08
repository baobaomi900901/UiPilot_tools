use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::Path,
    time::Duration,
};

use super::core::{
    CoreAuthority, ExpansionEntry, ExpansionOmissionReason, ExpansionSnapshot, HostInstanceId,
    Liveness, ProcessError, ProcessIntent, ProcessPlatformAdapter, ProcessRecord, ProcessTarget,
};
use super::operation::{MonotonicClock, ProcessOperationAdapter, ProcessOperationError};

const BATCH_TARGET_LIMIT: usize = 256;
const BATCH_SCHEDULING_DEADLINE: Duration = Duration::from_secs(30);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchTargetError {
    TargetChanged,
    InspectionDenied,
    ProtectedTarget,
    OperationAccessDenied,
    GracefulCloseUnavailable,
    GracefulCloseTimedOut,
    OperationUnavailable,
    TerminationPending,
    CompletionUnknown,
}

impl fmt::Display for ProcessBatchTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TargetChanged => "TargetChanged",
            Self::InspectionDenied => "InspectionDenied",
            Self::ProtectedTarget => "ProtectedTarget",
            Self::OperationAccessDenied => "OperationAccessDenied",
            Self::GracefulCloseUnavailable => "GracefulCloseUnavailable",
            Self::GracefulCloseTimedOut => "GracefulCloseTimedOut",
            Self::OperationUnavailable => "OperationUnavailable",
            Self::TerminationPending => "TerminationPending",
            Self::CompletionUnknown => "CompletionUnknown",
        })
    }
}

impl Error for ProcessBatchTargetError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchTargetOutcome {
    ObservedExited,
    AlreadyExited,
    Failed(ProcessBatchTargetError),
    NotAttemptedBatchDeadline,
}

pub(crate) struct ProcessBatchTargetResult {
    preview: ProcessBatchPreview,
    outcome: ProcessBatchTargetOutcome,
}

impl ProcessBatchTargetResult {
    pub(crate) fn preview(&self) -> &ProcessBatchPreview {
        &self.preview
    }

    pub(crate) fn outcome(&self) -> ProcessBatchTargetOutcome {
        self.outcome
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessBatchCompletion {
    Complete,
    Partial,
}

pub(crate) struct ProcessBatchReport {
    scope: ProcessBatchScope,
    operation: ProcessBatchOperation,
    results: Vec<ProcessBatchTargetResult>,
    omissions: ProcessBatchOmissions,
    observed_exited: u64,
    already_exited: u64,
    failed: u64,
    not_attempted: u64,
}

impl ProcessBatchReport {
    pub(crate) fn scope(&self) -> ProcessBatchScope {
        self.scope
    }

    pub(crate) fn operation(&self) -> ProcessBatchOperation {
        self.operation
    }

    pub(crate) fn results(&self) -> &[ProcessBatchTargetResult] {
        &self.results
    }

    pub(crate) fn omissions(&self) -> &ProcessBatchOmissions {
        &self.omissions
    }

    pub(crate) fn observed_exited(&self) -> u64 {
        self.observed_exited
    }

    pub(crate) fn already_exited(&self) -> u64 {
        self.already_exited
    }

    pub(crate) fn failed(&self) -> u64 {
        self.failed
    }

    pub(crate) fn not_attempted(&self) -> u64 {
        self.not_attempted
    }

    pub(crate) fn completion(&self) -> ProcessBatchCompletion {
        if self.failed == 0
            && self.not_attempted == 0
            && self.omissions.total() == 0
            && self.results.iter().all(|result| {
                matches!(
                    result.outcome,
                    ProcessBatchTargetOutcome::ObservedExited
                        | ProcessBatchTargetOutcome::AlreadyExited
                )
            })
        {
            ProcessBatchCompletion::Complete
        } else {
            ProcessBatchCompletion::Partial
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
    pub(super) fn plan_batch(
        &self,
        root: &ProcessTarget,
        scope: ProcessBatchScope,
        operation: ProcessBatchOperation,
    ) -> Result<ProcessBatchPlan, ProcessBatchError> {
        let prepared = self.prepare_batch_planning(root, operation)?;
        match scope {
            ProcessBatchScope::Tree => build_tree_plan(prepared),
            ProcessBatchScope::SameName => build_same_name_plan(self, prepared),
        }
    }

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

impl<A: ProcessOperationAdapter> CoreAuthority<A> {
    pub(super) fn execute_batch_with_clock<C: MonotonicClock>(
        &self,
        plan: ProcessBatchPlan,
        clock: &C,
    ) -> Result<ProcessBatchReport, ProcessBatchError> {
        if plan.authority_id != self.host_instance_id() {
            return Err(ProcessBatchError::ExpiredPlan);
        }

        let deadline = clock
            .now()
            .checked_add(BATCH_SCHEDULING_DEADLINE)
            .expect("thirty-second monotonic deadline is representable");
        let mut results = Vec::with_capacity(plan.targets.len());
        let mut entries = plan.targets.into_iter().zip(plan.previews);

        while let Some((target, preview)) = entries.next() {
            if clock.now() >= deadline {
                results.push(ProcessBatchTargetResult {
                    preview,
                    outcome: ProcessBatchTargetOutcome::NotAttemptedBatchDeadline,
                });
                results.extend(entries.map(|(_, preview)| ProcessBatchTargetResult {
                    preview,
                    outcome: ProcessBatchTargetOutcome::NotAttemptedBatchDeadline,
                }));
                break;
            }

            let admission = match self.admit(&target, plan.operation.intent()) {
                Ok(admission) => admission,
                Err(error) => {
                    results.push(ProcessBatchTargetResult {
                        preview,
                        outcome: map_admission_outcome(error),
                    });
                    continue;
                }
            };

            if clock.now() >= deadline {
                drop(admission);
                results.push(ProcessBatchTargetResult {
                    preview,
                    outcome: ProcessBatchTargetOutcome::NotAttemptedBatchDeadline,
                });
                results.extend(entries.map(|(_, preview)| ProcessBatchTargetResult {
                    preview,
                    outcome: ProcessBatchTargetOutcome::NotAttemptedBatchDeadline,
                }));
                break;
            }

            let outcome = match plan.operation {
                ProcessBatchOperation::Terminate => self.terminate_with_clock(admission, clock),
                ProcessBatchOperation::ForceTerminate => {
                    self.force_terminate_with_clock(admission, clock)
                }
            }
            .map_or_else(map_operation_outcome, |()| {
                ProcessBatchTargetOutcome::ObservedExited
            });
            results.push(ProcessBatchTargetResult { preview, outcome });
        }

        Ok(assemble_report(
            plan.scope,
            plan.operation,
            results,
            plan.omissions,
        ))
    }
}

pub(super) fn map_admission_outcome(error: ProcessError) -> ProcessBatchTargetOutcome {
    match error {
        ProcessError::TargetExited => ProcessBatchTargetOutcome::AlreadyExited,
        ProcessError::TargetChanged => failed(ProcessBatchTargetError::TargetChanged),
        ProcessError::InspectionDenied => failed(ProcessBatchTargetError::InspectionDenied),
        ProcessError::ProtectedTarget => failed(ProcessBatchTargetError::ProtectedTarget),
        ProcessError::InvalidTarget
        | ProcessError::ExpiredTarget
        | ProcessError::CatalogUnavailable
        | ProcessError::PlatformUnsupported => {
            failed(ProcessBatchTargetError::OperationUnavailable)
        }
    }
}

pub(super) fn map_operation_outcome(error: ProcessOperationError) -> ProcessBatchTargetOutcome {
    match error {
        ProcessOperationError::TargetExited => ProcessBatchTargetOutcome::AlreadyExited,
        ProcessOperationError::TargetChanged => failed(ProcessBatchTargetError::TargetChanged),
        ProcessOperationError::InspectionDenied => {
            failed(ProcessBatchTargetError::InspectionDenied)
        }
        ProcessOperationError::ProtectedTarget => failed(ProcessBatchTargetError::ProtectedTarget),
        ProcessOperationError::OperationAccessDenied => {
            failed(ProcessBatchTargetError::OperationAccessDenied)
        }
        ProcessOperationError::GracefulCloseUnavailable => {
            failed(ProcessBatchTargetError::GracefulCloseUnavailable)
        }
        ProcessOperationError::GracefulCloseTimedOut => {
            failed(ProcessBatchTargetError::GracefulCloseTimedOut)
        }
        ProcessOperationError::TerminationPending => {
            failed(ProcessBatchTargetError::TerminationPending)
        }
        ProcessOperationError::CompletionUnknown => {
            failed(ProcessBatchTargetError::CompletionUnknown)
        }
        ProcessOperationError::IntentMismatch
        | ProcessOperationError::OperationUnavailable
        | ProcessOperationError::PlatformUnsupported => {
            failed(ProcessBatchTargetError::OperationUnavailable)
        }
    }
}

fn failed(error: ProcessBatchTargetError) -> ProcessBatchTargetOutcome {
    ProcessBatchTargetOutcome::Failed(error)
}

fn assemble_report(
    scope: ProcessBatchScope,
    operation: ProcessBatchOperation,
    results: Vec<ProcessBatchTargetResult>,
    omissions: ProcessBatchOmissions,
) -> ProcessBatchReport {
    let mut observed_exited = 0_u64;
    let mut already_exited = 0_u64;
    let mut failed = 0_u64;
    let mut not_attempted = 0_u64;
    for result in &results {
        let count = match result.outcome {
            ProcessBatchTargetOutcome::ObservedExited => &mut observed_exited,
            ProcessBatchTargetOutcome::AlreadyExited => &mut already_exited,
            ProcessBatchTargetOutcome::Failed(_) => &mut failed,
            ProcessBatchTargetOutcome::NotAttemptedBatchDeadline => &mut not_attempted,
        };
        *count = count
            .checked_add(1)
            .expect("batch reports contain at most BATCH_TARGET_LIMIT results");
    }
    ProcessBatchReport {
        scope,
        operation,
        results,
        omissions,
        observed_exited,
        already_exited,
        failed,
        not_attempted,
    }
}

pub(super) fn build_tree_plan(
    prepared: PreparedBatchPlanning,
) -> Result<ProcessBatchPlan, ProcessBatchError> {
    let root_pid = prepared.root.pid();
    let mut by_pid = BTreeMap::<u32, Vec<&ExpansionEntry>>::new();
    for entry in prepared.snapshot.entries() {
        by_pid.entry(entry.pid_hint()).or_default().push(entry);
    }

    let mut omissions = ProcessBatchOmissions::new();
    let mut selected = vec![(prepared.root.clone(), 0_usize)];
    for entry in prepared
        .snapshot
        .entries()
        .iter()
        .filter(|entry| entry.pid_hint() != root_pid)
    {
        match resolve_tree_candidate(entry, prepared.root(), &by_pid) {
            TreeCandidateResolution::Selected(depth) => {
                let record = entry
                    .record()
                    .expect("selected tree candidates have reliable records");
                selected.push((record.clone(), depth));
            }
            TreeCandidateResolution::Omitted(reason) => omissions.increment(reason)?,
            TreeCandidateResolution::OutsideScope => {}
        }
    }

    if selected.len() > BATCH_TARGET_LIMIT {
        return Err(ProcessBatchError::TargetSetTooLarge);
    }
    selected.sort_by(|(left_record, left_depth), (right_record, right_depth)| {
        right_depth
            .cmp(left_depth)
            .then_with(|| left_record.pid().cmp(&right_record.pid()))
            .then_with(|| {
                left_record
                    .target()
                    .creation_time()
                    .cmp(&right_record.target().creation_time())
            })
    });

    assemble_plan(
        prepared.authority_id,
        ProcessBatchScope::Tree,
        prepared.operation,
        selected
            .into_iter()
            .map(|(record, depth)| (record, Some(depth)))
            .collect(),
        omissions,
    )
}

pub(super) fn build_same_name_plan<A: ProcessPlatformAdapter>(
    authority: &CoreAuthority<A>,
    prepared: PreparedBatchPlanning,
) -> Result<ProcessBatchPlan, ProcessBatchError> {
    let root_target = prepared.root.target();
    let root_name = prepared.root.name();
    let mut omissions = ProcessBatchOmissions::new();
    let mut selected = vec![prepared.root.clone()];

    for entry in prepared.snapshot.entries() {
        if entry.record().is_some_and(|record| {
            record.pid() == root_target.pid()
                && record.target().creation_time() == root_target.creation_time()
        }) {
            continue;
        }

        if entry.omission_reason() == Some(ExpansionOmissionReason::IdentityUnavailable)
            && !same_name_hint_is_comparable(entry.name_hint())
        {
            omissions.increment(BatchOmissionReason::IdentityUnavailable)?;
            continue;
        }

        let candidate_name = entry
            .record()
            .map_or(entry.name_hint(), ProcessRecord::name);
        match authority.adapter().process_names_equivalent(
            std::ffi::OsStr::new(candidate_name),
            std::ffi::OsStr::new(root_name),
        ) {
            Ok(false) => {}
            Ok(true) => match entry.record() {
                Some(record) => selected.push(record.clone()),
                None => omissions.increment(map_expansion_omission(
                    entry
                        .omission_reason()
                        .expect("unreliable expansion entries have omission reasons"),
                ))?,
            },
            Err(_) => omissions.increment(BatchOmissionReason::IdentityUnavailable)?,
        }
    }

    if selected.len() > BATCH_TARGET_LIMIT {
        return Err(ProcessBatchError::TargetSetTooLarge);
    }
    selected.sort_by(|left, right| {
        left.pid().cmp(&right.pid()).then_with(|| {
            left.target()
                .creation_time()
                .cmp(&right.target().creation_time())
        })
    });
    assemble_plan(
        prepared.authority_id,
        ProcessBatchScope::SameName,
        prepared.operation,
        selected.into_iter().map(|record| (record, None)).collect(),
        omissions,
    )
}

fn same_name_hint_is_comparable(name: &str) -> bool {
    !name.is_empty() && name == name.trim() && !name.contains(['\0', '\\', '/'])
}

fn assemble_plan(
    authority_id: HostInstanceId,
    scope: ProcessBatchScope,
    operation: ProcessBatchOperation,
    selected: Vec<(ProcessRecord, Option<usize>)>,
    omissions: ProcessBatchOmissions,
) -> Result<ProcessBatchPlan, ProcessBatchError> {
    if selected.len() > BATCH_TARGET_LIMIT {
        return Err(ProcessBatchError::TargetSetTooLarge);
    }
    let targets = selected
        .iter()
        .map(|(record, _)| record.target().clone())
        .collect();
    let previews = selected
        .into_iter()
        .enumerate()
        .map(|(ordinal, (record, depth))| ProcessBatchPreview {
            ordinal,
            pid: record.pid(),
            name: record.name().to_owned(),
            executable_path: record.executable_path().map(Path::to_path_buf),
            depth,
        })
        .collect();
    Ok(ProcessBatchPlan {
        authority_id,
        scope,
        operation,
        targets,
        previews,
        omissions,
    })
}

enum TreeCandidateResolution {
    Selected(usize),
    Omitted(BatchOmissionReason),
    OutsideScope,
}

fn resolve_tree_candidate(
    candidate: &ExpansionEntry,
    root: &ProcessRecord,
    by_pid: &BTreeMap<u32, Vec<&ExpansionEntry>>,
) -> TreeCandidateResolution {
    if by_pid
        .get(&candidate.pid_hint())
        .is_some_and(|entries| entries.len() != 1)
    {
        return TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified);
    }

    let mut current = candidate;
    let mut visited = BTreeSet::new();
    let mut depth = 0_usize;
    let candidate_unreliable = candidate.record().is_none();
    let mut ancestry_invalid = false;
    let mut root_parent_encountered = false;

    loop {
        if !visited.insert(current.pid_hint()) {
            return TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified);
        }
        if root.observed_parent_pid() == Some(current.pid_hint()) {
            root_parent_encountered = true;
        }
        let Some(parent_pid) = current
            .observed_parent_pid_hint()
            .filter(|parent_pid| *parent_pid != 0)
        else {
            return if ancestry_invalid || candidate_unreliable {
                TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified)
            } else {
                TreeCandidateResolution::OutsideScope
            };
        };
        depth = match depth.checked_add(1) {
            Some(depth) => depth,
            None => {
                return TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified)
            }
        };

        if parent_pid == root.pid() {
            ancestry_invalid |= root_parent_encountered;
            if let Some(child) = current.record() {
                ancestry_invalid |= root.target().creation_time() > child.target().creation_time();
            } else if !std::ptr::eq(current, candidate) {
                ancestry_invalid = true;
            }
            if ancestry_invalid {
                return TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified);
            }
            return match candidate.omission_reason() {
                Some(reason) => TreeCandidateResolution::Omitted(map_expansion_omission(reason)),
                None => TreeCandidateResolution::Selected(depth),
            };
        }

        let Some(parents) = by_pid.get(&parent_pid) else {
            return TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified);
        };
        if parents.len() != 1 {
            return TreeCandidateResolution::Omitted(BatchOmissionReason::AncestryUnverified);
        }
        let parent = parents[0];
        match (parent.record(), current.record()) {
            (Some(parent_record), Some(child_record)) => {
                ancestry_invalid |=
                    parent_record.target().creation_time() > child_record.target().creation_time();
            }
            (None, _) => ancestry_invalid = true,
            _ => {}
        }
        current = parent;
    }
}

fn map_expansion_omission(reason: ExpansionOmissionReason) -> BatchOmissionReason {
    match reason {
        ExpansionOmissionReason::ExitedOrChanged => BatchOmissionReason::ExitedOrChanged,
        ExpansionOmissionReason::InspectionDenied => BatchOmissionReason::InspectionDenied,
        ExpansionOmissionReason::IdentityUnavailable => BatchOmissionReason::IdentityUnavailable,
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
