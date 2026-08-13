use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    manifest::PublicPermission, package, runtime_label, stage_public_package,
    EffectivePluginConfig, PluginApiRequest, PluginCommandCompletion, PluginCompletionOutcome,
    PluginRequestScheduler, PluginRuntimeApi, PluginRuntimeError, PluginSecretStore,
    PluginStateError, PluginStateStore, PluginStorageStore, PreparedPublicPlugin, PublicManifestV1,
    PublicPackageError, PublicPackageSource, PublicPluginFault, PublicPluginHost, PublicResource,
};

const PREPARE_TTL: Duration = Duration::from_secs(5 * 60);
const RUNTIME_HOST_PATH: &str = "__uipilot_runtime.html";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum PublicPluginInstallSource {
    Archive { path: PathBuf },
    DevelopmentDirectory { path: PathBuf },
}

impl PublicPluginInstallSource {
    fn into_package_source(self) -> PublicPackageSource {
        match self {
            Self::Archive { path } => PublicPackageSource::Archive(path),
            Self::DevelopmentDirectory { path } => PublicPackageSource::DevelopmentDirectory(path),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginPrepareSummary {
    pub(crate) token: String,
    pub(crate) plugin_id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) permissions: Vec<PublicPermission>,
    pub(crate) is_update: bool,
    pub(crate) source_verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginMutation {
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
    pub(crate) inventory_revision: u64,
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicRuntimeCandidate {
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
    pub(crate) label: String,
    pub(crate) runtime_entry: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicActivationCommit {
    pub(crate) mutation: PublicPluginMutation,
    pub(crate) runtime: PublicRuntimeCandidate,
    pub(crate) previous_runtime_label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicEnabledCommit {
    pub(crate) mutation: PublicPluginMutation,
    pub(crate) runtime: Option<PublicRuntimeCandidate>,
    pub(crate) closed_runtime_label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PublicPluginManagementError {
    InvalidPackage,
    IncompatiblePlatform,
    IncompatibleApi,
    UnsupportedPermission,
    PermissionDenied,
    RuntimeNotReady,
    NameConflict,
    InvalidToken,
    ExpiredToken,
    InvalidCaller,
    Unavailable,
}

impl PublicPluginManagementError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::InvalidPackage => "invalidPackage",
            Self::IncompatiblePlatform => "incompatiblePlatform",
            Self::IncompatibleApi => "incompatibleApi",
            Self::UnsupportedPermission => "unsupportedPermission",
            Self::PermissionDenied => "permissionDenied",
            Self::RuntimeNotReady => "runtimeNotReady",
            Self::NameConflict => "nameConflict",
            Self::InvalidToken => "invalidToken",
            Self::ExpiredToken => "expiredToken",
            Self::InvalidCaller => "invalidCaller",
            Self::Unavailable => "pluginUnavailable",
        }
    }
}

impl fmt::Display for PublicPluginManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for PublicPluginManagementError {}

impl From<PublicPackageError> for PublicPluginManagementError {
    fn from(error: PublicPackageError) -> Self {
        match error {
            PublicPackageError::InvalidPackage => Self::InvalidPackage,
            PublicPackageError::IncompatiblePlatform => Self::IncompatiblePlatform,
            PublicPackageError::IncompatibleApi => Self::IncompatibleApi,
            PublicPackageError::UnsupportedPermission => Self::UnsupportedPermission,
        }
    }
}

impl From<PluginStateError> for PublicPluginManagementError {
    fn from(error: PluginStateError) -> Self {
        match error {
            PluginStateError::NameConflict { .. } => Self::NameConflict,
            PluginStateError::InvalidPermissions => Self::PermissionDenied,
            _ => Self::Unavailable,
        }
    }
}

#[derive(Clone)]
struct RuntimeResource {
    mime: &'static str,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct RuntimeSnapshot {
    manifest: PublicManifestV1,
    digest: String,
    package_root: PathBuf,
    generation: u64,
    label: String,
    resources: BTreeMap<String, RuntimeResource>,
}

impl RuntimeSnapshot {
    fn candidate(&self) -> PublicRuntimeCandidate {
        PublicRuntimeCandidate {
            plugin_id: self.manifest.plugin_id.clone(),
            generation: self.generation,
            label: self.label.clone(),
            runtime_entry: self.manifest.runtime.entry.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicRuntimeAsset {
    pub(crate) mime: &'static str,
    pub(crate) bytes: Vec<u8>,
}

struct PendingActivation {
    caller_label: String,
    expires_at: Instant,
    prepared: PreparedPublicPlugin,
}

#[derive(Default)]
struct ActivationData {
    pending: HashMap<String, PendingActivation>,
    active_by_plugin: HashMap<String, Arc<RuntimeSnapshot>>,
    staged_by_label: HashMap<String, Arc<RuntimeSnapshot>>,
}

pub(crate) struct PublicPluginManager {
    host: PublicPluginHost,
    staging_root: PathBuf,
    packages_root: PathBuf,
    state: Arc<PluginStateStore>,
    storage: Arc<PluginStorageStore>,
    secrets: Arc<PluginSecretStore>,
    scheduler: Arc<PluginRequestScheduler>,
    api: PluginRuntimeApi,
    mutation: Mutex<()>,
    data: Mutex<ActivationData>,
    next_token: AtomicU64,
}

impl PublicPluginManager {
    pub(crate) fn load(
        app_data_dir: &Path,
        host: PublicPluginHost,
        reserved_names: impl IntoIterator<Item = String>,
    ) -> Result<Self, PublicPluginManagementError> {
        let root = app_data_dir.join("public-plugins");
        let staging_root = root.join("staging");
        let packages_root = root.join("packages");
        fs::create_dir_all(&staging_root).map_err(|_| PublicPluginManagementError::Unavailable)?;
        fs::create_dir_all(&packages_root).map_err(|_| PublicPluginManagementError::Unavailable)?;
        let state = Arc::new(PluginStateStore::load(&root.join("state"), reserved_names)?);
        let storage = Arc::new(
            PluginStorageStore::load(&root.join("storage"))
                .map_err(|_| PublicPluginManagementError::Unavailable)?,
        );
        let secrets = Arc::new(
            PluginSecretStore::load(&root.join("secrets"))
                .map_err(|_| PublicPluginManagementError::Unavailable)?,
        );
        let scheduler = Arc::new(PluginRequestScheduler::default());
        let api = PluginRuntimeApi::new(
            Arc::clone(&scheduler),
            Arc::clone(&state),
            Arc::clone(&storage),
            Arc::clone(&secrets),
        );
        let mut active_by_plugin = HashMap::new();
        for config in state.configs()? {
            if !config.installed || config.active_generation == 0 {
                continue;
            }
            let Some(digest) = config.package_digest.as_deref() else {
                continue;
            };
            let package_root = package_destination(&packages_root, &config.plugin_id, digest);
            match load_runtime_snapshot(&package_root, &host, digest, config.active_generation) {
                Ok(snapshot)
                    if snapshot.manifest.plugin_id == config.plugin_id
                        && snapshot.manifest.version == config.version =>
                {
                    let _ = scheduler
                        .invalidate_plugin(&config.plugin_id, Some(config.active_generation));
                    active_by_plugin.insert(config.plugin_id, Arc::new(snapshot));
                }
                _ => {
                    let _ = state.disable_for_fault(
                        &config.plugin_id,
                        PublicPluginFault::RuntimeUnavailable,
                    );
                }
            }
        }
        Ok(Self {
            host,
            staging_root,
            packages_root,
            state,
            storage,
            secrets,
            scheduler,
            api,
            mutation: Mutex::new(()),
            data: Mutex::new(ActivationData {
                active_by_plugin,
                ..ActivationData::default()
            }),
            next_token: AtomicU64::new(1),
        })
    }

    pub(crate) fn prepare(
        &self,
        caller_label: &str,
        source: PublicPluginInstallSource,
        now: Instant,
    ) -> Result<PublicPluginPrepareSummary, PublicPluginManagementError> {
        if caller_label.is_empty() {
            return Err(PublicPluginManagementError::InvalidCaller);
        }
        let _mutation = self.lock_mutation()?;
        self.cleanup_expired(now)?;
        let prepared =
            stage_public_package(source.into_package_source(), &self.staging_root, &self.host)?;
        let is_update = self
            .state
            .config(&prepared.manifest.plugin_id)?
            .is_some_and(|config| config.installed);
        let token_number = self
            .next_token
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let token = format!(
            "public-prepare-{:016x}-{token_number:016x}",
            std::process::id()
        );
        let summary = PublicPluginPrepareSummary {
            token: token.clone(),
            plugin_id: prepared.manifest.plugin_id.clone(),
            name: prepared.manifest.name.clone(),
            version: prepared.manifest.version.clone(),
            permissions: prepared.manifest.permissions.clone(),
            is_update,
            source_verified: false,
        };
        let expires_at = now
            .checked_add(PREPARE_TTL)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        self.lock_data()?.pending.insert(
            token,
            PendingActivation {
                caller_label: caller_label.into(),
                expires_at,
                prepared,
            },
        );
        Ok(summary)
    }

    pub(crate) fn cancel(
        &self,
        caller_label: &str,
        token: &str,
        now: Instant,
    ) -> Result<(), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let mut data = self.lock_data()?;
        let pending = data
            .pending
            .get(token)
            .ok_or(PublicPluginManagementError::InvalidToken)?;
        if pending.expires_at <= now {
            data.pending.remove(token);
            return Err(PublicPluginManagementError::ExpiredToken);
        }
        if pending.caller_label != caller_label {
            return Err(PublicPluginManagementError::PermissionDenied);
        }
        data.pending.remove(token);
        Ok(())
    }

    pub(crate) fn commit_with_readiness<F>(
        &self,
        caller_label: &str,
        token: &str,
        permission_grants: BTreeSet<PublicPermission>,
        now: Instant,
        readiness: F,
    ) -> Result<PublicActivationCommit, PublicPluginManagementError>
    where
        F: FnOnce(&PublicRuntimeCandidate) -> bool,
    {
        let (pending, previous_config, candidate, runtime, previous_runtime_label) = {
            let _mutation = self.lock_mutation()?;
            let pending = {
                let mut data = self.lock_data()?;
                let pending = data
                    .pending
                    .get(token)
                    .ok_or(PublicPluginManagementError::InvalidToken)?;
                if pending.expires_at <= now {
                    data.pending.remove(token);
                    return Err(PublicPluginManagementError::ExpiredToken);
                }
                if pending.caller_label != caller_label {
                    return Err(PublicPluginManagementError::PermissionDenied);
                }
                data.pending
                    .remove(token)
                    .ok_or(PublicPluginManagementError::InvalidToken)?
            };
            let declared = pending
                .prepared
                .manifest
                .permissions
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if declared != permission_grants {
                return Err(PublicPluginManagementError::PermissionDenied);
            }
            let previous_config = self.state.config(&pending.prepared.manifest.plugin_id)?;
            let generation = previous_config
                .as_ref()
                .map_or(Some(1), |config| config.active_generation.checked_add(1))
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let candidate = Arc::new(snapshot_from_prepared(&pending.prepared, generation)?);
            let runtime = candidate.candidate();
            let previous_runtime_label = self
                .lock_data()?
                .active_by_plugin
                .get(&runtime.plugin_id)
                .map(|snapshot| snapshot.label.clone());
            let replaced = self
                .lock_data()?
                .staged_by_label
                .insert(runtime.label.clone(), Arc::clone(&candidate));
            if replaced.is_some() {
                return Err(PublicPluginManagementError::Unavailable);
            }
            (
                pending,
                previous_config,
                candidate,
                runtime,
                previous_runtime_label,
            )
        };

        if !readiness(&runtime) {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::RuntimeNotReady);
        }

        let _mutation = self.lock_mutation()?;
        if self.state.config(&runtime.plugin_id)? != previous_config {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::Unavailable);
        }
        let destination =
            package_destination(&self.packages_root, &runtime.plugin_id, &candidate.digest);
        let created = match pending.prepared.persist(&destination) {
            Ok(created) => created,
            Err(error) => {
                self.unstage(&runtime.label);
                return Err(error.into());
            }
        };
        let config = match self.state.activate(
            &candidate.manifest,
            permission_grants,
            runtime.generation,
            Some(candidate.digest.clone()),
        ) {
            Ok(config) => config,
            Err(error) => {
                if created {
                    package::remove_package_tree(destination);
                }
                self.unstage(&runtime.label);
                return Err(error.into());
            }
        };
        self.scheduler
            .invalidate_plugin(&runtime.plugin_id, Some(runtime.generation))
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        {
            let mut data = self.lock_data()?;
            let staged = data
                .staged_by_label
                .remove(&runtime.label)
                .filter(|staged| Arc::ptr_eq(staged, &candidate))
                .ok_or(PublicPluginManagementError::Unavailable)?;
            data.active_by_plugin
                .insert(runtime.plugin_id.clone(), staged);
        }
        Ok(PublicActivationCommit {
            mutation: mutation_from_config(&config),
            runtime,
            previous_runtime_label,
        })
    }
    pub(crate) fn set_enabled_with_readiness<F>(
        &self,
        plugin_id: &str,
        enabled: bool,
        readiness: F,
    ) -> Result<PublicEnabledCommit, PublicPluginManagementError>
    where
        F: FnOnce(&PublicRuntimeCandidate) -> bool,
    {
        let (before, snapshot, runtime) = {
            let _mutation = self.lock_mutation()?;
            let before = self
                .state
                .config(plugin_id)?
                .filter(|config| config.installed)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let snapshot = self
                .lock_data()?
                .active_by_plugin
                .get(plugin_id)
                .cloned()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let runtime = snapshot.candidate();
            if before.enabled == enabled {
                return Ok(PublicEnabledCommit {
                    mutation: mutation_from_config(&before),
                    runtime: enabled.then_some(runtime),
                    closed_runtime_label: None,
                });
            }
            if !enabled {
                let config = self.state.set_enabled(plugin_id, false)?;
                self.scheduler
                    .invalidate_plugin(plugin_id, None)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                return Ok(PublicEnabledCommit {
                    mutation: mutation_from_config(&config),
                    runtime: None,
                    closed_runtime_label: Some(runtime.label),
                });
            }
            let replaced = self
                .lock_data()?
                .staged_by_label
                .insert(runtime.label.clone(), Arc::clone(&snapshot));
            if replaced.is_some() {
                return Err(PublicPluginManagementError::Unavailable);
            }
            (before, snapshot, runtime)
        };

        if !readiness(&runtime) {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::RuntimeNotReady);
        }

        let _mutation = self.lock_mutation()?;
        if self.state.config(plugin_id)? != Some(before) {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::Unavailable);
        }
        let staged_matches = self
            .lock_data()?
            .staged_by_label
            .get(&runtime.label)
            .is_some_and(|staged| Arc::ptr_eq(staged, &snapshot));
        if !staged_matches {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::Unavailable);
        }
        self.unstage(&runtime.label);
        let config = self.state.set_enabled(plugin_id, true)?;
        Ok(PublicEnabledCommit {
            mutation: mutation_from_config(&config),
            runtime: Some(runtime),
            closed_runtime_label: None,
        })
    }
    pub(crate) fn rename(
        &self,
        plugin_id: &str,
        name_override: Option<&str>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let config = self.state.rename(plugin_id, name_override)?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(mutation_from_config(&config))
    }

    pub(crate) fn save_settings(
        &self,
        plugin_id: &str,
        updates: &BTreeMap<String, Value>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let manifest = self.active_manifest(plugin_id)?;
        let config = self
            .state
            .save_settings(plugin_id, &manifest.settings, updates)?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(mutation_from_config(&config))
    }

    pub(crate) fn save_secret(
        &self,
        plugin_id: &str,
        key: &str,
        value: Option<&str>,
    ) -> Result<(), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let manifest = self.active_manifest(plugin_id)?;
        if !manifest
            .settings
            .iter()
            .any(|setting| setting.key() == key && setting.is_secret())
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        match value {
            Some(value) => self
                .secrets
                .write(plugin_id, key, value)
                .map_err(|_| PublicPluginManagementError::Unavailable),
            None => self
                .secrets
                .remove(plugin_id, key)
                .map_err(|_| PublicPluginManagementError::Unavailable),
        }
    }

