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
use tauri::{AppHandle, Manager};

use crate::message_center::{
    MessageCenterService, MessagePostGuardEffect, MessagePublishOutcome, MessagePublishRequest,
    MessagePublisher,
};

use super::{
    activation_bundle::{
        ActivationCandidate, ActivationIdAllocator, ActivationReservation,
        ActivationReservationBook, AdmissionEpochAllocator, ReservationError,
    },
    data_call_gate::{PluginDataCallDrain, PluginDataCallGate, PluginDataCallIdentity},
    delayed_messages::{DelayedMessageScheduler, ScheduledPluginMessage},
    icon::{self, IconRequest, ICON_PATH},
    manifest::{PublicActivationMode, PublicOutputMode, PublicPermission, PublicSettingV1},
    owner_cleanup::{
        remove_owned_directory, OwnerCleanupError, PluginOwnerCleanupReceipt,
        PluginOwnerCleanupStore,
    },
    package, runtime_label, stage_public_package,
    state::{DurableStateOutcome, PreparedStateCommit},
    timers::{
        ClaimTicket, Clock, PluginTimerService, PluginTimerStartInput, PluginTimerState,
        SystemClock, TimerAudioCompletion, TimerError, TimerKey, TimerPostLockEffect,
    },
    EffectivePluginConfig, PluginApiExecution, PluginApiRequest, PluginCommandCompletion,
    PluginCompletionOutcome, PluginDataScope, PluginRequestContext, PluginRequestScheduler,
    PluginRuntimeApi, PluginRuntimeError, PluginSecretStore, PluginStateError, PluginStateStore,
    PluginStorageStore, PreparedPublicPlugin, PublicManifestV1, PublicPackageError,
    PublicPackageSource, PublicPluginFault, PublicPluginHost, PublicResource, ValidatedAlarmAsset,
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
    pub(crate) icon_url: Option<String>,
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
    pub(crate) icon_url: Option<String>,
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
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) runtime_recovery_needed: bool,
    pub(crate) runtime_label: String,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) output_mode: PublicOutputMode,
    pub(crate) input: String,
    pub(crate) input_required: bool,
    pub(crate) input_placeholder: Option<String>,
    pub(crate) window_entry: Option<String>,
    pub(crate) icon_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicCommandSuggestion {
    pub(crate) plugin_id: String,
    pub(crate) effective_name: String,
    pub(crate) display_name: String,
    pub(crate) summary: Option<String>,
    pub(crate) icon_url: Option<String>,
    pub(crate) favorite: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginWindowIdentity {
    pub(crate) name: String,
    pub(crate) icon_url: Option<String>,
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
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) runtime_recovery_needed: bool,
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

pub(crate) struct PublicPluginUninstallTransaction {
    plugin_id: String,
    retain_data: bool,
    previous_generation: u64,
    previous_activation_id: Option<u64>,
    pub(crate) runtime_label: Option<String>,
    transaction_id: String,
    reservation: ActivationReservation,
    prepared_state: PreparedStateCommit,
    previous_bundle: Option<Arc<super::activation_bundle::ActivationBundle>>,
    data_identity: Option<PluginDataCallIdentity>,
    data_drain: Option<PluginDataCallDrain>,
    data_drained: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PublicPluginUninstallCommit {
    pub(crate) plugin_id: String,
    pub(crate) retain_data: bool,
    pub(crate) runtime_label: Option<String>,
    receipt: Option<PluginOwnerCleanupReceipt>,
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
    DataCleanupPending,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowStorageError {
    InvalidCaller,
    InvalidContext,
    ExpiredWindowSessionError,
    InvalidOperation,
    StorageError,
}

impl WindowStorageError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::InvalidCaller => "InvalidCaller",
            Self::InvalidContext => "InvalidContext",
            Self::ExpiredWindowSessionError => "ExpiredWindowSessionError",
            Self::InvalidOperation => "InvalidOperation",
            Self::StorageError => "StorageError",
        }
    }
}

pub(crate) struct PublicPluginUninstallCommitFailure {
    pub(crate) error: PublicPluginManagementError,
    pub(crate) transaction: Option<Box<PublicPluginUninstallTransaction>>,
}

impl fmt::Debug for PublicPluginUninstallCommitFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicPluginUninstallCommitFailure")
            .field("error", &self.error)
            .field("recoverable", &self.transaction.is_some())
            .finish()
    }
}

impl From<PublicPluginManagementError> for PublicPluginUninstallCommitFailure {
    fn from(error: PublicPluginManagementError) -> Self {
        Self {
            error,
            transaction: None,
        }
    }
}

impl From<PluginStateError> for PublicPluginUninstallCommitFailure {
    fn from(error: PluginStateError) -> Self {
        PublicPluginManagementError::from(error).into()
    }
}

impl From<ReservationError> for PublicPluginUninstallCommitFailure {
    fn from(error: ReservationError) -> Self {
        PublicPluginManagementError::from(error).into()
    }
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
            Self::DataCleanupPending => "dataCleanupPending",
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
pub(super) struct RuntimeResource {
    pub(super) mime: &'static str,
    pub(super) bytes: Vec<u8>,
}

#[derive(Clone)]
pub(super) struct RuntimeSnapshot {
    pub(super) manifest: PublicManifestV1,
    pub(super) digest: String,
    pub(super) generation: u64,
    pub(super) label: String,
    pub(super) resources: BTreeMap<String, RuntimeResource>,
}

impl RuntimeSnapshot {
    fn candidate(
        &self,
        activation_id: u64,
        admission_epoch: u64,
        runtime_recovery_needed: bool,
    ) -> PublicRuntimeCandidate {
        PublicRuntimeCandidate {
            plugin_id: self.manifest.plugin_id.clone(),
            generation: self.generation,
            activation_id,
            admission_epoch,
            runtime_recovery_needed,
            label: self.label.clone(),
            runtime_entry: self.manifest.runtime.entry.clone(),
        }
    }

