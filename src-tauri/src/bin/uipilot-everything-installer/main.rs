mod model;
mod transaction;
mod windows;

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use model::{
    parse_request, Action, OperationId, Phase, Request, RuntimeManifest, StagingJournal,
    TransactionJournal, PIPE_SDDL_TEMPLATE, RUNTIME_MANIFEST_NAME, TRANSACTION_MANIFEST_NAME,
};
use transaction::{
    candidate_directory_disposition, classify_matching_service, rollback_cleanup_step,
    rollback_decision, validate_recovery_handoff, validate_same_transaction,
    CandidateDirectoryDisposition, LiveService, RollbackCleanupStep, RollbackDecision,
    ServiceActivity, UninstallStep,
};
use windows::{AdmissionLease, ExitCode, ServiceState};

#[derive(Debug)]
struct Cli {
    action: Action,
    operation_id: OperationId,
    staging_dir: PathBuf,
    request_path: PathBuf,
    journal_path: PathBuf,
    parent_pid: u32,
    parent_creation_time: u64,
    install_dir: Option<PathBuf>,
    recovered_transaction_id: Option<OperationId>,
}

impl Cli {
    fn parse() -> Result<Self, String> {
        let mut args = env::args().skip(1);
        let action = match args.next().as_deref() {
            Some("prepare-install") => Action::PrepareInstall,
            Some("recover-install") => Action::RecoverInstall,
            Some("commit-install") => Action::CommitInstall,
            Some("rollback-install") => Action::RollbackInstall,
            Some("pre-uninstall") => Action::PreUninstall,
            Some("post-uninstall") => Action::PostUninstall,
            _ => return Err("invalid-action".into()),
        };
        let mut values = BTreeMap::new();
        while let Some(name) = args.next() {
            if !name.starts_with("--")
                || values
                    .insert(name, args.next().ok_or("missing-option-value")?)
                    .is_some()
            {
                return Err("invalid-option".into());
            }
        }
        let required = |name: &str| {
            values
                .get(name)
                .cloned()
                .ok_or_else(|| "missing-option".to_string())
        };
        let operation_id = OperationId::parse(&required("--operation-id")?)?;
        let parent_pid = required("--parent-pid")?
            .parse()
            .map_err(|_| "invalid-parent-pid")?;
        let parent_creation_time = required("--parent-creation-time")?
            .parse()
            .map_err(|_| "invalid-parent-time")?;
        let staging_dir = PathBuf::from(required("--staging-dir")?);
        let request_path = PathBuf::from(required("--request")?);
        let journal_path = PathBuf::from(required("--journal")?);
        let install_dir = values.get("--install-dir").map(PathBuf::from);
        let recovered_transaction_id = values
            .get("--recovered-transaction-id")
            .map(|value| OperationId::parse(value))
            .transpose()?;
        let known = [
            "--operation-id",
            "--parent-pid",
            "--parent-creation-time",
            "--staging-dir",
            "--request",
            "--journal",
            "--install-dir",
            "--recovered-transaction-id",
        ];
        if values.keys().any(|key| !known.contains(&key.as_str())) {
            return Err("unknown-option".into());
        }
        Ok(Self {
            action,
            operation_id,
            staging_dir,
            request_path,
            journal_path,
            parent_pid,
            parent_creation_time,
            install_dir,
            recovered_transaction_id,
        })
    }

    fn request(&self) -> Request {
        Request {
            schema_version: 1,
            action: self.action,
            operation_id: self.operation_id.clone(),
            staging_dir: self.staging_dir.clone(),
            request_path: self.request_path.clone(),
            journal_path: self.journal_path.clone(),
            parent_pid: self.parent_pid,
            parent_creation_time: self.parent_creation_time,
            install_dir: self.install_dir.clone(),
            recovered_transaction_id: self.recovered_transaction_id.clone(),
        }
    }
}

fn main() {
    let code = match run() {
        Ok(()) => ExitCode::Success,
        Err((code, reason)) => {
            eprintln!("UIPILOT_EVERYTHING_INSTALLER_ERROR:{reason}");
            code
        }
    };
    std::process::exit(code as i32);
}

type HelperResult<T> = Result<T, (ExitCode, String)>;

fn fail<T>(code: ExitCode, reason: impl Into<String>) -> HelperResult<T> {
    Err((code, reason.into()))
}

fn run() -> HelperResult<()> {
    let cli = Cli::parse().map_err(|reason| (ExitCode::InvalidInvocation, reason))?;
    let executable = env::current_exe()
        .map_err(|_| (ExitCode::AdmissionDenied, "current-exe-unavailable".into()))?;
    windows::verify_current_image_as_invoker()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    windows::verify_staging_layout(
        &cli.staging_dir,
        &executable,
        &cli.request_path,
        &cli.journal_path,
    )
    .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    let _lease = windows::acquire_admission_lease(cli.parent_pid, cli.parent_creation_time)
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    verify_vm_disposable_marker()?;
    let request = parse_request(
        &windows::read_direct_file(&cli.request_path)
            .map_err(|reason| (ExitCode::AdmissionDenied, reason))?,
    )
    .map_err(|reason| (ExitCode::InvalidInvocation, reason))?;
    if request != cli.request() {
        return fail(ExitCode::InvalidInvocation, "request-cli-mismatch");
    }
    dispatch(&request, &_lease)
}

