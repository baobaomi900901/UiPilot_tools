use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
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

use crate::message_center::MessageCenterService;

use super::{
    manifest::{PublicActivationMode, PublicOutputMode, PublicPermission, PublicSettingV1},
    package, runtime_label, stage_public_package, EffectivePluginConfig, PluginApiRequest,
    PluginApiExecution, PluginCommandCompletion, PluginCompletionOutcome, PluginRequestContext,
    PluginRequestScheduler, PluginRuntimeApi, PluginRuntimeError, PluginSecretStore,
    PluginStateError, PluginStateStore,
    PluginStorageStore, PreparedPublicPlugin, PublicManifestV1, PublicPackageError,
    PublicPackageSource, PublicPluginFault, PublicPluginHost, PublicResource,
};

const PREPARE_TTL: Duration = Duration::from_secs(5 * 60);
const RUNTIME_HOST_PATH: &str = "__uipilot_runtime.html";
const RUNTIME_FAULT_WINDOW: Duration = Duration::from_secs(5 * 60);
const RUNTIME_FAULT_LIMIT: usize = 3;

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

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginInventory {
    pub(crate) revision: String,
    pub(crate) items: Vec<PublicPluginInventoryItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginInventoryItem {
    pub(crate) plugin_id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) version: String,
    pub(crate) source: &'static str,
    pub(crate) default_name: String,
    pub(crate) effective_name: String,
    pub(crate) enabled: bool,
    pub(crate) fault: Option<PublicPluginFault>,
    pub(crate) generation: u64,
    pub(crate) permissions: Vec<PublicPermissionView>,
    pub(crate) settings: Vec<PublicSettingView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPermissionView {
    pub(crate) permission: PublicPermission,
    pub(crate) supported: bool,
    pub(crate) granted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicSettingView {
    pub(crate) definition: PublicSettingV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) secret_configured: Option<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicPluginRoute {
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
    pub(crate) runtime_label: String,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) output_mode: PublicOutputMode,
    pub(crate) input: String,
    pub(crate) input_required: bool,
    pub(crate) input_placeholder: Option<String>,
    pub(crate) window_entry: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicMainResult {
    pub(crate) plugin_result_id: String,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) copy_text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicWindowResponse {
    pub(crate) request_id: String,
    pub(crate) data: Value,
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

#[derive(Default)]
struct RuntimeFaultWindow {
    recent: VecDeque<Instant>,
}

impl RuntimeFaultWindow {
    fn record(&mut self, now: Instant) -> bool {
        self.recent.retain(|fault| {
            now.checked_duration_since(*fault)
                .is_some_and(|age| age <= RUNTIME_FAULT_WINDOW)
        });
        self.recent.push_back(now);
        self.recent.len() >= RUNTIME_FAULT_LIMIT
    }

    fn clear(&mut self) {
        self.recent.clear();
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
    message_center: Arc<MessageCenterService>,
    api: PluginRuntimeApi,
    mutation: Mutex<()>,
    data: Mutex<ActivationData>,
    runtime_faults: Mutex<HashMap<String, RuntimeFaultWindow>>,
    next_token: AtomicU64,
}

impl PublicPluginManager {
    pub(crate) fn load(
        app_data_dir: &Path,
        host: PublicPluginHost,
        reserved_names: impl IntoIterator<Item = String>,
        message_center: Arc<MessageCenterService>,
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
            message_center.clone(),
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
            message_center,
            api,
            mutation: Mutex::new(()),
            data: Mutex::new(ActivationData {
                active_by_plugin,
                ..ActivationData::default()
            }),
            runtime_faults: Mutex::new(HashMap::new()),
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
        self.clear_runtime_faults(&runtime.plugin_id)?;
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
        let (before, snapshot, runtime, previous_runtime_label) = {
            let _mutation = self.lock_mutation()?;
            let before = self
                .state
                .config(plugin_id)?
                .filter(|config| config.installed)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let current_snapshot = self
                .lock_data()?
                .active_by_plugin
                .get(plugin_id)
                .cloned()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let current_runtime = current_snapshot.candidate();
            if before.enabled == enabled {
                return Ok(PublicEnabledCommit {
                    mutation: mutation_from_config(&before),
                    runtime: enabled.then_some(current_runtime),
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
                    closed_runtime_label: Some(current_runtime.label),
                });
            }
            let generation = before
                .active_generation
                .checked_add(1)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let mut next_snapshot = (*current_snapshot).clone();
            next_snapshot.generation = generation;
            next_snapshot.label = runtime_label(plugin_id, generation)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let snapshot = Arc::new(next_snapshot);
            let runtime = snapshot.candidate();
            let replaced = self
                .lock_data()?
                .staged_by_label
                .insert(runtime.label.clone(), Arc::clone(&snapshot));
            if replaced.is_some() {
                return Err(PublicPluginManagementError::Unavailable);
            }
            (before, snapshot, runtime, Some(current_runtime.label))
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
        let mut data = self.lock_data()?;
        let staged_matches = data
            .staged_by_label
            .get(&runtime.label)
            .is_some_and(|staged| Arc::ptr_eq(staged, &snapshot));
        if !staged_matches {
            return Err(PublicPluginManagementError::Unavailable);
        }
        if self
            .scheduler
            .invalidate_plugin(plugin_id, Some(runtime.generation))
            .is_err()
        {
            data.staged_by_label.remove(&runtime.label);
            return Err(PublicPluginManagementError::Unavailable);
        }
        let config = match self
            .state
            .enable_with_generation(plugin_id, runtime.generation)
        {
            Ok(config) => config,
            Err(error) => {
                data.staged_by_label.remove(&runtime.label);
                return Err(error.into());
            }
        };
        let staged = data
            .staged_by_label
            .remove(&runtime.label)
            .filter(|staged| Arc::ptr_eq(staged, &snapshot))
            .ok_or(PublicPluginManagementError::Unavailable)?;
        data.active_by_plugin.insert(plugin_id.into(), staged);
        drop(data);
        self.clear_runtime_faults(plugin_id)?;
        Ok(PublicEnabledCommit {
            mutation: mutation_from_config(&config),
            runtime: Some(runtime),
            closed_runtime_label: previous_runtime_label,
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
        self.clear_runtime_faults(plugin_id)?;
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

    pub(crate) fn inventory(&self) -> Result<PublicPluginInventory, PublicPluginManagementError> {
        let configs = self.state.configs()?;
        let data = self.lock_data()?;
        let revision = configs
            .iter()
            .map(|config| config.inventory_revision)
            .max()
            .unwrap_or(0);
        let mut items = Vec::new();
        for config in configs.into_iter().filter(|config| config.installed) {
            let Some(snapshot) = data.active_by_plugin.get(&config.plugin_id) else {
                continue;
            };
            let scope = super::PluginDataScope::new(&config.plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let settings = snapshot
                .manifest
                .settings
                .iter()
                .cloned()
                .map(|definition| {
                    let key = definition.key();
                    if definition.is_secret() {
                        let configured = self
                            .secrets
                            .is_configured(&scope, &config.plugin_id, key)
                            .map_err(|_| PublicPluginManagementError::Unavailable)?;
                        Ok(PublicSettingView {
                            definition,
                            value: None,
                            secret_configured: Some(configured),
                        })
                    } else {
                        Ok(PublicSettingView {
                            value: config.settings.get(key).cloned(),
                            definition,
                            secret_configured: None,
                        })
                    }
                })
                .collect::<Result<Vec<_>, PublicPluginManagementError>>()?;
            items.push(PublicPluginInventoryItem {
                plugin_id: config.plugin_id.clone(),
                name: snapshot.manifest.name.clone(),
                description: snapshot.manifest.description.clone(),
                version: config.version,
                source: "localPackage",
                default_name: snapshot.manifest.command.default_name.clone(),
                effective_name: config.effective_name,
                enabled: config.enabled,
                fault: config.fault,
                generation: config.active_generation,
                permissions: snapshot
                    .manifest
                    .permissions
                    .iter()
                    .copied()
                    .map(|permission| PublicPermissionView {
                        permission,
                        supported: permission.is_available(self.host.platform),
                        granted: config.permission_grants.contains(&permission),
                    })
                    .collect(),
                settings,
            });
        }
        items.sort_by(|left, right| left.effective_name.cmp(&right.effective_name));
        Ok(PublicPluginInventory {
            revision: revision.to_string(),
            items,
        })
    }
    pub(crate) fn route(
        &self,
        query: &str,
    ) -> Result<Option<PublicPluginRoute>, PublicPluginManagementError> {
        let Some(command) = query.strip_prefix('/') else {
            return Ok(None);
        };
        let (effective_name, input) = command
            .split_once(' ')
            .map_or((command, ""), |(name, body)| (name, body.trim()));
        if effective_name.is_empty() {
            return Ok(None);
        }
        let data = self.lock_data()?;
        for (plugin_id, snapshot) in &data.active_by_plugin {
            let Some(config) = self.state.config(plugin_id)? else {
                continue;
            };
            if !config.installed
                || !config.enabled
                || config.fault.is_some()
                || config.active_generation != snapshot.generation
                || config.effective_name != effective_name
            {
                continue;
            }
            let command = &snapshot.manifest.command;
            return Ok(Some(PublicPluginRoute {
                plugin_id: plugin_id.clone(),
                generation: snapshot.generation,
                runtime_label: snapshot.label.clone(),
                activation_mode: command.activation_mode,
                output_mode: command.output_mode,
                input: input.to_owned(),
                input_required: command.input_required,
                input_placeholder: command.input_placeholder.clone(),
                window_entry: snapshot
                    .manifest
                    .window
                    .as_ref()
                    .map(|window| window.entry.clone()),
            }));
        }
        Ok(None)
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

    pub(crate) fn window_asset(
        &self,
        plugin_id: &str,
        request_path: &str,
    ) -> Option<PublicRuntimeAsset> {
        let snapshot = self
            .data
            .lock()
            .ok()?
            .active_by_plugin
            .get(plugin_id)
            .cloned()?;
        let config = self.state.config(plugin_id).ok()??;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || config.active_generation != snapshot.generation
            || snapshot.manifest.command.output_mode != PublicOutputMode::Window
            || !snapshot
                .manifest
                .permissions
                .contains(&PublicPermission::UiWindow)
        {
            return None;
        }
        let relative = decode_request_path(request_path)?;
        let resource = snapshot.resources.get(&relative)?;
        Some(PublicRuntimeAsset {
            mime: resource.mime,
            bytes: resource.bytes.clone(),
        })
    }
    pub(crate) fn can_copy_text(&self, plugin_id: &str, generation: u64) -> bool {
        self.state
            .config(plugin_id)
            .ok()
            .flatten()
            .is_some_and(|config| {
                config.installed
                    && config.enabled
                    && config.fault.is_none()
                    && config.active_generation == generation
                    && config
                        .permission_grants
                        .contains(&PublicPermission::ClipboardWrite)
            })
            && self
                .lock_data()
                .ok()
                .and_then(|data| data.active_by_plugin.get(plugin_id).cloned())
                .is_some_and(|snapshot| snapshot.generation == generation)
    }
    pub(crate) fn execute_api(
        &self,
        caller_label: &str,
        request: PluginApiRequest,
    ) -> PluginApiExecution {
        let Some(manifest) = self.manifest_for_label(caller_label) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidCaller);
        };
        self.api.execute(caller_label, request, &manifest)
    }

    pub(crate) fn message_center(&self) -> &Arc<MessageCenterService> {
        &self.message_center
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

    #[cfg(test)]
    pub(crate) fn state(&self) -> &Arc<PluginStateStore> {
        &self.state
    }

    pub(crate) fn replace_runtime_generation(
        &self,
        plugin_id: &str,
        previous_generation: u64,
        new_generation: u64,
    ) -> Result<PublicRuntimeCandidate, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let config = self
            .state
            .config(plugin_id)?
            .filter(|config| {
                config.installed
                    && config.enabled
                    && config.fault.is_none()
                    && config.active_generation == previous_generation
            })
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let mut data = self.lock_data()?;
        let current = data
            .active_by_plugin
            .get(plugin_id)
            .filter(|snapshot| snapshot.generation == previous_generation)
            .cloned()
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let mut replacement = (*current).clone();
        replacement.generation = new_generation;
        replacement.label = runtime_label(plugin_id, new_generation)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        self.state.activate(
            &replacement.manifest,
            config.permission_grants,
            new_generation,
            Some(replacement.digest.clone()),
        )?;
        let replacement = Arc::new(replacement);
        let candidate = replacement.candidate();
        data.active_by_plugin.insert(plugin_id.into(), replacement);
        Ok(candidate)
    }

    pub(crate) fn record_runtime_result(
        &self,
        plugin_id: &str,
        success: bool,
        now: Instant,
    ) -> Result<bool, PublicPluginManagementError> {
        let disable = {
            let mut faults = self
                .runtime_faults
                .lock()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let window = faults.entry(plugin_id.into()).or_default();
            if success {
                window.clear();
                false
            } else {
                window.record(now)
            }
        };
        if !disable {
            return Ok(false);
        }
        let _mutation = self.lock_mutation()?;
        self.state
            .disable_for_fault(plugin_id, PublicPluginFault::RuntimeUnavailable)?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(true)
    }

    fn clear_runtime_faults(&self, plugin_id: &str) -> Result<(), PublicPluginManagementError> {
        self.runtime_faults
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .remove(plugin_id);
        Ok(())
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

const MAX_MAIN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_MAIN_RESULTS: usize = 20;
const MAX_RESULT_TITLE_CHARS: usize = 256;
const MAX_RESULT_SUBTITLE_CHARS: usize = 512;
const MAX_RESULT_DETAIL_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MainResultResponseWire {
    request_id: String,
    results: Vec<MainResultWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MainResultWire {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    default_action: Option<CopyTextActionWire>,
}

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum CopyTextActionWire {
    #[serde(rename = "copyText")]
    CopyText { text: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowResponseWire {
    request_id: String,
    data: Value,
}

pub(crate) fn parse_window_response(
    context: &PluginRequestContext,
    value: Value,
) -> Result<PublicWindowResponse, PluginRuntimeError> {
    if serde_json::to_vec(&value)
        .map_err(|_| PluginRuntimeError::InvalidOperation)?
        .len()
        > MAX_MAIN_RESPONSE_BYTES
    {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    let response = serde_json::from_value::<WindowResponseWire>(value)
        .map_err(|_| PluginRuntimeError::InvalidOperation)?;
    if response.request_id != context.request_id || !super::valid_json_value(&response.data) {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    Ok(PublicWindowResponse {
        request_id: response.request_id,
        data: response.data,
    })
}
pub(crate) fn parse_main_result_response(
    context: &PluginRequestContext,
    value: Value,
) -> Result<Vec<PublicMainResult>, PluginRuntimeError> {
    if serde_json::to_vec(&value)
        .map_err(|_| PluginRuntimeError::InvalidOperation)?
        .len()
        > MAX_MAIN_RESPONSE_BYTES
    {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    let response = serde_json::from_value::<MainResultResponseWire>(value)
        .map_err(|_| PluginRuntimeError::InvalidOperation)?;
    if response.request_id != context.request_id || response.results.len() > MAX_MAIN_RESULTS {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    let mut ids = BTreeSet::new();
    response
        .results
        .into_iter()
        .map(|result| {
            if result.id.is_empty()
                || !ids.insert(result.id.clone())
                || result.title.chars().count() > MAX_RESULT_TITLE_CHARS
                || result
                    .subtitle
                    .as_ref()
                    .is_some_and(|value| value.chars().count() > MAX_RESULT_SUBTITLE_CHARS)
                || result
                    .detail
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_RESULT_DETAIL_BYTES)
            {
                return Err(PluginRuntimeError::InvalidOperation);
            }
            let copy_text = result.default_action.map(|action| match action {
                CopyTextActionWire::CopyText { text } => text,
            });
            Ok(PublicMainResult {
                plugin_result_id: result.id,
                title: result.title,
                subtitle: result.subtitle,
                detail: result.detail,
                copy_text,
            })
        })
        .collect()
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
        let message_center = Arc::new(MessageCenterService::load(dir.path()));
        PublicPluginManager::load(
            dir.path(),
            PublicPluginHost::current(PublicPlatform::Windows),
            ["find".into(), "math".into()],
            message_center,
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

    #[test]
    fn inventory_exposes_settings_and_secret_presence_without_runtime_paths_or_secret_values() {
        let dir = TestDir::new("inventory");
        write_package(&dir.source(), "1.0.0", "inventory");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();

        let value = serde_json::to_value(manager.inventory().unwrap()).unwrap();
        assert_eq!(value["items"][0]["pluginId"], "com.example.activation");
        assert_eq!(value["items"][0]["effectiveName"], "activation");
        assert_eq!(value["items"][0]["source"], "localPackage");
        assert!(value["items"][0].get("runtime").is_none());
        assert!(value["items"][0].get("outputMode").is_none());
        assert!(value.to_string().find("runtime.js").is_none());
    }
    #[test]
    fn public_route_preserves_internal_spaces_and_honors_activation_mode() {
        let dir = TestDir::new("route");
        write_package(&dir.source(), "1.0.0", "route");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();

        assert_eq!(
            manager
                .route("/activation   I am  Jack  ")
                .unwrap()
                .unwrap(),
            PublicPluginRoute {
                plugin_id: "com.example.activation".into(),
                generation: 1,
                runtime_label: runtime_label("com.example.activation", 1).unwrap(),
                activation_mode: PublicActivationMode::Live,
                output_mode: PublicOutputMode::MainResult,
                input: "I am  Jack".into(),
                input_required: false,
                input_placeholder: None,
                window_entry: None,
            }
        );
        assert!(manager.route("/activationX nope").unwrap().is_none());
        manager
            .set_enabled_with_readiness("com.example.activation", false, |_| false)
            .unwrap();
        assert!(manager.route("/activation").unwrap().is_none());
    }

    #[test]
    fn main_results_reject_unknown_fields_and_keep_copy_payload_private() {
        let context = PluginRequestContext {
            plugin_id: "com.example.activation".into(),
            plugin_generation: 3,
            request_id: "public-request-1".into(),
        };
        assert_eq!(
            parse_main_result_response(
                &context,
                json!({
                    "requestId": "public-request-1",
                    "results": [{
                        "id": "answer",
                        "title": "Answer",
                        "subtitle": "plain text",
                        "defaultAction": { "type": "copyText", "text": "42" }
                    }]
                })
            )
            .unwrap(),
            vec![PublicMainResult {
                plugin_result_id: "answer".into(),
                title: "Answer".into(),
                subtitle: Some("plain text".into()),
                detail: None,
                copy_text: Some("42".into()),
            }]
        );
        assert!(parse_main_result_response(
            &context,
            json!({
                "requestId": "public-request-1",
                "results": [{ "id": "answer", "title": "Answer", "actions": [] }]
            })
        )
        .is_err());
        assert!(parse_main_result_response(
            &context,
            json!({ "requestId": "wrong", "results": [] })
        )
        .is_err());
    }

    #[test]
    fn window_response_is_bounded_exact_and_request_owned() {
        let context = PluginRequestContext {
            plugin_id: "com.example.activation".into(),
            plugin_generation: 3,
            request_id: "public-request-1".into(),
        };
        assert_eq!(
            parse_window_response(
                &context,
                json!({
                    "requestId": "public-request-1",
                    "data": { "message": "hello", "items": [1, true, null] }
                })
            )
            .unwrap(),
            PublicWindowResponse {
                request_id: "public-request-1".into(),
                data: json!({ "message": "hello", "items": [1, true, null] }),
            }
        );
        for invalid in [
            json!({ "requestId": "wrong", "data": {} }),
            json!({ "requestId": "public-request-1", "data": {}, "actions": [] }),
            json!({ "requestId": "public-request-1", "data": { "__proto__": true } }),
        ] {
            assert!(parse_window_response(&context, invalid).is_err());
        }
        assert!(parse_window_response(
            &context,
            json!({ "requestId": "public-request-1", "data": "x".repeat(65_536) })
        )
        .is_err());
    }

    #[test]
    fn runtime_fault_window_disables_on_third_recent_fault_and_success_resets() {
        let start = Instant::now();
        let mut faults = RuntimeFaultWindow::default();
        assert!(!faults.record(start));
        assert!(!faults.record(start + Duration::from_secs(30)));
        assert!(faults.record(start + Duration::from_secs(60)));

        faults.clear();
        assert!(!faults.record(start + Duration::from_secs(90)));
        assert!(!faults.record(start + Duration::from_secs(7 * 60)));
        assert!(!faults.record(start + Duration::from_secs(7 * 60 + 1)));
    }
    #[test]
    fn runtime_replacement_updates_route_and_third_fault_persistently_disables() {
        let dir = TestDir::new("runtime-replacement-faults");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let committed = manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        assert_eq!(committed.runtime.generation, 1);

        let replacement = manager
            .replace_runtime_generation("com.example.activation", 1, 2)
            .unwrap();
        assert_eq!(replacement.generation, 2);
        assert_eq!(manager.route("/activation").unwrap().unwrap().generation, 2);
        assert_eq!(
            manager
                .state()
                .config("com.example.activation")
                .unwrap()
                .unwrap()
                .active_generation,
            2
        );

        assert!(!manager
            .record_runtime_result("com.example.activation", false, now)
            .unwrap());
        assert!(!manager
            .record_runtime_result(
                "com.example.activation",
                false,
                now + Duration::from_secs(10),
            )
            .unwrap());
        assert!(manager
            .record_runtime_result(
                "com.example.activation",
                false,
                now + Duration::from_secs(20),
            )
            .unwrap());
        let disabled = manager
            .state()
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.fault, Some(PublicPluginFault::RuntimeUnavailable));
        let reenabled_generation = disabled.active_generation.checked_add(1).unwrap();

        let reenabled_commit = manager
            .set_enabled_with_readiness("com.example.activation", true, |candidate| {
                assert_eq!(candidate.generation, reenabled_generation);
                true
            })
            .unwrap();
        assert_eq!(
            reenabled_commit.closed_runtime_label,
            runtime_label("com.example.activation", disabled.active_generation)
        );
        assert_eq!(
            reenabled_commit
                .runtime
                .as_ref()
                .map(|runtime| runtime.generation),
            Some(reenabled_generation)
        );
        let reenabled = manager
            .state()
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        assert!(reenabled.enabled);
        assert_eq!(reenabled.active_generation, reenabled_generation);
        assert_eq!(
            manager.route("/activation").unwrap().unwrap().generation,
            reenabled_generation
        );
        assert!(!manager
            .record_runtime_result(
                "com.example.activation",
                false,
                now + Duration::from_secs(30),
            )
            .unwrap());
    }
}
