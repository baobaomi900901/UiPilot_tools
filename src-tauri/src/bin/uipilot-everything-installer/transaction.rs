use crate::model::{OperationId, Phase};

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
    MatchingOld,
    MatchingNew,
    Foreign,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackDecision {
    RemoveNewThenRestoreOld,
    RestoreOld,
    KeepOld,
    FailClosedRetainAll,
}

pub fn rollback_decision(live: LiveService) -> RollbackDecision {
    match live {
        LiveService::MatchingNew => RollbackDecision::RemoveNewThenRestoreOld,
        LiveService::Absent => RollbackDecision::RestoreOld,
        LiveService::MatchingOld => RollbackDecision::KeepOld,
        LiveService::Foreign | LiveService::Ambiguous => RollbackDecision::FailClosedRetainAll,
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
            rollback_decision(LiveService::MatchingNew),
            RollbackDecision::RemoveNewThenRestoreOld
        );
        assert_eq!(
            rollback_decision(LiveService::Absent),
            RollbackDecision::RestoreOld
        );
        assert_eq!(
            rollback_decision(LiveService::MatchingOld),
            RollbackDecision::KeepOld
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
