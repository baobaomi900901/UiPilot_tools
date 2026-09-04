use crate::model::{OperationId, Phase};
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
        .is_some_and(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
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
pub enum LiveService {
    Absent,
    MatchingOldRunning,
    MatchingOldStopped,
    MatchingNewRunning,
    MatchingNewStopped,
    Foreign,
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
        (_, true, true) => LiveService::Ambiguous,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackDecision {
    StopAndRemoveNewThenRestoreOld,
    RemoveNewThenRestoreOld,
    RestoreOld,
    KeepOld,
    FailClosedRetainAll,
}

pub fn rollback_decision(live: LiveService) -> RollbackDecision {
    match live {
        LiveService::MatchingNewRunning => RollbackDecision::StopAndRemoveNewThenRestoreOld,
        LiveService::MatchingNewStopped => RollbackDecision::RemoveNewThenRestoreOld,
        LiveService::Absent => RollbackDecision::RestoreOld,
        LiveService::MatchingOldRunning => RollbackDecision::KeepOld,
        LiveService::MatchingOldStopped => RollbackDecision::RestoreOld,
        LiveService::Foreign | LiveService::Ambiguous => RollbackDecision::FailClosedRetainAll,
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
            rollback_decision(LiveService::Foreign),
            RollbackDecision::FailClosedRetainAll
        );
        assert_eq!(
            rollback_decision(LiveService::Ambiguous),
            RollbackDecision::FailClosedRetainAll
        );
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
            LiveService::Ambiguous
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