fn dispatch(request: &Request, lease: &AdmissionLease) -> HelperResult<()> {
    match request.action {
        Action::PrepareInstall => prepare_install(request),
        Action::RecoverInstall => recover_install(request, lease),
        Action::CommitInstall => commit_install(request, lease),
        Action::RollbackInstall => rollback_install(request, lease),
        Action::PreUninstall => pre_uninstall(request, lease),
        Action::PostUninstall => post_uninstall(request, lease),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> HelperResult<T> {
    let bytes =
        windows::read_direct_file(path).map_err(|reason| (ExitCode::ForeignState, reason))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| (ExitCode::ForeignState, "strict-json-invalid".into()))
}

fn atomic_json<T: serde::Serialize>(
    path: &Path,
    value: &T,
    temp_id: &OperationId,
) -> HelperResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| (ExitCode::NativeFailure, "atomic-parent-missing".into()))?;
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("managed-transaction");
    let temp = parent.join(transaction::durable_temp_name(stem, temp_id));
    if temp.exists() {
        windows::verify_managed_file(&temp).map_err(|reason| (ExitCode::ForeignState, reason))?;
        fs::remove_file(&temp).map_err(|_| {
            (
                ExitCode::NativeFailure,
                "stale-transaction-temp-delete-failed".into(),
            )
        })?;
    }
    let bytes = serde_json::to_vec(value)
        .map_err(|_| (ExitCode::NativeFailure, "json-serialize-failed".into()))?;
    let mut file =
        windows::create_protected_file(&temp).map_err(|reason| (ExitCode::ForeignState, reason))?;
    checkpoint("transaction-temp-created")?;
    file.write_all(&bytes)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            (
                ExitCode::NativeFailure,
                "transaction-temp-flush-failed".into(),
            )
        })?;
    checkpoint("transaction-temp-flushed")?;
    drop(file);
    windows::atomic_replace(&temp, path).map_err(|reason| (ExitCode::NativeFailure, reason))?;
    checkpoint("transaction-temp-renamed")?;
    Ok(())
}

fn read_runtime_manifest(root: &Path) -> HelperResult<Option<RuntimeManifest>> {
    let path = root.join(RUNTIME_MANIFEST_NAME);
    if !path.exists() {
        return Ok(None);
    }
    windows::verify_managed_file(&path).map_err(|reason| (ExitCode::ForeignState, reason))?;
    let manifest: RuntimeManifest = read_json(&path)?;
    validate_persisted_manifest_identity(root, &manifest)?;
    Ok(Some(manifest))
}

fn validate_persisted_manifest_identity(
    root: &Path,
    manifest: &RuntimeManifest,
) -> HelperResult<()> {
    manifest
        .validate_fixed_identity()
        .map_err(|reason| (ExitCode::ForeignState, reason))?;
    validate_manifest_location(root, manifest)
}

fn validate_manifest_location(root: &Path, manifest: &RuntimeManifest) -> HelperResult<()> {
    if !model::is_within_managed_root(&manifest.service_binary_path, root)
        || manifest.service_binary_path.parent().and_then(Path::parent) != Some(root)
        || manifest
            .service_binary_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(manifest.version.as_str())
    {
        return fail(
            ExitCode::ForeignState,
            "manifest-binary-outside-managed-root",
        );
    }
    Ok(())
}

fn validate_persisted_manifest(root: &Path, manifest: &RuntimeManifest) -> HelperResult<()> {
    validate_persisted_manifest_identity(root, manifest)?;
    let version_dir = manifest.service_binary_path.parent().ok_or_else(|| {
        (
            ExitCode::ForeignState,
            "manifest-version-parent-missing".into(),
        )
    })?;
    windows::verify_managed_directory(version_dir)
        .map_err(|reason| (ExitCode::ForeignState, reason))?;
    windows::validate_managed_binary(&manifest.service_binary_path, manifest)
        .map_err(|reason| (ExitCode::ForeignState, reason))
}

fn validate_persisted_transaction_identity(
    root: &Path,
    journal: &TransactionJournal,
) -> HelperResult<()> {
    if journal.schema_version != 1 {
        return fail(ExitCode::ForeignState, "foreign-transaction-schema");
    }
    validate_persisted_manifest_identity(root, &journal.new_manifest)?;
    if let Some(old) = journal.old_manifest.as_ref() {
        validate_persisted_manifest_identity(root, old)?;
    }
    Ok(())
}

fn invoke_validated_persisted_service_command(
    root: &Path,
    manifest: &RuntimeManifest,
    verb: &str,
    sddl: Option<&str>,
) -> HelperResult<()> {
    transaction::validated_persisted_execution(
        || validate_persisted_manifest(root, manifest),
        || {
            windows::invoke_service_command(&manifest.service_binary_path, verb, sddl)
                .map_err(|reason| (ExitCode::NativeFailure, reason))
        },
    )
}