    pub(crate) fn uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<Option<String>, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        self.state.uninstall(plugin_id, retain_data)?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let runtime_label = self
            .lock_data()?
            .active_by_plugin
            .remove(plugin_id)
            .map(|snapshot| snapshot.label.clone());
        if !retain_data {
            self.storage
                .uninstall(plugin_id, false)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            self.secrets
                .uninstall(plugin_id, false)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
        }
        let packages = self.packages_root.join(plugin_id);
        if packages.exists() {
            package::remove_package_tree(packages);
        }
        Ok(runtime_label)
    }

    pub(crate) fn runtime_candidates(
        &self,
    ) -> Result<Vec<PublicRuntimeCandidate>, PublicPluginManagementError> {
        let data = self.lock_data()?;
        let mut candidates = Vec::new();
        for (plugin_id, snapshot) in &data.active_by_plugin {
            if self
                .state
                .config(plugin_id)?
                .is_some_and(|config| config.installed && config.enabled)
            {
                candidates.push(snapshot.candidate());
            }
        }
        Ok(candidates)
    }

    pub(crate) fn asset(&self, label: &str, request_path: &str) -> Option<PublicRuntimeAsset> {
        let snapshot = {
            let data = self.data.lock().ok()?;
            data.staged_by_label.get(label).cloned().or_else(|| {
                data.active_by_plugin
                    .values()
                    .find(|snapshot| snapshot.label == label)
                    .cloned()
            })?
        };
        if request_path == "/" || request_path == format!("/{RUNTIME_HOST_PATH}") {
            return Some(PublicRuntimeAsset {
                mime: "text/html",
                bytes: runtime_host_document(&snapshot.manifest.runtime.entry).into_bytes(),
            });
        }
        let relative = decode_request_path(request_path)?;
        let resource = snapshot.resources.get(&relative)?;
        Some(PublicRuntimeAsset {
            mime: resource.mime,
            bytes: resource.bytes.clone(),
        })
    }

    pub(crate) fn execute_api(
        &self,
        caller_label: &str,
        request: PluginApiRequest,
    ) -> Result<Value, PluginRuntimeError> {
        let manifest = self
            .manifest_for_label(caller_label)
            .ok_or(PluginRuntimeError::InvalidCaller)?;
        self.api.execute(caller_label, request, &manifest)
    }

    pub(crate) fn complete(
        &self,
        caller_label: &str,
        completion: &PluginCommandCompletion,
        now: Instant,
    ) -> Result<PluginCompletionOutcome, PluginRuntimeError> {
        let identity =
            super::parse_runtime_label(caller_label).ok_or(PluginRuntimeError::InvalidCaller)?;
        if identity.plugin_id != completion.context.plugin_id
            || identity.generation != completion.context.plugin_generation
            || self.manifest_for_label(caller_label).is_none()
        {
            return Err(PluginRuntimeError::InvalidContext);
        }
        self.scheduler
            .complete(&completion.context, now)
            .map_err(
                |_| match self.scheduler.context_status(&completion.context) {
                    super::PluginContextStatus::Expired => PluginRuntimeError::ExpiredRequest,
                    _ => PluginRuntimeError::InvalidContext,
                },
            )
    }

    pub(crate) fn scheduler(&self) -> &Arc<PluginRequestScheduler> {
        &self.scheduler
    }

    pub(crate) fn state(&self) -> &Arc<PluginStateStore> {
        &self.state
    }

    pub(crate) fn mark_runtime_unavailable(
        &self,
        plugin_id: &str,
    ) -> Result<(), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        self.state
            .disable_for_fault(plugin_id, PublicPluginFault::RuntimeUnavailable)?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(())
    }

    fn active_manifest(
        &self,
        plugin_id: &str,
    ) -> Result<PublicManifestV1, PublicPluginManagementError> {
        self.lock_data()?
            .active_by_plugin
            .get(plugin_id)
            .map(|snapshot| snapshot.manifest.clone())
            .ok_or(PublicPluginManagementError::Unavailable)
    }

    fn manifest_for_label(&self, label: &str) -> Option<PublicManifestV1> {
        let data = self.data.lock().ok()?;
        let snapshot = data
            .active_by_plugin
            .values()
            .find(|snapshot| snapshot.label == label)?;
        self.state
            .config(&snapshot.manifest.plugin_id)
            .ok()
            .flatten()
            .filter(|config| {
                config.installed
                    && config.enabled
                    && config.active_generation == snapshot.generation
                    && config.package_digest.as_deref() == Some(snapshot.digest.as_str())
            })?;
        Some(snapshot.manifest.clone())
    }

    fn cleanup_expired(&self, now: Instant) -> Result<(), PublicPluginManagementError> {
        self.lock_data()?
            .pending
            .retain(|_, pending| pending.expires_at > now);
        Ok(())
    }

    fn unstage(&self, label: &str) {
        if let Ok(mut data) = self.data.lock() {
            data.staged_by_label.remove(label);
        }
    }

    fn lock_data(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ActivationData>, PublicPluginManagementError> {
        self.data
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    fn lock_mutation(&self) -> Result<std::sync::MutexGuard<'_, ()>, PublicPluginManagementError> {
        self.mutation
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }
}

