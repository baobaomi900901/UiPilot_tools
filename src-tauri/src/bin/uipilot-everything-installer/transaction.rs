use crate::model::{canonical_nonzero_lower_hex, OperationId, Phase};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapDisposition {
    RecoverableEmpty,
    RecoverableTemp,
    Foreign,
}

pub fn classify_bootstrap(entries: &[&str]) -> BootstrapDisposition {
    match entries {
        [] => BootstrapDisposition::RecoverableEmpty,
        [name] if valid_transaction_temp_name(name) => BootstrapDisposition::RecoverableTemp,
        _ => BootstrapDisposition::Foreign,
    }
}

fn valid_transaction_temp_name(name: &str) -> bool {
    const PREFIX: &str = "managed-transaction.";
    const SUFFIX: &str = ".tmp";
    name.strip_prefix(PREFIX)
        .and_then(|value| value.strip_suffix(SUFFIX))
        .is_some_and(canonical_nonzero_lower_hex)
}

pub fn durable_temp_name(stem: &str, transaction_id: &OperationId) -> String {
    format!("{stem}.{}.tmp", transaction_id.as_str().replace('-', ""))
}

#[cfg(test)]
pub fn temp_belongs_to(name: &str, stem: &str, transaction_id: &OperationId) -> bool {
    name == durable_temp_name(stem, transaction_id)
}

pub fn validate_recovery_handoff(
    current: &OperationId,
    recovered: Option<&OperationId>,
    durable: &OperationId,
) -> Result<(), &'static str> {
    match recovered {
        Some(recovered) if recovered == durable && recovered != current => Ok(()),
        _ => Err("transaction-handoff-mismatch"),
    }
}