fn service_matches(manifest: &RuntimeManifest) -> HelperResult<bool> {
    match windows::query_service().map_err(|reason| (ExitCode::NativeFailure, reason))? {
        ServiceState::Absent => Ok(false),
        ServiceState::Running(command) | ServiceState::Stopped(command)
            if windows::validate_service_command(&command, manifest) =>
        {
            Ok(true)
        }
        _ => fail(ExitCode::ForeignState, "foreign-or-ambiguous-service"),
    }
}

fn prepare_install(request: &Request) -> HelperResult<()> {
    let root = windows::managed_root().map_err(|reason| (ExitCode::NativeFailure, reason))?;
    if root.exists() {
        windows::verify_managed_root(&root).map_err(|reason| (ExitCode::ForeignState, reason))?;
    }
    let durable = root.join(TRANSACTION_MANIFEST_NAME);
    if durable.exists() {
        windows::verify_managed_file(&durable)
            .map_err(|reason| (ExitCode::ForeignState, reason))?;
        let journal: TransactionJournal = read_json(&durable)?;
        write_staging_text(&request.journal_path, journal.transaction_id.as_str())?;
        return fail(ExitCode::RecoveryRequired, "recovery-required");
    }
    let old_manifest = if root.exists() {
        match read_runtime_manifest(&root)? {
            Some(manifest) => {
                windows::validate_managed_binary(&manifest.service_binary_path, &manifest)
                    .map_err(|reason| (ExitCode::ForeignState, reason))?;
                service_matches(&manifest)?;
                Some(manifest)
            }
            None => {
                let entries = fs::read_dir(&root)
                    .map_err(|_| (ExitCode::ForeignState, "managed-root-read-failed".into()))?
                    .map(|entry| {
                        entry
                            .ok()
                            .and_then(|entry| entry.file_name().into_string().ok())
                            .ok_or(())
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| (ExitCode::ForeignState, "managed-root-entry-invalid".into()))?;
                let names = entries.iter().map(String::as_str).collect::<Vec<_>>();
                match transaction::classify_bootstrap(&names) {
                    transaction::BootstrapDisposition::RecoverableEmpty
                    | transaction::BootstrapDisposition::RecoverableTemp => {
                        write_staging_text(&request.journal_path, "bootstrap")?;
                        return fail(ExitCode::RecoveryRequired, "bootstrap-recovery-required");
                    }
                    transaction::BootstrapDisposition::Foreign => {
                        return fail(ExitCode::ForeignState, "foreign-managed-root")
                    }
                }
            }
        }
    } else {
        None
    };
    let sid = old_manifest
        .as_ref()
        .map(|manifest| manifest.desktop_user_sid.clone())
        .map(Ok)
        .unwrap_or_else(|| windows::desktop_user_sid(request.parent_pid))
        .map_err(|reason| (ExitCode::ValidationFailed, reason))?;
    let staging = StagingJournal {
        schema_version: 1,
        operation_id: request.operation_id.clone(),
        expanded_sddl: PIPE_SDDL_TEMPLATE.replace("${desktop_user_sid}", &sid),
        desktop_user_sid: sid,
        old_manifest,
        recovered_transaction_id: None,
    };
    write_staging_json(&request.journal_path, &staging)
}

fn write_staging_json<T: serde::Serialize>(path: &Path, value: &T) -> HelperResult<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| {
        (
            ExitCode::NativeFailure,
            "staging-json-serialize-failed".into(),
        )
    })?;
    let mut file = File::create(path).map_err(|_| {
        (
            ExitCode::NativeFailure,
            "staging-journal-create-failed".into(),
        )
    })?;
    file.write_all(&bytes)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            (
                ExitCode::NativeFailure,
                "staging-journal-flush-failed".into(),
            )
        })
}

fn write_staging_text(path: &Path, value: &str) -> HelperResult<()> {
    let mut file = File::create(path).map_err(|_| {
        (
            ExitCode::NativeFailure,
            "staging-output-create-failed".into(),
        )
    })?;
    file.write_all(value.as_bytes())
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            (
                ExitCode::NativeFailure,
                "staging-output-flush-failed".into(),
            )
        })
}