fn mutation_from_config(config: &EffectivePluginConfig) -> PublicPluginMutation {
    PublicPluginMutation {
        plugin_id: config.plugin_id.clone(),
        generation: config.active_generation,
        inventory_revision: config.inventory_revision,
        enabled: config.enabled,
    }
}

fn snapshot_from_prepared(
    prepared: &PreparedPublicPlugin,
    generation: u64,
) -> Result<RuntimeSnapshot, PublicPluginManagementError> {
    prepared.revalidate()?;
    snapshot_from_parts(
        &prepared.package_root,
        prepared.manifest.clone(),
        prepared.digest.clone(),
        prepared.resources.clone(),
        generation,
    )
}

fn load_runtime_snapshot(
    package_root: &Path,
    host: &PublicPluginHost,
    digest: &str,
    generation: u64,
) -> Result<RuntimeSnapshot, PublicPluginManagementError> {
    let (manifest, resources) = package::load_existing(package_root, host, digest)?;
    snapshot_from_parts(package_root, manifest, digest.into(), resources, generation)
}

fn snapshot_from_parts(
    package_root: &Path,
    manifest: PublicManifestV1,
    digest: String,
    resources: BTreeMap<String, PublicResource>,
    generation: u64,
) -> Result<RuntimeSnapshot, PublicPluginManagementError> {
    let label = runtime_label(&manifest.plugin_id, generation)
        .ok_or(PublicPluginManagementError::InvalidPackage)?;
    let mut loaded = BTreeMap::new();
    for (path, resource) in resources {
        let bytes = fs::read(package_root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .map_err(|_| PublicPluginManagementError::InvalidPackage)?;
        if bytes.len() as u64 != resource.length {
            return Err(PublicPluginManagementError::InvalidPackage);
        }
        loaded.insert(
            path,
            RuntimeResource {
                mime: resource.mime,
                bytes,
            },
        );
    }
    Ok(RuntimeSnapshot {
        manifest,
        digest,
        package_root: package_root.to_path_buf(),
        generation,
        label,
        resources: loaded,
    })
}

fn package_destination(root: &Path, plugin_id: &str, digest: &str) -> PathBuf {
    root.join(plugin_id).join(digest)
}

fn runtime_host_document(entry: &str) -> String {
    format!(
        "<!doctype html><html data-runtime-entry=\"/{entry}\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'\"></head><body></body></html>"
    )
}

fn decode_request_path(path: &str) -> Option<String> {
    let raw = path.strip_prefix('/')?;
    if raw.is_empty() || raw.contains(['\\', '?', '#']) {
        return None;
    }
    let bytes = raw.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(output).ok()?;
    (!decoded.starts_with('/')
        && decoded
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != ".."))
    .then_some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use serde_json::json;

    use super::*;
    use crate::public_plugins::PublicPlatform;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-public-activation-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn source(&self) -> PathBuf {
            self.0.join("source")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                package::remove_package_tree(self.0.clone());
            }
        }
    }

    fn manager(dir: &TestDir) -> PublicPluginManager {
        PublicPluginManager::load(
            dir.path(),
            PublicPluginHost::current(PublicPlatform::Windows),
            ["find".into(), "math".into()],
        )
        .unwrap()
    }

    fn write_package(root: &Path, version: &str, marker: &str) {
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pluginId": "com.example.activation",
                "version": version,
                "apiVersion": 1,
                "minimumHostVersion": "0.2.0",
                "name": "Activation",
                "supportedPlatforms": ["windows"],
                "command": {
                    "defaultName": "activation",
                    "activationMode": "live",
                    "outputMode": "mainResult",
                    "inputRequired": false
                },
                "runtime": { "entry": "dist/runtime.js" },
                "permissions": []
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("dist/runtime.js"),
            format!("export async function onCommand() {{ return {{ marker: '{marker}' }}; }}"),
        )
        .unwrap();
    }

    fn source(path: &Path) -> PublicPluginInstallSource {
        PublicPluginInstallSource::DevelopmentDirectory {
            path: path.to_path_buf(),
        }
    }

    fn staging_is_empty(manager: &PublicPluginManager) -> bool {
        fs::read_dir(&manager.staging_root)
            .unwrap()
            .next()
            .is_none()
    }
    fn runtime_staging_is_empty(manager: &PublicPluginManager) -> bool {
        manager.data.lock().unwrap().staged_by_label.is_empty()
    }

    #[test]
    fn prepare_tokens_are_caller_bound_expire_and_cancel_cleanup_staging() {
        let dir = TestDir::new("tokens");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();

        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(prepared.plugin_id, "com.example.activation");
        assert!(!prepared.is_update);
        assert!(!prepared.source_verified);
        assert!(!staging_is_empty(&manager));
        assert_eq!(
            manager.cancel("secondary", &prepared.token, now),
            Err(PublicPluginManagementError::PermissionDenied)
        );
        manager.cancel("main", &prepared.token, now).unwrap();
        assert!(staging_is_empty(&manager));

        let expired = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            manager.cancel("main", &expired.token, now + PREPARE_TTL),
            Err(PublicPluginManagementError::ExpiredToken)
        );
        assert!(staging_is_empty(&manager));
    }

    #[test]
    fn commit_waits_for_ready_and_failed_updates_keep_old_generation_and_assets() {
        let dir = TestDir::new("commit");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let first = manager.prepare("main", source(&dir.source()), now).unwrap();
        let installed = manager
            .commit_with_readiness("main", &first.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        assert_eq!(installed.mutation.generation, 1);
        assert_eq!(installed.previous_runtime_label, None);
        assert!(manager
            .asset(&installed.runtime.label, "/dist/runtime.js")
            .is_some_and(|asset| String::from_utf8(asset.bytes).unwrap().contains("one")));

        write_package(&dir.source(), "1.1.0", "two");
        let update = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert!(update.is_update);
        assert_eq!(
            manager.commit_with_readiness("main", &update.token, BTreeSet::new(), now, |_| false),
            Err(PublicPluginManagementError::RuntimeNotReady)
        );
        let after_failure = manager
            .state
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        assert_eq!(after_failure.version, "1.0.0");
        assert_eq!(after_failure.active_generation, 1);
        assert!(manager
            .asset(&installed.runtime.label, "/dist/runtime.js")
            .is_some_and(|asset| String::from_utf8(asset.bytes).unwrap().contains("one")));
        assert!(staging_is_empty(&manager));

        let changed = manager.prepare("main", source(&dir.source()), now).unwrap();
        let state = Arc::clone(manager.state());
        assert_eq!(
            manager.commit_with_readiness(
                "main",
                &changed.token,
                BTreeSet::new(),
                now,
                move |_| {
                    state
                        .rename("com.example.activation", Some("activation-alt"))
                        .unwrap();
                    true
                }
            ),
            Err(PublicPluginManagementError::Unavailable)
        );
        assert_eq!(
            manager
                .state
                .config("com.example.activation")
                .unwrap()
                .unwrap()
                .active_generation,
            1
        );
        assert!(manager
            .asset(&installed.runtime.label, "/dist/runtime.js")
            .is_some_and(|asset| String::from_utf8(asset.bytes).unwrap().contains("one")));

        let committed = manager.prepare("main", source(&dir.source()), now).unwrap();
        let upgraded = manager
            .commit_with_readiness("main", &committed.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        assert_eq!(upgraded.mutation.generation, 2);
        assert_eq!(
            upgraded.previous_runtime_label,
            Some(installed.runtime.label.clone())
        );
        assert!(manager
            .asset(&installed.runtime.label, "/dist/runtime.js")
            .is_none());
        assert!(manager
            .asset(&upgraded.runtime.label, "/dist/runtime.js")
            .is_some_and(|asset| String::from_utf8(asset.bytes).unwrap().contains("two")));
        assert!(staging_is_empty(&manager));
    }

    #[test]
    fn enable_readiness_runs_without_mutation_lock_and_cleans_stale_runtime() {
        let dir = TestDir::new("enable-stale");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        manager
            .set_enabled_with_readiness("com.example.activation", false, |_| false)
            .unwrap();

        let state = Arc::clone(manager.state());
        assert_eq!(
            manager.set_enabled_with_readiness("com.example.activation", true, move |_| {
                state
                    .rename("com.example.activation", Some("activation-alt"))
                    .unwrap();
                true
            }),
            Err(PublicPluginManagementError::Unavailable)
        );
        assert!(
            !manager
                .state
                .config("com.example.activation")
                .unwrap()
                .unwrap()
                .enabled
        );
        assert!(runtime_staging_is_empty(&manager));
    }
}