pub fn validate_same_transaction(
    operation: &OperationId,
    transaction: &OperationId,
) -> Result<(), &'static str> {
    (operation == transaction)
        .then_some(())
        .ok_or("transaction-id-mismatch")
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    StageNewBinary,
    StopOldService,
    RemoveOldService,
    InstallNewService,
    StartNewService,
    ReplaceRuntimeManifest,
    CleanupUnreferencedVersions,
    None,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn next_effect(phase: Phase) -> Effect {
    match phase {
        Phase::JournalCreated => Effect::StageNewBinary,
        Phase::NewBinaryVerified => Effect::StopOldService,
        Phase::OldServiceStopped => Effect::RemoveOldService,
        Phase::OldServiceRemoved => Effect::InstallNewService,
        Phase::NewServiceInstalled => Effect::StartNewService,
        Phase::NewServiceRunning => Effect::ReplaceRuntimeManifest,
        Phase::ManifestReplaced => Effect::CleanupUnreferencedVersions,
        _ => Effect::None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureDisposition {
    RollbackPreCommit,
    FinishForward,
    FailClosed,
}

pub fn failure_disposition(phase: Phase) -> FailureDisposition {
    match phase {
        Phase::JournalCreated
        | Phase::NewBinaryVerified
        | Phase::OldServiceStopped
        | Phase::OldServiceRemoved
        | Phase::NewServiceInstalled
        | Phase::NewServiceRunning => FailureDisposition::RollbackPreCommit,
        Phase::ManifestReplaced | Phase::CleanupComplete => FailureDisposition::FinishForward,
        Phase::Prepared
        | Phase::RollbackStarted
        | Phase::RollbackComplete
        | Phase::UninstallServiceRemoved
        | Phase::UninstallDirectoriesRemoved => FailureDisposition::FailClosed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveService {
    Absent,
    MatchingOldRunning,
    MatchingOldStopped,
    MatchingNewRunning,
    MatchingNewStopped,
    MatchingBothRunning,
    MatchingBothStopped,
    Foreign,
    #[cfg_attr(not(test), allow(dead_code))]
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceActivity {
    Running,
    Stopped,
}

pub fn classify_matching_service(
    activity: ServiceActivity,
    old_matches: bool,
    new_matches: bool,
) -> LiveService {
    match (activity, old_matches, new_matches) {
        (ServiceActivity::Running, true, false) => LiveService::MatchingOldRunning,
        (ServiceActivity::Stopped, true, false) => LiveService::MatchingOldStopped,
        (ServiceActivity::Running, false, true) => LiveService::MatchingNewRunning,
        (ServiceActivity::Stopped, false, true) => LiveService::MatchingNewStopped,
        (_, false, false) => LiveService::Foreign,
        (ServiceActivity::Running, true, true) => LiveService::MatchingBothRunning,
        (ServiceActivity::Stopped, true, true) => LiveService::MatchingBothStopped,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackDecision {
    StopAndRemoveNewThenRestoreOld,
    RemoveNewThenRestoreOld,
    RestoreOld,
    StartMatchingThenKeep,
    KeepOld,
    FailClosedRetainAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OldRecoveryState {
    NotRequired,
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackAuthorization {
    Authorized(RollbackDecision),
    FailClosedRetainAll,
}

pub fn rollback_decision(live: LiveService) -> RollbackDecision {
    match live {
        LiveService::MatchingNewRunning => RollbackDecision::StopAndRemoveNewThenRestoreOld,
        LiveService::MatchingNewStopped => RollbackDecision::RemoveNewThenRestoreOld,
        LiveService::Absent => RollbackDecision::RestoreOld,
        LiveService::MatchingOldRunning => RollbackDecision::KeepOld,
        LiveService::MatchingOldStopped => RollbackDecision::RestoreOld,
        LiveService::MatchingBothRunning => RollbackDecision::KeepOld,
        LiveService::MatchingBothStopped => RollbackDecision::StartMatchingThenKeep,
        LiveService::Foreign | LiveService::Ambiguous => RollbackDecision::FailClosedRetainAll,
    }
}

pub fn authorize_rollback(
    decision: RollbackDecision,
    old_recovery: OldRecoveryState,
) -> RollbackAuthorization {
    let authorized = match decision {
        RollbackDecision::StopAndRemoveNewThenRestoreOld
        | RollbackDecision::RemoveNewThenRestoreOld => {
            matches!(
                old_recovery,
                OldRecoveryState::Ready | OldRecoveryState::NotRequired
            )
        }
        RollbackDecision::RestoreOld => {
            matches!(
                old_recovery,
                OldRecoveryState::Ready | OldRecoveryState::NotRequired
            )
        }
        RollbackDecision::StartMatchingThenKeep | RollbackDecision::KeepOld => {
            old_recovery == OldRecoveryState::Ready
        }
        RollbackDecision::FailClosedRetainAll => false,
    };
    if authorized {
        RollbackAuthorization::Authorized(decision)
    } else {
        RollbackAuthorization::FailClosedRetainAll
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeManifestState {
    Missing,
    MatchingOld,
    MatchingNew,
    MatchingBoth,
    Foreign,
}

pub fn classify_runtime_manifest(
    present: bool,
    old_matches: bool,
    new_matches: bool,
) -> RuntimeManifestState {
    match (present, old_matches, new_matches) {
        (false, false, false) => RuntimeManifestState::Missing,
        (true, true, false) => RuntimeManifestState::MatchingOld,
        (true, false, true) => RuntimeManifestState::MatchingNew,
        (true, true, true) => RuntimeManifestState::MatchingBoth,
        _ => RuntimeManifestState::Foreign,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardAction {
    KeepNewAndCleanup,
    StartNewAndCleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvergenceDecision {
    Rollback(RollbackDecision),
    Forward(ForwardAction),
    FailClosed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvergenceInput {
    pub phase: Phase,
    pub shared_identity: bool,
    pub runtime_manifest: RuntimeManifestState,
    pub live_service: LiveService,
    pub old_recovery: OldRecoveryState,
}

fn forward_action(input: &ConvergenceInput) -> Option<ForwardAction> {
    let phase_committed =
        failure_disposition(input.phase.clone()) == FailureDisposition::FinishForward;
    let manifest_write_won_phase_race = input.phase == Phase::NewServiceRunning
        && input.runtime_manifest == RuntimeManifestState::MatchingNew
        && !input.shared_identity;
    if !phase_committed && !manifest_write_won_phase_race {
        return None;
    }
    let runtime_is_new = (!input.shared_identity
        && input.runtime_manifest == RuntimeManifestState::MatchingNew)
        || (input.shared_identity && input.runtime_manifest == RuntimeManifestState::MatchingBoth);
    if !runtime_is_new {
        return None;
    }
    match input.live_service {
        LiveService::MatchingNewRunning if !input.shared_identity => {
            Some(ForwardAction::KeepNewAndCleanup)
        }
        LiveService::MatchingNewStopped if !input.shared_identity => {
            Some(ForwardAction::StartNewAndCleanup)
        }
        LiveService::MatchingBothRunning if input.shared_identity => {
            Some(ForwardAction::KeepNewAndCleanup)
        }
        LiveService::MatchingBothStopped if input.shared_identity => {
            Some(ForwardAction::StartNewAndCleanup)
        }
        _ => None,
    }
}

fn precommit_runtime_matches(input: &ConvergenceInput) -> bool {
    match input.old_recovery {
        OldRecoveryState::NotRequired => {
            !input.shared_identity && input.runtime_manifest == RuntimeManifestState::Missing
        }
        OldRecoveryState::Ready | OldRecoveryState::Unavailable if input.shared_identity => {
            input.runtime_manifest == RuntimeManifestState::MatchingBoth
        }
        OldRecoveryState::Ready | OldRecoveryState::Unavailable => {
            input.runtime_manifest == RuntimeManifestState::MatchingOld
        }
    }
}

fn precommit_live_matches(input: &ConvergenceInput) -> bool {
    let old = matches!(
        input.live_service,
        LiveService::MatchingOldRunning | LiveService::MatchingOldStopped
    ) && !input.shared_identity
        && input.old_recovery != OldRecoveryState::NotRequired;
    let shared = matches!(
        input.live_service,
        LiveService::MatchingBothRunning | LiveService::MatchingBothStopped
    ) && input.shared_identity
        && input.old_recovery != OldRecoveryState::NotRequired;
    let new = matches!(
        input.live_service,
        LiveService::MatchingNewRunning | LiveService::MatchingNewStopped
    ) && !input.shared_identity;
    match input.phase {
        Phase::JournalCreated | Phase::NewBinaryVerified => {
            input.live_service == LiveService::Absent || old || shared
        }
        Phase::OldServiceStopped => {
            input.live_service == LiveService::Absent
                || (old && input.live_service == LiveService::MatchingOldStopped)
                || (shared && input.live_service == LiveService::MatchingBothStopped)
        }
        Phase::OldServiceRemoved => {
            input.live_service == LiveService::Absent
                || (new && input.live_service == LiveService::MatchingNewStopped)
                || (shared && input.live_service == LiveService::MatchingBothStopped)
        }
        Phase::NewServiceInstalled => new || shared,
        Phase::NewServiceRunning => {
            (new && input.live_service == LiveService::MatchingNewRunning)
                || (shared && input.live_service == LiveService::MatchingBothRunning)
        }
        _ => false,
    }
}

pub fn convergence_decision(input: &ConvergenceInput) -> ConvergenceDecision {
    let disposition = failure_disposition(input.phase.clone());
    if disposition == FailureDisposition::FinishForward
        || (input.phase == Phase::NewServiceRunning
            && input.runtime_manifest == RuntimeManifestState::MatchingNew
            && !input.shared_identity)
    {
        return forward_action(input)
            .map(ConvergenceDecision::Forward)
            .unwrap_or(ConvergenceDecision::FailClosed);
    }
    if disposition != FailureDisposition::RollbackPreCommit
        || !precommit_runtime_matches(input)
        || !precommit_live_matches(input)
    {
        return ConvergenceDecision::FailClosed;
    }
    match authorize_rollback(rollback_decision(input.live_service), input.old_recovery) {
        RollbackAuthorization::Authorized(decision) => ConvergenceDecision::Rollback(decision),
        RollbackAuthorization::FailClosedRetainAll => ConvergenceDecision::FailClosed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackCleanupStep {
    RemoveCandidateDirectory,
    KeepReferencedCandidate,
    FailClosedRetainCandidate,
    Complete,
}

pub fn rollback_cleanup_step(
    candidate_exists: bool,
    candidate_is_unreferenced: bool,
    candidate_is_referenced_by_old: bool,
) -> RollbackCleanupStep {
    if !candidate_exists {
        RollbackCleanupStep::Complete
    } else if candidate_is_unreferenced {
        RollbackCleanupStep::RemoveCandidateDirectory
    } else if candidate_is_referenced_by_old {
        RollbackCleanupStep::KeepReferencedCandidate
    } else {
        RollbackCleanupStep::FailClosedRetainCandidate
    }
}

pub fn rollback_candidate_entries_are_scoped(
    entries: &[&str],
    transaction_id: &OperationId,
) -> bool {
    let expected_temp = format!(
        "Everything.{}.tmp",
        transaction_id.as_str().replace('-', "")
    );
    let mut executable_seen = false;
    let mut temp_seen = false;
    entries.iter().all(|entry| match *entry {
        "Everything.exe" if !executable_seen => {
            executable_seen = true;
            true
        }
        name if name == expected_temp && !temp_seen => {
            temp_seen = true;
            true
        }
        _ => false,
    })
}

pub fn validated_persisted_execution<T, E>(
    validate: impl FnOnce() -> Result<(), E>,
    execute: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    validate()?;
    execute()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateDirectoryDisposition {
    MustBeAbsent,
    ExistingManagedRepair,
}

pub fn candidate_directory_disposition(
    old_binary: Option<&Path>,
    candidate_binary: &Path,
) -> CandidateDirectoryDisposition {
    if old_binary == Some(candidate_binary) {
        CandidateDirectoryDisposition::ExistingManagedRepair
    } else {
        CandidateDirectoryDisposition::MustBeAbsent
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallStep {
    DeleteNewDirectory,
    DeleteOldDirectory,
    DeleteManifest,
    Complete,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn uninstall_step(new_exists: bool, old_exists: bool, manifest_exists: bool) -> UninstallStep {
    if new_exists {
        UninstallStep::DeleteNewDirectory
    } else if old_exists {
        UninstallStep::DeleteOldDirectory
    } else if manifest_exists {
        UninstallStep::DeleteManifest
    } else {
        UninstallStep::Complete
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRootResidue {
    Missing,
    ProtectedEmpty,
    HasContent,
    #[cfg_attr(not(test), allow(dead_code))]
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateServiceResidue {
    Absent,
    Present,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreUninstallResidueDecision {
    Continue,
    FailClosed,
}

pub fn pre_uninstall_residue_decision(
    root: ManagedRootResidue,
    service: PrivateServiceResidue,
    recovered_id_present: bool,
) -> PreUninstallResidueDecision {
    if matches!(
        root,
        ManagedRootResidue::Missing | ManagedRootResidue::ProtectedEmpty
    ) && service == PrivateServiceResidue::Absent
        && !recovered_id_present
    {
        PreUninstallResidueDecision::Continue
    } else {
        PreUninstallResidueDecision::FailClosed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostUninstallResidueDecision {
    RemoveEmptyRoot,
    Complete,
    FailClosed,
}

pub fn post_uninstall_residue_decision(
    root: ManagedRootResidue,
    service: PrivateServiceResidue,
) -> PostUninstallResidueDecision {
    match (root, service) {
        (ManagedRootResidue::ProtectedEmpty, PrivateServiceResidue::Absent) => {
            PostUninstallResidueDecision::RemoveEmptyRoot
        }
        (ManagedRootResidue::Missing, PrivateServiceResidue::Absent) => {
            PostUninstallResidueDecision::Complete
        }
        _ => PostUninstallResidueDecision::FailClosed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_residue_is_exact_and_fail_closed() {
        assert_eq!(
            classify_bootstrap(&[]),
            BootstrapDisposition::RecoverableEmpty
        );
        assert_eq!(
            classify_bootstrap(&["managed-transaction.00112233445566778899aabbccddeeff.tmp"]),
            BootstrapDisposition::RecoverableTemp
        );
        assert_eq!(
            classify_bootstrap(&["foreign.txt"]),
            BootstrapDisposition::Foreign
        );
        assert_eq!(
            classify_bootstrap(&["managed-transaction.00000000000000000000000000000000.tmp"]),
            BootstrapDisposition::Foreign
        );
        assert_eq!(
            classify_bootstrap(&["managed-transaction.00112233445566778899AABBCCDDEEFF.tmp"]),
            BootstrapDisposition::Foreign
        );
        assert_eq!(
            classify_bootstrap(&["managed-transaction.00112233445566778899aabbccddeef.tmp"]),
            BootstrapDisposition::Foreign
        );
        assert_eq!(
            classify_bootstrap(&[
                "managed-transaction.00112233445566778899aabbccddeeff.tmp",
                "extra"
            ]),
            BootstrapDisposition::Foreign
        );
    }

    #[test]
    fn recovery_requires_separate_validated_old_transaction_id() {
        let current = OperationId::fixture(1);
        let old = OperationId::fixture(2);
        assert_eq!(
            validate_recovery_handoff(&current, Some(&old), &old),
            Ok(())
        );
        assert!(validate_recovery_handoff(&current, Some(&current), &old).is_err());
        assert!(validate_recovery_handoff(&current, None, &old).is_err());
    }

    #[test]
    fn commit_and_rollback_require_current_operation_id() {
        let current = OperationId::fixture(1);
        assert!(validate_same_transaction(&current, &current).is_ok());
        assert!(validate_same_transaction(&current, &OperationId::fixture(2)).is_err());
    }

    #[test]
    fn every_mutating_phase_is_persisted_before_its_effect() {
        assert_eq!(next_effect(Phase::JournalCreated), Effect::StageNewBinary);
        assert_eq!(
            next_effect(Phase::NewBinaryVerified),
            Effect::StopOldService
        );
        assert_eq!(
            next_effect(Phase::OldServiceStopped),
            Effect::RemoveOldService
        );
        assert_eq!(
            next_effect(Phase::OldServiceRemoved),
            Effect::InstallNewService
        );
        assert_eq!(
            next_effect(Phase::NewServiceInstalled),
            Effect::StartNewService
        );
        assert_eq!(
            next_effect(Phase::NewServiceRunning),
            Effect::ReplaceRuntimeManifest
        );
        assert_eq!(
            next_effect(Phase::ManifestReplaced),
            Effect::CleanupUnreferencedVersions
        );
    }

    #[test]
    fn failure_disposition_switches_to_forward_only_at_manifest_commit() {
        for phase in [
            Phase::JournalCreated,
            Phase::NewBinaryVerified,
            Phase::OldServiceStopped,
            Phase::OldServiceRemoved,
            Phase::NewServiceInstalled,
            Phase::NewServiceRunning,
        ] {
            assert_eq!(
                failure_disposition(phase),
                FailureDisposition::RollbackPreCommit
            );
        }
        for phase in [Phase::ManifestReplaced, Phase::CleanupComplete] {
            assert_eq!(
                failure_disposition(phase),
                FailureDisposition::FinishForward
            );
        }
        for phase in [
            Phase::Prepared,
            Phase::RollbackStarted,
            Phase::RollbackComplete,
            Phase::UninstallServiceRemoved,
            Phase::UninstallDirectoriesRemoved,
        ] {
            assert_eq!(failure_disposition(phase), FailureDisposition::FailClosed);
        }
    }

    #[test]
    fn rollback_never_removes_ambiguous_service() {
        assert_eq!(
            rollback_decision(LiveService::MatchingNewRunning),
            RollbackDecision::StopAndRemoveNewThenRestoreOld
        );
        assert_eq!(
            rollback_decision(LiveService::MatchingNewStopped),
            RollbackDecision::RemoveNewThenRestoreOld
        );
        assert_eq!(
            rollback_decision(LiveService::Absent),
            RollbackDecision::RestoreOld
        );
        assert_eq!(
            rollback_decision(LiveService::MatchingOldRunning),
            RollbackDecision::KeepOld
        );
        assert_eq!(
            rollback_decision(LiveService::MatchingOldStopped),
            RollbackDecision::RestoreOld
        );
        assert_eq!(
            rollback_decision(LiveService::MatchingBothRunning),
            RollbackDecision::KeepOld
        );
        assert_eq!(
            rollback_decision(LiveService::MatchingBothStopped),
            RollbackDecision::StartMatchingThenKeep
        );
        assert_eq!(
            rollback_decision(LiveService::Foreign),
            RollbackDecision::FailClosedRetainAll
        );
        assert_eq!(
            rollback_decision(LiveService::Ambiguous),
            RollbackDecision::FailClosedRetainAll
        );
    }

    #[test]
    fn rollback_requires_old_preflight_before_authorizing_new_service_removal() {
        assert_eq!(
            authorize_rollback(
                RollbackDecision::StopAndRemoveNewThenRestoreOld,
                OldRecoveryState::Unavailable,
            ),
            RollbackAuthorization::FailClosedRetainAll
        );
        assert_eq!(
            authorize_rollback(
                RollbackDecision::StopAndRemoveNewThenRestoreOld,
                OldRecoveryState::Ready,
            ),
            RollbackAuthorization::Authorized(RollbackDecision::StopAndRemoveNewThenRestoreOld)
        );
        assert_eq!(
            authorize_rollback(
                RollbackDecision::RemoveNewThenRestoreOld,
                OldRecoveryState::NotRequired,
            ),
            RollbackAuthorization::Authorized(RollbackDecision::RemoveNewThenRestoreOld)
        );
        assert_eq!(
            authorize_rollback(
                RollbackDecision::StartMatchingThenKeep,
                OldRecoveryState::Unavailable,
            ),
            RollbackAuthorization::FailClosedRetainAll
        );
    }

    #[test]
    fn convergence_uses_phase_runtime_identity_and_repair_state() {
        assert_eq!(
            classify_runtime_manifest(false, false, false),
            RuntimeManifestState::Missing
        );
        assert_eq!(
            classify_runtime_manifest(true, true, false),
            RuntimeManifestState::MatchingOld
        );
        assert_eq!(
            classify_runtime_manifest(true, false, true),
            RuntimeManifestState::MatchingNew
        );
        assert_eq!(
            classify_runtime_manifest(true, true, true),
            RuntimeManifestState::MatchingBoth
        );
        assert_eq!(
            classify_runtime_manifest(true, false, false),
            RuntimeManifestState::Foreign
        );
        let committed_update = ConvergenceInput {
            phase: Phase::ManifestReplaced,
            shared_identity: false,
            runtime_manifest: RuntimeManifestState::MatchingNew,
            live_service: LiveService::MatchingNewRunning,
            old_recovery: OldRecoveryState::Unavailable,
        };
        assert_eq!(
            convergence_decision(&committed_update),
            ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup)
        );
        assert_eq!(
            convergence_decision(&ConvergenceInput {
                phase: Phase::NewServiceRunning,
                ..committed_update
            }),
            ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup)
        );
        assert_eq!(
            convergence_decision(&ConvergenceInput {
                runtime_manifest: RuntimeManifestState::MatchingOld,
                ..committed_update
            }),
            ConvergenceDecision::FailClosed
        );

        let repair_before_commit = ConvergenceInput {
            phase: Phase::OldServiceStopped,
            shared_identity: true,
            runtime_manifest: RuntimeManifestState::MatchingBoth,
            live_service: LiveService::MatchingBothStopped,
            old_recovery: OldRecoveryState::Ready,
        };
        assert_eq!(
            convergence_decision(&repair_before_commit),
            ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep)
        );
        assert_eq!(
            convergence_decision(&ConvergenceInput {
                phase: Phase::ManifestReplaced,
                ..repair_before_commit
            }),
            ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup)
        );
    }

    #[test]
    fn convergence_matrix_covers_fresh_update_repair_and_impossible_states() {
        let fresh = [
            (
                Phase::JournalCreated,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceStopped,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
        ];
        for (phase, runtime_manifest, live_service, expected) in fresh {
            assert_eq!(
                convergence_decision(&ConvergenceInput {
                    phase,
                    shared_identity: false,
                    runtime_manifest,
                    live_service,
                    old_recovery: OldRecoveryState::NotRequired,
                }),
                expected
            );
        }

        let update = [
            (
                Phase::JournalCreated,
                LiveService::MatchingOldRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::NewBinaryVerified,
                LiveService::MatchingOldStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceStopped,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceInstalled,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
        ];
        for (phase, live_service, expected) in update {
            assert_eq!(
                convergence_decision(&ConvergenceInput {
                    phase,
                    shared_identity: false,
                    runtime_manifest: RuntimeManifestState::MatchingOld,
                    live_service,
                    old_recovery: OldRecoveryState::Ready,
                }),
                expected
            );
        }
        for (phase, live_service, expected) in [
            (
                Phase::ManifestReplaced,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
        ] {
            assert_eq!(
                convergence_decision(&ConvergenceInput {
                    phase,
                    shared_identity: false,
                    runtime_manifest: RuntimeManifestState::MatchingNew,
                    live_service,
                    old_recovery: OldRecoveryState::Unavailable,
                }),
                expected
            );
        }

        let repair = [
            (
                Phase::JournalCreated,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::NewBinaryVerified,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::OldServiceStopped,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::NewServiceInstalled,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::NewServiceRunning,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::ManifestReplaced,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
        ];
        for (phase, live_service, expected) in repair {
            assert_eq!(
                convergence_decision(&ConvergenceInput {
                    phase,
                    shared_identity: true,
                    runtime_manifest: RuntimeManifestState::MatchingBoth,
                    live_service,
                    old_recovery: OldRecoveryState::Ready,
                }),
                expected
            );
        }

        let install_phases = [
            Phase::JournalCreated,
            Phase::NewBinaryVerified,
            Phase::OldServiceStopped,
            Phase::OldServiceRemoved,
            Phase::NewServiceInstalled,
            Phase::NewServiceRunning,
            Phase::ManifestReplaced,
            Phase::CleanupComplete,
        ];
        for phase in install_phases {
            for live_service in [LiveService::Foreign, LiveService::Ambiguous] {
                assert_eq!(
                    convergence_decision(&ConvergenceInput {
                        phase: phase.clone(),
                        shared_identity: false,
                        runtime_manifest: RuntimeManifestState::MatchingOld,
                        live_service,
                        old_recovery: OldRecoveryState::Ready,
                    }),
                    ConvergenceDecision::FailClosed
                );
            }
        }
        for phase in [
            Phase::Prepared,
            Phase::RollbackStarted,
            Phase::RollbackComplete,
            Phase::UninstallServiceRemoved,
            Phase::UninstallDirectoriesRemoved,
        ] {
            assert_eq!(
                convergence_decision(&ConvergenceInput {
                    phase,
                    shared_identity: false,
                    runtime_manifest: RuntimeManifestState::MatchingOld,
                    live_service: LiveService::MatchingOldRunning,
                    old_recovery: OldRecoveryState::Ready,
                }),
                ConvergenceDecision::FailClosed
            );
        }

        for input in [
            ConvergenceInput {
                phase: Phase::NewServiceInstalled,
                shared_identity: false,
                runtime_manifest: RuntimeManifestState::MatchingOld,
                live_service: LiveService::MatchingNewStopped,
                old_recovery: OldRecoveryState::Unavailable,
            },
            ConvergenceInput {
                phase: Phase::NewServiceRunning,
                shared_identity: true,
                runtime_manifest: RuntimeManifestState::MatchingBoth,
                live_service: LiveService::MatchingBothRunning,
                old_recovery: OldRecoveryState::Unavailable,
            },
            ConvergenceInput {
                phase: Phase::OldServiceRemoved,
                shared_identity: false,
                runtime_manifest: RuntimeManifestState::MatchingNew,
                live_service: LiveService::MatchingNewRunning,
                old_recovery: OldRecoveryState::Ready,
            },
            ConvergenceInput {
                phase: Phase::NewServiceRunning,
                shared_identity: false,
                runtime_manifest: RuntimeManifestState::MatchingOld,
                live_service: LiveService::MatchingBothRunning,
                old_recovery: OldRecoveryState::Ready,
            },
            ConvergenceInput {
                phase: Phase::ManifestReplaced,
                shared_identity: false,
                runtime_manifest: RuntimeManifestState::Foreign,
                live_service: LiveService::MatchingNewRunning,
                old_recovery: OldRecoveryState::Ready,
            },
        ] {
            assert_eq!(
                convergence_decision(&input),
                ConvergenceDecision::FailClosed
            );
        }
    }

    #[test]
    fn convergence_matrix_defaults_every_unlisted_phase_runtime_live_tuple_to_fail_closed() {
        type Case = (
            Phase,
            RuntimeManifestState,
            LiveService,
            ConvergenceDecision,
        );

        fn assert_complete_matrix(
            shared_identity: bool,
            old_recovery: OldRecoveryState,
            allowed: &[Case],
        ) {
            for phase in [
                Phase::JournalCreated,
                Phase::NewBinaryVerified,
                Phase::OldServiceStopped,
                Phase::OldServiceRemoved,
                Phase::NewServiceInstalled,
                Phase::NewServiceRunning,
                Phase::ManifestReplaced,
                Phase::CleanupComplete,
            ] {
                for runtime_manifest in [
                    RuntimeManifestState::Missing,
                    RuntimeManifestState::MatchingOld,
                    RuntimeManifestState::MatchingNew,
                    RuntimeManifestState::MatchingBoth,
                    RuntimeManifestState::Foreign,
                ] {
                    for live_service in [
                        LiveService::Absent,
                        LiveService::MatchingOldRunning,
                        LiveService::MatchingOldStopped,
                        LiveService::MatchingNewRunning,
                        LiveService::MatchingNewStopped,
                        LiveService::MatchingBothRunning,
                        LiveService::MatchingBothStopped,
                        LiveService::Foreign,
                        LiveService::Ambiguous,
                    ] {
                        let expected = allowed
                            .iter()
                            .find(|(allowed_phase, allowed_runtime, allowed_live, _)| {
                                allowed_phase == &phase
                                    && allowed_runtime == &runtime_manifest
                                    && allowed_live == &live_service
                            })
                            .map(|(_, _, _, decision)| *decision)
                            .unwrap_or(ConvergenceDecision::FailClosed);
                        assert_eq!(
                            convergence_decision(&ConvergenceInput {
                                phase: phase.clone(),
                                shared_identity,
                                runtime_manifest,
                                live_service,
                                old_recovery,
                            }),
                            expected,
                            "unexpected decision for {phase:?}/{runtime_manifest:?}/{live_service:?}"
                        );
                    }
                }
            }
        }

        let fresh = vec![
            (
                Phase::JournalCreated,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceStopped,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::Missing,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::Missing,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
        ];
        assert_complete_matrix(false, OldRecoveryState::NotRequired, &fresh);

        let update = vec![
            (
                Phase::JournalCreated,
                RuntimeManifestState::MatchingOld,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::JournalCreated,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingOldRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::JournalCreated,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingOldStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::MatchingOld,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingOldRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingOldStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceStopped,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingOldStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceStopped,
                RuntimeManifestState::MatchingOld,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::MatchingOld,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Rollback(RollbackDecision::RemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingOld,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Rollback(RollbackDecision::StopAndRemoveNewThenRestoreOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingNew,
                LiveService::MatchingNewRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
        ];
        assert_complete_matrix(false, OldRecoveryState::Ready, &update);

        let repair = vec![
            (
                Phase::JournalCreated,
                RuntimeManifestState::MatchingBoth,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::JournalCreated,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::JournalCreated,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::MatchingBoth,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::NewBinaryVerified,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::OldServiceStopped,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::OldServiceStopped,
                RuntimeManifestState::MatchingBoth,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::MatchingBoth,
                LiveService::Absent,
                ConvergenceDecision::Rollback(RollbackDecision::RestoreOld),
            ),
            (
                Phase::OldServiceRemoved,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Rollback(RollbackDecision::StartMatchingThenKeep),
            ),
            (
                Phase::NewServiceInstalled,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::NewServiceRunning,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Rollback(RollbackDecision::KeepOld),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::ManifestReplaced,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothStopped,
                ConvergenceDecision::Forward(ForwardAction::StartNewAndCleanup),
            ),
            (
                Phase::CleanupComplete,
                RuntimeManifestState::MatchingBoth,
                LiveService::MatchingBothRunning,
                ConvergenceDecision::Forward(ForwardAction::KeepNewAndCleanup),
            ),
        ];
        assert_complete_matrix(true, OldRecoveryState::Ready, &repair);

        let update_without_old = update
            .iter()
            .filter(|(phase, runtime, _, _)| {
                matches!(phase, Phase::ManifestReplaced | Phase::CleanupComplete)
                    || (phase == &Phase::NewServiceRunning
                        && runtime == &RuntimeManifestState::MatchingNew)
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_complete_matrix(false, OldRecoveryState::Unavailable, &update_without_old);
        let repair_without_old = repair
            .iter()
            .filter(|(phase, _, _, _)| {
                matches!(phase, Phase::ManifestReplaced | Phase::CleanupComplete)
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_complete_matrix(true, OldRecoveryState::Unavailable, &repair_without_old);
    }

    #[test]
    fn matching_service_classification_preserves_running_and_stopped_states() {
        assert_eq!(
            classify_matching_service(ServiceActivity::Running, true, false),
            LiveService::MatchingOldRunning
        );
        assert_eq!(
            classify_matching_service(ServiceActivity::Stopped, true, false),
            LiveService::MatchingOldStopped
        );
        assert_eq!(
            classify_matching_service(ServiceActivity::Running, false, true),
            LiveService::MatchingNewRunning
        );
        assert_eq!(
            classify_matching_service(ServiceActivity::Stopped, false, true),
            LiveService::MatchingNewStopped
        );
        assert_eq!(
            classify_matching_service(ServiceActivity::Running, false, false),
            LiveService::Foreign
        );
        assert_eq!(
            classify_matching_service(ServiceActivity::Stopped, true, true),
            LiveService::MatchingBothStopped
        );
        assert_eq!(
            classify_matching_service(ServiceActivity::Running, true, true),
            LiveService::MatchingBothRunning
        );
    }

    #[test]
    fn fresh_repair_and_update_candidate_directories_are_distinguished() {
        use std::path::Path;

        let old = Path::new(r"C:\ProgramData\UiPilot_tools\everything\1\Everything.exe");
        let candidate_same = old;
        let candidate_new = Path::new(r"C:\ProgramData\UiPilot_tools\everything\2\Everything.exe");
        assert_eq!(
            candidate_directory_disposition(None, candidate_new),
            CandidateDirectoryDisposition::MustBeAbsent
        );
        assert_eq!(
            candidate_directory_disposition(Some(old), candidate_same),
            CandidateDirectoryDisposition::ExistingManagedRepair
        );
        assert_eq!(
            candidate_directory_disposition(Some(old), candidate_new),
            CandidateDirectoryDisposition::MustBeAbsent
        );
    }

    #[test]
    fn rollback_removes_only_an_unreferenced_candidate_and_is_retryable() {
        assert_eq!(
            rollback_cleanup_step(true, true, false),
            RollbackCleanupStep::RemoveCandidateDirectory
        );
        assert_eq!(
            rollback_cleanup_step(false, true, false),
            RollbackCleanupStep::Complete
        );
        assert_eq!(
            rollback_cleanup_step(true, false, true),
            RollbackCleanupStep::KeepReferencedCandidate
        );
        assert_eq!(
            rollback_cleanup_step(true, false, false),
            RollbackCleanupStep::FailClosedRetainCandidate
        );
        let transaction = OperationId::fixture(1);
        let temp = format!("Everything.{}.tmp", transaction.as_str().replace('-', ""));
        assert!(rollback_candidate_entries_are_scoped(&[], &transaction));
        assert!(rollback_candidate_entries_are_scoped(
            &["Everything.exe"],
            &transaction
        ));
        assert!(rollback_candidate_entries_are_scoped(
            &[temp.as_str()],
            &transaction
        ));
        assert!(rollback_candidate_entries_are_scoped(
            &["Everything.exe", temp.as_str()],
            &transaction
        ));
        assert!(!rollback_candidate_entries_are_scoped(
            &["foreign.txt"],
            &transaction
        ));
        assert!(!rollback_candidate_entries_are_scoped(
            &["Everything.ffffffffffffffffffffffffffffffff.tmp"],
            &transaction
        ));
    }

    #[test]
    fn persisted_binary_is_validated_before_any_execution() {
        use std::cell::RefCell;

        let events = RefCell::new(Vec::new());
        let result = validated_persisted_execution(
            || {
                events.borrow_mut().push("validate");
                Err("invalid")
            },
            || {
                events.borrow_mut().push("execute");
                Ok(())
            },
        );
        assert_eq!(result, Err("invalid"));
        assert_eq!(*events.borrow(), vec!["validate"]);

        events.borrow_mut().clear();
        let result = validated_persisted_execution(
            || {
                events.borrow_mut().push("validate");
                Ok::<_, &str>(())
            },
            || {
                events.borrow_mut().push("execute");
                Ok::<_, &str>(())
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(*events.borrow(), vec!["validate", "execute"]);
    }

    #[test]
    fn uninstall_deletes_manifest_last_and_is_idempotent() {
        assert_eq!(
            uninstall_step(true, true, true),
            UninstallStep::DeleteNewDirectory
        );
        assert_eq!(
            uninstall_step(false, true, true),
            UninstallStep::DeleteOldDirectory
        );
        assert_eq!(
            uninstall_step(false, false, true),
            UninstallStep::DeleteManifest
        );
        assert_eq!(uninstall_step(false, false, false), UninstallStep::Complete);
    }

    #[test]
    fn pre_uninstall_residue_requires_absent_service_and_exact_empty_root() {
        assert_eq!(
            pre_uninstall_residue_decision(
                ManagedRootResidue::ProtectedEmpty,
                PrivateServiceResidue::Absent,
                false,
            ),
            PreUninstallResidueDecision::Continue
        );
        assert_eq!(
            pre_uninstall_residue_decision(
                ManagedRootResidue::Missing,
                PrivateServiceResidue::Absent,
                false,
            ),
            PreUninstallResidueDecision::Continue
        );
        for root in [
            ManagedRootResidue::HasContent,
            ManagedRootResidue::Untrusted,
        ] {
            assert_eq!(
                pre_uninstall_residue_decision(root, PrivateServiceResidue::Absent, false,),
                PreUninstallResidueDecision::FailClosed
            );
        }
        assert_eq!(
            pre_uninstall_residue_decision(
                ManagedRootResidue::ProtectedEmpty,
                PrivateServiceResidue::Present,
                false,
            ),
            PreUninstallResidueDecision::FailClosed
        );
        assert_eq!(
            pre_uninstall_residue_decision(
                ManagedRootResidue::ProtectedEmpty,
                PrivateServiceResidue::Absent,
                true,
            ),
            PreUninstallResidueDecision::FailClosed
        );
    }

    #[test]
    fn manifest_deletion_failure_composes_across_uninstall_retry() {
        let mut referenced_directory_exists = true;
        let mut manifest_exists = true;
        assert_eq!(
            uninstall_step(referenced_directory_exists, false, manifest_exists),
            UninstallStep::DeleteNewDirectory
        );
        referenced_directory_exists = false;
        assert_eq!(
            uninstall_step(referenced_directory_exists, false, manifest_exists),
            UninstallStep::DeleteManifest
        );
        manifest_exists = false;
        assert_eq!(
            uninstall_step(referenced_directory_exists, false, manifest_exists),
            UninstallStep::Complete
        );

        assert_eq!(
            pre_uninstall_residue_decision(
                ManagedRootResidue::ProtectedEmpty,
                PrivateServiceResidue::Absent,
                false,
            ),
            PreUninstallResidueDecision::Continue
        );
        assert_eq!(
            post_uninstall_residue_decision(
                ManagedRootResidue::ProtectedEmpty,
                PrivateServiceResidue::Absent,
            ),
            PostUninstallResidueDecision::RemoveEmptyRoot
        );
        assert_eq!(
            post_uninstall_residue_decision(
                ManagedRootResidue::Missing,
                PrivateServiceResidue::Absent,
            ),
            PostUninstallResidueDecision::Complete
        );
    }

    #[test]
    fn crash_temp_recovery_is_scoped_to_the_validated_transaction() {
        let transaction = OperationId::parse("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        assert_eq!(
            durable_temp_name("managed-transaction", &transaction),
            "managed-transaction.00112233445566778899aabbccddeeff.tmp"
        );
        assert!(temp_belongs_to(
            "managed-transaction.00112233445566778899aabbccddeeff.tmp",
            "managed-transaction",
            &transaction
        ));
        assert!(!temp_belongs_to(
            "managed-transaction.10112233445566778899aabbccddeeff.tmp",
            "managed-transaction",
            &transaction
        ));
    }
}