fn recover_install(request: &Request, lease: &AdmissionLease) -> HelperResult<()> {
    lease
        .ensure_parent_alive()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    let root = windows::managed_root().map_err(|reason| (ExitCode::NativeFailure, reason))?;
    windows::verify_managed_root(&root).map_err(|reason| (ExitCode::ForeignState, reason))?;
    let durable_path = root.join(TRANSACTION_MANIFEST_NAME);
    if durable_path.exists() {
        windows::verify_managed_file(&durable_path)
            .map_err(|reason| (ExitCode::ForeignState, reason))?;
        let durable: TransactionJournal = read_json(&durable_path)?;
        validate_recovery_handoff(
            &request.operation_id,
            request.recovered_transaction_id.as_ref(),
            &durable.transaction_id,
        )
        .map_err(|reason| (ExitCode::ForeignState, reason.into()))?;
        converge_rollback(&root, &durable)?;
        fs::remove_file(&durable_path).map_err(|_| {
            (
                ExitCode::NativeFailure,
                "durable-journal-delete-failed".into(),
            )
        })?;
        remove_durable_temp_if_present(&root, &durable.transaction_id)?;
    } else {
        if request.recovered_transaction_id.is_some() {
            return fail(
                ExitCode::ForeignState,
                "unexpected-bootstrap-transaction-id",
            );
        }
        let entries = fs::read_dir(&root)
            .map_err(|_| (ExitCode::ForeignState, "bootstrap-root-read-failed".into()))?
            .map(|entry| {
                entry
                    .ok()
                    .and_then(|entry| entry.file_name().into_string().ok())
                    .ok_or(())
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| (ExitCode::ForeignState, "bootstrap-entry-invalid".into()))?;
        let names = entries.iter().map(String::as_str).collect::<Vec<_>>();
        match transaction::classify_bootstrap(&names) {
            transaction::BootstrapDisposition::RecoverableEmpty => {}
            transaction::BootstrapDisposition::RecoverableTemp => {
                windows::verify_managed_file(&root.join(&entries[0]))
                    .map_err(|reason| (ExitCode::ForeignState, reason))?;
                fs::remove_file(root.join(&entries[0])).map_err(|_| {
                    (
                        ExitCode::NativeFailure,
                        "bootstrap-temp-delete-failed".into(),
                    )
                })?;
            }
            transaction::BootstrapDisposition::Foreign => {
                return fail(ExitCode::ForeignState, "foreign-bootstrap-state")
            }
        }
        fs::remove_dir(&root).map_err(|_| {
            (
                ExitCode::NativeFailure,
                "bootstrap-root-delete-failed".into(),
            )
        })?;
    }
    let mut fresh = request.clone();
    fresh.action = Action::PrepareInstall;
    fresh.recovered_transaction_id = None;
    prepare_install(&fresh)
}

fn remove_durable_temp_if_present(root: &Path, id: &OperationId) -> HelperResult<()> {
    let path = root.join(transaction::durable_temp_name("managed-transaction", id));
    if path.exists() {
        windows::verify_managed_file(&path).map_err(|reason| (ExitCode::ForeignState, reason))?;
        fs::remove_file(path)
            .map_err(|_| (ExitCode::NativeFailure, "durable-temp-delete-failed".into()))?;
    }
    Ok(())
}

fn persist_phase(
    root: &Path,
    journal: &mut TransactionJournal,
    phase: Phase,
    lease: &AdmissionLease,
) -> HelperResult<()> {
    lease
        .ensure_parent_alive()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    journal.phase = phase;
    atomic_json(
        &root.join(TRANSACTION_MANIFEST_NAME),
        journal,
        &journal.transaction_id,
    )
}

fn commit_install(request: &Request, lease: &AdmissionLease) -> HelperResult<()> {
    let staging: StagingJournal = read_json(&request.journal_path)?;
    validate_same_transaction(&request.operation_id, &staging.operation_id)
        .map_err(|reason| (ExitCode::ForeignState, reason.into()))?;
    let install_dir = request
        .install_dir
        .as_ref()
        .ok_or_else(|| (ExitCode::InvalidInvocation, "missing-install-dir".into()))?;
    let (mut source, lock) = windows::validate_bundle_sources(install_dir)
        .map_err(|reason| (ExitCode::ValidationFailed, reason))?;
    let program_data_root =
        windows::managed_root().map_err(|reason| (ExitCode::NativeFailure, reason))?;
    lease
        .ensure_parent_alive()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    if !program_data_root.exists() {
        let parent = program_data_root
            .parent()
            .ok_or_else(|| (ExitCode::NativeFailure, "managed-parent-missing".into()))?;
        if !parent.exists() {
            windows::create_protected_directory(parent, windows::MANAGED_ROOT_SDDL)
                .map_err(|reason| (ExitCode::ForeignState, reason))?;
        } else {
            windows::verify_managed_parent(parent)
                .map_err(|reason| (ExitCode::ForeignState, reason))?;
        }
        windows::create_protected_directory(&program_data_root, windows::MANAGED_ROOT_SDDL)
            .map_err(|reason| (ExitCode::ForeignState, reason))?;
        checkpoint("bootstrap-root-created")?;
    }
    windows::verify_managed_root(&program_data_root)
        .map_err(|reason| (ExitCode::ForeignState, reason))?;
    let version_dir = program_data_root.join(&lock.version);
    let target = version_dir.join("Everything.exe");
    let new_manifest = RuntimeManifest {
        schema_version: 1,
        version: lock.version,
        architecture: lock.architecture,
        sha256: lock.everything_exe_sha256,
        publisher: lock.authenticode_publisher,
        instance: model::INSTANCE_NAME.into(),
        service_name: model::SERVICE_NAME.into(),
        pipe_name: model::PIPE_NAME.into(),
        desktop_user_sid: staging.desktop_user_sid.clone(),
        expanded_sddl: staging.expanded_sddl.clone(),
        service_binary_path: target.clone(),
    };
    new_manifest
        .validate_fixed_identity()
        .map_err(|reason| (ExitCode::ValidationFailed, reason))?;
    let mut durable = TransactionJournal {
        schema_version: 1,
        transaction_id: request.operation_id.clone(),
        phase: Phase::JournalCreated,
        old_manifest: staging.old_manifest.clone(),
        new_manifest,
    };
    atomic_json(
        &program_data_root.join(TRANSACTION_MANIFEST_NAME),
        &durable,
        &request.operation_id,
    )?;
    let result = (|| {
        if version_dir.exists() {
            let disposition = candidate_directory_disposition(
                durable
                    .old_manifest
                    .as_ref()
                    .map(|old| old.service_binary_path.as_path()),
                &target,
            );
            if disposition == CandidateDirectoryDisposition::MustBeAbsent {
                return fail(
                    ExitCode::ForeignState,
                    "foreign-preexisting-version-directory",
                );
            }
            windows::verify_managed_directory(&version_dir)
                .map_err(|reason| (ExitCode::ForeignState, reason))?;
        } else {
            windows::create_protected_directory(&version_dir, windows::MANAGED_ROOT_SDDL)
                .map_err(|reason| (ExitCode::ForeignState, reason))?;
        }
        let target_temp = version_dir.join(format!(
            "Everything.{}.tmp",
            request.operation_id.as_str().replace('-', "")
        ));
        let copied_hash = windows::copy_from_held_source(&mut source, &target_temp)
            .map_err(|reason| (ExitCode::ValidationFailed, reason))?;
        if copied_hash != durable.new_manifest.sha256 {
            return fail(ExitCode::ValidationFailed, "copied-target-hash-mismatch");
        }
        windows::verify_authenticode_publisher(&target_temp, &durable.new_manifest.publisher)
            .map_err(|reason| (ExitCode::ValidationFailed, reason))?;
        windows::atomic_replace(&target_temp, &target)
            .map_err(|reason| (ExitCode::NativeFailure, reason))?;
        windows::validate_managed_binary(&target, &durable.new_manifest)
            .map_err(|reason| (ExitCode::ValidationFailed, reason))?;
        persist_phase(
            &program_data_root,
            &mut durable,
            Phase::NewBinaryVerified,
            lease,
        )?;
        checkpoint("new-binary-verified")?;
        if let Some(old) = durable.old_manifest.as_ref() {
            if service_matches(old)? {
                invoke_validated_persisted_service_command(
                    &program_data_root,
                    old,
                    "-stop-service",
                    None,
                )?;
            }
        }
        persist_phase(
            &program_data_root,
            &mut durable,
            Phase::OldServiceStopped,
            lease,
        )?;
        checkpoint("old-service-stopped")?;
        if let Some(old) = durable.old_manifest.as_ref() {
            invoke_validated_persisted_service_command(
                &program_data_root,
                old,
                "-uninstall-service",
                None,
            )?;
        }
        persist_phase(
            &program_data_root,
            &mut durable,
            Phase::OldServiceRemoved,
            lease,
        )?;
        checkpoint("old-service-removed")?;
        windows::invoke_service_command(
            &target,
            "-install-service",
            Some(&durable.new_manifest.expanded_sddl),
        )
        .map_err(|reason| (ExitCode::NativeFailure, reason))?;
        persist_phase(
            &program_data_root,
            &mut durable,
            Phase::NewServiceInstalled,
            lease,
        )?;
        checkpoint("new-service-installed")?;
        windows::invoke_service_command(&target, "-start-service", None)
            .map_err(|reason| (ExitCode::NativeFailure, reason))?;
        persist_phase(
            &program_data_root,
            &mut durable,
            Phase::NewServiceRunning,
            lease,
        )?;
        checkpoint("new-service-started")?;
        match windows::query_service().map_err(|reason| (ExitCode::NativeFailure, reason))? {
            ServiceState::Running(command)
                if windows::validate_service_command(&command, &durable.new_manifest) => {}
            _ => {
                return fail(
                    ExitCode::ValidationFailed,
                    "new-service-not-running-or-mismatched",
                )
            }
        }
        atomic_json(
            &program_data_root.join(RUNTIME_MANIFEST_NAME),
            &durable.new_manifest,
            &request.operation_id,
        )?;
        persist_phase(
            &program_data_root,
            &mut durable,
            Phase::ManifestReplaced,
            lease,
        )?;
        checkpoint("runtime-manifest-replaced")?;
        if let Some(old) = durable.old_manifest.as_ref() {
            if old.service_binary_path != durable.new_manifest.service_binary_path {
                let old_dir = old
                    .service_binary_path
                    .parent()
                    .ok_or_else(|| (ExitCode::ForeignState, "old-version-parent-missing".into()))?;
                windows::verify_managed_directory(old_dir)
                    .map_err(|reason| (ExitCode::ForeignState, reason))?;
                fs::remove_dir_all(old_dir)
                    .map_err(|_| (ExitCode::NativeFailure, "old-version-delete-failed".into()))?;
            }
        }
        fs::remove_file(program_data_root.join(TRANSACTION_MANIFEST_NAME)).map_err(|_| {
            (
                ExitCode::NativeFailure,
                "durable-journal-delete-failed".into(),
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let rollback = converge_rollback(&program_data_root, &durable);
        if rollback.is_ok() {
            let _ = fs::remove_file(program_data_root.join(TRANSACTION_MANIFEST_NAME));
        }
    }
    result
}

fn classify_live(journal: &TransactionJournal) -> HelperResult<LiveService> {
    match windows::query_service().map_err(|reason| (ExitCode::NativeFailure, reason))? {
        ServiceState::Absent => Ok(LiveService::Absent),
        ServiceState::Running(command) => {
            let new_matches = windows::validate_service_command(&command, &journal.new_manifest);
            let old_matches = journal
                .old_manifest
                .as_ref()
                .is_some_and(|old| windows::validate_service_command(&command, old));
            Ok(classify_matching_service(
                ServiceActivity::Running,
                old_matches,
                new_matches,
            ))
        }
        ServiceState::Stopped(command) => {
            let new_matches = windows::validate_service_command(&command, &journal.new_manifest);
            let old_matches = journal
                .old_manifest
                .as_ref()
                .is_some_and(|old| windows::validate_service_command(&command, old));
            Ok(classify_matching_service(
                ServiceActivity::Stopped,
                old_matches,
                new_matches,
            ))
        }
    }
}

fn converge_rollback(root: &Path, journal: &TransactionJournal) -> HelperResult<()> {
    validate_persisted_transaction_identity(root, journal)?;
    checkpoint("rollback-started")?;
    match rollback_decision(classify_live(journal)?) {
        RollbackDecision::StopAndRemoveNewThenRestoreOld => {
            invoke_validated_persisted_service_command(
                root,
                &journal.new_manifest,
                "-stop-service",
                None,
            )
            .map_err(|(_, reason)| (ExitCode::RollbackFailed, reason))?;
            invoke_validated_persisted_service_command(
                root,
                &journal.new_manifest,
                "-uninstall-service",
                None,
            )
            .map_err(|(_, reason)| (ExitCode::RollbackFailed, reason))?;
            restore_old(root, journal)?;
        }
        RollbackDecision::RemoveNewThenRestoreOld => {
            invoke_validated_persisted_service_command(
                root,
                &journal.new_manifest,
                "-uninstall-service",
                None,
            )
            .map_err(|(_, reason)| (ExitCode::RollbackFailed, reason))?;
            restore_old(root, journal)?;
        }
        RollbackDecision::RestoreOld => restore_old(root, journal)?,
        RollbackDecision::KeepOld => {}
        RollbackDecision::FailClosedRetainAll => {
            return fail(ExitCode::RollbackFailed, "rollback-live-state-ambiguous")
        }
    }
    if let Some(old) = journal.old_manifest.as_ref() {
        atomic_json(
            &root.join(RUNTIME_MANIFEST_NAME),
            old,
            &journal.transaction_id,
        )?;
    } else if root.join(RUNTIME_MANIFEST_NAME).exists() {
        fs::remove_file(root.join(RUNTIME_MANIFEST_NAME)).map_err(|_| {
            (
                ExitCode::RollbackFailed,
                "rollback-manifest-delete-failed".into(),
            )
        })?;
    }
    cleanup_rollback_candidate(root, journal)?;
    checkpoint("rollback-complete")
}

fn cleanup_rollback_candidate(root: &Path, journal: &TransactionJournal) -> HelperResult<()> {
    let candidate_dir = journal
        .new_manifest
        .service_binary_path
        .parent()
        .ok_or_else(|| {
            (
                ExitCode::RollbackFailed,
                "new-version-parent-missing".into(),
            )
        })?;
    let referenced_by_old = journal
        .old_manifest
        .as_ref()
        .is_some_and(|old| old.service_binary_path.parent() == Some(candidate_dir));
    let unreferenced = !referenced_by_old;
    match rollback_cleanup_step(candidate_dir.exists(), unreferenced, referenced_by_old) {
        RollbackCleanupStep::RemoveCandidateDirectory => {
            validate_persisted_manifest_identity(root, &journal.new_manifest)?;
            windows::verify_managed_directory(candidate_dir)
                .map_err(|reason| (ExitCode::RollbackFailed, reason))?;
            let entries = fs::read_dir(candidate_dir)
                .map_err(|_| {
                    (
                        ExitCode::RollbackFailed,
                        "rollback-candidate-read-failed".into(),
                    )
                })?
                .map(|entry| {
                    let entry = entry.map_err(|_| {
                        (
                            ExitCode::RollbackFailed,
                            "rollback-candidate-entry-invalid".into(),
                        )
                    })?;
                    let name = entry.file_name().into_string().map_err(|_| {
                        (
                            ExitCode::RollbackFailed,
                            "rollback-candidate-entry-invalid".into(),
                        )
                    })?;
                    Ok((name, entry.path()))
                })
                .collect::<HelperResult<Vec<_>>>()?;
            let names = entries
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>();
            if !transaction::rollback_candidate_entries_are_scoped(&names, &journal.transaction_id)
            {
                return fail(
                    ExitCode::RollbackFailed,
                    "rollback-candidate-content-ambiguous",
                );
            }
            for (name, path) in &entries {
                if name == "Everything.exe" {
                    validate_persisted_manifest(root, &journal.new_manifest)
                        .map_err(|(_, reason)| (ExitCode::RollbackFailed, reason))?;
                } else {
                    windows::verify_managed_file(path)
                        .map_err(|reason| (ExitCode::RollbackFailed, reason))?;
                }
            }
            fs::remove_dir_all(candidate_dir).map_err(|_| {
                (
                    ExitCode::RollbackFailed,
                    "rollback-candidate-delete-failed".into(),
                )
            })
        }
        RollbackCleanupStep::KeepReferencedCandidate | RollbackCleanupStep::Complete => Ok(()),
        RollbackCleanupStep::FailClosedRetainCandidate => fail(
            ExitCode::RollbackFailed,
            "rollback-candidate-reference-ambiguous",
        ),
    }
}

fn restore_old(root: &Path, journal: &TransactionJournal) -> HelperResult<()> {
    if let Some(old) = journal.old_manifest.as_ref() {
        invoke_validated_persisted_service_command(
            root,
            old,
            "-install-service",
            Some(&old.expanded_sddl),
        )
        .map_err(|(_, reason)| (ExitCode::RollbackFailed, reason))?;
        invoke_validated_persisted_service_command(root, old, "-start-service", None)
            .map_err(|(_, reason)| (ExitCode::RollbackFailed, reason))?;
        if !service_matches(old)? {
            return fail(ExitCode::RollbackFailed, "old-service-restore-mismatch");
        }
    }
    Ok(())
}

fn rollback_install(request: &Request, lease: &AdmissionLease) -> HelperResult<()> {
    lease
        .ensure_parent_alive()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    let root = windows::managed_root().map_err(|reason| (ExitCode::NativeFailure, reason))?;
    windows::verify_managed_root(&root).map_err(|reason| (ExitCode::ForeignState, reason))?;
    let durable = root.join(TRANSACTION_MANIFEST_NAME);
    windows::verify_managed_file(&durable).map_err(|reason| (ExitCode::ForeignState, reason))?;
    let journal: TransactionJournal = read_json(&durable)?;
    validate_same_transaction(&request.operation_id, &journal.transaction_id)
        .map_err(|reason| (ExitCode::ForeignState, reason.into()))?;
    converge_rollback(&root, &journal)?;
    fs::remove_file(durable).map_err(|_| {
        (
            ExitCode::RollbackFailed,
            "rollback-journal-delete-failed".into(),
        )
    })?;
    remove_durable_temp_if_present(&root, &journal.transaction_id)
}

fn pre_uninstall(_request: &Request, lease: &AdmissionLease) -> HelperResult<()> {
    lease
        .ensure_parent_alive()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    let root = windows::managed_root().map_err(|reason| (ExitCode::NativeFailure, reason))?;
    if root.exists() {
        windows::verify_managed_root(&root).map_err(|reason| (ExitCode::ForeignState, reason))?;
    }
    let durable = root.join(TRANSACTION_MANIFEST_NAME);
    if durable.exists() {
        windows::verify_managed_file(&durable)
            .map_err(|reason| (ExitCode::ForeignState, reason))?;
        let journal: TransactionJournal = read_json(&durable)?;
        converge_rollback(&root, &journal)?;
        fs::remove_file(&durable).map_err(|_| {
            (
                ExitCode::RollbackFailed,
                "uninstall-recovery-journal-delete-failed".into(),
            )
        })?;
        remove_durable_temp_if_present(&root, &journal.transaction_id)?;
    }
    let Some(manifest) = read_runtime_manifest(&root)? else {
        return if root.exists() {
            fail(ExitCode::ForeignState, "uninstall-manifest-missing")
        } else {
            Ok(())
        };
    };
    match windows::query_service().map_err(|reason| (ExitCode::NativeFailure, reason))? {
        ServiceState::Absent => Ok(()),
        ServiceState::Running(command) | ServiceState::Stopped(command)
            if windows::validate_service_command(&command, &manifest) =>
        {
            lease
                .ensure_parent_alive()
                .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
            invoke_validated_persisted_service_command(&root, &manifest, "-stop-service", None)?;
            invoke_validated_persisted_service_command(&root, &manifest, "-uninstall-service", None)
        }
        _ => fail(ExitCode::ForeignState, "foreign-service-on-uninstall"),
    }
}

fn post_uninstall(_request: &Request, lease: &AdmissionLease) -> HelperResult<()> {
    lease
        .ensure_parent_alive()
        .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
    let root = windows::managed_root().map_err(|reason| (ExitCode::NativeFailure, reason))?;
    if !root.exists() {
        return Ok(());
    }
    windows::verify_managed_root(&root).map_err(|reason| (ExitCode::ForeignState, reason))?;
    let Some(manifest) = read_runtime_manifest(&root)? else {
        let names = fs::read_dir(&root)
            .map_err(|_| {
                (
                    ExitCode::ForeignState,
                    "post-uninstall-root-read-failed".into(),
                )
            })?
            .map(|entry| {
                entry
                    .ok()
                    .and_then(|entry| entry.file_name().into_string().ok())
                    .ok_or(())
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                (
                    ExitCode::ForeignState,
                    "post-uninstall-entry-invalid".into(),
                )
            })?;
        return if names.is_empty() {
            fs::remove_dir(&root)
                .map_err(|_| (ExitCode::NativeFailure, "empty-root-delete-failed".into()))
        } else {
            fail(ExitCode::ForeignState, "manifest-missing-with-content")
        };
    };
    if !matches!(
        windows::query_service().map_err(|reason| (ExitCode::NativeFailure, reason))?,
        ServiceState::Absent
    ) {
        return fail(ExitCode::ForeignState, "service-still-present");
    }
    let version_dir = manifest.service_binary_path.parent().ok_or_else(|| {
        (
            ExitCode::ForeignState,
            "manifest-version-parent-missing".into(),
        )
    })?;
    let manifest_path = root.join(RUNTIME_MANIFEST_NAME);
    loop {
        match transaction::uninstall_step(version_dir.exists(), false, manifest_path.exists()) {
            UninstallStep::DeleteNewDirectory => {
                lease
                    .ensure_parent_alive()
                    .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
                windows::verify_managed_directory(version_dir)
                    .map_err(|reason| (ExitCode::ForeignState, reason))?;
                fs::remove_dir_all(version_dir).map_err(|_| {
                    (
                        ExitCode::NativeFailure,
                        "referenced-version-delete-failed".into(),
                    )
                })?;
                checkpoint("uninstall-referenced-directory-deleted")?;
            }
            UninstallStep::DeleteManifest => {
                lease
                    .ensure_parent_alive()
                    .map_err(|reason| (ExitCode::AdmissionDenied, reason))?;
                fs::remove_file(&manifest_path).map_err(|_| {
                    (
                        ExitCode::NativeFailure,
                        "runtime-manifest-delete-failed".into(),
                    )
                })?;
                checkpoint("uninstall-manifest-deleted")?;
            }
            UninstallStep::Complete => break,
            UninstallStep::DeleteOldDirectory => {
                return fail(
                    ExitCode::ForeignState,
                    "unexpected-second-manifest-reference",
                )
            }
        }
    }
    fs::remove_dir(&root).map_err(|_| (ExitCode::ForeignState, "managed-root-not-empty".into()))
}

#[cfg(feature = "fault-injection")]
fn checkpoint(name: &str) -> HelperResult<()> {
    verify_vm_disposable_marker()?;
    if env::var("UIPILOT_VM_FAULT_CHECKPOINT").ok().as_deref() == Some(name) {
        return fail(ExitCode::NativeFailure, format!("fault-injection:{name}"));
    }
    Ok(())
}

#[cfg(feature = "fault-injection")]
fn verify_vm_disposable_marker() -> HelperResult<()> {
    let marker = env::var_os("UIPILOT_VM_DISPOSABLE_MARKER").ok_or_else(|| {
        (
            ExitCode::AdmissionDenied,
            "vm-disposable-marker-missing".into(),
        )
    })?;
    if !Path::new(&marker).is_file() {
        return fail(ExitCode::AdmissionDenied, "vm-disposable-marker-invalid");
    }
    Ok(())
}

#[cfg(not(feature = "fault-injection"))]
fn verify_vm_disposable_marker() -> HelperResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_manifest_semantics_and_location_are_validated_before_native_use() {
        let root = Path::new(r"C:\ProgramData\UiPilot_tools\everything");
        let manifest = RuntimeManifest::fixture();
        assert!(validate_persisted_manifest_identity(root, &manifest).is_ok());

        let mut foreign = manifest.clone();
        foreign.service_name = "Everything".into();
        assert!(validate_persisted_manifest_identity(root, &foreign).is_err());

        let mut escaped = manifest;
        escaped.service_binary_path = PathBuf::from(r"C:\foreign\Everything.exe");
        assert!(validate_persisted_manifest_identity(root, &escaped).is_err());
    }

    #[test]
    fn persisted_transaction_rejects_invalid_manifest_semantics_before_convergence() {
        let root = Path::new(r"C:\ProgramData\UiPilot_tools\everything");
        let mut journal = TransactionJournal {
            schema_version: 1,
            transaction_id: OperationId::fixture(1),
            phase: Phase::NewBinaryVerified,
            old_manifest: Some(RuntimeManifest::fixture()),
            new_manifest: RuntimeManifest::fixture(),
        };
        assert!(validate_persisted_transaction_identity(root, &journal).is_ok());
        journal.new_manifest.service_name = "Everything".into();
        assert!(validate_persisted_transaction_identity(root, &journal).is_err());
        journal.new_manifest = RuntimeManifest::fixture();
        journal.schema_version = 2;
        assert!(validate_persisted_transaction_identity(root, &journal).is_err());
    }
}

#[cfg(not(feature = "fault-injection"))]
fn checkpoint(_name: &str) -> HelperResult<()> {
    Ok(())
}