    fn icon_url(&self) -> Option<String> {
        self.resources
            .contains_key(ICON_PATH)
            .then(|| icon::installed_url(&self.manifest.plugin_id, self.generation))
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicIconAsset {
    pub(crate) bytes: Vec<u8>,
    pub(crate) cache_control: &'static str,
}

struct PendingActivation {
    caller_label: String,
    expires_at: Instant,
    prepared: PreparedPublicPlugin,
    icon: Option<Vec<u8>>,
}

#[derive(Default)]
struct ActivationData {
    pending: HashMap<String, PendingActivation>,
    active_by_plugin: HashMap<String, Arc<RuntimeSnapshot>>,
    staged_by_label: HashMap<String, Arc<ActivationCandidate>>,
}

#[derive(Debug, Eq, PartialEq)]
struct TimerClaimDispatch {
    effect: Option<MessagePostGuardEffect>,
    audio: Option<TimerAudioCompletion>,
}

pub(crate) struct PublicPluginManager {
    host: PublicPluginHost,
    #[cfg(test)]
    app_data_dir: PathBuf,
    staging_root: PathBuf,
    packages_root: PathBuf,
    state: Arc<PluginStateStore>,
    storage: Arc<PluginStorageStore>,
    secrets: Arc<PluginSecretStore>,
    scheduler: Arc<PluginRequestScheduler>,
    data_gate: Arc<PluginDataCallGate>,
    delayed_messages: Arc<DelayedMessageScheduler>,
    message_center: Arc<MessageCenterService>,
    timer_publisher: Arc<dyn MessagePublisher>,
    timers: Arc<PluginTimerService>,
    api: PluginRuntimeApi,
    activation_ids: ActivationIdAllocator,
    admission_epochs: AdmissionEpochAllocator,
    owner_cleanup: Arc<PluginOwnerCleanupStore>,
    bundles: Arc<ActivationReservationBook>,
    mutation: Mutex<()>,
    data: Mutex<ActivationData>,
    runtime_faults: Mutex<HashMap<(String, u64), RuntimeFaultWindow>>,
    next_token: AtomicU64,
    next_uninstall_transaction: AtomicU64,
}

impl PublicPluginManager {
    pub(crate) fn load(
        app_data_dir: &Path,
        host: PublicPluginHost,
        reserved_names: impl IntoIterator<Item = String>,
        message_center: Arc<MessageCenterService>,
    ) -> Result<Self, PublicPluginManagementError> {
        let timer_publisher: Arc<dyn MessagePublisher> = message_center.clone();
        Self::load_with_timer_dependencies(
            app_data_dir,
            host,
            reserved_names,
            message_center,
            Arc::new(SystemClock),
            timer_publisher,
        )
    }

    fn load_with_timer_dependencies(
        app_data_dir: &Path,
        host: PublicPluginHost,
        reserved_names: impl IntoIterator<Item = String>,
        message_center: Arc<MessageCenterService>,
        timer_clock: Arc<dyn Clock>,
        timer_publisher: Arc<dyn MessagePublisher>,
    ) -> Result<Self, PublicPluginManagementError> {
        let root = app_data_dir.join("public-plugins");
        let staging_root = root.join("staging");
        let packages_root = root.join("packages");
        let owner_cleanup = Arc::new(
            PluginOwnerCleanupStore::load(&app_data_dir.join("public-plugin-owner-cleanup"))
                .map_err(|_| PublicPluginManagementError::Unavailable)?,
        );
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
        let data_gate = Arc::new(PluginDataCallGate::default());
        let delayed_messages = Arc::new(DelayedMessageScheduler::default());
        let timers = Arc::new(PluginTimerService::new(timer_clock));
        let api = PluginRuntimeApi::new(
            Arc::clone(&scheduler),
            Arc::clone(&data_gate),
            Arc::clone(&state),
            Arc::clone(&storage),
            Arc::clone(&secrets),
            Arc::clone(&delayed_messages),
            message_center.clone(),
        );
        let activation_ids = ActivationIdAllocator::default();
        let admission_epochs = AdmissionEpochAllocator::default();
        let bundles = Arc::new(ActivationReservationBook::default());
        let mut active_by_plugin = HashMap::new();
        for config in state.configs()? {
            if !config.installed || config.active_generation == 0 {
                continue;
            }
            if owner_cleanup
                .is_blocked(&config.plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?
            {
                continue;
            }
            let Some(digest) = config.package_digest.as_deref() else {
                continue;
            };
            let package_root = package_destination(&packages_root, &config.plugin_id, digest);
            match load_runtime_snapshot(&package_root, &host, digest, config.active_generation) {
                Ok((snapshot, alarm))
                    if snapshot.manifest.plugin_id == config.plugin_id
                        && snapshot.manifest.version == config.version =>
                {
                    let runtime = Arc::new(snapshot);
                    if config.enabled && config.fault.is_none() {
                        let Some(activation_id) = activation_ids.allocate() else {
                            let _ = state.disable_for_fault(
                                &config.plugin_id,
                                PublicPluginFault::RuntimeUnavailable,
                            );
                            continue;
                        };
                        let Some(admission_epoch) = admission_epochs.allocate() else {
                            let _ = state.disable_for_fault(
                                &config.plugin_id,
                                PublicPluginFault::RuntimeUnavailable,
                            );
                            continue;
                        };
                        let candidate = ActivationCandidate::new(
                            Arc::clone(&runtime),
                            alarm.as_ref(),
                            activation_id,
                            admission_epoch,
                        );
                        let identity = PluginDataCallIdentity::new(
                            &config.plugin_id,
                            config.active_generation,
                            activation_id,
                            admission_epoch,
                        )
                        .map_err(|_| PublicPluginManagementError::Unavailable)?;
                        if data_gate.activate(identity.clone()).is_err() {
                            let _ = state.disable_for_fault(
                                &config.plugin_id,
                                PublicPluginFault::RuntimeUnavailable,
                            );
                            continue;
                        }
                        if bundles
                            .install_initial(&config.plugin_id, candidate.bundle(config.clone()))
                            .is_err()
                        {
                            let _ = data_gate.remove(&identity);
                            let _ = state.disable_for_fault(
                                &config.plugin_id,
                                PublicPluginFault::RuntimeUnavailable,
                            );
                            continue;
                        }
                        let _ = scheduler.invalidate_plugin(
                            &config.plugin_id,
                            Some((config.active_generation, activation_id, admission_epoch)),
                        );
                    }
                    active_by_plugin.insert(config.plugin_id, runtime);
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
            #[cfg(test)]
            app_data_dir: app_data_dir.to_path_buf(),
            staging_root,
            packages_root,
            state,
            storage,
            secrets,
            scheduler,
            data_gate,
            delayed_messages,
            message_center,
            timer_publisher,
            timers,
            api,
            activation_ids,
            admission_epochs,
            owner_cleanup,
            bundles,
            mutation: Mutex::new(()),
            data: Mutex::new(ActivationData {
                active_by_plugin,
                ..ActivationData::default()
            }),
            runtime_faults: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            next_uninstall_transaction: AtomicU64::new(1),
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
        if self.bundles.is_terminal() {
            return Err(PublicPluginManagementError::Unavailable);
        }
        let _mutation = self.lock_mutation()?;
        self.cleanup_expired(now)?;
        let prepared =
            stage_public_package(source.into_package_source(), &self.staging_root, &self.host)?;
        if self
            .owner_cleanup
            .is_blocked(&prepared.manifest.plugin_id)
            .map_err(|_| PublicPluginManagementError::Unavailable)?
        {
            return Err(PublicPluginManagementError::DataCleanupPending);
        }
        let icon = if prepared.resources.contains_key(ICON_PATH) {
            Some(
                fs::read(prepared.package_root.join(ICON_PATH))
                    .map_err(|_| PublicPluginManagementError::InvalidPackage)?,
            )
        } else {
            None
        };
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
            icon_url: icon.as_ref().map(|_| icon::prepared_url(&token)),
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
                icon,
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
        let (
            pending,
            previous_config,
            candidate,
            runtime,
            previous_runtime_label,
            previous_activation_id,
        ) = {
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
            let runtime_snapshot = Arc::new(snapshot_from_prepared(&pending.prepared, generation)?);
            let activation_id = self
                .activation_ids
                .allocate()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let admission_epoch = self
                .admission_epochs
                .allocate()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let candidate = Arc::new(ActivationCandidate::new(
                runtime_snapshot,
                pending.prepared.alarm.as_ref(),
                activation_id,
                admission_epoch,
            ));
            let runtime = candidate.runtime.candidate(
                candidate.activation_id,
                candidate.admission_epoch,
                candidate.runtime_recovery_needed,
            );
            let previous_bundle = self
                .bundles
                .bundle(&runtime.plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let previous_runtime_label = previous_bundle
                .as_ref()
                .map(|bundle| bundle.runtime.label.clone());
            let previous_activation_id =
                previous_bundle.as_ref().map(|bundle| bundle.activation_id);
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
                previous_activation_id,
            )
        };

        if !readiness(&runtime) {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::RuntimeNotReady);
        }

        let (reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            if self.state.config(&runtime.plugin_id)? != previous_config
                || self
                    .bundles
                    .bundle(&runtime.plugin_id)?
                    .as_ref()
                    .map(|bundle| bundle.activation_id)
                    != previous_activation_id
            {
                self.unstage(&runtime.label);
                return Err(PublicPluginManagementError::Unavailable);
            }
            let reservation = self.bundles.reserve(&runtime.plugin_id)?;
            let prepared_state = self.state.prepare_activation(
                &candidate.runtime.manifest,
                permission_grants,
                runtime.generation,
                Some(candidate.runtime.digest.clone()),
            )?;
            (reservation, prepared_state)
        };
        let destination = package_destination(
            &self.packages_root,
            &runtime.plugin_id,
            &candidate.runtime.digest,
        );
        let created = match pending.prepared.persist(&destination) {
            Ok(created) => created,
            Err(error) => {
                self.unstage(&runtime.label);
                return Err(error.into());
            }
        };
        reservation.begin_committing()?;
        match self.state.persist_prepared(&prepared_state) {
            DurableStateOutcome::Committed => reservation.mark_durable()?,
            DurableStateOutcome::NotCommitted => {
                if !self.state.revalidate_previous(&prepared_state) {
                    return Err(PublicPluginManagementError::Unavailable);
                }
                reservation.rollback_not_committed()?;
                if created {
                    package::remove_package_tree(destination);
                }
                self.unstage(&runtime.label);
                return Err(PublicPluginManagementError::Unavailable);
            }
            DurableStateOutcome::Unknown => return Err(PublicPluginManagementError::Unavailable),
        }

        let _mutation = self.lock_mutation()?;
        self.scheduler
            .invalidate_plugin(
                &runtime.plugin_id,
                Some((
                    runtime.generation,
                    runtime.activation_id,
                    runtime.admission_epoch,
                )),
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        self.activate_runtime_data_gate(&runtime)?;
        self.cancel_delayed_messages(&runtime.plugin_id);
        let timer_effects = previous_config
            .as_ref()
            .zip(previous_activation_id)
            .map(|(config, activation_id)| {
                self.cancel_timer_generation(
                    &runtime.plugin_id,
                    config.active_generation,
                    activation_id,
                )
            })
            .unwrap_or_default();
        let mut data = self.lock_data()?;
        let staged_matches = data
            .staged_by_label
            .get(&runtime.label)
            .is_some_and(|staged| Arc::ptr_eq(staged, &candidate));
        if !staged_matches {
            return Err(PublicPluginManagementError::Unavailable);
        }
        self.clear_runtime_faults(&runtime.plugin_id)?;
        let config = self.state.publish_prepared(prepared_state)?;
        let runtime_active = config.enabled && config.fault.is_none();
        reservation.publish(runtime_active.then(|| candidate.bundle(config.clone())))?;
        let staged = data
            .staged_by_label
            .remove(&runtime.label)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        data.active_by_plugin.remove(&runtime.plugin_id);
        if runtime_active {
            data.active_by_plugin
                .insert(runtime.plugin_id.clone(), Arc::clone(&staged.runtime));
        }
        drop(data);
        let commit = PublicActivationCommit {
            mutation: mutation_from_config(&config),
            runtime,
            previous_runtime_label,
        };
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        if let Some(previous_digest) = previous_config
            .as_ref()
            .and_then(|config| config.package_digest.as_deref())
            .filter(|digest| *digest != candidate.runtime.digest)
        {
            package::remove_package_tree(package_destination(
                &self.packages_root,
                &candidate.runtime.manifest.plugin_id,
                previous_digest,
            ));
        }
        Ok(commit)
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
        if !enabled {
            return self.disable_plugin(plugin_id);
        }
        let (before, candidate, runtime, previous_runtime_label) = {
            let _mutation = self.lock_mutation()?;
            let before = self
                .state
                .config(plugin_id)?
                .filter(|config| config.installed)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let current_bundle = self
                .bundles
                .bundle(plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            if before.enabled == enabled {
                return Ok(PublicEnabledCommit {
                    mutation: mutation_from_config(&before),
                    runtime: current_bundle.map(|bundle| {
                        bundle.runtime.candidate(
                            bundle.activation_id,
                            bundle.admission_epoch,
                            bundle.runtime_recovery_needed,
                        )
                    }),
                    closed_runtime_label: None,
                });
            }
            let generation = before
                .active_generation
                .checked_add(1)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let digest = before
                .package_digest
                .as_deref()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let package_root = package_destination(&self.packages_root, plugin_id, digest);
            let (snapshot, alarm) =
                load_runtime_snapshot(&package_root, &self.host, digest, generation)?;
            if snapshot.manifest.plugin_id != plugin_id
                || snapshot.manifest.version != before.version
            {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let activation_id = self
                .activation_ids
                .allocate()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let admission_epoch = self
                .admission_epochs
                .allocate()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let candidate = Arc::new(ActivationCandidate::new(
                Arc::new(snapshot),
                alarm.as_ref(),
                activation_id,
                admission_epoch,
            ));
            let runtime = candidate.runtime.candidate(
                candidate.activation_id,
                candidate.admission_epoch,
                candidate.runtime_recovery_needed,
            );
            let replaced = self
                .lock_data()?
                .staged_by_label
                .insert(runtime.label.clone(), Arc::clone(&candidate));
            if replaced.is_some() {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let previous_runtime_label = self
                .lock_data()?
                .active_by_plugin
                .get(plugin_id)
                .map(|snapshot| snapshot.label.clone());
            (before, candidate, runtime, previous_runtime_label)
        };

        if !readiness(&runtime) {
            self.unstage(&runtime.label);
            return Err(PublicPluginManagementError::RuntimeNotReady);
        }

        let (reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            if self.state.config(plugin_id)? != Some(before.clone())
                || self.bundles.bundle(plugin_id)?.is_some()
            {
                self.unstage(&runtime.label);
                return Err(PublicPluginManagementError::Unavailable);
            }
            let staged_matches = self
                .lock_data()?
                .staged_by_label
                .get(&runtime.label)
                .is_some_and(|staged| Arc::ptr_eq(staged, &candidate));
            if !staged_matches {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self
                .state
                .prepare_enable_with_generation(plugin_id, runtime.generation)?;
            (reservation, prepared_state)
        };

        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        let mut data = self.lock_data()?;
        let staged_matches = data
            .staged_by_label
            .get(&runtime.label)
            .is_some_and(|staged| Arc::ptr_eq(staged, &candidate));
        if !staged_matches {
            return Err(PublicPluginManagementError::Unavailable);
        }
        if self
            .scheduler
            .invalidate_plugin(
                plugin_id,
                Some((
                    runtime.generation,
                    runtime.activation_id,
                    runtime.admission_epoch,
                )),
            )
            .is_err()
        {
            data.staged_by_label.remove(&runtime.label);
            return Err(PublicPluginManagementError::Unavailable);
        }
        self.activate_runtime_data_gate(&runtime)?;
        self.cancel_delayed_messages(plugin_id);
        let timer_effects = Vec::new();
        self.clear_runtime_faults(plugin_id)?;
        let config = self.state.publish_prepared(prepared_state)?;
        reservation.publish(Some(candidate.bundle(config.clone())))?;
        let staged = data
            .staged_by_label
            .remove(&runtime.label)
            .filter(|staged| Arc::ptr_eq(staged, &candidate))
            .ok_or(PublicPluginManagementError::Unavailable)?;
        data.active_by_plugin
            .insert(plugin_id.into(), Arc::clone(&staged.runtime));
        drop(data);
        let commit = PublicEnabledCommit {
            mutation: mutation_from_config(&config),
            runtime: Some(runtime),
            closed_runtime_label: previous_runtime_label,
        };
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        Ok(commit)
    }

    fn disable_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<PublicEnabledCommit, PublicPluginManagementError> {
        let (before, runtime, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let before = self
                .state
                .config(plugin_id)?
                .filter(|config| config.installed)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            if !before.enabled {
                return Ok(PublicEnabledCommit {
                    mutation: mutation_from_config(&before),
                    runtime: None,
                    closed_runtime_label: None,
                });
            }
            let bundle = self
                .bundles
                .bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let runtime = bundle.runtime.candidate(
                bundle.activation_id,
                bundle.admission_epoch,
                bundle.runtime_recovery_needed,
            );
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_set_enabled(plugin_id, false)?;
            (before, runtime, reservation, prepared_state)
        };

        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let _ = self.close_runtime_data_gate(&runtime);
        self.cancel_delayed_messages(plugin_id);
        let timer_effects = self.cancel_timer_generation(
            plugin_id,
            before.active_generation,
            runtime.activation_id,
        );
        let config = self.state.publish_prepared(prepared_state)?;
        reservation.publish(None)?;
        let commit = PublicEnabledCommit {
            mutation: mutation_from_config(&config),
            runtime: None,
            closed_runtime_label: Some(runtime.label),
        };
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        Ok(commit)
    }

    pub(crate) fn rename(
        &self,
        plugin_id: &str,
        name_override: Option<&str>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let bundle = self
                .bundles
                .bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_rename(plugin_id, name_override)?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let config = self.state.publish_prepared(prepared_state)?;
        reservation.publish(Some(bundle.with_config(config.clone())))?;
        Ok(mutation_from_config(&config))
    }

    pub(crate) fn set_favorite(
        &self,
        plugin_id: &str,
        favorite: bool,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let bundle = self
                .bundles
                .bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            if !bundle.config.installed {
                return Err(PublicPluginManagementError::Unavailable);
            }
            if bundle.config.favorite == favorite {
                return Ok(mutation_from_config(&bundle.config));
            }
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_set_favorite(plugin_id, favorite)?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        let config = self.state.publish_prepared(prepared_state)?;
        reservation.publish(Some(bundle.with_config(config.clone())))?;
        Ok(mutation_from_config(&config))
    }

    pub(crate) fn save_settings(
        &self,
        plugin_id: &str,
        updates: &BTreeMap<String, Value>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let bundle = self
                .bundles
                .bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_save_settings(
                plugin_id,
                &bundle.runtime.manifest.settings,
                updates,
            )?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let config = self.state.publish_prepared(prepared_state)?;
        reservation.publish(Some(bundle.with_config(config.clone())))?;
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

    pub(crate) fn begin_uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<Option<PublicPluginUninstallTransaction>, PublicPluginManagementError> {
        if self
            .owner_cleanup
            .is_blocked(plugin_id)
            .map_err(|_| PublicPluginManagementError::Unavailable)?
        {
            return Err(PublicPluginManagementError::DataCleanupPending);
        }
        let (
            previous_generation,
            previous_activation_id,
            runtime_label,
            reservation,
            prepared_state,
            previous_bundle,
            data_identity,
            data_drain,
        ) = {
            let _mutation = self.lock_mutation()?;
            let Some(config) = self.state.config(plugin_id)? else {
                return Ok(None);
            };
            let bundle = self.bundles.bundle(plugin_id)?;
            let runtime_label = bundle.as_ref().map(|bundle| bundle.runtime.label.clone());
            let previous_activation_id = bundle.as_ref().map(|bundle| bundle.activation_id);
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self
                .state
                .prepare_uninstall(plugin_id, retain_data)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            if retain_data {
                self.scheduler
                    .invalidate_plugin(plugin_id, None)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
            } else {
                self.scheduler
                    .forget_plugin(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
            }
            let data_identity = bundle
                .as_ref()
                .map(|bundle| {
                    PluginDataCallIdentity::new(
                        plugin_id,
                        bundle.runtime.generation,
                        bundle.activation_id,
                        bundle.admission_epoch,
                    )
                    .map_err(|_| PublicPluginManagementError::Unavailable)
                })
                .transpose()?;
            let data_drain = data_identity
                .as_ref()
                .map(|identity| {
                    self.data_gate
                        .close(identity)
                        .map_err(|_| PublicPluginManagementError::Unavailable)
                })
                .transpose()?;
            (
                config.active_generation,
                previous_activation_id,
                runtime_label,
                reservation,
                prepared_state,
                bundle,
                data_identity,
                data_drain,
            )
        };
        let number = self
            .next_uninstall_transaction
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value != 0).then_some(value)?.checked_add(1)
            })
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let transaction_id = format!("{:016x}{number:016x}", std::process::id());
        Ok(Some(PublicPluginUninstallTransaction {
            plugin_id: plugin_id.into(),
            retain_data,
            previous_generation,
            previous_activation_id,
            runtime_label,
            transaction_id,
            reservation,
            prepared_state,
            previous_bundle,
            data_identity,
            data_drained: data_drain.is_none(),
            data_drain,
        }))
    }

    pub(crate) fn drain_uninstall_data(
        &self,
        transaction: &mut PublicPluginUninstallTransaction,
    ) -> Result<(), PublicPluginManagementError> {
        if let Some(drain) = transaction.data_drain.take() {
            drain
                .wait()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
        }
        transaction.data_drained = true;
        Ok(())
    }

    pub(crate) fn abort_uninstall_before_commit(
        &self,
        transaction: PublicPluginUninstallTransaction,
    ) -> Result<Option<PublicRuntimeCandidate>, PublicPluginManagementError> {
        let PublicPluginUninstallTransaction {
            plugin_id,
            previous_bundle,
            reservation,
            ..
        } = transaction;
        drop(reservation);
        let Some(previous) = previous_bundle else {
            return Ok(None);
        };
        let _mutation = self.lock_mutation()?;
        let config = self
            .state
            .config(&plugin_id)?
            .filter(|config| config.installed && config.enabled && config.fault.is_none())
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let activation_id = self
            .activation_ids
            .allocate()
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let admission_epoch = self
            .admission_epochs
            .allocate()
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let candidate = ActivationCandidate::recovery(
            Arc::clone(&previous.runtime),
            previous.alarm.as_ref(),
            activation_id,
            admission_epoch,
        );
        let runtime = candidate.runtime.candidate(
            candidate.activation_id,
            candidate.admission_epoch,
            candidate.runtime_recovery_needed,
        );
        let reservation = self.bundles.reserve(&plugin_id)?;
        self.scheduler
            .invalidate_plugin(
                &plugin_id,
                Some((
                    runtime.generation,
                    runtime.activation_id,
                    runtime.admission_epoch,
                )),
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        self.activate_runtime_data_gate(&runtime)?;
        reservation.begin_committing()?;
        reservation.mark_durable()?;
        reservation.publish(Some(candidate.bundle(config)))?;
        self.lock_data()?.active_by_plugin.remove(&plugin_id);
        Ok(Some(runtime))
    }

    pub(crate) fn commit_uninstall(
        &self,
        transaction: PublicPluginUninstallTransaction,
    ) -> Result<PublicPluginUninstallCommit, PublicPluginUninstallCommitFailure> {
        if !transaction.data_drained || transaction.data_drain.is_some() {
            return Err(PublicPluginUninstallCommitFailure {
                error: PublicPluginManagementError::Unavailable,
                transaction: Some(Box::new(transaction)),
            });
        }
        let receipt = (!transaction.retain_data)
            .then(|| {
                PluginOwnerCleanupReceipt::new(
                    &transaction.plugin_id,
                    &transaction.transaction_id,
                    transaction.previous_generation,
                    transaction.previous_activation_id,
                )
            })
            .transpose()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        if let Some(receipt) = receipt.as_ref() {
            if let Err(error) = self.owner_cleanup.commit(receipt.clone()) {
                return Err(PublicPluginUninstallCommitFailure {
                    error: match error {
                        OwnerCleanupError::AlreadyPending => {
                            PublicPluginManagementError::DataCleanupPending
                        }
                        _ => PublicPluginManagementError::Unavailable,
                    },
                    transaction: Some(Box::new(transaction)),
                });
            }
        }
        if receipt.is_some() {
            transaction.reservation.begin_committing()?;
            transaction.reservation.mark_durable()?;
            let _ = self.state.persist_prepared(&transaction.prepared_state);
        } else if let Err(error) =
            self.make_state_durable(&transaction.reservation, &transaction.prepared_state)
        {
            return Err(PublicPluginUninstallCommitFailure {
                error,
                transaction: Some(Box::new(transaction)),
            });
        }
        let _mutation = self.lock_mutation()?;
        self.clear_runtime_faults(&transaction.plugin_id)?;
        self.cancel_delayed_messages(&transaction.plugin_id);
        let timer_effects = transaction
            .previous_activation_id
            .map(|activation_id| {
                self.cancel_timer_generation(
                    &transaction.plugin_id,
                    transaction.previous_generation,
                    activation_id,
                )
            })
            .unwrap_or_default();
        self.state.publish_prepared(transaction.prepared_state)?;
        transaction.reservation.publish(None)?;
        if let Some(identity) = transaction.data_identity.as_ref() {
            let _ = self.data_gate.remove(identity);
        }
        self.lock_data()?
            .active_by_plugin
            .remove(&transaction.plugin_id);
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        Ok(PublicPluginUninstallCommit {
            plugin_id: transaction.plugin_id,
            retain_data: transaction.retain_data,
            runtime_label: transaction.runtime_label,
            receipt,
        })
    }

    pub(crate) fn finish_uninstall_cleanup(
        &self,
        committed: &PublicPluginUninstallCommit,
        settings: &crate::settings::SettingsStore,
    ) -> Result<(), PublicPluginManagementError> {
        let mut failed = false;
        if !committed.retain_data {
            failed |= self.storage.uninstall(&committed.plugin_id, false).is_err();
            failed |= self.secrets.uninstall(&committed.plugin_id, false).is_err();
            failed |= self
                .state
                .cleanup_uninstalled_owner(&committed.plugin_id)
                .is_err();
        }
        failed |= remove_owned_directory(&self.packages_root.join(&committed.plugin_id)).is_err();
        failed |= settings
            .remove_plugin_window_position(&committed.plugin_id)
            .is_err();
        if failed {
            return Err(if committed.retain_data {
                PublicPluginManagementError::Unavailable
            } else {
                PublicPluginManagementError::DataCleanupPending
            });
        }
        if let Some(receipt) = committed.receipt.as_ref() {
            let cleared = self
                .owner_cleanup
                .clear(&receipt.plugin_id, &receipt.transaction_id)
                .map_err(|_| PublicPluginManagementError::DataCleanupPending)?;
            if !cleared {
                return Err(PublicPluginManagementError::DataCleanupPending);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<Option<String>, PublicPluginManagementError> {
        let Some(mut transaction) = self.begin_uninstall(plugin_id, retain_data)? else {
            return Ok(None);
        };
        self.drain_uninstall_data(&mut transaction)?;
        let committed = match self.commit_uninstall(transaction) {
            Ok(committed) => committed,
            Err(mut failure) => {
                if let Some(transaction) = failure.transaction.take() {
                    let _ = self.abort_uninstall_before_commit(*transaction);
                }
                return Err(failure.error);
            }
        };
        let settings = crate::settings::SettingsStore::load(&self.app_data_dir)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        self.finish_uninstall_cleanup(&committed, &settings)?;
        Ok(committed.runtime_label)
    }

    pub(crate) fn inventory(&self) -> Result<PublicPluginInventory, PublicPluginManagementError> {
        let configs = self.state.configs()?;
        let active = self
            .bundles
            .bundles()?
            .into_iter()
            .map(|bundle| (bundle.config.plugin_id.clone(), bundle))
            .collect::<HashMap<_, _>>();
        let data = self.lock_data()?;
        let revision = configs
            .iter()
            .map(|config| config.inventory_revision)
            .max()
            .unwrap_or(0);
        let mut items = Vec::new();
        for stored_config in configs.into_iter().filter(|config| config.installed) {
            let active_bundle = active.get(&stored_config.plugin_id);
            let config = active_bundle
                .map(|bundle| bundle.config.clone())
                .unwrap_or(stored_config);
            let snapshot = active_bundle
                .map(|bundle| bundle.runtime.as_ref())
                .or_else(|| {
                    data.active_by_plugin
                        .get(&config.plugin_id)
                        .map(Arc::as_ref)
                });
            let Some(snapshot) = snapshot else {
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
                icon_url: snapshot.icon_url(),
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
        for bundle in self.bundles.bundles()? {
            let config = &bundle.config;
            let snapshot = &bundle.runtime;
            if self
                .owner_cleanup
                .is_blocked(&config.plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?
            {
                continue;
            }
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
                plugin_id: config.plugin_id.clone(),
                generation: snapshot.generation,
                activation_id: bundle.activation_id,
                admission_epoch: bundle.admission_epoch,
                runtime_recovery_needed: bundle.runtime_recovery_needed,
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
                icon_url: snapshot.icon_url(),
            }));
        }
        Ok(None)
    }

    pub(crate) fn command_suggestions(
        &self,
        prefix: &str,
    ) -> Result<Vec<PublicCommandSuggestion>, PublicPluginManagementError> {
        let folded_prefix = prefix.to_lowercase();
        let mut suggestions = Vec::new();
        for bundle in self.bundles.bundles()? {
            let config = &bundle.config;
            let snapshot = &bundle.runtime;
            if !config.installed
                || !config.enabled
                || config.fault.is_some()
                || config.active_generation != snapshot.generation
                || bundle.runtime_recovery_needed
                || (!config.effective_name.starts_with(prefix)
                    && !snapshot
                        .manifest
                        .name
                        .to_lowercase()
                        .contains(&folded_prefix))
            {
                continue;
            }
            suggestions.push(PublicCommandSuggestion {
                plugin_id: config.plugin_id.clone(),
                effective_name: config.effective_name.clone(),
                display_name: snapshot.manifest.name.clone(),
                summary: snapshot.manifest.command.summary.clone(),
                icon_url: snapshot.icon_url(),
                favorite: config.favorite,
            });
        }
        suggestions.sort_by(|left, right| {
            left.effective_name
                .cmp(&right.effective_name)
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
        });
        Ok(suggestions)
    }

    pub(crate) fn launcher_command_suggestions(
        &self,
        query: &str,
    ) -> Result<Vec<PublicCommandSuggestion>, PublicPluginManagementError> {
        let folded_query = query.to_lowercase();
        let mut suggestions = Vec::new();
        for bundle in self.bundles.bundles()? {
            let config = &bundle.config;
            let snapshot = &bundle.runtime;
            if !config.installed
                || !config.enabled
                || config.fault.is_some()
                || config.active_generation != snapshot.generation
                || bundle.runtime_recovery_needed
                || (!config.favorite
                    && !config.effective_name.to_lowercase().contains(&folded_query))
            {
                continue;
            }
            suggestions.push(PublicCommandSuggestion {
                plugin_id: config.plugin_id.clone(),
                effective_name: config.effective_name.clone(),
                display_name: snapshot.manifest.name.clone(),
                summary: snapshot.manifest.command.summary.clone(),
                icon_url: snapshot.icon_url(),
                favorite: config.favorite,
            });
        }
        suggestions.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| left.effective_name.cmp(&right.effective_name))
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
        });
        Ok(suggestions)
    }
    pub(crate) fn runtime_candidates(
        &self,
    ) -> Result<Vec<PublicRuntimeCandidate>, PublicPluginManagementError> {
        Ok(self
            .bundles
            .bundles()?
            .into_iter()
            .filter(|bundle| bundle.config.installed && bundle.config.enabled)
            .filter(|bundle| !bundle.runtime_recovery_needed)
            .map(|bundle| {
                bundle.runtime.candidate(
                    bundle.activation_id,
                    bundle.admission_epoch,
                    bundle.runtime_recovery_needed,
                )
            })
            .collect())
    }

    pub(crate) fn runtime_recovery_candidate(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<Option<PublicRuntimeCandidate>, PublicPluginManagementError> {
        Ok(self
            .bundles
            .bundle(plugin_id)?
            .filter(|bundle| {
                bundle.runtime_recovery_needed
                    && bundle.config.installed
                    && bundle.config.enabled
                    && bundle.config.fault.is_none()
                    && bundle.runtime.generation == generation
                    && bundle.activation_id == activation_id
                    && bundle.admission_epoch == admission_epoch
            })
            .map(|bundle| {
                bundle.runtime.candidate(
                    bundle.activation_id,
                    bundle.admission_epoch,
                    bundle.runtime_recovery_needed,
                )
            }))
    }

    pub(crate) fn finish_runtime_recovery(
        &self,
        candidate: &PublicRuntimeCandidate,
    ) -> Result<bool, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let Some(bundle) = self.bundles.bundle(&candidate.plugin_id)? else {
            return Ok(false);
        };
        if !bundle.runtime_recovery_needed
            || bundle.runtime.generation != candidate.generation
            || bundle.activation_id != candidate.activation_id
            || bundle.admission_epoch != candidate.admission_epoch
        {
            return Ok(false);
        }
        let reservation = self.bundles.reserve(&candidate.plugin_id)?;
        reservation.begin_committing()?;
        reservation.mark_durable()?;
        reservation.publish(Some(bundle.with_runtime_recovery(false)))?;
        self.lock_data()?
            .active_by_plugin
            .insert(candidate.plugin_id.clone(), Arc::clone(&bundle.runtime));
        Ok(true)
    }

    pub(crate) fn asset(&self, label: &str, request_path: &str) -> Option<PublicRuntimeAsset> {
        let staged = {
            let data = self.data.lock().ok()?;
            data.staged_by_label
                .get(label)
                .map(|candidate| Arc::clone(&candidate.runtime))
        };
        let snapshot = staged.or_else(|| {
            self.bundles
                .bundles()
                .ok()?
                .into_iter()
                .find(|bundle| bundle.runtime.label == label)
                .map(|bundle| Arc::clone(&bundle.runtime))
        })?;
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
        let bundle = self.bundles.bundle(plugin_id).ok()??;
        let snapshot = &bundle.runtime;
        let config = &bundle.config;
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

    pub(crate) fn icon_asset(
        &self,
        caller_label: &str,
        request_path: &str,
        now: Instant,
    ) -> Option<PublicIconAsset> {
        match icon::parse_request_path(request_path)? {
            IconRequest::Prepared { token } => {
                if caller_label != "main" {
                    return None;
                }
                let data = self.data.lock().ok()?;
                let pending = data.pending.get(&token)?;
                if pending.caller_label != caller_label || pending.expires_at <= now {
                    return None;
                }
                Some(PublicIconAsset {
                    bytes: pending.icon.clone()?,
                    cache_control: "no-store",
                })
            }
            IconRequest::Installed {
                plugin_id,
                generation,
            } => {
                let config = self.state.config(&plugin_id).ok()??;
                let active_bundle = self.bundles.bundle(&plugin_id).ok()?;
                let snapshot = active_bundle
                    .as_ref()
                    .map(|bundle| Arc::clone(&bundle.runtime))
                    .or_else(|| {
                        self.data
                            .lock()
                            .ok()?
                            .active_by_plugin
                            .get(&plugin_id)
                            .cloned()
                    })?;
                if !config.installed
                    || config.active_generation != generation
                    || snapshot.generation != generation
                    || config.package_digest.as_deref() != Some(snapshot.digest.as_str())
                {
                    return None;
                }
                let main_allowed = caller_label == "main";
                let shell_allowed = active_bundle.is_some()
                    && config.enabled
                    && config.fault.is_none()
                    && snapshot.manifest.command.output_mode == PublicOutputMode::Window
                    && snapshot
                        .manifest
                        .permissions
                        .contains(&PublicPermission::UiWindow)
                    && crate::plugin_window::plugin_id_from_shell_label(caller_label).as_deref()
                        == Some(plugin_id.as_str());
                if !main_allowed && !shell_allowed {
                    return None;
                }
                Some(PublicIconAsset {
                    bytes: snapshot.resources.get(ICON_PATH)?.bytes.clone(),
                    cache_control: "public, max-age=31536000, immutable",
                })
            }
        }
    }

    pub(crate) fn window_identity(
        &self,
        plugin_id: &str,
    ) -> Result<PublicPluginWindowIdentity, PublicPluginManagementError> {
        let bundle = self
            .bundles
            .bundle(plugin_id)?
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let snapshot = &bundle.runtime;
        let config = &bundle.config;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || config.active_generation != snapshot.generation
            || config.package_digest.as_deref() != Some(snapshot.digest.as_str())
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        if snapshot.manifest.command.output_mode != PublicOutputMode::Window
            || !snapshot
                .manifest
                .permissions
                .contains(&PublicPermission::UiWindow)
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok(PublicPluginWindowIdentity {
            name: snapshot.manifest.name.clone(),
            icon_url: snapshot.icon_url(),
        })
    }

    pub(crate) fn message_icon_url(&self, plugin_id: &str) -> Option<String> {
        let bundle = self.bundles.bundle(plugin_id).ok()??;
        bundle.runtime.icon_url()
    }

    pub(crate) fn can_copy_text(&self, plugin_id: &str, generation: u64) -> bool {
        self.bundles
            .bundle(plugin_id)
            .ok()
            .flatten()
            .is_some_and(|bundle| {
                let config = &bundle.config;
                config.installed
                    && config.enabled
                    && config.fault.is_none()
                    && config.active_generation == generation
                    && bundle.runtime.generation == generation
                    && config
                        .permission_grants
                        .contains(&PublicPermission::ClipboardWrite)
            })
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

    pub(crate) fn timer_service(&self) -> Arc<PluginTimerService> {
        Arc::clone(&self.timers)
    }

    pub(crate) fn window_storage_get(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        key: &str,
    ) -> Result<Option<Value>, WindowStorageError> {
        let (scope, _lease) =
            self.authorize_window_storage(plugin_id, generation, activation_id, admission_epoch)?;
        self.storage
            .get(&scope, plugin_id, key)
            .map_err(map_window_storage_error)
    }

    pub(crate) fn window_storage_set(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        key: &str,
        value: Value,
    ) -> Result<(), WindowStorageError> {
        let (scope, _lease) =
            self.authorize_window_storage(plugin_id, generation, activation_id, admission_epoch)?;
        self.storage
            .set(&scope, plugin_id, key, value)
            .map_err(map_window_storage_error)
    }

    pub(crate) fn window_storage_remove(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        key: &str,
    ) -> Result<(), WindowStorageError> {
        let (scope, _lease) =
            self.authorize_window_storage(plugin_id, generation, activation_id, admission_epoch)?;
        self.storage
            .remove(&scope, plugin_id, key)
            .map_err(map_window_storage_error)
    }

    fn authorize_window_storage(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<(PluginDataScope, super::data_call_gate::PluginDataCallLease), WindowStorageError>
    {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| WindowStorageError::StorageError)?;
        let bundle = self
            .bundles
            .bundle(plugin_id)
            .map_err(|_| WindowStorageError::StorageError)?
            .filter(|bundle| {
                bundle.config.installed
                    && bundle.config.enabled
                    && bundle.config.fault.is_none()
                    && !bundle.runtime_recovery_needed
                    && bundle.runtime.generation == generation
                    && bundle.activation_id == activation_id
                    && bundle.admission_epoch == admission_epoch
            })
            .ok_or(WindowStorageError::ExpiredWindowSessionError)?;
        let identity = PluginDataCallIdentity::new(
            plugin_id,
            bundle.runtime.generation,
            bundle.activation_id,
            bundle.admission_epoch,
        )
        .map_err(|_| WindowStorageError::InvalidContext)?;
        let lease = self
            .data_gate
            .try_acquire(&identity)
            .map_err(|error| match error {
                super::data_call_gate::DataCallGateError::Expired => {
                    WindowStorageError::ExpiredWindowSessionError
                }
                super::data_call_gate::DataCallGateError::InvalidIdentity => {
                    WindowStorageError::InvalidContext
                }
                super::data_call_gate::DataCallGateError::Unavailable => {
                    WindowStorageError::StorageError
                }
            })?;
        let scope =
            PluginDataScope::new(plugin_id).map_err(|_| WindowStorageError::InvalidContext)?;
        Ok((scope, lease))
    }

    pub(crate) fn window_timer_get_state(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
    ) -> Result<PluginTimerState, TimerError> {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| TimerError::TimerUnavailable)?;
        let (key, _, _) = self.authorize_window_timer(plugin_id, generation, activation_id)?;
        self.timers.get_state(&key)
    }

    pub(crate) fn window_timer_start(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        input: Option<PluginTimerStartInput>,
    ) -> Result<PluginTimerState, TimerError> {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| TimerError::TimerUnavailable)?;
        let (key, plugin_name, alarm) =
            self.authorize_window_timer(plugin_id, generation, activation_id)?;
        let operation = self.timers.start(
            &key,
            &plugin_name,
            input,
            self.timer_publisher.is_available(),
            &alarm,
        );
        drop(_mutation);
        self.apply_timer_post_lock_effects(operation.post_lock_effects);
        Ok(operation.result?.state)
    }

    pub(crate) fn window_timer_stop(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
    ) -> Result<PluginTimerState, TimerError> {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| TimerError::TimerUnavailable)?;
        let (key, _, _) = self.authorize_window_timer(plugin_id, generation, activation_id)?;
        let operation = self.timers.stop(&key);
        drop(_mutation);
        self.apply_timer_post_lock_effects(operation.post_lock_effects);
        Ok(operation.result?.state)
    }

    pub(crate) fn window_timer_reset(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
    ) -> Result<PluginTimerState, TimerError> {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| TimerError::TimerUnavailable)?;
        let (key, _, _) = self.authorize_window_timer(plugin_id, generation, activation_id)?;
        let operation = self.timers.reset(&key);
        drop(_mutation);
        self.apply_timer_post_lock_effects(operation.post_lock_effects);
        Ok(operation.result?.state)
    }

    fn authorize_window_timer(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
    ) -> Result<(TimerKey, String, ValidatedAlarmAsset), TimerError> {
        let bundle = self
            .bundles
            .bundle(plugin_id)
            .map_err(|_| TimerError::TimerUnavailable)?
            .ok_or(TimerError::ExpiredWindowSessionError)?;
        let config = &bundle.config;
        let snapshot = &bundle.runtime;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || config.active_generation != generation
            || snapshot.generation != generation
            || bundle.activation_id != activation_id
        {
            return Err(TimerError::ExpiredWindowSessionError);
        }
        let required = [
            PublicPermission::UiWindow,
            PublicPermission::NotificationsPublish,
            PublicPermission::TimerControl,
        ];
        if !required.iter().all(|permission| {
            config.permission_grants.contains(permission)
                && snapshot.manifest.permissions.contains(permission)
        }) {
            return Err(TimerError::PermissionDenied);
        }
        let key = TimerKey::new(plugin_id, generation, activation_id)
            .ok_or(TimerError::TimerUnavailable)?;
        let alarm = bundle.alarm.clone().ok_or(TimerError::TimerUnavailable)?;
        Ok((key, config.effective_name.clone(), alarm))
    }

    pub(crate) fn start_timers(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), PublicPluginManagementError> {
        let manager = Arc::downgrade(self);
        let app = app.clone();
        self.timers
            .start_worker(Arc::new(move |ticket| {
                let Some(manager) = manager.upgrade() else {
                    return;
                };
                let key = ticket.key().clone();
                let dispatch = manager.commit_timer_claim(ticket);
                manager.message_center.dispatch_timer_post_guard(
                    &app,
                    dispatch.effect,
                    dispatch.audio,
                );
                if let Ok(state) = manager.timers.get_state(&key) {
                    let controller =
                        app.state::<Arc<crate::plugin_window::PluginWindowController>>();
                    crate::plugin_window::publish_timer_state(
                        &app,
                        controller.inner().as_ref(),
                        &key,
                        &state,
                    );
                }
            }))
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    pub(crate) fn shutdown_timers(&self) {
        let operation = self.timers.shutdown();
        self.apply_timer_post_lock_effects(operation.post_lock_effects);
    }

    fn commit_timer_claim(&self, ticket: ClaimTicket) -> TimerClaimDispatch {
        let admitted = {
            let Ok(mutation) = self.lock_mutation() else {
                return TimerClaimDispatch {
                    effect: None,
                    audio: None,
                };
            };
            let key = ticket.key();
            let eligible = self
                .bundles
                .bundle(&key.plugin_id)
                .ok()
                .flatten()
                .is_some_and(|bundle| {
                    let config = &bundle.config;
                    let snapshot = &bundle.runtime;
                    config.installed
                        && config.enabled
                        && config.fault.is_none()
                        && config.active_generation == key.plugin_generation
                        && snapshot.generation == key.plugin_generation
                        && bundle.activation_id == key.activation_id
                        && [
                            PublicPermission::UiWindow,
                            PublicPermission::NotificationsPublish,
                            PublicPermission::TimerControl,
                        ]
                        .into_iter()
                        .all(|permission| {
                            config.permission_grants.contains(&permission)
                                && snapshot.manifest.permissions.contains(&permission)
                        })
                });
            if !eligible {
                drop(mutation);
                let _ = self.timers.complete_claim(&ticket, false);
                return TimerClaimDispatch {
                    effect: None,
                    audio: None,
                };
            }
            let admitted = self.timers.admit_claim(&ticket).unwrap_or(false);
            drop(mutation);
            admitted
        };
        if !admitted {
            return TimerClaimDispatch {
                effect: None,
                audio: None,
            };
        }

        let outcome = self.timer_publisher.commit_publish(MessagePublishRequest {
            plugin_id: ticket.frozen_completion.plugin_id.clone(),
            plugin_name_snapshot: ticket.frozen_completion.plugin_name_snapshot.clone(),
            content: ticket.frozen_completion.completion_message.clone(),
        });
        let (persisted, effect) = match outcome {
            MessagePublishOutcome::Published(published) => {
                (true, Some(MessagePostGuardEffect::Published(published)))
            }
            MessagePublishOutcome::BecameUnavailable => {
                (false, Some(MessagePostGuardEffect::BecameUnavailable))
            }
            MessagePublishOutcome::OperationFailed | MessagePublishOutcome::Unavailable => {
                (false, None)
            }
        };
        let completion = self
            .timers
            .complete_claim(&ticket, persisted)
            .ok()
            .flatten();
        TimerClaimDispatch {
            effect,
            audio: completion.and_then(|completion| completion.audio),
        }
    }

    pub(crate) fn start_delayed_messages(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), PublicPluginManagementError> {
        let manager = Arc::downgrade(self);
        let app = app.clone();
        self.delayed_messages
            .start(move |message| {
                let Some(manager) = manager.upgrade() else {
                    return;
                };
                let effect = manager.commit_scheduled_message(message);
                manager.message_center.dispatch_post_guard(&app, effect);
            })
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    pub(crate) fn shutdown_delayed_messages(&self) {
        self.delayed_messages.shutdown();
    }

    pub(crate) fn commit_scheduled_message(
        &self,
        message: ScheduledPluginMessage,
    ) -> Option<MessagePostGuardEffect> {
        let mutation = self.lock_mutation().ok()?;
        let eligible = self
            .bundles
            .bundle(&message.plugin_id)
            .ok()
            .flatten()
            .is_some_and(|bundle| {
                let config = &bundle.config;
                let snapshot = &bundle.runtime;
                config.installed
                    && config.enabled
                    && config.fault.is_none()
                    && bundle.activation_id == message.activation_id
                    && config.active_generation == message.plugin_generation
                    && snapshot.generation == message.plugin_generation
                    && config
                        .permission_grants
                        .contains(&PublicPermission::NotificationsPublish)
                    && snapshot
                        .manifest
                        .permissions
                        .contains(&PublicPermission::NotificationsPublish)
            });
        drop(mutation);
        if !eligible {
            return None;
        }
        match self.message_center.commit_publish(MessagePublishRequest {
            plugin_id: message.plugin_id,
            plugin_name_snapshot: message.plugin_name_snapshot,
            content: message.content,
        }) {
            MessagePublishOutcome::Published(published) => {
                Some(MessagePostGuardEffect::Published(published))
            }
            MessagePublishOutcome::BecameUnavailable => {
                Some(MessagePostGuardEffect::BecameUnavailable)
            }
            MessagePublishOutcome::OperationFailed | MessagePublishOutcome::Unavailable => None,
        }
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

    #[cfg(test)]
    fn activation_id(&self, plugin_id: &str) -> Option<u64> {
        self.bundles
            .bundle(plugin_id)
            .ok()
            .flatten()
            .map(|bundle| bundle.activation_id)
    }

    #[cfg(test)]
    fn admission_epoch(&self, plugin_id: &str) -> Option<u64> {
        self.bundles
            .bundle(plugin_id)
            .ok()
            .flatten()
            .map(|bundle| bundle.admission_epoch)
    }

    pub(crate) fn replace_runtime_generation(
        &self,
        plugin_id: &str,
        previous_generation: u64,
        new_generation: u64,
    ) -> Result<PublicRuntimeCandidate, PublicPluginManagementError> {
        let (candidate, runtime, previous_activation_id, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let current = self
                .bundles
                .bundle(plugin_id)?
                .filter(|bundle| {
                    bundle.config.installed
                        && bundle.config.enabled
                        && bundle.config.fault.is_none()
                        && bundle.config.active_generation == previous_generation
                        && bundle.runtime.generation == previous_generation
                })
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let mut replacement = (*current.runtime).clone();
            replacement.generation = new_generation;
            replacement.label = runtime_label(plugin_id, new_generation)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let activation_id = self
                .activation_ids
                .allocate()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let admission_epoch = self
                .admission_epochs
                .allocate()
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let candidate = Arc::new(ActivationCandidate::reactivate(
                Arc::new(replacement),
                current.alarm.as_ref(),
                activation_id,
                admission_epoch,
            ));
            let runtime = candidate.runtime.candidate(
                candidate.activation_id,
                candidate.admission_epoch,
                candidate.runtime_recovery_needed,
            );
            let previous_activation_id = current.activation_id;
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_activation(
                &candidate.runtime.manifest,
                current.config.permission_grants.clone(),
                new_generation,
                Some(candidate.runtime.digest.clone()),
            )?;
            (
                candidate,
                runtime,
                previous_activation_id,
                reservation,
                prepared_state,
            )
        };
        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        self.cancel_delayed_messages(plugin_id);
        let timer_effects =
            self.cancel_timer_generation(plugin_id, previous_generation, previous_activation_id);
        let config = self.state.publish_prepared(prepared_state)?;
        self.activate_runtime_data_gate(&runtime)?;
        reservation.publish(Some(candidate.bundle(config)))?;
        self.lock_data()?
            .active_by_plugin
            .insert(plugin_id.into(), Arc::clone(&candidate.runtime));
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        Ok(runtime)
    }

    pub(crate) fn record_runtime_result(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        success: bool,
        now: Instant,
    ) -> Result<bool, PublicPluginManagementError> {
        if !self.bundles.bundle(plugin_id)?.is_some_and(|bundle| {
            bundle.activation_id == activation_id && bundle.runtime.generation == generation
        }) {
            return Ok(false);
        }
        let disable = {
            let mut faults = self
                .runtime_faults
                .lock()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let window = faults.entry((plugin_id.into(), activation_id)).or_default();
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
        self.disable_plugin_for_fault_if_current(
            plugin_id,
            generation,
            activation_id,
            PublicPluginFault::RuntimeUnavailable,
        )
    }

    fn clear_runtime_faults(&self, plugin_id: &str) -> Result<(), PublicPluginManagementError> {
        self.runtime_faults
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .retain(|(owner, _), _| owner != plugin_id);
        Ok(())
    }
    pub(crate) fn mark_runtime_unavailable(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
    ) -> Result<(), PublicPluginManagementError> {
        self.disable_plugin_for_fault_if_current(
            plugin_id,
            generation,
            activation_id,
            PublicPluginFault::RuntimeUnavailable,
        )?;
        Ok(())
    }

    #[cfg(test)]
    fn disable_plugin_for_fault(
        &self,
        plugin_id: &str,
        fault: PublicPluginFault,
    ) -> Result<(), PublicPluginManagementError> {
        let Some(bundle) = self.bundles.bundle(plugin_id)? else {
            return Ok(());
        };
        self.disable_plugin_for_fault_if_current(
            plugin_id,
            bundle.runtime.generation,
            bundle.activation_id,
            fault,
        )?;
        Ok(())
    }

    fn disable_plugin_for_fault_if_current(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        fault: PublicPluginFault,
    ) -> Result<bool, PublicPluginManagementError> {
        let (generation, runtime, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let config = self
                .state
                .config(plugin_id)?
                .filter(|config| config.installed && config.enabled)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let bundle = self
                .bundles
                .bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            if bundle.activation_id != activation_id || bundle.runtime.generation != generation {
                return Ok(false);
            }
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_disable_for_fault(plugin_id, fault)?;
            (
                config.active_generation,
                bundle.runtime.candidate(
                    bundle.activation_id,
                    bundle.admission_epoch,
                    bundle.runtime_recovery_needed,
                ),
                reservation,
                prepared_state,
            )
        };
        self.make_state_durable(&reservation, &prepared_state)?;
        let _mutation = self.lock_mutation()?;
        self.scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let _ = self.close_runtime_data_gate(&runtime);
        self.cancel_delayed_messages(plugin_id);
        let timer_effects = self.cancel_timer_generation(plugin_id, generation, activation_id);
        self.state.publish_prepared(prepared_state)?;
        reservation.publish(None)?;
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        Ok(true)
    }

    fn cancel_delayed_messages(&self, plugin_id: &str) {
        let _ = self.delayed_messages.cancel_plugin(plugin_id);
    }

    fn cancel_timer_generation(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
    ) -> Vec<TimerPostLockEffect> {
        let Some(key) = TimerKey::new(plugin_id, generation, activation_id) else {
            return Vec::new();
        };
        self.timers.cancel_generation(&key).post_lock_effects
    }

    fn apply_timer_post_lock_effects(&self, effects: Vec<TimerPostLockEffect>) {
        for effect in effects {
            match effect {
                TimerPostLockEffect::AudioCancelled(audio) => {
                    self.message_center.cancel_timer_audio(audio);
                }
            }
        }
    }

    fn active_manifest(
        &self,
        plugin_id: &str,
    ) -> Result<PublicManifestV1, PublicPluginManagementError> {
        self.bundles
            .bundle(plugin_id)?
            .map(|bundle| bundle.runtime.manifest.clone())
            .ok_or(PublicPluginManagementError::Unavailable)
    }

    fn manifest_for_label(&self, label: &str) -> Option<PublicManifestV1> {
        let bundle = self
            .bundles
            .bundles()
            .ok()?
            .into_iter()
            .find(|bundle| bundle.runtime.label == label)?;
        let snapshot = &bundle.runtime;
        let config = &bundle.config;
        (config.installed
            && config.enabled
            && config.fault.is_none()
            && config.active_generation == snapshot.generation
            && config.package_digest.as_deref() == Some(snapshot.digest.as_str()))
        .then(|| snapshot.manifest.clone())
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

    fn make_state_durable(
        &self,
        reservation: &ActivationReservation,
        prepared: &PreparedStateCommit,
    ) -> Result<(), PublicPluginManagementError> {
        reservation.begin_committing()?;
        match self.state.persist_prepared(prepared) {
            DurableStateOutcome::Committed => reservation.mark_durable().map_err(Into::into),
            DurableStateOutcome::NotCommitted => {
                if !self.state.revalidate_previous(prepared) {
                    return Err(PublicPluginManagementError::Unavailable);
                }
                reservation.rollback_not_committed()?;
                Err(PublicPluginManagementError::Unavailable)
            }
            DurableStateOutcome::Unknown => Err(PublicPluginManagementError::Unavailable),
        }
    }

    fn lock_data(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ActivationData>, PublicPluginManagementError> {
        self.data
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    fn activate_runtime_data_gate(
        &self,
        runtime: &PublicRuntimeCandidate,
    ) -> Result<(), PublicPluginManagementError> {
        let identity = PluginDataCallIdentity::new(
            &runtime.plugin_id,
            runtime.generation,
            runtime.activation_id,
            runtime.admission_epoch,
        )
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
        self.data_gate
            .activate(identity)
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    fn close_runtime_data_gate(
        &self,
        runtime: &PublicRuntimeCandidate,
    ) -> Result<PluginDataCallDrain, PublicPluginManagementError> {
        let identity = PluginDataCallIdentity::new(
            &runtime.plugin_id,
            runtime.generation,
            runtime.activation_id,
            runtime.admission_epoch,
        )
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
        self.data_gate
            .close(&identity)
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

fn map_window_storage_error(error: super::storage::PluginStorageError) -> WindowStorageError {
    match error {
        super::storage::PluginStorageError::InvalidKey
        | super::storage::PluginStorageError::InvalidValue => WindowStorageError::InvalidOperation,
        super::storage::PluginStorageError::Storage
        | super::storage::PluginStorageError::QuotaExceeded => WindowStorageError::StorageError,
        super::storage::PluginStorageError::InvalidPlugin
        | super::storage::PluginStorageError::InvalidScope => WindowStorageError::InvalidContext,
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
) -> Result<(RuntimeSnapshot, Option<super::PreparedAlarmAsset>), PublicPluginManagementError> {
    let (manifest, resources, alarm) = package::load_existing(package_root, host, digest)?;
    Ok((
        snapshot_from_parts(package_root, manifest, digest.into(), resources, generation)?,
        alarm,
    ))
}

impl From<ReservationError> for PublicPluginManagementError {
    fn from(_: ReservationError) -> Self {
        Self::Unavailable
    }
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
        "<!doctype html><html data-runtime-entry=\"/{entry}\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'self'; connect-src ipc: http://ipc.localhost; media-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'\"></head><body></body></html>"
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
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc, Arc,
        },
        thread,
        time::Duration,
    };

    use serde_json::json;

    use super::*;
    use crate::message_center::MessagePostGuardEffect;
    use crate::public_plugins::{
        delayed_messages::{DelayedMessageRegistration, ScheduledPluginMessage},
        scheduler::{PluginRequestCandidate, PluginScheduleOutcome, PluginSubmissionOwner},
        timers::{Clock, PluginTimerPhase, PluginTimerStartInput, TimerKey},
        PublicPlatform,
    };
    use crate::settings::{SettingsStore, WindowPosition};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct TestClock(AtomicU64);

    impl TestClock {
        fn advance(&self, millis: u64) {
            self.0.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl Clock for TestClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    struct TestPublisher {
        outcome: MessagePublishOutcome,
        calls: Mutex<Vec<MessagePublishRequest>>,
    }

    impl TestPublisher {
        fn successful() -> Self {
            Self {
                outcome: MessagePublishOutcome::Published(
                    crate::message_center::MessagePublished {
                        id: "1".into(),
                        plugin_id: "com.example.timer".into(),
                        plugin_name_snapshot: "Timer".into(),
                        created_at: "2026-08-20T00:00:00Z".into(),
                        content: "finished".into(),
                        revision: "1".into(),
                        unread_count: 1,
                    },
                ),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                outcome: MessagePublishOutcome::OperationFailed,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl MessagePublisher for TestPublisher {
        fn is_available(&self) -> bool {
            true
        }

        fn commit_publish(&self, request: MessagePublishRequest) -> MessagePublishOutcome {
            self.calls.lock().unwrap().push(request);
            self.outcome.clone()
        }
    }

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
        PublicPluginManager::load_with_timer_dependencies(
            dir.path(),
            PublicPluginHost::current(PublicPlatform::Windows),
            ["find".into(), "math".into()],
            message_center,
            Arc::new(TestClock::default()),
            Arc::new(TestPublisher::successful()),
        )
        .unwrap()
    }

    fn timer_manager(
        dir: &TestDir,
        clock: Arc<TestClock>,
        publisher: Arc<TestPublisher>,
    ) -> PublicPluginManager {
        PublicPluginManager::load_with_timer_dependencies(
            dir.path(),
            PublicPluginHost::current(PublicPlatform::Windows),
            ["find".into(), "math".into()],
            Arc::new(MessageCenterService::load(dir.path())),
            clock,
            publisher,
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
                "permissions": [],
                "settings": [{
                    "type": "boolean",
                    "key": "compact",
                    "label": "Compact",
                    "default": false
                }]
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

    fn write_notification_package(root: &Path, version: &str) {
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
                    "activationMode": "submit",
                    "outputMode": "mainResult",
                    "inputRequired": false
                },
                "runtime": { "entry": "dist/runtime.js" },
                "permissions": ["notifications.publish"]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("dist/runtime.js"),
            "export async function onCommand() { return { results: [] }; }",
        )
        .unwrap();
    }

    fn write_timer_package(root: &Path, version: &str) {
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::create_dir_all(root.join("assets/sounds")).unwrap();
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pluginId": "com.example.timer",
                "version": version,
                "apiVersion": 1,
                "minimumHostVersion": "0.2.0",
                "name": "Timer",
                "supportedPlatforms": ["windows"],
                "command": {
                    "defaultName": "timer",
                    "activationMode": "submit",
                    "outputMode": "window",
                    "inputRequired": false
                },
                "runtime": { "entry": "dist/runtime.js" },
                "window": { "entry": "dist/window.html" },
                "permissions": ["ui.window", "notifications.publish", "timer.control"]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("dist/runtime.js"),
            "export async function onCommand() { return { data: {} }; }",
        )
        .unwrap();
        fs::write(root.join("dist/window.html"), "<!doctype html>").unwrap();
        fs::write(root.join("assets/sounds/timer-alarm.wav"), test_alarm_wav()).unwrap();
    }

    fn write_timer_package_with_alarm_marker(root: &Path, version: &str, marker: u8) {
        write_timer_package(root, version);
        let mut bytes = test_alarm_wav();
        bytes[44] = marker;
        fs::write(root.join("assets/sounds/timer-alarm.wav"), bytes).unwrap();
    }

    fn test_alarm_wav() -> Vec<u8> {
        let channels = 1_u16;
        let sample_rate = 44_100_u32;
        let bits_per_sample = 16_u16;
        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let data = vec![0_u8; usize::from(block_align) * 100];
        let riff_size = 36_u32 + u32::try_from(data.len()).unwrap();
        let mut wav = Vec::with_capacity(44 + data.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&riff_size.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    fn install_timer(manager: &PublicPluginManager, source_path: &Path) -> u64 {
        let now = Instant::now();
        let prepared = manager.prepare("main", source(source_path), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                    PublicPermission::TimerControl,
                ]),
                now,
                |_| true,
            )
            .unwrap()
            .runtime
            .generation
    }

    fn start_due_timer(
        manager: &PublicPluginManager,
        clock: &TestClock,
        generation: u64,
    ) -> (TimerKey, super::super::timers::ClaimTicket) {
        let bundle = manager
            .bundles
            .bundle("com.example.timer")
            .unwrap()
            .unwrap();
        let key = TimerKey::new("com.example.timer", generation, bundle.activation_id).unwrap();
        let alarm = bundle.alarm.as_ref().unwrap();
        manager
            .timers
            .start(
                &key,
                "Timer",
                Some(PluginTimerStartInput {
                    duration_ms: 1_000,
                    completion_message: "finished".into(),
                }),
                true,
                alarm,
            )
            .result
            .unwrap();
        clock.advance(1_000);
        let ticket = manager.timers.claim_next_due().unwrap().unwrap();
        (key, ticket)
    }

    fn schedule_test_message(
        manager: &PublicPluginManager,
        generation: u64,
        request_id: &str,
        now: Instant,
    ) {
        manager
            .delayed_messages
            .schedule(
                DelayedMessageRegistration {
                    plugin_id: "com.example.activation".into(),
                    plugin_generation: generation,
                    activation_id: manager.activation_id("com.example.activation").unwrap(),
                    plugin_name_snapshot: "Activation".into(),
                    request_id: request_id.into(),
                    content: request_id.into(),
                    delay_ms: 1_000,
                },
                now,
            )
            .unwrap();
    }

    fn write_discovery_package(
        root: &Path,
        plugin_id: &str,
        name: &str,
        default_name: &str,
        summary: &str,
    ) {
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pluginId": plugin_id,
                "version": "1.0.0",
                "apiVersion": 1,
                "minimumHostVersion": "0.2.0",
                "name": name,
                "supportedPlatforms": ["windows"],
                "command": {
                    "defaultName": default_name,
                    "summary": summary,
                    "activationMode": "submit",
                    "outputMode": "mainResult",
                    "inputRequired": true,
                    "inputPlaceholder": "请输入信息回车"
                },
                "runtime": { "entry": "dist/runtime.js" },
                "permissions": []
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("dist/runtime.js"),
            "export async function onCommand() {}",
        )
        .unwrap();
    }

    fn install_discovery_package(manager: &PublicPluginManager, source: &Path, now: Instant) {
        let prepared = manager.prepare("main", self::source(source), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
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
    fn fault_disabled_upgrade_discards_the_probe_runtime_and_can_be_enabled() {
        let dir = TestDir::new("fault-disabled-upgrade");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        manager
            .disable_plugin_for_fault(
                "com.example.activation",
                PublicPluginFault::RuntimeUnavailable,
            )
            .unwrap();

        write_package(&dir.source(), "1.1.0", "two");
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let upgraded = manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();

        assert!(!upgraded.mutation.enabled);
        assert!(manager
            .bundles
            .bundle("com.example.activation")
            .unwrap()
            .is_none());
        assert!(!manager
            .data
            .lock()
            .unwrap()
            .active_by_plugin
            .contains_key("com.example.activation"));
        assert!(manager
            .asset(&upgraded.runtime.label, "/dist/runtime.js")
            .is_none());

        let enabled = manager
            .set_enabled_with_readiness("com.example.activation", true, |_| true)
            .unwrap();
        assert!(enabled.mutation.enabled);
        assert!(enabled.runtime.is_some());
        assert!(manager
            .bundles
            .bundle("com.example.activation")
            .unwrap()
            .is_some());
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
    fn disable_update_and_uninstall_cancel_pending_delayed_messages() {
        let dir = TestDir::new("delayed-lifecycle");
        write_notification_package(&dir.source(), "1.0.0");
        let manager = manager(&dir);
        let now = Instant::now();
        let grants = BTreeSet::from([PublicPermission::NotificationsPublish]);
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let installed = manager
            .commit_with_readiness("main", &prepared.token, grants.clone(), now, |_| true)
            .unwrap();

        schedule_test_message(&manager, installed.runtime.generation, "disabled", now);
        manager
            .set_enabled_with_readiness("com.example.activation", false, |_| false)
            .unwrap();
        assert_eq!(
            manager
                .delayed_messages
                .claim_due(now + Duration::from_secs(2)),
            Ok(None)
        );

        let enabled = manager
            .set_enabled_with_readiness("com.example.activation", true, |_| true)
            .unwrap()
            .runtime
            .unwrap();
        schedule_test_message(&manager, enabled.generation, "updated", now);
        write_notification_package(&dir.source(), "1.1.0");
        let update = manager.prepare("main", source(&dir.source()), now).unwrap();
        let updated = manager
            .commit_with_readiness("main", &update.token, grants, now, |_| true)
            .unwrap();
        assert_eq!(
            manager
                .delayed_messages
                .claim_due(now + Duration::from_secs(2)),
            Ok(None)
        );

        schedule_test_message(&manager, updated.runtime.generation, "uninstalled", now);
        manager.uninstall("com.example.activation", true).unwrap();
        assert_eq!(
            manager
                .delayed_messages
                .claim_due(now + Duration::from_secs(2)),
            Ok(None)
        );
    }

    #[test]
    fn delayed_delivery_commits_only_for_the_current_authorized_generation() {
        let dir = TestDir::new("delayed-delivery");
        write_notification_package(&dir.source(), "1.0.0");
        let manager = manager(&dir);
        let now = Instant::now();
        let grants = BTreeSet::from([PublicPermission::NotificationsPublish]);
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let installed = manager
            .commit_with_readiness("main", &prepared.token, grants, now, |_| true)
            .unwrap();
        let message = |generation, activation_id, request_id: &str| ScheduledPluginMessage {
            schedule_id: generation,
            plugin_id: "com.example.activation".into(),
            plugin_generation: generation,
            activation_id,
            plugin_name_snapshot: "Activation".into(),
            request_id: request_id.into(),
            content: request_id.into(),
            due_at: now,
        };

        let effect = manager.commit_scheduled_message(message(
            installed.runtime.generation,
            installed.runtime.activation_id,
            "current",
        ));
        assert!(matches!(
            effect,
            Some(MessagePostGuardEffect::Published(ref published))
                if published.content == "current"
        ));
        assert_eq!(manager.message_center.summary().unwrap().unread_count, 1);

        assert_eq!(
            manager.commit_scheduled_message(message(
                installed.runtime.generation + 1,
                installed.runtime.activation_id,
                "stale",
            )),
            None
        );
        assert_eq!(manager.message_center.summary().unwrap().unread_count, 1);

        manager.uninstall("com.example.activation", false).unwrap();
        let later = now + Duration::from_secs(1);
        let prepared = manager
            .prepare("main", source(&dir.source()), later)
            .unwrap();
        let reinstalled = manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NotificationsPublish]),
                later,
                |_| true,
            )
            .unwrap();
        assert_eq!(reinstalled.runtime.generation, installed.runtime.generation);
        assert_ne!(
            reinstalled.runtime.activation_id,
            installed.runtime.activation_id
        );
        assert_eq!(
            manager.commit_scheduled_message(message(
                reinstalled.runtime.generation,
                installed.runtime.activation_id,
                "old-activation",
            )),
            None
        );
        assert!(manager
            .commit_scheduled_message(message(
                reinstalled.runtime.generation,
                reinstalled.runtime.activation_id,
                "new-activation",
            ))
            .is_some());
    }

    #[test]
    fn timer_delivery_persists_then_returns_audio_for_coordinator_admission() {
        let dir = TestDir::new("timer-delivery");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let publisher = Arc::new(TestPublisher::successful());
        let manager = timer_manager(&dir, clock.clone(), publisher.clone());
        let generation = install_timer(&manager, &dir.source());
        let (key, ticket) = start_due_timer(&manager, &clock, generation);

        let effect = manager.commit_timer_claim(ticket);

        assert!(matches!(
            effect.effect,
            Some(MessagePostGuardEffect::Published(_))
        ));
        assert!(effect.audio.is_some());
        assert_eq!(publisher.calls.lock().unwrap().len(), 1);
        assert_eq!(
            manager.timers.get_state(&key).unwrap().phase,
            PluginTimerPhase::Fired
        );
    }

    #[test]
    fn successful_update_freezes_the_new_alarm_only_into_new_rounds() {
        let dir = TestDir::new("timer-alarm-update");
        write_timer_package_with_alarm_marker(&dir.source(), "1.0.0", 1);
        let clock = Arc::new(TestClock::default());
        let manager = timer_manager(&dir, clock.clone(), Arc::new(TestPublisher::successful()));
        install_timer(&manager, &dir.source());
        let old_alarm = manager
            .bundles
            .bundle("com.example.timer")
            .unwrap()
            .unwrap()
            .alarm
            .clone()
            .unwrap();

        write_timer_package_with_alarm_marker(&dir.source(), "1.1.0", 2);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let updated = manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                    PublicPermission::TimerControl,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        let (_key, claim) = start_due_timer(&manager, &clock, updated.runtime.generation);

        assert_ne!(
            claim.frozen_completion.alarm.identity.activation_id,
            old_alarm.identity.activation_id
        );
        assert_ne!(
            claim.frozen_completion.alarm.identity.resource_sha256,
            old_alarm.identity.resource_sha256
        );
        assert_eq!(claim.frozen_completion.alarm.bytes[44], 2);
        assert_eq!(old_alarm.bytes[44], 1);
    }

    #[test]
    fn reinstalled_generation_one_rejects_the_previous_activation_ticket() {
        let dir = TestDir::new("timer-reinstall-identity");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let manager = timer_manager(&dir, clock.clone(), Arc::new(TestPublisher::successful()));
        let first_generation = install_timer(&manager, &dir.source());
        let (old_key, old_claim) = start_due_timer(&manager, &clock, first_generation);

        manager.uninstall("com.example.timer", false).unwrap();
        write_timer_package(&dir.source(), "1.0.0");
        let later = Instant::now();
        let prepared = manager
            .prepare("main", source(&dir.source()), later)
            .unwrap();
        let reinstalled = manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                    PublicPermission::TimerControl,
                ]),
                later,
                |_| true,
            )
            .unwrap();
        let new_key = TimerKey::new(
            "com.example.timer",
            reinstalled.runtime.generation,
            reinstalled.runtime.activation_id,
        )
        .unwrap();

        assert_eq!(reinstalled.runtime.generation, old_key.plugin_generation);
        assert_ne!(new_key.activation_id, old_key.activation_id);
        assert!(!manager.timers.admit_claim(&old_claim).unwrap());
        assert!(manager
            .timers
            .complete_claim(&old_claim, true)
            .unwrap()
            .is_none());
        assert_eq!(
            manager.timers.get_state(&new_key).unwrap().phase,
            PluginTimerPhase::Idle
        );
    }

    #[test]
    fn window_timer_calls_revalidate_the_current_activation_identity() {
        let dir = TestDir::new("timer-window-authorization");
        write_timer_package(&dir.source(), "1.0.0");
        let manager = timer_manager(
            &dir,
            Arc::new(TestClock::default()),
            Arc::new(TestPublisher::successful()),
        );
        let generation = install_timer(&manager, &dir.source());
        let activation_id = manager.activation_id("com.example.timer").unwrap();

        assert_eq!(
            manager
                .window_timer_get_state("com.example.timer", generation, activation_id)
                .unwrap()
                .phase,
            PluginTimerPhase::Idle
        );
        assert_eq!(
            manager
                .window_timer_start(
                    "com.example.timer",
                    generation,
                    activation_id,
                    Some(PluginTimerStartInput {
                        duration_ms: 1_000,
                        completion_message: "done".into(),
                    }),
                )
                .unwrap()
                .phase,
            PluginTimerPhase::Running
        );
        assert_eq!(
            manager.window_timer_get_state("com.example.timer", generation + 1, activation_id,),
            Err(TimerError::ExpiredWindowSessionError)
        );
        assert_eq!(
            manager.window_timer_get_state("com.example.timer", generation, activation_id + 1,),
            Err(TimerError::ExpiredWindowSessionError)
        );
        manager
            .set_enabled_with_readiness("com.example.timer", false, |_| false)
            .unwrap();
        assert_eq!(
            manager.window_timer_get_state("com.example.timer", generation, activation_id),
            Err(TimerError::ExpiredWindowSessionError)
        );
    }

    #[test]
    fn lifecycle_before_timer_admission_skips_message_and_audio() {
        let dir = TestDir::new("timer-lifecycle-first");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let publisher = Arc::new(TestPublisher::successful());
        let manager = timer_manager(&dir, clock.clone(), publisher.clone());
        let generation = install_timer(&manager, &dir.source());
        let (_key, ticket) = start_due_timer(&manager, &clock, generation);
        manager
            .set_enabled_with_readiness("com.example.timer", false, |_| false)
            .unwrap();

        assert_eq!(
            manager.commit_timer_claim(ticket),
            TimerClaimDispatch {
                effect: None,
                audio: None,
            }
        );
        assert!(publisher.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn timer_message_failure_returns_idle_without_audio() {
        let dir = TestDir::new("timer-message-failure");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let publisher = Arc::new(TestPublisher::failing());
        let manager = timer_manager(&dir, clock.clone(), publisher.clone());
        let generation = install_timer(&manager, &dir.source());
        let (key, ticket) = start_due_timer(&manager, &clock, generation);

        assert_eq!(
            manager.commit_timer_claim(ticket),
            TimerClaimDispatch {
                effect: None,
                audio: None,
            }
        );
        assert_eq!(publisher.calls.lock().unwrap().len(), 1);
        let state = manager.timers.get_state(&key).unwrap();
        assert_eq!(state.phase, PluginTimerPhase::Idle);
        assert_eq!(state.remaining_ms, Some(1_000));
    }

    #[test]
    fn failed_timer_upgrade_preserves_the_current_generation_timer() {
        let dir = TestDir::new("timer-failed-upgrade");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let manager = timer_manager(&dir, clock, Arc::new(TestPublisher::successful()));
        let generation = install_timer(&manager, &dir.source());
        let bundle = manager
            .bundles
            .bundle("com.example.timer")
            .unwrap()
            .unwrap();
        let key = TimerKey::new("com.example.timer", generation, bundle.activation_id).unwrap();
        manager
            .timers
            .start(
                &key,
                "Timer",
                Some(PluginTimerStartInput {
                    duration_ms: 10_000,
                    completion_message: "finished".into(),
                }),
                true,
                bundle.alarm.as_ref().unwrap(),
            )
            .result
            .unwrap();

        write_timer_package(&dir.source(), "1.1.0");
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            manager.commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                    PublicPermission::TimerControl,
                ]),
                now,
                |_| false,
            ),
            Err(PublicPluginManagementError::RuntimeNotReady)
        );
        assert_eq!(
            manager.timers.get_state(&key).unwrap().phase,
            PluginTimerPhase::Running
        );
    }

    #[test]
    fn committed_generation_lifecycle_changes_cancel_timers() {
        let dir = TestDir::new("timer-generation-lifecycle");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let manager = timer_manager(&dir, clock, Arc::new(TestPublisher::successful()));
        let start = |manager: &PublicPluginManager, generation| {
            let bundle = manager
                .bundles
                .bundle("com.example.timer")
                .unwrap()
                .unwrap();
            let key = TimerKey::new("com.example.timer", generation, bundle.activation_id).unwrap();
            manager
                .timers
                .start(
                    &key,
                    "Timer",
                    Some(PluginTimerStartInput {
                        duration_ms: 10_000,
                        completion_message: "finished".into(),
                    }),
                    true,
                    bundle.alarm.as_ref().unwrap(),
                )
                .result
                .unwrap();
            key
        };

        let first_generation = install_timer(&manager, &dir.source());
        let first_key = start(&manager, first_generation);
        write_timer_package(&dir.source(), "1.1.0");
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let updated = manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                    PublicPermission::TimerControl,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        assert_eq!(
            manager.timers.get_state(&first_key).unwrap().timer_revision,
            "0"
        );

        let updated_key = start(&manager, updated.runtime.generation);
        manager
            .mark_runtime_unavailable(
                "com.example.timer",
                updated.runtime.generation,
                updated.runtime.activation_id,
            )
            .unwrap();
        assert_eq!(
            manager
                .timers
                .get_state(&updated_key)
                .unwrap()
                .timer_revision,
            "0"
        );

        let enabled = manager
            .set_enabled_with_readiness("com.example.timer", true, |_| true)
            .unwrap()
            .runtime
            .unwrap();
        let enabled_key = start(&manager, enabled.generation);
        let activation_id = manager.activation_id("com.example.timer").unwrap();
        manager
            .rename("com.example.timer", Some("timer-renamed"))
            .unwrap();
        assert_eq!(
            manager.activation_id("com.example.timer"),
            Some(activation_id)
        );
        assert_eq!(
            manager.timers.get_state(&enabled_key).unwrap().phase,
            PluginTimerPhase::Running
        );
        manager.uninstall("com.example.timer", true).unwrap();
        assert_eq!(
            manager
                .timers
                .get_state(&enabled_key)
                .unwrap()
                .timer_revision,
            "0"
        );
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
                activation_id: manager.activation_id("com.example.activation").unwrap(),
                admission_epoch: manager.admission_epoch("com.example.activation").unwrap(),
                runtime_recovery_needed: false,
                runtime_label: runtime_label("com.example.activation", 1).unwrap(),
                activation_mode: PublicActivationMode::Live,
                output_mode: PublicOutputMode::MainResult,
                input: "I am  Jack".into(),
                input_required: false,
                input_placeholder: None,
                window_entry: None,
                icon_url: None,
            }
        );
        assert!(manager.route("/activationX nope").unwrap().is_none());
        manager
            .set_enabled_with_readiness("com.example.activation", false, |_| false)
            .unwrap();
        assert!(manager.route("/activation").unwrap().is_none());
    }

    #[test]
    fn command_suggestions_filter_match_sort_and_follow_effective_state() {
        let dir = TestDir::new("command-suggestions");
        let manager = manager(&dir);
        let now = Instant::now();
        let demo_win = dir.path().join("source-demo-win");
        let demo_return = dir.path().join("source-demo-return");
        write_discovery_package(
            &demo_win,
            "com.example.demo-win",
            "Public Plugin Demo Window",
            "demo-win",
            "打开演示子窗口",
        );
        write_discovery_package(
            &demo_return,
            "com.example.demo-return",
            "Public Plugin Demo Return",
            "demo-return",
            "返回示例文本到主界面",
        );
        install_discovery_package(&manager, &demo_win, now);
        install_discovery_package(&manager, &demo_return, now);

        assert_eq!(
            manager.command_suggestions("d").unwrap(),
            vec![
                PublicCommandSuggestion {
                    plugin_id: "com.example.demo-return".into(),
                    effective_name: "demo-return".into(),
                    display_name: "Public Plugin Demo Return".into(),
                    summary: Some("返回示例文本到主界面".into()),
                    icon_url: None,
                    favorite: false,
                },
                PublicCommandSuggestion {
                    plugin_id: "com.example.demo-win".into(),
                    effective_name: "demo-win".into(),
                    display_name: "Public Plugin Demo Window".into(),
                    summary: Some("打开演示子窗口".into()),
                    icon_url: None,
                    favorite: false,
                },
            ]
        );
        assert_eq!(
            manager.command_suggestions("window").unwrap()[0].effective_name,
            "demo-win"
        );
        assert_eq!(
            manager.launcher_command_suggestions("").unwrap(),
            manager.command_suggestions("").unwrap()
        );
        assert_eq!(
            manager.launcher_command_suggestions("WIN").unwrap(),
            vec![PublicCommandSuggestion {
                plugin_id: "com.example.demo-win".into(),
                effective_name: "demo-win".into(),
                display_name: "Public Plugin Demo Window".into(),
                summary: Some("打开演示子窗口".into()),
                icon_url: None,
                favorite: false,
            }]
        );
        assert!(manager
            .launcher_command_suggestions("window")
            .unwrap()
            .is_empty());

        manager
            .rename("com.example.demo-win", Some("alpha-win"))
            .unwrap();
        assert_eq!(
            manager.command_suggestions("a").unwrap()[0].effective_name,
            "alpha-win"
        );
        assert_eq!(
            manager.launcher_command_suggestions("ALPHA").unwrap()[0].effective_name,
            "alpha-win"
        );

        let current = manager
            .data
            .lock()
            .unwrap()
            .active_by_plugin
            .get("com.example.demo-win")
            .unwrap()
            .clone();
        let mut stale = (*current).clone();
        stale.generation += 1;
        manager
            .data
            .lock()
            .unwrap()
            .active_by_plugin
            .insert("com.example.demo-win".into(), Arc::new(stale));
        assert_eq!(
            manager.command_suggestions("a").unwrap()[0].effective_name,
            "alpha-win"
        );
        manager
            .data
            .lock()
            .unwrap()
            .active_by_plugin
            .insert("com.example.demo-win".into(), current);

        manager
            .set_enabled_with_readiness("com.example.demo-return", false, |_| false)
            .unwrap();
        assert!(manager
            .command_suggestions("demo-return")
            .unwrap()
            .is_empty());
        manager
            .disable_plugin_for_fault(
                "com.example.demo-win",
                PublicPluginFault::ConsecutiveFailures,
            )
            .unwrap();
        assert!(manager.command_suggestions("a").unwrap().is_empty());
        assert!(manager
            .launcher_command_suggestions("alpha")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn favorite_mutation_publishes_config_without_reloading_runtime_and_is_idempotent() {
        let dir = TestDir::new("favorite-mutation");
        let manager = manager(&dir);
        let source = dir.path().join("source-favorite");
        write_discovery_package(
            &source,
            "com.example.favorite",
            "Favorite Plugin",
            "favorite",
            "Favorite summary",
        );
        install_discovery_package(&manager, &source, Instant::now());

        let before = manager
            .bundles
            .bundle("com.example.favorite")
            .unwrap()
            .unwrap();
        assert!(!before.config.favorite);
        let before_revision = before.config.inventory_revision;
        let before_activation = before.activation_id;
        let before_admission = before.admission_epoch;

        let mutation = manager.set_favorite("com.example.favorite", true).unwrap();
        let favorited = manager
            .bundles
            .bundle("com.example.favorite")
            .unwrap()
            .unwrap();
        assert!(favorited.config.favorite);
        assert!(mutation.inventory_revision > before_revision);
        assert_eq!(
            favorited.config.inventory_revision,
            mutation.inventory_revision
        );
        assert_eq!(favorited.activation_id, before_activation);
        assert_eq!(favorited.admission_epoch, before_admission);
        assert!(Arc::ptr_eq(&favorited.runtime, &before.runtime));

        let same = manager.set_favorite("com.example.favorite", true).unwrap();
        let after_noop = manager
            .bundles
            .bundle("com.example.favorite")
            .unwrap()
            .unwrap();
        assert_eq!(same.inventory_revision, mutation.inventory_revision);
        assert!(Arc::ptr_eq(&after_noop, &favorited));
    }

    #[test]
    fn favorite_launcher_catalog_groups_deduplicates_and_excludes_recovery_pending() {
        let dir = TestDir::new("favorite-catalog");
        let manager = manager(&dir);
        let now = Instant::now();
        let alpha = dir.path().join("source-alpha");
        let demo = dir.path().join("source-demo");
        write_discovery_package(
            &alpha,
            "com.example.alpha",
            "Alpha Plugin",
            "alpha",
            "Alpha summary",
        );
        write_discovery_package(
            &demo,
            "com.example.demo",
            "Demo Plugin",
            "demo",
            "Demo summary",
        );
        install_discovery_package(&manager, &alpha, now);
        install_discovery_package(&manager, &demo, now);
        manager.set_favorite("com.example.demo", true).unwrap();

        let unrelated = manager.launcher_command_suggestions("zzz").unwrap();
        assert_eq!(unrelated.len(), 1);
        assert_eq!(unrelated[0].plugin_id, "com.example.demo");
        assert!(unrelated[0].favorite);

        let matching = manager.launcher_command_suggestions("a").unwrap();
        assert_eq!(
            matching
                .iter()
                .map(|suggestion| suggestion.effective_name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo", "alpha"]
        );
        assert_eq!(matching.iter().filter(|item| item.favorite).count(), 1);

        let bundle = manager.bundles.bundle("com.example.demo").unwrap().unwrap();
        let reservation = manager.bundles.reserve("com.example.demo").unwrap();
        reservation.begin_committing().unwrap();
        reservation.mark_durable().unwrap();
        reservation
            .publish(Some(bundle.with_runtime_recovery(true)))
            .unwrap();
        assert!(manager
            .launcher_command_suggestions("zzz")
            .unwrap()
            .is_empty());
        assert!(manager.command_suggestions("demo").unwrap().is_empty());
    }

    #[test]
    fn public_plugin_icon_urls_are_generation_and_caller_bound() {
        let dir = TestDir::new("icon-ownership");
        let manager = manager(&dir);
        let now = Instant::now();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let source = workspace.join("examples/public-plugins/com.uipilot.demo-win/package");
        let expected = fs::read(source.join(super::super::icon::ICON_PATH)).unwrap();

        let prepared = manager.prepare("main", self::source(&source), now).unwrap();
        let prepared_url = prepared.icon_url.clone().unwrap();
        let prepared_path = tauri::Url::parse(&prepared_url).unwrap().path().to_owned();
        let prepared_asset = manager.icon_asset("main", &prepared_path, now).unwrap();
        assert_eq!(prepared_asset.bytes, expected);
        assert_eq!(prepared_asset.cache_control, "no-store");
        assert!(manager.icon_asset("find", &prepared_path, now).is_none());
        assert!(manager
            .icon_asset("plugin-runtime-forged", &prepared_path, now)
            .is_none());

        let commit = manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        assert!(manager.icon_asset("main", &prepared_path, now).is_none());

        let inventory = manager.inventory().unwrap();
        let installed_url = inventory.items[0].icon_url.clone().unwrap();
        let installed_path = tauri::Url::parse(&installed_url).unwrap().path().to_owned();
        let installed_asset = manager.icon_asset("main", &installed_path, now).unwrap();
        assert_eq!(installed_asset.bytes, expected);
        assert_eq!(
            installed_asset.cache_control,
            "public, max-age=31536000, immutable"
        );

        let shell = crate::plugin_window::plugin_shell_label(&commit.mutation.plugin_id).unwrap();
        assert!(manager.icon_asset(&shell, &installed_path, now).is_some());
        assert!(manager
            .icon_asset(&commit.runtime.label, &installed_path, now)
            .is_none());
        assert!(manager
            .icon_asset(
                &crate::plugin_window::plugin_content_label(&commit.mutation.plugin_id).unwrap(),
                &installed_path,
                now,
            )
            .is_none());
        assert!(manager.icon_asset("find", &installed_path, now).is_none());

        let stale_url = super::super::icon::installed_url(
            &commit.mutation.plugin_id,
            commit.mutation.generation + 1,
        );
        let stale_path = tauri::Url::parse(&stale_url).unwrap().path().to_owned();
        assert!(manager.icon_asset("main", &stale_path, now).is_none());

        let identity = manager.window_identity(&commit.mutation.plugin_id).unwrap();
        assert_eq!(identity.name, "Public Plugin Demo Window");
        assert_eq!(identity.icon_url.as_deref(), Some(installed_url.as_str()));
        assert_eq!(
            manager.message_icon_url(&commit.mutation.plugin_id),
            Some(installed_url.clone())
        );
        assert_eq!(
            manager.command_suggestions("demo-win").unwrap()[0].icon_url,
            Some(installed_url)
        );
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
    fn runtime_host_document_denies_media() {
        assert!(runtime_host_document("dist/runtime.js").contains("media-src 'none'"));
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
            .record_runtime_result(
                "com.example.activation",
                replacement.generation,
                replacement.activation_id,
                false,
                now,
            )
            .unwrap());
        assert!(!manager
            .record_runtime_result(
                "com.example.activation",
                replacement.generation,
                replacement.activation_id,
                false,
                now + Duration::from_secs(10),
            )
            .unwrap());
        assert!(manager
            .record_runtime_result(
                "com.example.activation",
                replacement.generation,
                replacement.activation_id,
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
                reenabled_commit.runtime.as_ref().unwrap().generation,
                reenabled_commit.runtime.as_ref().unwrap().activation_id,
                false,
                now + Duration::from_secs(30),
            )
            .unwrap());
    }

    #[test]
    fn activation_identity_survives_config_only_changes_and_is_never_reused() {
        let dir = TestDir::new("activation-identity");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let first_id = manager.activation_id("com.example.activation").unwrap();
        let first_epoch = manager.admission_epoch("com.example.activation").unwrap();
        let first_data_identity =
            PluginDataCallIdentity::new("com.example.activation", 1, first_id, first_epoch)
                .unwrap();
        assert!(manager.data_gate.try_acquire(&first_data_identity).is_ok());

        manager
            .rename("com.example.activation", Some("renamed"))
            .unwrap();
        assert_eq!(
            manager.activation_id("com.example.activation"),
            Some(first_id)
        );
        assert_eq!(
            manager.admission_epoch("com.example.activation"),
            Some(first_epoch)
        );
        assert!(manager.data_gate.try_acquire(&first_data_identity).is_ok());
        manager
            .save_settings(
                "com.example.activation",
                &BTreeMap::from([("compact".into(), json!(true))]),
            )
            .unwrap();
        assert_eq!(
            manager.activation_id("com.example.activation"),
            Some(first_id)
        );
        assert_eq!(
            manager.admission_epoch("com.example.activation"),
            Some(first_epoch)
        );

        let reservation = manager.bundles.reserve("com.example.activation").unwrap();
        assert_eq!(manager.route("/renamed").unwrap().unwrap().generation, 1);
        assert_eq!(
            manager.rename("com.example.activation", Some("blocked")),
            Err(PublicPluginManagementError::Unavailable)
        );
        drop(reservation);

        manager.uninstall("com.example.activation", false).unwrap();
        assert_eq!(manager.activation_id("com.example.activation"), None);
        assert_eq!(manager.admission_epoch("com.example.activation"), None);
        assert_eq!(
            manager
                .data_gate
                .try_acquire(&first_data_identity)
                .unwrap_err(),
            crate::public_plugins::data_call_gate::DataCallGateError::Expired
        );
        let later = now + Duration::from_secs(1);
        let prepared = manager
            .prepare("main", source(&dir.source()), later)
            .unwrap();
        let commit = manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), later, |_| true)
            .unwrap();
        assert_eq!(commit.runtime.generation, 1);
        let second_id = manager.activation_id("com.example.activation").unwrap();
        let second_epoch = manager.admission_epoch("com.example.activation").unwrap();
        assert_ne!(second_id, first_id);
        assert_ne!(second_epoch, first_epoch);
        let second_data_identity =
            PluginDataCallIdentity::new("com.example.activation", 1, second_id, second_epoch)
                .unwrap();
        assert!(manager.data_gate.try_acquire(&second_data_identity).is_ok());
        for offset in 0..3 {
            assert!(!manager
                .record_runtime_result(
                    "com.example.activation",
                    1,
                    first_id,
                    false,
                    later + Duration::from_secs(offset),
                )
                .unwrap());
        }
        assert!(
            manager
                .state()
                .config("com.example.activation")
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn pending_owner_cleanup_blocks_same_plugin_before_staging() {
        let dir = TestDir::new("cleanup-block");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        manager
            .owner_cleanup
            .commit(
                PluginOwnerCleanupReceipt::new(
                    "com.example.activation",
                    "00000000000000000000000000000001",
                    1,
                    Some(1),
                )
                .unwrap(),
            )
            .unwrap();

        assert_eq!(
            manager.prepare("main", source(&dir.source()), Instant::now()),
            Err(PublicPluginManagementError::DataCleanupPending)
        );
        assert!(staging_is_empty(&manager));
    }

    #[test]
    fn pending_cleanup_crash_snapshot_never_constructs_a_bundle() {
        let dir = TestDir::new("cleanup-crash-snapshot");
        write_package(&dir.source(), "1.0.0", "one");
        let installed_manager = manager(&dir);
        let now = Instant::now();
        let prepared = installed_manager
            .prepare("main", source(&dir.source()), now)
            .unwrap();
        installed_manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let activation_id = installed_manager
            .activation_id("com.example.activation")
            .unwrap();
        drop(installed_manager);

        let store =
            PluginOwnerCleanupStore::load(&dir.path().join("public-plugin-owner-cleanup")).unwrap();
        store
            .commit(
                PluginOwnerCleanupReceipt::new(
                    "com.example.activation",
                    "00000000000000000000000000000001",
                    1,
                    Some(activation_id),
                )
                .unwrap(),
            )
            .unwrap();

        let recovered = manager(&dir);
        assert!(
            recovered
                .state()
                .config("com.example.activation")
                .unwrap()
                .unwrap()
                .installed
        );
        assert_eq!(recovered.activation_id("com.example.activation"), None);
        assert!(recovered.runtime_candidates().unwrap().is_empty());
        assert!(recovered.route("/activation value").unwrap().is_none());
    }

    #[test]
    fn complete_uninstall_commits_receipt_before_owner_cleanup() {
        let dir = TestDir::new("uninstall-transaction");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let settings = SettingsStore::load(dir.path()).unwrap();
        settings
            .set_plugin_window_position("com.example.activation", WindowPosition { x: 12, y: 34 })
            .unwrap();
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let package_owner = manager.packages_root.join("com.example.activation");
        assert!(package_owner.exists());

        let mut transaction = manager
            .begin_uninstall("com.example.activation", false)
            .unwrap()
            .unwrap();
        assert!(!manager
            .owner_cleanup
            .is_blocked("com.example.activation")
            .unwrap());
        manager.drain_uninstall_data(&mut transaction).unwrap();
        let committed = manager.commit_uninstall(transaction).unwrap();

        assert_eq!(manager.activation_id("com.example.activation"), None);
        assert!(manager
            .owner_cleanup
            .is_blocked("com.example.activation")
            .unwrap());
        assert!(package_owner.exists());
        assert_eq!(
            settings.plugin_window_position("com.example.activation"),
            Some(WindowPosition { x: 12, y: 34 })
        );

        manager
            .finish_uninstall_cleanup(&committed, &settings)
            .unwrap();
        assert!(!package_owner.exists());
        assert_eq!(
            settings.plugin_window_position("com.example.activation"),
            None
        );
        assert!(!manager
            .owner_cleanup
            .is_blocked("com.example.activation")
            .unwrap());
    }

    #[test]
    fn begin_uninstall_waits_for_runtime_current_guard_and_data_lease() {
        let dir = TestDir::new("uninstall-runtime-data-lease");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = Arc::new(manager(&dir));
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let route = manager.route("/activation value").unwrap().unwrap();
        let request = match manager
            .scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: route.plugin_id.clone(),
                    plugin_generation: route.generation,
                    activation_id: route.activation_id,
                    admission_epoch: route.admission_epoch,
                    activation_mode: route.activation_mode,
                    input: route.input,
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 1,
                        control_value: "/activation value".into(),
                        submission_token: "runtime-data-lease".into(),
                    },
                },
                now,
            )
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("first request must dispatch"),
        };
        let (entered_sender, entered_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let runtime_manager = Arc::clone(&manager);
        let context = request.context.clone();
        let runtime = thread::spawn(move || {
            runtime_manager
                .scheduler
                .with_current(&context, |current| {
                    let identity = PluginDataCallIdentity::new(
                        &context.plugin_id,
                        current.plugin_generation(),
                        current.activation_id(),
                        current.admission_epoch(),
                    )
                    .unwrap();
                    let lease = runtime_manager.data_gate.try_acquire(&identity).unwrap();
                    entered_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    drop(lease);
                })
                .unwrap();
        });
        entered_receiver.recv().unwrap();
        let uninstall_manager = Arc::clone(&manager);
        let (uninstall_sender, uninstall_receiver) = mpsc::channel();
        let uninstall = thread::spawn(move || {
            let result = uninstall_manager.begin_uninstall("com.example.activation", false);
            uninstall_sender.send(result).unwrap();
        });

        assert!(uninstall_receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release_sender.send(()).unwrap();
        runtime.join().unwrap();
        let mut transaction = uninstall_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()
            .unwrap();
        uninstall.join().unwrap();
        manager.drain_uninstall_data(&mut transaction).unwrap();
        manager.abort_uninstall_before_commit(transaction).unwrap();
    }

    #[test]
    fn abort_before_uninstall_commit_requires_runtime_recovery_with_new_identity() {
        let dir = TestDir::new("uninstall-abort-recovery");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let old_activation = manager.activation_id("com.example.activation").unwrap();
        let old_epoch = manager.admission_epoch("com.example.activation").unwrap();

        let transaction = manager
            .begin_uninstall("com.example.activation", false)
            .unwrap()
            .unwrap();
        let recovery = manager
            .abort_uninstall_before_commit(transaction)
            .unwrap()
            .unwrap();
        let route = manager.route("/activation next").unwrap().unwrap();

        assert_eq!(route.generation, 1);
        assert_ne!(route.activation_id, old_activation);
        assert_ne!(route.admission_epoch, old_epoch);
        assert!(route.runtime_recovery_needed);
        assert_eq!(recovery.activation_id, route.activation_id);
        assert_eq!(recovery.admission_epoch, route.admission_epoch);
        assert!(recovery.runtime_recovery_needed);
        assert!(manager.runtime_candidates().unwrap().is_empty());
        assert!(!manager
            .record_runtime_result("com.example.activation", 1, old_activation, false, now,)
            .unwrap());
    }

    #[test]
    fn window_storage_shares_runtime_namespace_and_revalidates_activation_identity() {
        let dir = TestDir::new("window-storage");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let installed = manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let plugin_id = "com.example.activation";
        let generation = installed.runtime.generation;
        let activation_id = manager.activation_id(plugin_id).unwrap();
        let admission_epoch = manager.admission_epoch(plugin_id).unwrap();
        let scheduled = match manager
            .scheduler
            .enqueue(
                crate::public_plugins::PluginRequestCandidate {
                    plugin_id: plugin_id.into(),
                    plugin_generation: generation,
                    activation_id,
                    admission_epoch,
                    activation_mode: PublicActivationMode::Live,
                    input: "storage".into(),
                    owner: crate::public_plugins::PluginSubmissionOwner {
                        ui_intent_epoch: 1,
                        control_value: "/activation storage".into(),
                        submission_token: "window-storage".into(),
                    },
                },
                now,
            )
            .unwrap()
        {
            crate::public_plugins::PluginScheduleOutcome::Dispatched(request) => request,
            crate::public_plugins::PluginScheduleOutcome::Waiting { .. } => {
                panic!("first request must dispatch")
            }
        };
        let runtime_label = runtime_label(plugin_id, generation).unwrap();
        let runtime_call = |operation, value| PluginApiRequest {
            context: scheduled.context.clone(),
            operation,
            key: Some("shared.value".into()),
            value,
            notification: None,
        };

        assert_eq!(
            manager
                .execute_api(
                    &runtime_label,
                    runtime_call(
                        super::super::runtime::PluginApiOperation::StorageSet,
                        Some(json!(10)),
                    ),
                )
                .result,
            Ok(Value::Null)
        );
        assert_eq!(
            manager.window_storage_get(
                plugin_id,
                generation,
                activation_id,
                admission_epoch,
                "shared.value",
            ),
            Ok(Some(json!(10)))
        );

        manager
            .window_storage_set(
                plugin_id,
                generation,
                activation_id,
                admission_epoch,
                "shared.value",
                json!(25),
            )
            .unwrap();
        assert_eq!(
            manager
                .execute_api(
                    &runtime_label,
                    runtime_call(super::super::runtime::PluginApiOperation::StorageGet, None),
                )
                .result,
            Ok(json!(25))
        );
        let other_scope = PluginDataScope::new("com.example.other").unwrap();
        assert_eq!(
            manager
                .storage
                .get(&other_scope, "com.example.other", "shared.value"),
            Ok(None)
        );

        for (stale_generation, stale_activation, stale_epoch) in [
            (generation + 1, activation_id, admission_epoch),
            (generation, activation_id + 1, admission_epoch),
            (generation, activation_id, admission_epoch + 1),
        ] {
            assert_eq!(
                manager.window_storage_get(
                    plugin_id,
                    stale_generation,
                    stale_activation,
                    stale_epoch,
                    "shared.value",
                ),
                Err(WindowStorageError::ExpiredWindowSessionError)
            );
        }
        assert_eq!(
            manager.window_storage_set(
                plugin_id,
                generation,
                activation_id,
                admission_epoch,
                "Invalid_Key",
                json!(30),
            ),
            Err(WindowStorageError::InvalidOperation)
        );
    }
}
