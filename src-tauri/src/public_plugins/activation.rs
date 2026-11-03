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

#[cfg(test)]
use crate::clipboard_history::ClipboardCapture;
use crate::clipboard_history::{
    ClipboardHistoryBridgeError, ClipboardHistoryError, ClipboardHistoryPasteError,
    ClipboardHistoryRecord, ClipboardHistoryService, ClipboardHistorySnapshot,
};
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
    manifest::{
        valid_plugin_id, PanelHostKeyDeclaration, PanelHostKeyFocus, PublicActivationMode,
        PublicOutputMode, PublicPermission, PublicSettingV1,
    },
    network::{
        PluginNetworkAuthorityGate, PluginNetworkAuthoritySnapshot, PluginNetworkErrorCode,
        PluginNetworkResponse,
    },
    owner_cleanup::{
        remove_owned_directory, OwnerCleanupError, PluginOwnerCleanupReceipt,
        PluginOwnerCleanupStore,
    },
    package,
    runtime::{PluginNetworkCommandInput, PluginRuntimeIdentity},
    runtime_label,
    scheduler::{PluginRuntimeReplacement, PluginScheduleError},
    stage_public_package,
    state::{DurableStateOutcome, PreparedStateCommit},
    timers::{
        ClaimTicket, Clock, PluginTimerService, PluginTimerStartInput, PluginTimerState,
        SystemClock, TimerAudioCompletion, TimerError, TimerKey, TimerPostLockEffect,
    },
    EffectivePluginConfig, PluginApiExecution, PluginApiRequest, PluginCommandCompletion,
    PluginCompletionOutcome, PluginDataScope, PluginRequestContext, PluginRequestScheduler,
    PluginRuntimeApi, PluginRuntimeError, PluginScheduleOutcome, PluginSecretStore,
    PluginStateError, PluginStateStore, PluginStorageStore, PreparedPublicPlugin, PublicManifestV1,
    PublicPackageError, PublicPackageSource, PublicPluginFault, PublicPluginHost, PublicResource,
    ReservedPluginRequest, ScheduledPluginRequest, ValidatedAlarmAsset,
};

use super::diagnostics::{
    self, CommitOutcome, DiagnosticReport, DiagnosticState, Observations, Operation, Phase,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicPluginRefreshTarget {
    pub(crate) plugin_id: String,
    pub(crate) expected_inventory_revision: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PrepareOperation {
    Install,
    Replace,
    DevelopmentRefresh,
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
    pub(crate) operation: PrepareOperation,
    pub(crate) previous_version: Option<String>,
    pub(crate) source_verified: bool,
    pub(crate) icon_url: Option<String>,
    pub(crate) network: Option<PublicPluginPrepareNetwork>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginPrepareNetwork {
    pub(crate) https_hosts: Vec<String>,
    pub(crate) added_https_hosts: Vec<String>,
    pub(crate) credentials: Vec<super::manifest::PluginNetworkCredential>,
    pub(crate) added_credential_ids: Vec<String>,
    pub(crate) requires_network_consent: bool,
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
    pub(crate) hotkey_revision: String,
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
    pub(crate) installation_directory: Option<String>,
    pub(crate) commands: Vec<PublicPluginInventoryCommand>,
    pub(crate) enabled: bool,
    pub(crate) fault: Option<PublicPluginFault>,
    pub(crate) generation: u64,
    pub(crate) permissions: Vec<PublicPermissionView>,
    pub(crate) settings: Vec<PublicSettingView>,
    pub(crate) icon_url: Option<String>,
    pub(crate) network: Option<PublicPluginInventoryNetwork>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginInventoryCommand {
    pub(crate) command_id: String,
    pub(crate) default_name: String,
    pub(crate) effective_name: String,
    pub(crate) name_override: Option<String>,
    pub(crate) favorite: bool,
    pub(crate) hotkey: Option<String>,
    pub(crate) hotkey_status: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicPluginInventoryNetwork {
    pub(crate) https_hosts: Vec<String>,
    pub(crate) credentials: Vec<super::manifest::PluginNetworkCredential>,
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
    pub(crate) result_publication: Option<super::result_action::ResultActionLease>,
    pub(crate) command_id: String,
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) runtime_recovery_needed: bool,
    pub(crate) runtime_label: String,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) output_mode: PublicOutputMode,
    pub(crate) command_label: String,
    pub(crate) input: String,
    pub(crate) input_required: bool,
    pub(crate) input_placeholder: Option<String>,
    pub(crate) window_entry: Option<String>,
    pub(crate) panel_entry: Option<String>,
    pub(crate) host_keys: Vec<PanelHostKeyDeclaration>,
    pub(crate) host_key_focus: PanelHostKeyFocus,
    pub(crate) icon_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicCommandSuggestion {
    pub(crate) command_id: String,
    pub(crate) plugin_id: String,
    pub(crate) effective_name: String,
    pub(crate) display_name: String,
    pub(crate) summary: Option<String>,
    pub(crate) icon_url: Option<String>,
    pub(crate) favorite: bool,
    pub(crate) input_required: bool,
    pub(crate) output_mode: PublicOutputMode,
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
pub(crate) struct PublicPanelResponse {
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
    pub(crate) network_https_declared: bool,
    pub(crate) panel_actions: bool,
    pub(crate) window_actions: bool,
    pub(crate) result_actions: bool,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublicPluginManagementError {
    RefreshTargetMismatch,
    RefreshVersionMismatch,
    StaleInventory,
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
    #[serde(rename = "pluginUnavailable")]
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
            Self::RefreshTargetMismatch => "refreshTargetMismatch",
            Self::RefreshVersionMismatch => "refreshVersionMismatch",
            Self::StaleInventory => "staleInventory",
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

impl From<ClipboardHistoryError> for PublicPluginManagementError {
    fn from(_error: ClipboardHistoryError) -> Self {
        Self::Unavailable
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
            result_actions: self.manifest.runtime.result_actions,
            window_actions: self
                .manifest
                .window
                .as_ref()
                .is_some_and(|w| w.runtime_actions),
            panel_actions: self
                .manifest
                .panel
                .as_ref()
                .is_some_and(|p| p.runtime_actions),
            network_https_declared: self
                .manifest
                .permissions
                .contains(&PublicPermission::NetworkHttps),
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
    operation: Operation,
    refresh_target: Option<PublicPluginRefreshTarget>,
    caller_label: String,
    expires_at: Instant,
    prepared: PreparedPublicPlugin,
    icon: Option<Vec<u8>>,
    previous_config: Option<EffectivePluginConfig>,
    previous_declared_https_hosts: BTreeSet<String>,
    previous_declared_credentials: Vec<super::manifest::PluginNetworkCredential>,
    requires_network_consent: bool,
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

#[derive(Default)]
pub(crate) struct ManagementEffects {
    pub(crate) retired: Vec<(PluginRequestContext, Option<String>)>,
    pub(crate) changed: bool,
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
    network_authority: Arc<PluginNetworkAuthorityGate>,
    data_gate: Arc<PluginDataCallGate>,
    delayed_messages: Arc<DelayedMessageScheduler>,
    message_center: Arc<MessageCenterService>,
    timer_publisher: Arc<dyn MessagePublisher>,
    timers: Arc<PluginTimerService>,
    clipboard_history: Arc<ClipboardHistoryService>,
    api: PluginRuntimeApi,
    activation_ids: ActivationIdAllocator,
    admission_epochs: AdmissionEpochAllocator,
    owner_cleanup: Arc<PluginOwnerCleanupStore>,
    bundles: Arc<ActivationReservationBook>,
    mutation: Mutex<()>,
    // Permission edits share the state's global inventory revision. Serialize
    // their durable phases even when the edited plugins differ.
    permission_mutation: Mutex<()>,
    hotkey_settings: Mutex<Option<crate::settings::SettingsStore>>,
    hotkey_changed: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    after_uncertain_close: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    after_state_durable: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    before_result_route_token: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    data: Mutex<ActivationData>,
    runtime_faults: Mutex<HashMap<(String, u64), RuntimeFaultWindow>>,
    observations: Observations,
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
        let network_authority = Arc::new(
            PluginNetworkAuthorityGate::new()
                .map_err(|_| PublicPluginManagementError::Unavailable)?,
        );
        Self::load_with_timer_and_network_dependencies(
            app_data_dir,
            host,
            reserved_names,
            message_center,
            timer_clock,
            timer_publisher,
            network_authority,
        )
    }

    fn load_with_timer_and_network_dependencies(
        app_data_dir: &Path,
        host: PublicPluginHost,
        reserved_names: impl IntoIterator<Item = String>,
        message_center: Arc<MessageCenterService>,
        timer_clock: Arc<dyn Clock>,
        timer_publisher: Arc<dyn MessagePublisher>,
        network_authority: Arc<PluginNetworkAuthorityGate>,
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
        let clipboard_history = ClipboardHistoryService::load(&root.join("clipboard-history"))
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
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
        for mut config in state.configs()? {
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
                    if !network_authorization_matches(&config, &snapshot.manifest) {
                        let declared_hosts = snapshot.manifest.network.as_ref().map_or_else(
                            || config.network_https_hosts_grant.clone(),
                            |network| network.https_hosts.iter().cloned().collect(),
                        );
                        let prepared = state.prepare_set_network_access(
                            &config.plugin_id,
                            declared_hosts,
                            false,
                        )?;
                        if state.persist_prepared(&prepared) != DurableStateOutcome::Committed {
                            return Err(PublicPluginManagementError::Unavailable);
                        }
                        config = state.publish_prepared(prepared)?;
                    }
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
                        let mut transition = network_authority
                            .begin_transition()
                            .map_err(|_| PublicPluginManagementError::Unavailable)?;
                        let authority = checked_network_authority_snapshot(
                            &bundles,
                            &state,
                            &secrets,
                            &config,
                            &runtime,
                            activation_id,
                            admission_epoch,
                        )?;
                        transition
                            .set_plugin_authority(&config.plugin_id, authority)
                            .map_err(|_| PublicPluginManagementError::Unavailable)?;
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
        let manager = Self {
            host,
            #[cfg(test)]
            app_data_dir: app_data_dir.to_path_buf(),
            staging_root,
            packages_root,
            state,
            storage,
            secrets,
            scheduler,
            network_authority,
            data_gate,
            delayed_messages,
            message_center,
            timer_publisher,
            timers,
            clipboard_history,
            api,
            activation_ids,
            admission_epochs,
            owner_cleanup,
            bundles,
            mutation: Mutex::new(()),
            permission_mutation: Mutex::new(()),
            hotkey_settings: Mutex::new(None),
            hotkey_changed: Mutex::new(None),
            #[cfg(test)]
            after_uncertain_close: Mutex::new(None),
            #[cfg(test)]
            after_state_durable: Mutex::new(None),
            #[cfg(test)]
            before_result_route_token: Mutex::new(None),
            data: Mutex::new(ActivationData {
                active_by_plugin,
                ..ActivationData::default()
            }),
            runtime_faults: Mutex::new(HashMap::new()),
            observations: Observations::default(),
            next_token: AtomicU64::new(1),
            next_uninstall_transaction: AtomicU64::new(1),
        };
        manager.refresh_clipboard_history_authorization()?;
        Ok(manager)
    }

    pub(crate) fn attach_command_hotkeys(
        &self,
        settings: crate::settings::SettingsStore,
        changed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(), PublicPluginManagementError> {
        *self
            .hotkey_settings
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)? = Some(settings);
        *self
            .hotkey_changed
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)? = Some(changed);
        self.prune_command_hotkeys()
    }
    fn prune_command_hotkeys_locked(&self) -> Result<(), PublicPluginManagementError> {
        if let Some(settings) = self
            .hotkey_settings
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .as_ref()
        {
            let installed = self
                .state
                .configs()?
                .into_iter()
                .filter(|c| c.installed)
                .map(|c| (c.plugin_id, c.commands.into_keys().collect()))
                .collect();
            settings
                .prune_command_hotkeys(&installed)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
        }
        Ok(())
    }
    pub(crate) fn prune_command_hotkeys(&self) -> Result<(), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        self.prune_command_hotkeys_locked()
    }
    fn publish_command_hotkeys(&self) -> Result<(), PublicPluginManagementError> {
        let result = self.prune_command_hotkeys();
        let changed = self
            .hotkey_changed
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .clone();
        if let Some(changed) = changed {
            changed();
        }
        result
    }
    fn command_authority_locked(
        &self,
        plugin: &str,
        command: &str,
    ) -> Result<(bool, u64), PublicPluginManagementError> {
        let config = self
            .state
            .config(plugin)?
            .filter(|c| c.installed && c.commands.contains_key(command))
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if self
            .owner_cleanup
            .is_blocked(plugin)
            .map_err(|_| PublicPluginManagementError::Unavailable)?
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok((
            config.enabled && config.fault.is_none(),
            config.active_generation,
        ))
    }
    pub(crate) fn selection_translation_authority(&self) -> Option<u64> {
        if self
            .owner_cleanup
            .is_blocked("com.uipilot.translate")
            .ok()?
        {
            return None;
        }
        let bundle = self.bundles.bundle("com.uipilot.translate").ok()??;
        let c = &bundle.config;
        let m = &bundle.runtime.manifest;
        (c.installed
            && c.enabled
            && c.fault.is_none()
            && c.active_generation == bundle.runtime.generation
            && m.permissions.contains(&PublicPermission::SelectionRead)
            && c.permission_grants
                .contains(&PublicPermission::SelectionRead)
            && !bundle.runtime_recovery_needed
            && (m
                .legacy_command()
                .is_some_and(|c| c.output_mode == PublicOutputMode::MainResult)
                || (m.translation_panel_compatible()
                    && c.permission_grants.contains(&PublicPermission::UiPanel))))
        .then_some(c.active_generation)
    }

    pub(crate) fn command_hotkey_authority(
        &self,
        plugin: &str,
        command: &str,
    ) -> Result<(bool, u64), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        self.command_authority_locked(plugin, command)
    }
    pub(crate) fn persist_command_hotkey(
        &self,
        settings: &crate::settings::SettingsStore,
        plugin: &str,
        command: &str,
        hotkey: Option<String>,
        expected: (bool, u64),
    ) -> Result<(), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        if self.command_authority_locked(plugin, command)? != expected {
            return Err(PublicPluginManagementError::Unavailable);
        }
        settings
            .set_command_hotkey(plugin, command, hotkey)
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    fn refresh_clipboard_history_authorization(&self) -> Result<(), PublicPluginManagementError> {
        let hotkey_cleanup = self.publish_command_hotkeys();
        let plugin_ids = self
            .state
            .configs()?
            .into_iter()
            .filter(clipboard_history_is_authorized)
            .map(|config| config.plugin_id);
        let clipboard_cleanup = self
            .clipboard_history
            .sync_authorized_plugins(plugin_ids)
            .map_err(PublicPluginManagementError::from);
        clipboard_cleanup.and(hotkey_cleanup)
    }

    pub(crate) fn shutdown_clipboard_history(&self) {
        self.clipboard_history.shutdown();
    }

    pub(crate) fn clipboard_history_for_selection(&self) -> Arc<ClipboardHistoryService> {
        Arc::clone(&self.clipboard_history)
    }

    pub(crate) fn clipboard_history_snapshot_for_panel(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<ClipboardHistorySnapshot, ClipboardHistoryBridgeError> {
        self.authorize_clipboard_history_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )?;
        self.clipboard_history
            .snapshot(plugin_id)
            .map_err(|_| ClipboardHistoryBridgeError::Unavailable)
    }

    pub(crate) fn clipboard_history_query_for_panel(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        query: Option<&str>,
    ) -> Result<ClipboardHistorySnapshot, ClipboardHistoryBridgeError> {
        self.authorize_clipboard_history_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )?;
        self.clipboard_history
            .snapshot_query(plugin_id, query)
            .map_err(|error| match error {
                ClipboardHistoryError::InvalidQuery => ClipboardHistoryBridgeError::InvalidQuery,
                _ => ClipboardHistoryBridgeError::Unavailable,
            })
    }

    pub(crate) fn panel_effect_declared(
        &self,
        identity: &crate::plugin_panel::PanelSessionIdentity,
        permission: PublicPermission,
    ) -> bool {
        self.bundles
            .confirmed_bundle(&identity.plugin_id)
            .ok()
            .flatten()
            .is_some_and(|b| {
                let m = &b.runtime.manifest;
                b.runtime.generation == identity.generation
                    && b.activation_id == identity.activation_id
                    && b.admission_epoch == identity.admission_epoch
                    && m.permissions.contains(&permission)
                    && m.permissions.contains(&PublicPermission::UiPanel)
                    && m.supported_platforms == [super::PublicPlatform::Windows]
                    && super::manifest::parse_canonical_version(&m.minimum_host_version)
                        .is_some_and(|v| v >= [0, 18, 0])
                    && m.legacy_command().is_some_and(|c| {
                        c.activation_mode == PublicActivationMode::Submit
                            && c.output_mode == PublicOutputMode::Panel
                    })
            })
    }
    pub(crate) fn with_panel_text_authorization<T>(
        &self,
        identity: &crate::plugin_panel::PanelSessionIdentity,
        permission: PublicPermission,
        effect: impl FnOnce(
            crate::panel_effects::EffectLease,
        ) -> Result<T, crate::panel_effects::EffectError>,
    ) -> Result<T, crate::panel_effects::EffectError> {
        use crate::panel_effects::EffectError as E;
        let _mutation = self.lock_mutation().map_err(|_| E::ExpiredPanelSession)?;
        let bundle = self
            .bundles
            .confirmed_bundle(&identity.plugin_id)
            .map_err(|_| E::ExpiredPanelSession)?
            .ok_or(E::ExpiredPanelSession)?;
        let config = &bundle.config;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || bundle.runtime_recovery_needed
            || config.active_generation != identity.generation
            || !self.panel_effect_declared(identity, permission)
        {
            return Err(E::ExpiredPanelSession);
        }
        if !config
            .permission_grants
            .contains(&PublicPermission::UiPanel)
            || !config.permission_grants.contains(&permission)
        {
            return Err(E::PermissionDenied);
        }
        let lease = self
            .bundles
            .speech_lease(&identity.plugin_id)
            .map_err(|_| E::ExpiredPanelSession)?;
        effect(lease)
    }

    /// Permission publication stays serialized until the guarded native write completes.
    pub(crate) fn with_clipboard_history_copy_authorization<T>(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        effect: impl FnOnce() -> Result<T, ClipboardHistoryPasteError>,
    ) -> Result<T, ClipboardHistoryPasteError> {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?;
        self.authorize_clipboard_history_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )
        .map_err(|error| match error {
            ClipboardHistoryBridgeError::PermissionDenied => {
                ClipboardHistoryPasteError::PermissionDenied
            }
            ClipboardHistoryBridgeError::ExpiredPanelSession => {
                ClipboardHistoryPasteError::ExpiredPanelSession
            }
            _ => ClipboardHistoryPasteError::RecordUnavailable,
        })?;
        let bundle = self
            .bundles
            .confirmed_bundle(plugin_id)
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
            .ok_or(ClipboardHistoryPasteError::ExpiredPanelSession)?;
        if !bundle
            .config
            .permission_grants
            .contains(&PublicPermission::ClipboardHistoryCopy)
            || !bundle
                .runtime
                .manifest
                .permissions
                .contains(&PublicPermission::ClipboardHistoryCopy)
        {
            return Err(ClipboardHistoryPasteError::PermissionDenied);
        }
        effect()
    }

    pub(crate) fn clipboard_history_write_copy_for_panel(
        &self,
        plugin_id: &str,
        id: &str,
    ) -> Result<(), ClipboardHistoryPasteError> {
        self.clipboard_history
            .with_temporary_copy(|_| {
                let record = self.clipboard_history.record_for_paste(plugin_id, id)?;
                crate::clipboard_history::paste_clipboard_history_record(&record)?;
                Ok(())
            })
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
    }

    pub(crate) fn clipboard_history_remove_for_panel(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        id: &str,
    ) -> Result<(), ClipboardHistoryBridgeError> {
        self.authorize_clipboard_history_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )?;
        self.clipboard_history
            .remove(plugin_id, id)
            .map(|_| ())
            .map_err(|_| ClipboardHistoryBridgeError::Unavailable)
    }

    pub(crate) fn clipboard_history_clear_for_panel(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<(), ClipboardHistoryBridgeError> {
        self.authorize_clipboard_history_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )?;
        self.clipboard_history
            .clear(plugin_id)
            .map_err(|_| ClipboardHistoryBridgeError::Unavailable)
    }

    pub(crate) fn clipboard_history_record_for_paste_panel(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        id: &str,
    ) -> Result<ClipboardHistoryRecord, ClipboardHistoryPasteError> {
        self.authorize_clipboard_history_paste_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )?;
        self.clipboard_history.record_for_paste(plugin_id, id)
    }

    pub(crate) fn clipboard_history_complete_paste_for_panel(
        &self,
        plugin_id: &str,
        id: &str,
    ) -> Result<(), ClipboardHistoryPasteError> {
        self.clipboard_history.complete_paste(plugin_id, id)
    }

    pub(crate) fn clipboard_history_begin_paste_restore_for_panel(
        &self,
    ) -> Result<(), ClipboardHistoryPasteError> {
        self.clipboard_history
            .begin_paste_restore_suppression()
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)
    }

    pub(crate) fn clipboard_history_cancel_paste_restore_for_panel(&self) {
        let _ = self.clipboard_history.cancel_paste_restore_suppression();
    }

    /// Recheck at native effect execution, serialized with permission publication.
    pub(crate) fn with_clipboard_history_paste_authorization<T>(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        expected_revision: Option<u64>,
        effect: impl FnOnce(u64) -> Result<T, ClipboardHistoryPasteError>,
    ) -> Result<T, ClipboardHistoryPasteError> {
        let _mutation = self
            .lock_mutation()
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?;
        self.authorize_clipboard_history_paste_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )?;
        let bundle = self
            .bundles
            .confirmed_bundle(plugin_id)
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
            .ok_or(ClipboardHistoryPasteError::ExpiredPanelSession)?;
        let revision = bundle.config.inventory_revision;
        if expected_revision.is_some_and(|expected| expected != revision) {
            return Err(ClipboardHistoryPasteError::ExpiredPanelSession);
        }
        effect(revision)
    }

    fn authorize_clipboard_history_panel_call(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<(), ClipboardHistoryBridgeError> {
        let bundle = self
            .bundles
            .bundle(plugin_id)
            .map_err(|_| ClipboardHistoryBridgeError::Unavailable)?
            .ok_or(ClipboardHistoryBridgeError::ExpiredPanelSession)?;
        let config = &bundle.config;
        let snapshot = &bundle.runtime;
        if !config
            .permission_grants
            .contains(&PublicPermission::UiPanel)
            || !config.installed
            || !config.enabled
            || config.fault.is_some()
            || bundle.runtime_recovery_needed
            || config.active_generation != generation
            || snapshot.generation != generation
            || bundle.activation_id != activation_id
            || bundle.admission_epoch != admission_epoch
            || snapshot
                .manifest
                .legacy_command()
                .is_none_or(|command| command.output_mode != PublicOutputMode::Panel)
            || !snapshot
                .manifest
                .permissions
                .contains(&PublicPermission::UiPanel)
        {
            return Err(ClipboardHistoryBridgeError::ExpiredPanelSession);
        }
        if !config
            .permission_grants
            .contains(&PublicPermission::ClipboardHistoryRead)
            || !snapshot
                .manifest
                .permissions
                .contains(&PublicPermission::ClipboardHistoryRead)
        {
            return Err(ClipboardHistoryBridgeError::PermissionDenied);
        }
        Ok(())
    }

    fn authorize_clipboard_history_paste_panel_call(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<(), ClipboardHistoryPasteError> {
        self.authorize_clipboard_history_panel_call(
            plugin_id,
            generation,
            activation_id,
            admission_epoch,
        )
        .map_err(|error| match error {
            ClipboardHistoryBridgeError::PermissionDenied => {
                ClipboardHistoryPasteError::PermissionDenied
            }
            ClipboardHistoryBridgeError::ExpiredPanelSession => {
                ClipboardHistoryPasteError::ExpiredPanelSession
            }
            ClipboardHistoryBridgeError::Unavailable
            | ClipboardHistoryBridgeError::InvalidQuery => {
                ClipboardHistoryPasteError::RecordUnavailable
            }
        })?;
        let bundle = self
            .bundles
            .bundle(plugin_id)
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
            .ok_or(ClipboardHistoryPasteError::ExpiredPanelSession)?;
        if !bundle
            .config
            .permission_grants
            .contains(&PublicPermission::ClipboardHistoryPaste)
            || !bundle
                .runtime
                .manifest
                .permissions
                .contains(&PublicPermission::ClipboardHistoryPaste)
        {
            return Err(ClipboardHistoryPasteError::PermissionDenied);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn clipboard_history_authorized_plugins_for_test(&self) -> Vec<String> {
        self.clipboard_history.authorized_plugins_for_test()
    }

    #[cfg(test)]
    pub(crate) fn clipboard_history_snapshot_for_test(
        &self,
        plugin_id: &str,
    ) -> Result<crate::clipboard_history::ClipboardHistorySnapshot, PublicPluginManagementError>
    {
        Ok(self.clipboard_history.snapshot(plugin_id)?)
    }

    #[cfg(test)]
    pub(crate) fn clipboard_history_capture_for_test(
        &self,
        capture: ClipboardCapture,
    ) -> Result<(), PublicPluginManagementError> {
        Ok(self.clipboard_history.capture_for_test(capture)?)
    }

    #[cfg(test)]
    pub(crate) fn clipboard_history_store_path_for_test(&self, plugin_id: &str) -> PathBuf {
        self.clipboard_history.store_path_for_test(plugin_id)
    }

    #[cfg(test)]
    pub(crate) fn prepare(
        &self,
        caller_label: &str,
        source: PublicPluginInstallSource,
        now: Instant,
    ) -> Result<PublicPluginPrepareSummary, PublicPluginManagementError> {
        self.prepare_targeted(caller_label, source, None, now)
    }

    pub(crate) fn prepare_targeted(
        &self,
        caller_label: &str,
        source: PublicPluginInstallSource,
        refresh_target: Option<PublicPluginRefreshTarget>,
        now: Instant,
    ) -> Result<PublicPluginPrepareSummary, PublicPluginManagementError> {
        if caller_label != "main" {
            return Err(PublicPluginManagementError::InvalidCaller);
        }
        let mut observation = refresh_target
            .as_ref()
            .filter(|t| valid_plugin_id(&t.plugin_id))
            .and_then(|t| {
                self.observations
                    .start(&t.plugin_id, Operation::DevelopmentRefresh)
            });
        let result = (|| {
            if self.bundles.is_terminal() {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let _mutation = self.lock_mutation()?;
            if let Some(target) = &refresh_target {
                if !valid_plugin_id(&target.plugin_id)
                    || !matches!(
                        source,
                        PublicPluginInstallSource::DevelopmentDirectory { .. }
                    )
                {
                    return Err(PublicPluginManagementError::RefreshTargetMismatch);
                }
                let revision = target
                    .expected_inventory_revision
                    .parse::<u64>()
                    .ok()
                    .filter(|v| v.to_string() == target.expected_inventory_revision)
                    .ok_or(PublicPluginManagementError::StaleInventory)?;
                if self.state.inventory_revision()? != revision {
                    return Err(PublicPluginManagementError::StaleInventory);
                }
                if !self
                    .state
                    .config(&target.plugin_id)?
                    .is_some_and(|c| c.installed)
                {
                    return Err(PublicPluginManagementError::RefreshTargetMismatch);
                }
            }
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
            let previous_config = self.state.config(&prepared.manifest.plugin_id)?;
            if let Some(target) = &refresh_target {
                if prepared.manifest.plugin_id != target.plugin_id {
                    return Err(PublicPluginManagementError::RefreshTargetMismatch);
                }
                if !previous_config
                    .as_ref()
                    .is_some_and(|c| c.installed && c.version == prepared.manifest.version)
                {
                    return Err(PublicPluginManagementError::RefreshVersionMismatch);
                }
            }
            let operation = if refresh_target.is_some() {
                Operation::DevelopmentRefresh
            } else if previous_config.as_ref().is_some_and(|c| c.installed) {
                Operation::Replace
            } else {
                Operation::Prepare
            };
            if observation.is_none() {
                observation = self
                    .observations
                    .start(&prepared.manifest.plugin_id, operation);
            }
            let previous_declared_https_hosts = previous_config
                .as_ref()
                .filter(|config| config.installed)
                .map(|config| self.installed_manifest(config))
                .transpose()?
                .and_then(|manifest| manifest.network)
                .map_or_else(BTreeSet::new, |network| {
                    network.https_hosts.into_iter().collect()
                });
            let previous_declared_credentials = previous_config
                .as_ref()
                .filter(|config| config.installed)
                .map(|config| self.installed_manifest(config))
                .transpose()?
                .as_ref()
                .map(super::state::manifest_credentials)
                .unwrap_or_default();
            let credentials = super::state::manifest_credentials(&prepared.manifest);
            let added_credential_ids = credentials
                .iter()
                .filter(|credential| !previous_declared_credentials.contains(credential))
                .map(|credential| credential.id.clone())
                .collect::<Vec<_>>();
            let candidate_https_hosts = prepared
                .manifest
                .network
                .as_ref()
                .map_or_else(BTreeSet::new, |network| {
                    network.https_hosts.iter().cloned().collect()
                });
            let added_https_hosts = candidate_https_hosts
                .difference(&previous_declared_https_hosts)
                .cloned()
                .collect::<Vec<_>>();
            let is_update = previous_config
                .as_ref()
                .is_some_and(|config| config.installed);
            let requires_network_consent = !candidate_https_hosts.is_empty()
                && (!is_update
                    || !added_https_hosts.is_empty()
                    || !added_credential_ids.is_empty());
            let network =
                prepared
                    .manifest
                    .network
                    .as_ref()
                    .map(|network| PublicPluginPrepareNetwork {
                        https_hosts: network.https_hosts.clone(),
                        added_https_hosts,
                        credentials,
                        added_credential_ids,
                        requires_network_consent,
                    });
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
                operation: match operation {
                    Operation::DevelopmentRefresh => PrepareOperation::DevelopmentRefresh,
                    Operation::Replace => PrepareOperation::Replace,
                    _ => PrepareOperation::Install,
                },
                previous_version: previous_config
                    .as_ref()
                    .filter(|c| c.installed)
                    .map(|c| c.version.clone()),
                source_verified: false,
                icon_url: icon.as_ref().map(|_| icon::prepared_url(&token)),
                network,
            };
            let expires_at = now
                .checked_add(PREPARE_TTL)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            self.lock_data()?.pending.insert(
                token,
                PendingActivation {
                    operation,
                    refresh_target,
                    caller_label: caller_label.into(),
                    expires_at,
                    prepared,
                    icon,
                    previous_config,
                    previous_declared_https_hosts,
                    previous_declared_credentials,
                    requires_network_consent,
                },
            );
            diagnostics::phase(
                &mut observation,
                Phase::Complete,
                CommitOutcome::NotAttempted,
            );
            Ok(summary)
        })();
        if let Some(observation) = observation {
            observation.finish(&result);
        }
        result
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

    #[cfg(test)]
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
        self.commit_with_readiness_with_effects(
            caller_label,
            token,
            permission_grants,
            now,
            readiness,
            &mut ManagementEffects::default(),
        )
    }

    pub(crate) fn commit_with_readiness_with_effects<F>(
        &self,
        caller_label: &str,
        token: &str,
        permission_grants: BTreeSet<PublicPermission>,
        now: Instant,
        readiness: F,
        effects: &mut ManagementEffects,
    ) -> Result<PublicActivationCommit, PublicPluginManagementError>
    where
        F: FnOnce(&PublicRuntimeCandidate) -> bool,
    {
        if caller_label != "main" {
            return Err(PublicPluginManagementError::InvalidCaller);
        }
        let mut observation = None;
        let result = (|| {
            let (
                pending,
                previous_config,
                effective_permission_grants,
                candidate,
                runtime,
                previous_runtime_label,
                previous_activation_id,
            ) = {
                let _mutation = self.lock_mutation()?;
                self.prune_command_hotkeys_locked()?;
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
                observation = self
                    .observations
                    .start(&pending.prepared.manifest.plugin_id, pending.operation);
                if let Some(target) = &pending.refresh_target {
                    if target.plugin_id != pending.prepared.manifest.plugin_id
                        || !pending.previous_config.as_ref().is_some_and(|c| {
                            c.installed && c.version == pending.prepared.manifest.version
                        })
                    {
                        return Err(PublicPluginManagementError::RefreshTargetMismatch);
                    }
                }
                let declared = pending
                    .prepared
                    .manifest
                    .permissions
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>();
                if declared
                    .iter()
                    .filter(|p| {
                        !p.is_host_service()
                            && !matches!(
                                **p,
                                PublicPermission::SelectionRead
                                    | PublicPermission::ClipboardHistoryCopy
                                    | PublicPermission::SpeechSpeak
                            )
                    })
                    .copied()
                    .collect::<BTreeSet<_>>()
                    != permission_grants
                        .iter()
                        .filter(|p| {
                            !p.is_host_service()
                                && !matches!(
                                    **p,
                                    PublicPermission::SelectionRead
                                        | PublicPermission::ClipboardHistoryCopy
                                    | PublicPermission::SpeechSpeak
                                )
                        })
                        .copied()
                        .collect::<BTreeSet<_>>()
                {
                    return Err(PublicPluginManagementError::PermissionDenied);
                }
                let previous_config = self.state.config(&pending.prepared.manifest.plugin_id)?;
                if previous_config != pending.previous_config {
                    return Err(PublicPluginManagementError::Unavailable);
                }
                let current_declared_https_hosts = previous_config
                    .as_ref()
                    .filter(|config| config.installed)
                    .map(|config| self.installed_manifest(config))
                    .transpose()?
                    .and_then(|manifest| manifest.network)
                    .map_or_else(BTreeSet::new, |network| {
                        network.https_hosts.into_iter().collect()
                    });
                let current_declared_credentials = previous_config
                    .as_ref()
                    .filter(|config| config.installed)
                    .map(|config| self.installed_manifest(config))
                    .transpose()?
                    .as_ref()
                    .map(super::state::manifest_credentials)
                    .unwrap_or_default();
                if current_declared_credentials != pending.previous_declared_credentials
                    || current_declared_https_hosts != pending.previous_declared_https_hosts
                {
                    return Err(PublicPluginManagementError::Unavailable);
                }
                let previous_network_granted = previous_config.as_ref().is_some_and(|config| {
                    config
                        .permission_grants
                        .contains(&PublicPermission::NetworkHttps)
                        && !pending.previous_declared_https_hosts.is_empty()
                        && config.network_https_hosts_grant == pending.previous_declared_https_hosts
                        && config.network_credentials_grant == pending.previous_declared_credentials
                });
                let mut effective_permission_grants = declared;
                effective_permission_grants.retain(|p| {
                    (!p.is_host_service()
                        && !matches!(
                            *p,
                            PublicPermission::SelectionRead
                                | PublicPermission::ClipboardHistoryCopy
                                    | PublicPermission::SpeechSpeak
                        ))
                        || previous_config
                            .as_ref()
                            .filter(|c| c.installed)
                            .is_some_and(|c| c.permission_grants.contains(p))
                });
                // Existing declarations retain the user's explicit revocations. New
                // declarations have already passed the complete consent check above.
                if let Some(previous) = previous_config.as_ref().filter(|c| c.installed) {
                    let previous_manifest = self.installed_manifest(previous)?;
                    for permission in previous_manifest.permissions {
                        if permission != PublicPermission::NetworkHttps
                            && !previous.permission_grants.contains(&permission)
                        {
                            effective_permission_grants.remove(&permission);
                        }
                    }
                }
                if pending.prepared.manifest.network.is_some()
                    && !pending.requires_network_consent
                    && !previous_network_granted
                {
                    effective_permission_grants.remove(&PublicPermission::NetworkHttps);
                }
                let generation = previous_config
                    .as_ref()
                    .map_or(Some(1), |config| config.active_generation.checked_add(1))
                    .ok_or(PublicPluginManagementError::Unavailable)?;
                let runtime_snapshot =
                    Arc::new(snapshot_from_prepared(&pending.prepared, generation)?);
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
                    effective_permission_grants,
                    candidate,
                    runtime,
                    previous_runtime_label,
                    previous_activation_id,
                )
            };

            diagnostics::phase(
                &mut observation,
                Phase::Readiness,
                CommitOutcome::NotAttempted,
            );
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
                let prepared_state = match self.state.prepare_activation(
                    &candidate.runtime.manifest,
                    effective_permission_grants,
                    runtime.generation,
                    Some(candidate.runtime.digest.clone()),
                ) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.unstage(&runtime.label);
                        return Err(error.into());
                    }
                };
                (reservation, prepared_state)
            };
            let destination = package_destination(
                &self.packages_root,
                &runtime.plugin_id,
                &candidate.runtime.digest,
            );
            diagnostics::phase(
                &mut observation,
                Phase::Persistence,
                CommitOutcome::NotAttempted,
            );
            let created = match pending.prepared.persist(&destination) {
                Ok(created) => created,
                Err(error) => {
                    self.unstage(&runtime.label);
                    return Err(error.into());
                }
            };
            reservation.begin_committing()?;
            diagnostics::phase(&mut observation, Phase::Persistence, CommitOutcome::Unknown);
            match self.state.persist_prepared(&prepared_state) {
                DurableStateOutcome::Committed => {
                    diagnostics::phase(
                        &mut observation,
                        Phase::Publication,
                        CommitOutcome::Committed,
                    );
                    reservation.mark_durable()?;
                }
                DurableStateOutcome::NotCommitted => {
                    if !self.state.revalidate_previous(&prepared_state) {
                        self.close_network_authority_with_effects(
                            &runtime.plugin_id,
                            Some(effects),
                        );
                        return Err(PublicPluginManagementError::Unavailable);
                    }
                    diagnostics::phase(
                        &mut observation,
                        Phase::Persistence,
                        CommitOutcome::NotCommitted,
                    );
                    reservation.rollback_not_committed()?;
                    if created {
                        package::remove_package_tree(destination);
                    }
                    self.unstage(&runtime.label);
                    return Err(PublicPluginManagementError::Unavailable);
                }
                DurableStateOutcome::Unknown => {
                    self.close_network_authority_with_effects(&runtime.plugin_id, Some(effects));
                    return Err(PublicPluginManagementError::Unavailable);
                }
            }

            let affected_plugin = runtime.plugin_id.clone();
            let result = (|| {
                let _mutation = self.lock_mutation()?;
                let mut network_transition = self
                    .network_authority
                    .begin_transition()
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                effects.retired.extend(
                    self.scheduler
                        .retire_plugin_requests(&runtime.plugin_id)
                        .map_err(|_| PublicPluginManagementError::Unavailable)?,
                );
                effects.changed = true;
                let expired = self
                    .scheduler
                    .invalidate_plugin(
                        &runtime.plugin_id,
                        Some((
                            runtime.generation,
                            runtime.activation_id,
                            runtime.admission_epoch,
                        )),
                    )
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                if let Some(expired) = expired.as_ref() {
                    network_transition.invalidate_context(expired);
                }
                network_transition
                    .set_plugin_authority(&runtime.plugin_id, None)
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
                let authority = checked_network_authority_snapshot(
                    &self.bundles,
                    &self.state,
                    &self.secrets,
                    &config,
                    &candidate.runtime,
                    runtime.activation_id,
                    runtime.admission_epoch,
                )?;
                network_transition
                    .set_plugin_authority(&runtime.plugin_id, authority)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                drop(data);
                let commit = PublicActivationCommit {
                    mutation: mutation_from_config(&config),
                    runtime,
                    previous_runtime_label,
                };
                drop(_mutation);
                self.apply_timer_post_lock_effects(timer_effects);
                self.refresh_clipboard_history_authorization()?;
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
            })();
            if result.is_err() {
                self.close_network_authority_with_effects(&affected_plugin, Some(effects));
            }
            if result.is_ok() {
                diagnostics::phase(&mut observation, Phase::Complete, CommitOutcome::Committed);
            }
            result
        })();
        if let Some(observation) = observation {
            observation.finish(&result);
        }
        result
    }
    #[cfg(test)]
    pub(crate) fn set_enabled_with_readiness<F>(
        &self,
        plugin_id: &str,
        enabled: bool,
        readiness: F,
    ) -> Result<PublicEnabledCommit, PublicPluginManagementError>
    where
        F: FnOnce(&PublicRuntimeCandidate) -> bool,
    {
        self.set_enabled_with_readiness_with_effects(
            plugin_id,
            enabled,
            readiness,
            &mut ManagementEffects::default(),
        )
    }

    pub(crate) fn set_enabled_with_readiness_with_effects<F>(
        &self,
        plugin_id: &str,
        enabled: bool,
        readiness: F,
        effects: &mut ManagementEffects,
    ) -> Result<PublicEnabledCommit, PublicPluginManagementError>
    where
        F: FnOnce(&PublicRuntimeCandidate) -> bool,
    {
        if !enabled {
            return self.disable_plugin(plugin_id, effects);
        }
        let mut observation = valid_plugin_id(plugin_id)
            .then(|| self.observations.start(plugin_id, Operation::Enable))
            .flatten();
        let result = (|| {
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

            diagnostics::phase(
                &mut observation,
                Phase::Readiness,
                CommitOutcome::NotAttempted,
            );
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

            self.make_state_durable_observed(
                &reservation,
                &prepared_state,
                Some(effects),
                &mut observation,
            )?;
            diagnostics::phase(
                &mut observation,
                Phase::Publication,
                CommitOutcome::Committed,
            );
            let result = (|| {
                let _mutation = self.lock_mutation()?;
                let staged_matches = self
                    .lock_data()?
                    .staged_by_label
                    .get(&runtime.label)
                    .is_some_and(|staged| Arc::ptr_eq(staged, &candidate));
                if !staged_matches {
                    return Err(PublicPluginManagementError::Unavailable);
                }
                let mut network_transition = self
                    .network_authority
                    .begin_transition()
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                effects.retired.extend(
                    self.scheduler
                        .retire_plugin_requests(plugin_id)
                        .map_err(|_| PublicPluginManagementError::Unavailable)?,
                );
                effects.changed = true;
                let expired = self
                    .scheduler
                    .invalidate_plugin(
                        plugin_id,
                        Some((
                            runtime.generation,
                            runtime.activation_id,
                            runtime.admission_epoch,
                        )),
                    )
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                if let Some(expired) = expired.as_ref() {
                    network_transition.invalidate_context(expired);
                }
                network_transition
                    .set_plugin_authority(plugin_id, None)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                self.activate_runtime_data_gate(&runtime)?;
                self.cancel_delayed_messages(plugin_id);
                let timer_effects = Vec::new();
                self.clear_runtime_faults(plugin_id)?;
                let config = self.state.publish_prepared(prepared_state)?;
                reservation.publish(Some(candidate.bundle(config.clone())))?;
                let authority = checked_network_authority_snapshot(
                    &self.bundles,
                    &self.state,
                    &self.secrets,
                    &config,
                    &candidate.runtime,
                    runtime.activation_id,
                    runtime.admission_epoch,
                )?;
                network_transition
                    .set_plugin_authority(plugin_id, authority)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?;
                let mut data = self.lock_data()?;
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
                self.refresh_clipboard_history_authorization()?;
                Ok(commit)
            })();
            if result.is_err() {
                self.close_network_authority_with_effects(plugin_id, Some(effects));
            }
            result
        })();
        if result.is_ok() {
            if let Some(attempt) = &mut observation {
                attempt.observation.phase = Phase::Complete;
            }
        }
        if let Some(observation) = observation {
            observation.finish(&result);
        }
        result
    }

    fn disable_plugin(
        &self,
        plugin_id: &str,
        effects: &mut ManagementEffects,
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

        self.make_state_durable_with_effects(&reservation, &prepared_state, Some(effects))?;
        let result = (|| {
            let _mutation = self.lock_mutation()?;
            let mut network_transition = self
                .network_authority
                .begin_transition()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            effects.retired.extend(
                self.scheduler
                    .retire_plugin_requests(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
            );
            effects.changed = true;
            let expired = self
                .scheduler
                .invalidate_plugin(plugin_id, None)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            if let Some(expired) = expired.as_ref() {
                network_transition.invalidate_context(expired);
            }
            network_transition
                .set_plugin_authority(plugin_id, None)
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
            self.refresh_clipboard_history_authorization()?;
            Ok(commit)
        })();
        if result.is_err() {
            self.close_network_authority_with_effects(plugin_id, Some(effects));
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn rename(
        &self,
        plugin_id: &str,
        name_override: Option<&str>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.rename_command(plugin_id, "default", name_override)
    }

    #[cfg(test)]
    pub(crate) fn rename_command(
        &self,
        plugin_id: &str,
        command_id: &str,
        name_override: Option<&str>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.rename_command_with_effects(
            plugin_id,
            command_id,
            name_override,
            &mut ManagementEffects::default(),
        )
    }

    pub(crate) fn rename_command_with_effects(
        &self,
        plugin_id: &str,
        command_id: &str,
        name_override: Option<&str>,
        effects: &mut ManagementEffects,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let bundle = self
                .bundles
                .confirmed_bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            bundle
                .runtime
                .manifest
                .command_by_id(command_id)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let Some(prepared_state) =
                self.state
                    .prepare_rename(plugin_id, command_id, name_override)?
            else {
                return Ok(mutation_from_config(&bundle.config));
            };
            let reservation = self.bundles.reserve(plugin_id)?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable_with_effects(&reservation, &prepared_state, Some(effects))?;
        let result = (|| {
            let _mutation = self.lock_mutation()?;
            let mut transition = self
                .network_authority
                .begin_transition()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            effects.retired.extend(
                self.scheduler
                    .retire_plugin_requests(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
            );
            effects.changed = true;
            transition
                .set_plugin_authority(plugin_id, None)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let config = self.state.publish_prepared(prepared_state)?;
            reservation.publish(Some(bundle.with_config(config.clone())))?;
            let authority = checked_network_authority_snapshot(
                &self.bundles,
                &self.state,
                &self.secrets,
                &config,
                &bundle.runtime,
                bundle.activation_id,
                bundle.admission_epoch,
            )?;
            transition
                .set_plugin_authority(plugin_id, authority)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            Ok(mutation_from_config(&config))
        })();
        if result.is_err() {
            self.close_network_authority_with_effects(plugin_id, Some(effects));
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn set_favorite(
        &self,
        plugin_id: &str,
        favorite: bool,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.set_command_favorite(plugin_id, "default", favorite)
    }

    #[cfg(test)]
    pub(crate) fn set_command_favorite(
        &self,
        plugin_id: &str,
        command_id: &str,
        favorite: bool,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.set_command_favorite_with_effects(
            plugin_id,
            command_id,
            favorite,
            &mut ManagementEffects::default(),
        )
    }

    pub(crate) fn set_command_favorite_with_effects(
        &self,
        plugin_id: &str,
        command_id: &str,
        favorite: bool,
        effects: &mut ManagementEffects,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let bundle = self
                .bundles
                .confirmed_bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            if !bundle.config.installed {
                return Err(PublicPluginManagementError::Unavailable);
            }
            bundle
                .runtime
                .manifest
                .command_by_id(command_id)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let Some(prepared_state) = self
                .state
                .prepare_set_favorite(plugin_id, command_id, favorite)?
            else {
                return Ok(mutation_from_config(&bundle.config));
            };
            let reservation = self.bundles.reserve(plugin_id)?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable_with_effects(&reservation, &prepared_state, Some(effects))?;
        let result = (|| {
            let _mutation = self.lock_mutation()?;
            let config = self.state.publish_prepared(prepared_state)?;
            reservation.publish(Some(bundle.with_config(config.clone())))?;
            Ok(mutation_from_config(&config))
        })();
        if result.is_err() {
            self.close_network_authority_with_effects(plugin_id, Some(effects));
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn set_network_access(
        &self,
        plugin_id: &str,
        granted: bool,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.set_network_access_with_revision(plugin_id, granted, None)
    }

    #[cfg(test)]
    pub(crate) fn set_network_access_with_revision(
        &self,
        plugin_id: &str,
        granted: bool,
        expected_inventory_revision: Option<&str>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.set_network_access_with_effects(
            plugin_id,
            granted,
            expected_inventory_revision,
            &mut ManagementEffects::default(),
        )
    }

    pub(crate) fn set_permission_access_with_effects(
        &self,
        plugin_id: &str,
        permission: PublicPermission,
        granted: bool,
        expected_inventory_revision: &str,
        effects: &mut ManagementEffects,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        if !permission.is_available(self.host.platform) {
            return Err(PublicPluginManagementError::PermissionDenied);
        }
        if permission == PublicPermission::NetworkHttps {
            return self.set_network_access_with_effects(
                plugin_id,
                granted,
                Some(expected_inventory_revision),
                effects,
            );
        }
        let _permission_mutation = self
            .permission_mutation
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            if expected_inventory_revision != self.state.inventory_revision()?.to_string() {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let config = self
                .state
                .config(plugin_id)?
                .filter(|c| c.installed)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let manifest = self.installed_manifest(&config)?;
            if !manifest.permissions.contains(&permission) {
                return Err(PublicPluginManagementError::PermissionDenied);
            }
            if config.permission_grants.contains(&permission) == granted {
                return Ok(mutation_from_config(&config));
            }
            let bundle = self.bundles.bundle(plugin_id)?;
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared = self
                .state
                .prepare_set_permission_access(&manifest, permission, granted)?;
            (bundle, reservation, prepared)
        };
        let mut timer_effects = Vec::new();
        let mut durable = false;
        let result = (|| {
            self.make_state_durable_with_effects(&reservation, &prepared_state, Some(effects))?;
            durable = true;
            let _mutation = self.lock_mutation()?;
            let mut transition = self
                .network_authority
                .begin_transition()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            effects.retired.extend(
                self.scheduler
                    .retire_plugin_requests(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
            );
            effects.changed = true;
            transition
                .set_plugin_authority(plugin_id, None)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            if !granted {
                if permission == PublicPermission::NotificationsPublish {
                    self.cancel_delayed_messages(plugin_id);
                }
                if matches!(
                    permission,
                    PublicPermission::UiWindow
                        | PublicPermission::NotificationsPublish
                        | PublicPermission::TimerControl
                ) {
                    if let Some(bundle) = &bundle {
                        timer_effects = self.cancel_timer_generation(
                            plugin_id,
                            bundle.runtime.generation,
                            bundle.activation_id,
                        );
                    }
                }
            }
            let config = self.state.publish_prepared(prepared_state)?;
            let next_bundle = bundle.as_ref().map(|b| b.with_config(config.clone()));
            reservation.publish(next_bundle.clone())?;
            let authority = next_bundle
                .as_ref()
                .map(|b| {
                    checked_network_authority_snapshot(
                        &self.bundles,
                        &self.state,
                        &self.secrets,
                        &config,
                        &b.runtime,
                        b.activation_id,
                        b.admission_epoch,
                    )
                })
                .transpose()?
                .flatten();
            transition
                .set_plugin_authority(plugin_id, authority)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            Ok(mutation_from_config(&config))
        })();
        self.apply_timer_post_lock_effects(timer_effects);
        if result.is_err() && (effects.changed || durable) {
            // Unknown durability/publication closes the reservation book as well
            // as requests. Retire persistent work even if mutation was poisoned.
            self.close_network_authority_with_effects(plugin_id, Some(effects));
            self.cancel_delayed_messages(plugin_id);
            // The capture service owns authority independently of runtime bundles.
            // Retire just this cached owner; persisted state may still be stale.
            let _ = self.clipboard_history.uninstall(plugin_id, true);
            if let Some(bundle) = &bundle {
                self.apply_timer_post_lock_effects(self.cancel_timer_generation(
                    plugin_id,
                    bundle.runtime.generation,
                    bundle.activation_id,
                ));
            }
        }
        if result.is_ok() {
            self.refresh_clipboard_history_authorization()?;
        }
        result
    }

    pub(crate) fn set_network_access_with_effects(
        &self,
        plugin_id: &str,
        granted: bool,
        expected_inventory_revision: Option<&str>,
        effects: &mut ManagementEffects,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let _permission_mutation = self
            .permission_mutation
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let config = self
                .state
                .config(plugin_id)?
                .filter(|config| config.installed)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let manifest = self.installed_manifest(&config)?;
            let credentials = super::state::manifest_credentials(&manifest);
            if (expected_inventory_revision.is_some() || (granted && !credentials.is_empty()))
                && expected_inventory_revision
                    != Some(self.state.inventory_revision()?.to_string().as_str())
            {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let declared_hosts = manifest
                .network
                .ok_or(PublicPluginManagementError::PermissionDenied)?
                .https_hosts
                .into_iter()
                .collect::<BTreeSet<_>>();
            let currently_granted = config
                .permission_grants
                .contains(&PublicPermission::NetworkHttps)
                && config.network_https_hosts_grant == declared_hosts
                && config.network_credentials_grant == credentials;
            if currently_granted == granted {
                return Ok(mutation_from_config(&config));
            }
            let bundle = self.bundles.bundle(plugin_id)?;
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_set_network_access_with_credentials(
                plugin_id,
                declared_hosts,
                credentials,
                granted,
            )?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable_with_effects(&reservation, &prepared_state, Some(effects))?;
        let result = (|| {
            let _mutation = self.lock_mutation()?;
            let mut network_transition = self
                .network_authority
                .begin_transition()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            effects.retired.extend(
                self.scheduler
                    .retire_plugin_requests(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
            );
            effects.changed = true;
            network_transition
                .set_plugin_authority(plugin_id, None)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let config = self.state.publish_prepared(prepared_state)?;
            let next_bundle = bundle.map(|bundle| bundle.with_config(config.clone()));
            reservation.publish(next_bundle.clone())?;
            let authority = next_bundle
                .as_ref()
                .map(|bundle| {
                    checked_network_authority_snapshot(
                        &self.bundles,
                        &self.state,
                        &self.secrets,
                        &config,
                        &bundle.runtime,
                        bundle.activation_id,
                        bundle.admission_epoch,
                    )
                })
                .transpose()?
                .flatten();
            network_transition
                .set_plugin_authority(plugin_id, authority)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            Ok(mutation_from_config(&config))
        })();
        if result.is_err() {
            // Durable state may already differ even if reacquiring mutation or
            // publishing fails. Close after all transition guards have dropped.
            self.close_network_authority_with_effects(plugin_id, Some(effects));
        }
        result
    }

    pub(crate) fn validate_settings_save(
        &self,
        plugin_id: &str,
        updates: &BTreeMap<String, Value>,
        secrets: &BTreeMap<String, Option<String>>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let bundle = self
            .bundles
            .confirmed_bundle(plugin_id)?
            .ok_or(PublicPluginManagementError::Unavailable)?;
        validate_settings_updates(&bundle.runtime.manifest.settings, updates)?;
        for key in secrets.keys() {
            if !bundle
                .runtime
                .manifest
                .settings
                .iter()
                .any(|definition| definition.key() == key && definition.is_secret())
            {
                return Err(PublicPluginManagementError::Unavailable);
            }
        }
        Ok(mutation_from_config(&bundle.config))
    }

    #[cfg(test)]
    pub(crate) fn save_settings(
        &self,
        plugin_id: &str,
        updates: &BTreeMap<String, Value>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.save_settings_with_effects(plugin_id, updates, &mut ManagementEffects::default())
    }

    pub(crate) fn save_settings_with_effects(
        &self,
        plugin_id: &str,
        updates: &BTreeMap<String, Value>,
        effects: &mut ManagementEffects,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        let (bundle, reservation, prepared_state) = {
            let _mutation = self.lock_mutation()?;
            let bundle = self
                .bundles
                .confirmed_bundle(plugin_id)?
                .ok_or(PublicPluginManagementError::Unavailable)?;
            validate_settings_updates(&bundle.runtime.manifest.settings, updates)?;
            if updates.iter().all(|(key, value)| {
                bundle
                    .config
                    .settings
                    .get(key)
                    .cloned()
                    .or_else(|| {
                        bundle
                            .runtime
                            .manifest
                            .settings
                            .iter()
                            .find(|definition| definition.key() == key)
                            .and_then(|definition| definition.default_value())
                    })
                    .as_ref()
                    == Some(value)
            }) {
                return Ok(mutation_from_config(&bundle.config));
            }
            let reservation = self.bundles.reserve(plugin_id)?;
            let prepared_state = self.state.prepare_save_settings(
                plugin_id,
                &bundle.runtime.manifest.settings,
                updates,
            )?;
            (bundle, reservation, prepared_state)
        };
        self.make_state_durable_with_effects(&reservation, &prepared_state, Some(effects))?;
        let result = (|| {
            let _mutation = self.lock_mutation()?;
            let mut network_transition = self
                .network_authority
                .begin_transition()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            effects.retired.extend(
                self.scheduler
                    .retire_plugin_requests(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
            );
            effects.changed = true;
            network_transition
                .set_plugin_authority(plugin_id, None)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            let config = self.state.publish_prepared(prepared_state)?;
            reservation.publish(Some(bundle.with_config(config.clone())))?;
            let authority = checked_network_authority_snapshot(
                &self.bundles,
                &self.state,
                &self.secrets,
                &config,
                &bundle.runtime,
                bundle.activation_id,
                bundle.admission_epoch,
            )?;
            network_transition
                .set_plugin_authority(plugin_id, authority)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            Ok(mutation_from_config(&config))
        })();
        if result.is_err() {
            // Durable state may already differ even if reacquiring mutation or
            // publishing fails. Close after all transition guards have dropped.
            self.close_network_authority_with_effects(plugin_id, Some(effects));
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn save_secret(
        &self,
        plugin_id: &str,
        key: &str,
        value: Option<&str>,
    ) -> Result<(), PublicPluginManagementError> {
        self.save_secret_with_effects(plugin_id, key, value, &mut ManagementEffects::default())
    }

    pub(crate) fn save_secret_with_effects(
        &self,
        plugin_id: &str,
        key: &str,
        value: Option<&str>,
        effects: &mut ManagementEffects,
    ) -> Result<(), PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let bundle = self
            .bundles
            .confirmed_bundle(plugin_id)?
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if !bundle
            .runtime
            .manifest
            .settings
            .iter()
            .any(|setting| setting.key() == key && setting.is_secret())
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        let prepared = value
            .map(|value| self.secrets.prepare_write(plugin_id, key, value))
            .transpose()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let mut transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        effects.retired.extend(
            self.scheduler
                .retire_plugin_requests(plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?,
        );
        effects.changed = true;
        transition
            .set_plugin_authority(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        match prepared {
            Some(prepared) => self.secrets.commit_write(prepared),
            None => self.secrets.remove(plugin_id, key),
        }
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let authority = checked_network_authority_snapshot(
            &self.bundles,
            &self.state,
            &self.secrets,
            &bundle.config,
            &bundle.runtime,
            bundle.activation_id,
            bundle.admission_epoch,
        )?;
        transition
            .set_plugin_authority(plugin_id, authority)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn begin_uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<Option<PublicPluginUninstallTransaction>, PublicPluginManagementError> {
        self.begin_uninstall_with_effects(plugin_id, retain_data, &mut ManagementEffects::default())
    }

    pub(crate) fn begin_uninstall_with_effects(
        &self,
        plugin_id: &str,
        retain_data: bool,
        effects: &mut ManagementEffects,
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
            let mut network_transition = self
                .network_authority
                .begin_transition()
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            effects.retired.extend(
                self.scheduler
                    .retire_plugin_requests(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
            );
            effects.changed = true;
            let expired = if retain_data {
                self.scheduler
                    .invalidate_plugin(plugin_id, None)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?
            } else {
                self.scheduler
                    .forget_plugin(plugin_id)
                    .map_err(|_| PublicPluginManagementError::Unavailable)?
            };
            if let Some(expired) = expired.as_ref() {
                network_transition.invalidate_context(expired);
            }
            network_transition
                .set_plugin_authority(plugin_id, None)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
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
        let mut network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let expired = self
            .scheduler
            .invalidate_plugin(
                &plugin_id,
                Some((
                    runtime.generation,
                    runtime.activation_id,
                    runtime.admission_epoch,
                )),
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        if let Some(expired) = expired.as_ref() {
            network_transition.invalidate_context(expired);
        }
        network_transition
            .set_plugin_authority(&plugin_id, None)
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
        self.refresh_clipboard_history_authorization()?;
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
        let mut failed = settings
            .remove_command_hotkeys(&committed.plugin_id)
            .is_err();
        failed |= self
            .clipboard_history
            .uninstall(&committed.plugin_id, committed.retain_data)
            .is_err();
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

    pub(crate) fn diagnostics(
        &self,
        plugin_id: &str,
    ) -> Result<DiagnosticReport, PublicPluginManagementError> {
        if !valid_plugin_id(plugin_id) {
            return Err(PublicPluginManagementError::InvalidPackage);
        }
        let last_operation = self.observations.latest(plugin_id);
        let state = (|| -> Result<DiagnosticState, PublicPluginManagementError> {
            let _mutation = self.lock_mutation()?;
            let bundle = self.bundles.confirmed_bundle(plugin_id)?;
            let revision = self.state.inventory_revision()?.to_string();
            match self.state.config(plugin_id)?.filter(|c| c.installed) {
                None => Ok(DiagnosticState::NotInstalled {
                    inventory_revision: revision,
                }),
                Some(config) => {
                    if config.enabled
                        && config.fault.is_none()
                        && !bundle.as_ref().is_some_and(|b| b.config == config)
                    {
                        return Err(PublicPluginManagementError::Unavailable);
                    }
                    self.installed_manifest(&config)?;
                    Ok(DiagnosticState::Installed {
                        package_version: config.version,
                        generation: config.active_generation.to_string(),
                        inventory_revision: revision,
                        enabled: config.enabled,
                        fault: config.fault,
                    })
                }
            }
        })()
        .ok();
        let report = DiagnosticReport {
            schema_version: 1,
            host_version: env!("CARGO_PKG_VERSION").into(),
            platform: self.host.platform,
            plugin_id: plugin_id.into(),
            state,
            last_operation,
        };
        report.preview()?;
        Ok(report)
    }

    pub(crate) fn inventory(&self) -> Result<PublicPluginInventory, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let configs = self.state.configs()?;
        let active = self
            .bundles
            .bundles()?
            .into_iter()
            .map(|bundle| (bundle.config.plugin_id.clone(), bundle))
            .collect::<HashMap<_, _>>();
        let data = self.lock_data()?;
        let revision = self.state.inventory_revision()?;
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
            // Disabled replacements discard the probe Runtime. Load validated management
            // metadata without publishing a bundle, asset route, or execution authority.
            let installed_snapshot;
            let snapshot = match snapshot {
                Some(snapshot) => snapshot,
                None => {
                    installed_snapshot = self.installed_snapshot(&config)?;
                    &installed_snapshot
                }
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
                version: config.version.clone(),
                source: "localPackage",
                installation_directory: config.package_digest.as_deref().map(|digest| {
                    package_destination(&self.packages_root, &config.plugin_id, digest)
                        .to_string_lossy()
                        .into_owned()
                }),
                commands: config
                    .commands
                    .iter()
                    .map(|(id, command)| PublicPluginInventoryCommand {
                        command_id: id.clone(),
                        default_name: command.default_name.clone(),
                        effective_name: command.effective_name.clone(),
                        name_override: command.name_override.clone(),
                        favorite: command.favorite,
                        hotkey: None,
                        hotkey_status: "unset",
                    })
                    .collect(),
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
                network: snapshot.manifest.network.as_ref().map(|network| {
                    PublicPluginInventoryNetwork {
                        https_hosts: network.https_hosts.clone(),
                        credentials: super::state::manifest_credentials(&snapshot.manifest),
                    }
                }),
            });
        }
        items.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
        });
        Ok(PublicPluginInventory {
            hotkey_revision: "0".into(),
            revision: revision.to_string(),
            items,
        })
    }

    pub(crate) fn installed_command_names(
        &self,
    ) -> Result<Vec<String>, PublicPluginManagementError> {
        Ok(self
            .state
            .configs()?
            .into_iter()
            .filter(|config| config.installed)
            .flat_map(|config| {
                config
                    .commands
                    .into_values()
                    .map(|command| command.effective_name)
            })
            .collect())
    }

    pub(crate) fn replace_external_reserved_names(
        &self,
        names: impl IntoIterator<Item = String>,
    ) -> Result<(), PublicPluginManagementError> {
        self.state.replace_external_reserved_names(names)?;
        Ok(())
    }

    pub(crate) fn route(
        &self,
        query: &str,
    ) -> Result<Option<PublicPluginRoute>, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let Some(command) = query.strip_prefix('/') else {
            return Ok(None);
        };
        let (effective_name, input) = command
            .split_once(' ')
            .map_or((command, ""), |(name, body)| (name, body.trim()));
        if effective_name.is_empty() {
            return Ok(None);
        }
        self.route_matching(effective_name, input, None)
    }

    pub(crate) fn route_by_id(
        &self,
        plugin: &str,
        command: &str,
        input: &str,
    ) -> Result<Option<PublicPluginRoute>, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        self.route_matching("", input, Some((plugin, command)))
    }

    fn route_matching(
        &self,
        effective_name: &str,
        input: &str,
        stable: Option<(&str, &str)>,
    ) -> Result<Option<PublicPluginRoute>, PublicPluginManagementError> {
        for bundle in self.bundles.bundles()? {
            let config = &bundle.config;
            let snapshot = &bundle.runtime;
            let Some((command_id, command_config)) =
                config.commands.iter().find(|(id, command)| match stable {
                    Some((plugin, wanted)) => config.plugin_id == plugin && id.as_str() == wanted,
                    None => command.effective_name == effective_name,
                })
            else {
                continue;
            };
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
                || (stable.is_none() && command_config.effective_name != effective_name)
            {
                continue;
            }
            let command = snapshot
                .manifest
                .command_by_id(command_id)
                .ok_or(PublicPluginManagementError::Unavailable)?;
            let ui_permission = match command.output_mode {
                PublicOutputMode::Window => Some(PublicPermission::UiWindow),
                PublicOutputMode::Panel => Some(PublicPermission::UiPanel),
                PublicOutputMode::MainResult => None,
            };
            if ui_permission.is_some_and(|p| !config.permission_grants.contains(&p)) {
                continue;
            }
            // Capture publication authority from the same confirmed owner snapshot.
            let confirmed = self.bundles.confirmed_bundle(&config.plugin_id)?;
            if !confirmed
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &bundle))
            {
                continue;
            }
            #[cfg(test)]
            if let Some(hook) = self.before_result_route_token.lock().unwrap().clone() {
                hook();
            }
            let result_publication = if snapshot.manifest.runtime.result_actions {
                Some(
                    self.scheduler
                        .capture_result_publication(
                            &config.plugin_id,
                            snapshot.generation,
                            bundle.activation_id,
                            bundle.admission_epoch,
                        )
                        .map_err(|_| PublicPluginManagementError::Unavailable)?,
                )
            } else {
                None
            };
            return Ok(Some(PublicPluginRoute {
                result_publication,
                command_id: command_id.clone(),
                plugin_id: config.plugin_id.clone(),
                generation: snapshot.generation,
                activation_id: bundle.activation_id,
                admission_epoch: bundle.admission_epoch,
                runtime_recovery_needed: bundle.runtime_recovery_needed,
                runtime_label: snapshot.label.clone(),
                activation_mode: command.activation_mode,
                output_mode: command.output_mode,
                command_label: command_config.effective_name.clone(),
                input: input.to_owned(),
                input_required: command.input_required,
                input_placeholder: command.input_placeholder.clone(),
                window_entry: snapshot
                    .manifest
                    .window
                    .as_ref()
                    .map(|window| window.entry.clone()),
                panel_entry: snapshot
                    .manifest
                    .panel
                    .as_ref()
                    .map(|panel| panel.entry.clone()),
                host_keys: snapshot
                    .manifest
                    .panel
                    .as_ref()
                    .map(|panel| panel.canonical_host_keys())
                    .unwrap_or_default(),
                host_key_focus: snapshot
                    .manifest
                    .panel
                    .as_ref()
                    .map(|panel| panel.host_key_focus())
                    .unwrap_or(PanelHostKeyFocus::Content),
                icon_url: snapshot.icon_url(),
            }));
        }
        Ok(None)
    }

    pub(crate) fn panel_route(
        &self,
        plugin_id: &str,
        input: &str,
    ) -> Result<Option<PublicPluginRoute>, PublicPluginManagementError> {
        if !valid_plugin_id(plugin_id) || input.len() > 64 * 1024 || input.contains('\0') {
            return Ok(None);
        }
        for bundle in self.bundles.bundles()? {
            let config = &bundle.config;
            let snapshot = &bundle.runtime;
            let Some(command_config) = config.command("default") else {
                continue;
            };
            if self
                .owner_cleanup
                .is_blocked(&config.plugin_id)
                .map_err(|_| PublicPluginManagementError::Unavailable)?
            {
                continue;
            }
            if config.plugin_id != plugin_id
                || !config.installed
                || !config.enabled
                || config.fault.is_some()
                || config.active_generation != snapshot.generation
            {
                continue;
            }
            let Some(command) = snapshot.manifest.legacy_command() else {
                return Ok(None);
            };
            if !config
                .permission_grants
                .contains(&PublicPermission::UiPanel)
                || command.output_mode != PublicOutputMode::Panel
                || command.activation_mode != PublicActivationMode::Submit
            {
                return Ok(None);
            }
            return Ok(Some(PublicPluginRoute {
                result_publication: None,
                command_id: "default".into(),
                plugin_id: config.plugin_id.clone(),
                generation: snapshot.generation,
                activation_id: bundle.activation_id,
                admission_epoch: bundle.admission_epoch,
                runtime_recovery_needed: bundle.runtime_recovery_needed,
                runtime_label: snapshot.label.clone(),
                activation_mode: command.activation_mode,
                output_mode: command.output_mode,
                command_label: command_config.effective_name.clone(),
                input: input.to_owned(),
                input_required: command.input_required,
                input_placeholder: command.input_placeholder.clone(),
                window_entry: None,
                panel_entry: snapshot
                    .manifest
                    .panel
                    .as_ref()
                    .map(|panel| panel.entry.clone()),
                host_keys: snapshot
                    .manifest
                    .panel
                    .as_ref()
                    .map(|panel| panel.canonical_host_keys())
                    .unwrap_or_default(),
                host_key_focus: snapshot
                    .manifest
                    .panel
                    .as_ref()
                    .map(|panel| panel.host_key_focus())
                    .unwrap_or(PanelHostKeyFocus::Content),
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
            for (command_id, command_config) in &config.commands {
                if !config.installed
                    || !config.enabled
                    || config.fault.is_some()
                    || config.active_generation != snapshot.generation
                    || bundle.runtime_recovery_needed
                    || (!command_config.effective_name.starts_with(prefix)
                        && !snapshot
                            .manifest
                            .name
                            .to_lowercase()
                            .contains(&folded_prefix))
                {
                    continue;
                }
                let command = snapshot
                    .manifest
                    .command_by_id(command_id)
                    .ok_or(PublicPluginManagementError::Unavailable)?;
                suggestions.push(PublicCommandSuggestion {
                    command_id: command_id.clone(),
                    plugin_id: config.plugin_id.clone(),
                    effective_name: command_config.effective_name.clone(),
                    display_name: snapshot.manifest.name.clone(),
                    summary: command.summary.clone(),
                    icon_url: snapshot.icon_url(),
                    favorite: command_config.favorite,
                    input_required: command.input_required,
                    output_mode: command.output_mode,
                });
            }
        }
        suggestions.sort_by(|left, right| {
            right.favorite.cmp(&left.favorite).then_with(|| {
                left.effective_name
                    .cmp(&right.effective_name)
                    .then_with(|| left.plugin_id.cmp(&right.plugin_id))
                    .then_with(|| left.command_id.cmp(&right.command_id))
            })
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
            for (command_id, command_config) in &config.commands {
                if !config.installed
                    || !config.enabled
                    || config.fault.is_some()
                    || config.active_generation != snapshot.generation
                    || bundle.runtime_recovery_needed
                    || (!command_config.favorite
                        && !command_config
                            .effective_name
                            .to_lowercase()
                            .contains(&folded_query))
                {
                    continue;
                }
                let command = snapshot
                    .manifest
                    .command_by_id(command_id)
                    .ok_or(PublicPluginManagementError::Unavailable)?;
                suggestions.push(PublicCommandSuggestion {
                    command_id: command_id.clone(),
                    plugin_id: config.plugin_id.clone(),
                    effective_name: command_config.effective_name.clone(),
                    display_name: snapshot.manifest.name.clone(),
                    summary: command.summary.clone(),
                    icon_url: snapshot.icon_url(),
                    favorite: command_config.favorite,
                    input_required: command.input_required,
                    output_mode: command.output_mode,
                });
            }
        }
        suggestions.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| left.effective_name.cmp(&right.effective_name))
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
                .then_with(|| left.command_id.cmp(&right.command_id))
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
        let mut network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let Some(bundle) = self.bundles.confirmed_bundle(&candidate.plugin_id)? else {
            return Ok(false);
        };
        if self.state.config(&candidate.plugin_id)?.as_ref() != Some(&bundle.config) {
            return Err(PublicPluginManagementError::Unavailable);
        }
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
        let authority = checked_network_authority_snapshot(
            &self.bundles,
            &self.state,
            &self.secrets,
            &bundle.config,
            &bundle.runtime,
            bundle.activation_id,
            bundle.admission_epoch,
        )?;
        network_transition
            .set_plugin_authority(&candidate.plugin_id, authority)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
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
        self.output_mode_asset(
            plugin_id,
            request_path,
            PublicOutputMode::Window,
            PublicPermission::UiWindow,
        )
    }

    pub(crate) fn panel_asset(
        &self,
        plugin_id: &str,
        request_path: &str,
    ) -> Option<PublicRuntimeAsset> {
        self.output_mode_asset(
            plugin_id,
            request_path,
            PublicOutputMode::Panel,
            PublicPermission::UiPanel,
        )
    }

    fn output_mode_asset(
        &self,
        plugin_id: &str,
        request_path: &str,
        output_mode: PublicOutputMode,
        permission: PublicPermission,
    ) -> Option<PublicRuntimeAsset> {
        let bundle = self.bundles.bundle(plugin_id).ok()??;
        let snapshot = &bundle.runtime;
        let config = &bundle.config;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || config.active_generation != snapshot.generation
            || snapshot
                .manifest
                .legacy_command()
                .is_none_or(|command| command.output_mode != output_mode)
            || !snapshot.manifest.permissions.contains(&permission)
            || !config.permission_grants.contains(&permission)
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
                    })
                    .or_else(|| {
                        // Only management UI may read a discarded probe's installed icon.
                        if caller_label != "main" {
                            return None;
                        }
                        self.installed_snapshot(&config).ok().map(Arc::new)
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
                    && config
                        .permission_grants
                        .contains(&PublicPermission::UiWindow)
                    && snapshot
                        .manifest
                        .legacy_command()
                        .is_some_and(|command| command.output_mode == PublicOutputMode::Window)
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
        if !config
            .permission_grants
            .contains(&PublicPermission::UiWindow)
            || snapshot
                .manifest
                .legacy_command()
                .is_none_or(|command| command.output_mode != PublicOutputMode::Window)
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

    pub(crate) fn invocation_command_id(
        &self,
        request: &ScheduledPluginRequest,
    ) -> Result<Option<String>, PublicPluginManagementError> {
        let bundle = self
            .bundles
            .confirmed_bundle(&request.context.plugin_id)?
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if !bundle.config.installed
            || !bundle.config.enabled
            || bundle.config.fault.is_some()
            || bundle.runtime.generation != request.context.plugin_generation
            || bundle.activation_id != request.candidate.activation_id
            || bundle.admission_epoch != request.candidate.admission_epoch
            || bundle
                .config
                .command(&request.candidate.command_id)
                .is_none()
            || bundle
                .runtime
                .manifest
                .command_by_id(&request.candidate.command_id)
                .is_none()
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok(bundle
            .runtime
            .manifest
            .is_multi_command()
            .then(|| request.candidate.command_id.clone()))
    }

    pub(crate) fn bind_result_command_route(
        &self,
        saved: &PublicPluginRoute,
        context: &PluginRequestContext,
        submission_token: &str,
    ) -> Result<PublicPluginRoute, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let invalid = || PublicPluginManagementError::Unavailable;
        let bundle = self
            .bundles
            .confirmed_bundle(&saved.plugin_id)?
            .ok_or_else(invalid)?;
        let c = &bundle.config;
        let m = &bundle.runtime.manifest;
        if context.plugin_id != saved.plugin_id
            || context.plugin_generation < saved.generation
            || context.plugin_generation != bundle.runtime.generation
            || !c.installed
            || !c.enabled
            || c.fault.is_some()
            || c.active_generation != context.plugin_generation
            || bundle.runtime_recovery_needed
            || !m.runtime.result_actions
            || m.command_by_id(&saved.command_id)
                .is_none_or(|cmd| cmd.output_mode != PublicOutputMode::MainResult)
            || c.command(&saved.command_id)
                .is_none_or(|command| command.effective_name != saved.command_label)
        {
            return Err(invalid());
        }
        // Same-generation work may never acquire post-mutation authority.
        if context.plugin_generation == saved.generation {
            self.checked_result_owner(
                saved,
                super::result_action::ResultActionRequirements::default(),
            )
            .map_err(|_| invalid())?;
        }
        let token = self
            .scheduler
            .bound_command_publication(
                context,
                submission_token,
                &saved.command_id,
                &saved.input,
                bundle.activation_id,
                bundle.admission_epoch,
            )
            .map_err(|_| invalid())?;
        if context.plugin_generation == saved.generation
            && saved.result_publication.as_ref() != Some(&token)
        {
            return Err(invalid());
        }
        let mut route = saved.clone();
        route.generation = context.plugin_generation;
        route.activation_id = bundle.activation_id;
        route.admission_epoch = bundle.admission_epoch;
        route.runtime_label = bundle.runtime.label.clone();
        route.runtime_recovery_needed = false;
        route.result_publication = Some(token);
        Ok(route)
    }
    pub(crate) fn result_actions_enabled(&self, route: &PublicPluginRoute) -> bool {
        self.bundles
            .confirmed_bundle(&route.plugin_id)
            .ok()
            .flatten()
            .is_some_and(|b| b.runtime.manifest.runtime.result_actions)
    }
    // Caller holds the shared legacy admission read until publication/effect admission.
    // No earlier lock is acquired by the registry callback.
    fn checked_result_owner(
        &self,
        route: &PublicPluginRoute,
        requirements: super::result_action::ResultActionRequirements,
    ) -> Result<(), super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        let bundle = self
            .bundles
            .confirmed_bundle(&route.plugin_id)
            .map_err(|_| E::ActionUnavailable)?
            .ok_or(E::StaleRequest)?;
        let c = &bundle.config;
        let m = &bundle.runtime.manifest;
        if !route.result_publication.as_ref().is_some_and(|token| {
            self.scheduler
                .result_publication_current(&route.plugin_id, token)
        }) {
            return Err(E::StaleRequest);
        }
        if !c.installed
            || !c.enabled
            || c.fault.is_some()
            || c.active_generation != route.generation
            || bundle.runtime.generation != route.generation
            || bundle.activation_id != route.activation_id
            || bundle.admission_epoch != route.admission_epoch
            || !m.runtime.result_actions
            || c.command(&route.command_id).is_none()
            || m.command_by_id(&route.command_id)
                .is_none_or(|cmd| cmd.output_mode != PublicOutputMode::MainResult)
        {
            return Err(E::StaleRequest);
        }
        if requirements.copy
            && (!c
                .permission_grants
                .contains(&PublicPermission::ClipboardWrite)
                || !m.permissions.contains(&PublicPermission::ClipboardWrite))
        {
            return Err(E::PluginPermissionDenied);
        }
        if requirements.paste
            && (!c
                .permission_grants
                .contains(&PublicPermission::ClipboardPaste)
                || !m.permissions.contains(&PublicPermission::ClipboardPaste))
        {
            return Err(E::PluginPermissionDenied);
        }
        if requirements.url
            && (!c.permission_grants.contains(&PublicPermission::UrlOpen)
                || !m.permissions.contains(&PublicPermission::UrlOpen))
        {
            return Err(E::PluginPermissionDenied);
        }
        if bundle.runtime_recovery_needed {
            return Err(E::ActionBusy);
        }
        Ok(())
    }
    pub(crate) fn publish_result_actions<T>(
        &self,
        route: &PublicPluginRoute,
        requirements: super::result_action::ResultActionRequirements,
        publish: impl FnOnce() -> T,
    ) -> Result<T, super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        let _mutation = self.lock_mutation().map_err(|_| E::ActionUnavailable)?;
        self.checked_result_owner(route, requirements)?;
        self.scheduler.with_result_publication(
            &route.plugin_id,
            route.result_publication.as_ref().ok_or(E::StaleRequest)?,
            publish,
        )
    }
    pub(crate) fn copy_result_action(
        &self,
        registry: &crate::result_registry::ResultRegistry,
        origin: &super::result_action::ResultActionOrigin,
        copy: impl FnOnce(&str) -> Result<(), ()>,
    ) -> super::result_action::ResultActionOutcome {
        use super::result_action::*;
        let admitted = (|| {
            let _mutation = self
                .lock_mutation()
                .map_err(|_| ResultActionErrorCode::ActionUnavailable)?;
            self.checked_result_owner(&origin.route, origin.requirements)?;
            let PublicResultAction::CopyText { text, .. } = &origin.action else {
                return Err(ResultActionErrorCode::UnknownResult);
            };
            self.scheduler.with_result_publication(
                &origin.route.plugin_id,
                origin
                    .route
                    .result_publication
                    .as_ref()
                    .ok_or(ResultActionErrorCode::StaleRequest)?,
                || registry.claim_result_action(origin, None),
            )??;
            Ok(copy(text).map_err(|_| ResultActionErrorCode::ClipboardWriteFailed))
        })();
        match admitted {
            Err(code) => ResultActionOutcome::failed(
                code,
                matches!(
                    code,
                    ResultActionErrorCode::ActionAlreadyExecuted
                        | ResultActionErrorCode::ActionInProgress
                ),
            ),
            Ok(Ok(())) => ResultActionOutcome::Copied,
            Ok(Err(code)) => ResultActionOutcome::failed(code, true),
        }
    }
    pub(crate) fn write_paste_result_action(
        &self,
        registry: &crate::result_registry::ResultRegistry,
        origin: &super::result_action::ResultActionOrigin,
        preclaim: impl FnOnce() -> Result<(), super::result_action::ResultActionErrorCode>,
        write: impl FnOnce(&str) -> Result<u32, ()>,
    ) -> Result<u32, super::result_action::ResultActionOutcome> {
        use super::result_action::*;
        let admitted = (|| {
            let _mutation = self
                .lock_mutation()
                .map_err(|_| ResultActionErrorCode::ActionUnavailable)?;
            self.checked_result_owner(&origin.route, origin.requirements)?;
            let PublicResultAction::PasteText { text, .. } = &origin.action else {
                return Err(ResultActionErrorCode::UnknownResult);
            };
            preclaim()?;
            self.scheduler.with_result_publication(
                &origin.route.plugin_id,
                origin
                    .route
                    .result_publication
                    .as_ref()
                    .ok_or(ResultActionErrorCode::StaleRequest)?,
                || registry.claim_result_action(origin, None),
            )??;
            Ok(write(text))
        })();
        match admitted {
            Err(code) => Err(ResultActionOutcome::failed(
                code,
                matches!(
                    code,
                    ResultActionErrorCode::ActionAlreadyExecuted
                        | ResultActionErrorCode::ActionInProgress
                ),
            )),
            Ok(Err(())) => Err(ResultActionOutcome::failed(
                ResultActionErrorCode::ClipboardWriteFailed,
                true,
            )),
            Ok(Ok(sequence)) if sequence != 0 => Ok(sequence),
            Ok(Ok(_)) => Err(ResultActionOutcome::failed(
                ResultActionErrorCode::ClipboardWriteFailed,
                true,
            )),
        }
    }
    pub(crate) fn open_url_result_action(
        &self,
        registry: &crate::result_registry::ResultRegistry,
        origin: &super::result_action::ResultActionOrigin,
        open: impl FnOnce(url::Url) -> Result<(), ()>,
    ) -> super::result_action::ResultActionOutcome {
        use super::result_action::*;
        let admitted = (|| {
            let _mutation = self
                .lock_mutation()
                .map_err(|_| ResultActionErrorCode::ActionUnavailable)?;
            self.checked_result_owner(&origin.route, origin.requirements)?;
            let PublicResultAction::OpenUrl { url, .. } = &origin.action else {
                return Err(ResultActionErrorCode::UnknownResult);
            };
            let url =
                validate_open_url(url).map_err(|_| ResultActionErrorCode::ActionInvalidResponse)?;
            self.scheduler.with_result_publication(
                &origin.route.plugin_id,
                origin
                    .route
                    .result_publication
                    .as_ref()
                    .ok_or(ResultActionErrorCode::StaleRequest)?,
                || registry.claim_result_action(origin, None),
            )??;
            Ok(open(url).map_err(|_| ResultActionErrorCode::UrlOpenFailed))
        })();
        match admitted {
            Err(code) => ResultActionOutcome::failed(
                code,
                matches!(
                    code,
                    ResultActionErrorCode::ActionAlreadyExecuted
                        | ResultActionErrorCode::ActionInProgress
                ),
            ),
            Ok(Ok(())) => ResultActionOutcome::UrlOpened,
            Ok(Err(code)) => ResultActionOutcome::failed(code, true),
        }
    }
    pub(crate) fn reserve_result_action(
        &self,
        origin: &super::result_action::ResultActionOrigin,
    ) -> Result<ReservedPluginRequest, super::result_action::ResultActionErrorCode> {
        if !origin.lease.live()
            || origin
                .route
                .result_publication
                .as_ref()
                .is_some_and(|token| !token.live())
        {
            return Err(super::result_action::ResultActionErrorCode::StaleRequest);
        }
        self.scheduler
            .reserve(super::PluginRequestCandidate {
                command_id: origin.route.command_id.clone(),
                plugin_id: origin.route.plugin_id.clone(),
                plugin_generation: origin.route.generation,
                activation_id: origin.route.activation_id,
                admission_epoch: origin.route.admission_epoch,
                activation_mode: PublicActivationMode::Submit,
                input: origin.route.input.clone(),
                owner: super::PluginRequestOwner::ResultAction(origin.clone()),
            })
            .map_err(|_| super::result_action::ResultActionErrorCode::ActionUnavailable)
    }
    pub(crate) fn admit_result_action(
        &self,
        registry: &crate::result_registry::ResultRegistry,
        origin: &super::result_action::ResultActionOrigin,
        reserved: ReservedPluginRequest,
        now: Instant,
    ) -> Result<ScheduledPluginRequest, super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        let _mutation = self.lock_mutation().map_err(|_| E::ActionUnavailable)?;
        self.checked_result_owner(&origin.route, origin.requirements)?;
        let _transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| E::ActionUnavailable)?;
        let context = reserved.context.clone();
        self.scheduler.admit_result_action(reserved, now, || {
            registry.claim_result_action(origin, Some(context))
        })
    }
    pub(crate) fn finish_result_action(
        &self,
        registry: &crate::result_registry::ResultRegistry,
        origin: &super::result_action::ResultActionOrigin,
        context: &PluginRequestContext,
        prepared: crate::result_registry::PreparedResultActionSet,
        requirements: super::result_action::ResultActionRequirements,
    ) -> Result<crate::model::SearchResponse, super::result_action::ResultActionErrorCode> {
        use super::result_action::ResultActionErrorCode as E;
        let _mutation = self.lock_mutation().map_err(|_| E::ActionUnavailable)?;
        self.checked_result_owner(&origin.route, requirements)?;
        let _transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| E::ActionUnavailable)?;
        self.scheduler
            .finish_result_action(context, &origin.lease, || {
                registry
                    .replace_result_actions(origin, prepared)
                    .ok_or(E::StaleRequest)
            })
    }
    pub(crate) fn can_copy_text(&self, plugin_id: &str, command_id: &str, generation: u64) -> bool {
        self.bundles
            .confirmed_bundle(plugin_id)
            .ok()
            .flatten()
            .is_some_and(|bundle| {
                let config = &bundle.config;
                config.command(command_id).is_some()
                    && config.installed
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
        snapshot
            .manifest
            .legacy_command()
            .filter(|command| command.output_mode == PublicOutputMode::Window)
            .ok_or(TimerError::ExpiredWindowSessionError)?;
        let command_config = config
            .command("default")
            .ok_or(TimerError::ExpiredWindowSessionError)?;
        Ok((key, command_config.effective_name.clone(), alarm))
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
        let (admitted, mutation) = {
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
            (admitted, mutation)
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
        // Finish an admitted native publication before permission revocation can
        // return. Audio admission remains outside this guard and is cancelable.
        drop(mutation);
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
        if !self.delayed_messages.is_current(&message) {
            return None;
        }
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
        if !eligible {
            return None;
        }
        let outcome = self.message_center.commit_publish(MessagePublishRequest {
            plugin_id: message.plugin_id,
            plugin_name_snapshot: message.plugin_name_snapshot,
            content: message.content,
        });
        drop(mutation);
        match outcome {
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
        let network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PluginRuntimeError::Unavailable)?;
        let outcome = self
            .scheduler
            .complete(&completion.context, now)
            .map_err(
                |_| match self.scheduler.context_status(&completion.context) {
                    super::PluginContextStatus::Expired => PluginRuntimeError::ExpiredRequest,
                    _ => PluginRuntimeError::InvalidContext,
                },
            )?;
        network_transition.invalidate_context(&completion.context);
        Ok(outcome)
    }

    pub(crate) fn scheduler(&self) -> &Arc<PluginRequestScheduler> {
        &self.scheduler
    }

    pub(crate) async fn youdao_translate(
        &self,
        caller: PluginRuntimeIdentity,
        input: super::host_services::YoudaoCommandInput,
        store: &crate::credentials::CredentialStore,
    ) -> Result<super::host_services::YoudaoResponse, super::host_services::ServiceError> {
        use super::host_services::{network_code, parse_response, ServiceError};
        let prepare = || -> Result<_, &'static str> {
            input.input.validate()?;
            let _mutation = self.lock_mutation().map_err(|_| "expiredRequest")?;
            if caller.plugin_id != input.context.plugin_id
                || caller.generation != input.context.plugin_generation
            {
                return Err("expiredRequest");
            }
            let bundle = self
                .bundles
                .confirmed_bundle(&caller.plugin_id)
                .map_err(|_| "expiredRequest")?
                .ok_or("expiredRequest")?;
            if !bundle.config.installed
                || !bundle.config.enabled
                || bundle.config.fault.is_some()
                || bundle.runtime_recovery_needed
                || bundle.runtime.generation != caller.generation
            {
                return Err("expiredRequest");
            }
            if !bundle
                .runtime
                .manifest
                .permissions
                .contains(&PublicPermission::CredentialsYoudaoTranslation)
                || !bundle
                    .config
                    .permission_grants
                    .contains(&PublicPermission::CredentialsYoudaoTranslation)
            {
                return Err("servicePermissionDenied");
            }
            let manifest = &bundle.runtime.manifest;
            let extended_supported =
                super::manifest::parse_canonical_version(&manifest.minimum_host_version)
                    .is_some_and(|v| v >= [0, 18, 0]);
            let legacy = !manifest.runtime.result_actions
                && manifest
                    .legacy_command()
                    .is_some_and(|command| command.output_mode == PublicOutputMode::MainResult);
            let panel = manifest.translation_panel_compatible()
                && bundle
                    .config
                    .permission_grants
                    .contains(&PublicPermission::UiPanel);
            if !(legacy || panel) || (input.input.extended_input() && !extended_supported) {
                return Err("invalidServiceRequest");
            }
            if panel
                && !self
                    .scheduler
                    .with_current(&input.context, |request| request.is_panel_action())
                    .map_err(|_| "expiredRequest")?
            {
                return Err("invalidServiceRequest");
            }
            drop(_mutation);
            let gate = Arc::clone(&self.network_authority);
            let scheduler = Arc::clone(&self.scheduler);
            let context = input.context.clone();
            store.with_youdao(
                &input.context,
                self.scheduler
                    .capture_result_publication(
                        &caller.plugin_id,
                        caller.generation,
                        bundle.activation_id,
                        bundle.admission_epoch,
                    )
                    .map_err(|_| "expiredRequest")?,
                Box::new(move || {
                    if let Ok(transition) = gate.begin_transition() {
                        let _ = scheduler.retire_request_context(&context);
                        transition.invalidate_context(&context);
                    }
                }),
                |revision, values| {
                    let salt = format!(
                        "{}-{}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                        std::process::id()
                    );
                    let timestamp = chrono::Utc::now().timestamp().to_string();
                    let request = input.input.request(values, &salt, &timestamp)?;
                    let mut prepared = self
                        .network_authority
                        .admit(
                            &caller.plugin_id,
                            caller.generation,
                            &input.context,
                            self.scheduler.as_ref(),
                            request,
                        )
                        .map_err(network_code)?;
                    prepared.deny_redirects();
                    Ok((revision, prepared))
                },
            )
        };
        let (revision, prepared) = prepare().map_err(ServiceError::from)?;
        if let Some(response) = input.input.unchanged_response() {
            return store
                .with_revision(revision, || {
                    self.network_authority
                        .deliver_local(prepared, self.scheduler.as_ref(), response)
                        .map_err(network_code)
                })
                .map_err(ServiceError::from);
        }
        let pending = self
            .network_authority
            .broker()
            .execute(
                prepared,
                self.network_authority
                    .redirect_authority(Arc::clone(&self.scheduler)),
            )
            .await
            .map_err(network_code)
            .map_err(ServiceError::from)?;
        store
            .with_revision(revision, || {
                let response = self
                    .network_authority
                    .deliver(pending, self.scheduler.as_ref())
                    .map_err(network_code)?;
                parse_response(response.status, &response.body, &input.input)
            })
            .map_err(ServiceError::from)
    }

    pub(crate) async fn request_network(
        &self,
        caller: PluginRuntimeIdentity,
        input: PluginNetworkCommandInput,
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        self.network_authority
            .request(
                &caller.plugin_id,
                caller.generation,
                &input.context,
                Arc::clone(&self.scheduler),
                input.request,
                self.secrets.as_ref(),
            )
            .await
    }

    pub(crate) fn enqueue_reserved(
        &self,
        request: ReservedPluginRequest,
        now: Instant,
    ) -> Result<PluginScheduleOutcome, PluginScheduleError> {
        let network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PluginScheduleError::Unavailable)?;
        let outcome = self.scheduler.enqueue_reserved(request, now)?;
        if let PluginScheduleOutcome::Waiting { expired, .. } = &outcome {
            network_transition.invalidate_context(expired);
        }
        Ok(outcome)
    }

    pub(crate) fn files_open_enabled(
        &self,
        owner: &crate::plugin_window::PluginWindowOwner,
    ) -> bool {
        self.authorize_file_open(owner).is_ok()
    }

    /// No Runtime scheduling or data-call lease: a user can leave the native picker open indefinitely.
    pub(crate) fn authorize_file_open(
        &self,
        owner: &crate::plugin_window::PluginWindowOwner,
    ) -> Result<(), super::file_open::FileOpenError> {
        use super::file_open::FileOpenError as E;
        let _mutation = self.lock_mutation().map_err(|_| E::FileOpenFailedError)?;
        let b = self
            .bundles
            .confirmed_bundle(&owner.plugin_id)
            .map_err(|_| E::FileOpenFailedError)?
            .ok_or(E::ExpiredWindowSessionError)?;
        if !b.config.installed
            || !b.config.enabled
            || b.config.fault.is_some()
            || b.runtime_recovery_needed
            || b.config.active_generation != owner.plugin_generation
            || b.runtime.generation != owner.plugin_generation
            || b.activation_id != owner.activation_id
            || b.admission_epoch != owner.admission_epoch
        {
            return Err(E::ExpiredWindowSessionError);
        }
        if ![PublicPermission::UiWindow, PublicPermission::FilesOpenText]
            .iter()
            .all(|p| {
                b.config.permission_grants.contains(p) && b.runtime.manifest.permissions.contains(p)
            })
            || b.runtime.manifest.window.is_none()
            || !b.runtime.manifest.legacy_command().is_some_and(|c| {
                c.activation_mode == PublicActivationMode::Submit
                    && c.output_mode == PublicOutputMode::Window
            })
        {
            return Err(E::FileOpenPermissionError);
        }
        if self.host.platform != super::PublicPlatform::Windows {
            return Err(E::FileOpenUnsupportedError);
        }
        Ok(())
    }

    pub(crate) fn window_actions_enabled(
        &self,
        identity: &crate::plugin_window::WindowActionIdentity,
    ) -> bool {
        self.bundles
            .confirmed_bundle(&identity.owner.plugin_id)
            .ok()
            .flatten()
            .is_some_and(|b| {
                b.config.installed
                    && b.config.enabled
                    && b.config.fault.is_none()
                    && b.config.active_generation == identity.owner.plugin_generation
                    && !b.runtime_recovery_needed
                    && b.config
                        .permission_grants
                        .contains(&PublicPermission::UiWindow)
                    && b.runtime
                        .manifest
                        .permissions
                        .contains(&PublicPermission::UiWindow)
                    && b.runtime.generation == identity.owner.plugin_generation
                    && b.activation_id == identity.owner.activation_id
                    && b.admission_epoch == identity.owner.admission_epoch
                    && b.runtime
                        .manifest
                        .window
                        .as_ref()
                        .is_some_and(|p| p.runtime_actions)
            })
    }

    pub(crate) fn admit_window_action(
        &self,
        identity: &crate::plugin_window::WindowActionIdentity,
        request: super::window_action::WindowActionRequest,
        now: Instant,
    ) -> Result<ScheduledPluginRequest, super::window_action::WindowActionError> {
        use super::window_action::WindowActionError as E;
        let _mutation = self
            .lock_mutation()
            .map_err(|_| E::WindowRequestUnavailableError)?;
        let bundle = self
            .bundles
            .confirmed_bundle(&identity.owner.plugin_id)
            .map_err(|_| E::WindowRequestUnavailableError)?
            .ok_or(E::ExpiredWindowSessionError)?;
        let config = &bundle.config;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || config.active_generation != identity.owner.plugin_generation
            || bundle.runtime.generation != identity.owner.plugin_generation
            || bundle.activation_id != identity.owner.activation_id
            || bundle.admission_epoch != identity.owner.admission_epoch
            || !config
                .permission_grants
                .contains(&PublicPermission::UiWindow)
            || !bundle
                .runtime
                .manifest
                .permissions
                .contains(&PublicPermission::UiWindow)
            || !bundle
                .runtime
                .manifest
                .window
                .as_ref()
                .is_some_and(|p| p.runtime_actions)
        {
            return Err(E::ExpiredWindowSessionError);
        }
        if bundle.runtime_recovery_needed {
            return Err(E::WindowRequestBusyError);
        }
        let command = bundle
            .runtime
            .manifest
            .legacy_command()
            .ok_or(E::ExpiredWindowSessionError)?;
        if command.output_mode != PublicOutputMode::Window
            || command.activation_mode != PublicActivationMode::Submit
        {
            return Err(E::ExpiredWindowSessionError);
        }
        request
            .validate()
            .map_err(super::window_action::WindowActionError::from)?;
        let reserved = self
            .scheduler
            .reserve(super::PluginRequestCandidate {
                command_id: "default".into(),
                plugin_id: identity.owner.plugin_id.clone(),
                plugin_generation: identity.owner.plugin_generation,
                activation_id: identity.owner.activation_id,
                admission_epoch: identity.owner.admission_epoch,
                activation_mode: PublicActivationMode::Submit,
                input: String::new(),
                owner: super::PluginRequestOwner::WindowAction {
                    identity: identity.clone(),
                    request,
                },
            })
            .map_err(|_| E::WindowRequestUnavailableError)?;
        match self.enqueue_reserved(reserved, now) {
            Ok(PluginScheduleOutcome::Dispatched(request)) => Ok(request),
            Err(PluginScheduleError::Busy) => Err(E::WindowRequestBusyError),
            _ => Err(E::WindowRequestUnavailableError),
        }
    }

    pub(crate) fn panel_actions_enabled(
        &self,
        identity: &crate::plugin_panel::PanelSessionIdentity,
    ) -> bool {
        self.bundles
            .confirmed_bundle(&identity.plugin_id)
            .ok()
            .flatten()
            .is_some_and(|b| {
                b.config.installed
                    && b.config.enabled
                    && b.config.fault.is_none()
                    && b.config.active_generation == identity.generation
                    && !b.runtime_recovery_needed
                    && b.config
                        .permission_grants
                        .contains(&PublicPermission::UiPanel)
                    && b.runtime
                        .manifest
                        .permissions
                        .contains(&PublicPermission::UiPanel)
                    && b.runtime.generation == identity.generation
                    && b.activation_id == identity.activation_id
                    && b.admission_epoch == identity.admission_epoch
                    && b.runtime
                        .manifest
                        .panel
                        .as_ref()
                        .is_some_and(|p| p.runtime_actions)
            })
    }

    pub(crate) fn admit_panel_action(
        &self,
        identity: &crate::plugin_panel::PanelSessionIdentity,
        request: super::panel_action::PanelActionRequest,
        now: Instant,
    ) -> Result<ScheduledPluginRequest, super::panel_action::PanelActionError> {
        use super::panel_action::PanelActionError as E;
        let _mutation = self
            .lock_mutation()
            .map_err(|_| E::PanelRequestUnavailableError)?;
        let bundle = self
            .bundles
            .confirmed_bundle(&identity.plugin_id)
            .map_err(|_| E::PanelRequestUnavailableError)?
            .ok_or(E::ExpiredPanelSession)?;
        let config = &bundle.config;
        if !config.installed
            || !config.enabled
            || config.fault.is_some()
            || config.active_generation != identity.generation
            || bundle.runtime.generation != identity.generation
            || bundle.activation_id != identity.activation_id
            || bundle.admission_epoch != identity.admission_epoch
            || !config
                .permission_grants
                .contains(&PublicPermission::UiPanel)
            || !bundle
                .runtime
                .manifest
                .permissions
                .contains(&PublicPermission::UiPanel)
            || !bundle
                .runtime
                .manifest
                .panel
                .as_ref()
                .is_some_and(|p| p.runtime_actions)
        {
            return Err(E::ExpiredPanelSession);
        }
        if bundle.runtime_recovery_needed {
            return Err(E::PanelRequestBusyError);
        }
        let command = bundle
            .runtime
            .manifest
            .legacy_command()
            .ok_or(E::ExpiredPanelSession)?;
        if command.output_mode != PublicOutputMode::Panel
            || command.activation_mode != PublicActivationMode::Submit
        {
            return Err(E::ExpiredPanelSession);
        }
        request.validate()?;
        let reserved = self
            .scheduler
            .reserve(super::PluginRequestCandidate {
                command_id: "default".into(),
                plugin_id: identity.plugin_id.clone(),
                plugin_generation: identity.generation,
                activation_id: identity.activation_id,
                admission_epoch: identity.admission_epoch,
                activation_mode: PublicActivationMode::Submit,
                input: String::new(),
                owner: super::PluginRequestOwner::PanelAction {
                    content_label: identity.content_label.clone(),
                    session_epoch: identity.session_epoch,
                    request,
                },
            })
            .map_err(|_| E::PanelRequestUnavailableError)?;
        match self.enqueue_reserved(reserved, now) {
            Ok(PluginScheduleOutcome::Dispatched(request)) => Ok(request),
            Err(PluginScheduleError::Busy) => Err(E::PanelRequestBusyError),
            _ => Err(E::PanelRequestUnavailableError),
        }
    }

    pub(crate) fn discard_panel_completion(&self, context: &PluginRequestContext) {
        let _transition = self.network_authority.begin_transition();
        self.scheduler.discard_panel_completion(context);
    }

    pub(crate) fn claim_panel_completion(
        &self,
        identity: &crate::plugin_panel::PanelSessionIdentity,
        context: &PluginRequestContext,
    ) -> Result<(), super::panel_action::PanelActionError> {
        use super::panel_action::PanelActionError as E;
        let _mutation = self
            .lock_mutation()
            .map_err(|_| E::PanelRequestUnavailableError)?;
        if !self.panel_actions_enabled(identity) {
            return Err(E::ExpiredPanelSession);
        }
        let _transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| E::PanelRequestUnavailableError)?;
        self.scheduler.claim_panel_completion(context)
    }

    pub(crate) fn claim_window_completion(
        &self,
        identity: &crate::plugin_window::WindowActionIdentity,
        context: &PluginRequestContext,
    ) -> Result<(), super::content_action::ContentActionError> {
        use super::content_action::ContentActionError as E;
        let _mutation = self.lock_mutation().map_err(|_| E::Unavailable)?;
        if !self.window_actions_enabled(identity) {
            return Err(E::Expired);
        }
        let _transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| E::Unavailable)?;
        self.scheduler.claim_action_completion(context)
    }

    pub(crate) fn retire_panel_action(&self, context: &PluginRequestContext) {
        if let Ok(transition) = self.network_authority.begin_transition() {
            let _ = self.scheduler.retire_context(context);
            transition.invalidate_context(context);
        } else {
            // Network poisoning already rejects network calls; storage authority must also close.
            let _ = self.scheduler.retire_context(context);
        }
    }

    pub(crate) fn cancel_scheduled_request(
        &self,
        context: &PluginRequestContext,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PluginScheduleError> {
        let network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PluginScheduleError::Unavailable)?;
        let next = self.scheduler.cancel(context, now)?;
        network_transition.invalidate_context(context);
        Ok(next)
    }

    pub(crate) fn expire_network_timeouts(
        &self,
        now: Instant,
    ) -> Result<Vec<PluginRuntimeReplacement>, PluginScheduleError> {
        let mut network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PluginScheduleError::Unavailable)?;
        let replacements = self.scheduler.expire_timeouts(now)?;
        for replacement in &replacements {
            network_transition.invalidate_runtime(
                &replacement.plugin_id,
                replacement.previous_generation,
                replacement.previous_activation_id,
                replacement.previous_admission_epoch,
            );
        }
        Ok(replacements)
    }

    pub(crate) fn finish_runtime_replacement(
        &self,
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let mut network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let bundle = self
            .bundles
            .confirmed_bundle(plugin_id)?
            .filter(|bundle| {
                bundle.runtime.generation == generation
                    && bundle.activation_id == activation_id
                    && bundle.admission_epoch == admission_epoch
            })
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let authority = checked_network_authority_snapshot(
            &self.bundles,
            &self.state,
            &self.secrets,
            &bundle.config,
            &bundle.runtime,
            activation_id,
            admission_epoch,
        )?;
        let next = self
            .scheduler
            .runtime_replaced(plugin_id, generation, activation_id, admission_epoch, now)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        network_transition
            .set_plugin_authority(plugin_id, authority)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(next)
    }

    pub(crate) fn invalidate_runtime_generation(&self, plugin_id: &str, generation: u64) -> usize {
        self.network_authority
            .begin_transition()
            .map_or(0, |mut transition| {
                transition.invalidate_generation(plugin_id, generation)
            })
    }

    pub(crate) fn shutdown_network(&self) -> usize {
        self.scheduler.close_result_publications();
        self.network_authority.shutdown()
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
        let mut network_transition = self
            .network_authority
            .begin_transition()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let expired = self
            .scheduler
            .invalidate_plugin(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        if let Some(expired) = expired.as_ref() {
            network_transition.invalidate_context(expired);
        }
        network_transition
            .set_plugin_authority(plugin_id, None)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let _ = self.close_runtime_data_gate(&runtime);
        self.cancel_delayed_messages(plugin_id);
        let timer_effects = self.cancel_timer_generation(plugin_id, generation, activation_id);
        self.state.publish_prepared(prepared_state)?;
        reservation.publish(None)?;
        drop(_mutation);
        self.apply_timer_post_lock_effects(timer_effects);
        self.refresh_clipboard_history_authorization()?;
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

    fn installed_manifest(
        &self,
        config: &EffectivePluginConfig,
    ) -> Result<PublicManifestV1, PublicPluginManagementError> {
        Ok(self.installed_snapshot(config)?.manifest)
    }

    fn installed_snapshot(
        &self,
        config: &EffectivePluginConfig,
    ) -> Result<RuntimeSnapshot, PublicPluginManagementError> {
        let digest = config
            .package_digest
            .as_deref()
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let package_root = package_destination(&self.packages_root, &config.plugin_id, digest);
        let (snapshot, _) =
            load_runtime_snapshot(&package_root, &self.host, digest, config.active_generation)?;
        if snapshot.manifest.plugin_id != config.plugin_id
            || snapshot.manifest.version != config.version
            || snapshot.generation != config.active_generation
            || snapshot.digest != digest
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok(snapshot)
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
        self.make_state_durable_with_effects(reservation, prepared, None)
    }

    fn make_state_durable_with_effects(
        &self,
        reservation: &ActivationReservation,
        prepared: &PreparedStateCommit,
        effects: Option<&mut ManagementEffects>,
    ) -> Result<(), PublicPluginManagementError> {
        self.make_state_durable_observed(reservation, prepared, effects, &mut None)
    }
    fn make_state_durable_observed(
        &self,
        reservation: &ActivationReservation,
        prepared: &PreparedStateCommit,
        mut effects: Option<&mut ManagementEffects>,
        observation: &mut Option<diagnostics::Attempt<'_>>,
    ) -> Result<(), PublicPluginManagementError> {
        diagnostics::phase(observation, Phase::Persistence, CommitOutcome::NotAttempted);
        reservation.begin_committing()?;
        diagnostics::phase(observation, Phase::Persistence, CommitOutcome::Unknown);
        match self.state.persist_prepared(prepared) {
            DurableStateOutcome::Committed => {
                diagnostics::phase(observation, Phase::Publication, CommitOutcome::Committed);
                reservation.mark_durable()?;
                #[cfg(test)]
                if let Some(hook) = self.after_state_durable.lock().unwrap().clone() {
                    hook();
                }
                Ok(())
            }
            DurableStateOutcome::NotCommitted => {
                if !self.state.revalidate_previous(prepared) {
                    self.close_network_authority_with_effects(
                        prepared.plugin_id(),
                        effects.as_deref_mut(),
                    );
                    return Err(PublicPluginManagementError::Unavailable);
                }
                diagnostics::phase(observation, Phase::Persistence, CommitOutcome::NotCommitted);
                reservation.rollback_not_committed()?;
                Err(PublicPluginManagementError::Unavailable)
            }
            DurableStateOutcome::Unknown => {
                self.close_network_authority_with_effects(
                    prepared.plugin_id(),
                    effects.as_deref_mut(),
                );
                Err(PublicPluginManagementError::Unavailable)
            }
        }
    }

    fn close_network_authority_with_effects(
        &self,
        plugin_id: &str,
        effects: Option<&mut ManagementEffects>,
    ) {
        if let Some(effects) = effects {
            // A poisoned mutation mutex must not prevent fail-closed cleanup.
            let _mutation = self
                .mutation
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Ok(mut transition) = self.network_authority.begin_transition() {
                if let Ok(retired) = self.scheduler.retire_plugin_requests(plugin_id) {
                    effects.retired.extend(retired);
                }
                effects.changed = true;
                let _ = transition.set_plugin_authority(plugin_id, None);
            } else {
                effects.changed = true;
                if let Ok(retired) = self.scheduler.retire_plugin_requests(plugin_id) {
                    effects.retired.extend(retired);
                }
                self.close_network_authority(plugin_id);
            }
        } else {
            self.close_network_authority(plugin_id);
        }
        #[cfg(test)]
        if let Some(hook) = self.after_uncertain_close.lock().unwrap().clone() {
            hook();
        }
    }

    fn close_network_authority(&self, plugin_id: &str) {
        self.bundles.revoke_speech(plugin_id);
        self.network_authority.close_plugin(plugin_id);
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

fn network_authorization_matches(
    config: &EffectivePluginConfig,
    manifest: &PublicManifestV1,
) -> bool {
    let declared_hosts = manifest
        .network
        .as_ref()
        .map(|network| network.https_hosts.iter().cloned().collect::<BTreeSet<_>>());
    match declared_hosts {
        Some(declared_hosts)
            if config
                .permission_grants
                .contains(&PublicPermission::NetworkHttps) =>
        {
            config.network_https_hosts_grant == declared_hosts
                && config.network_credentials_grant == super::state::manifest_credentials(manifest)
        }
        Some(_) => {
            config.network_https_hosts_grant.is_empty()
                && config.network_credentials_grant.is_empty()
        }
        None => {
            !config
                .permission_grants
                .contains(&PublicPermission::NetworkHttps)
                && config.network_https_hosts_grant.is_empty()
                && config.network_credentials_grant.is_empty()
        }
    }
}

fn clipboard_history_is_authorized(config: &EffectivePluginConfig) -> bool {
    config.installed
        && config.enabled
        && config.fault.is_none()
        && config
            .permission_grants
            .contains(&PublicPermission::ClipboardHistoryRead)
}

fn validate_settings_updates(
    definitions: &[PublicSettingV1],
    updates: &BTreeMap<String, Value>,
) -> Result<(), PublicPluginManagementError> {
    for (key, value) in updates {
        let definition = definitions
            .iter()
            .find(|definition| definition.key() == key)
            .ok_or(PluginStateError::InvalidSetting)?;
        if definition.is_secret() || !definition.accepts_value(value) {
            return Err(PluginStateError::InvalidSetting.into());
        }
    }
    Ok(())
}

fn checked_network_authority_snapshot(
    bundles: &ActivationReservationBook,
    state: &PluginStateStore,
    secrets: &PluginSecretStore,
    config: &EffectivePluginConfig,
    runtime: &RuntimeSnapshot,
    activation_id: u64,
    admission_epoch: u64,
) -> Result<Option<PluginNetworkAuthoritySnapshot>, PublicPluginManagementError> {
    let Some(bundle) = bundles.confirmed_bundle(&config.plugin_id)? else {
        return Ok(None);
    };
    if bundle.config != *config
        || state.config(&config.plugin_id)?.as_ref() != Some(config)
        || bundle.runtime.generation != runtime.generation
        || bundle.runtime.digest != runtime.digest
        || bundle.activation_id != activation_id
        || bundle.admission_epoch != admission_epoch
        || config.package_digest.as_deref() != Some(runtime.digest.as_str())
    {
        return Err(PublicPluginManagementError::Unavailable);
    }
    let Some(grant) = config.network_grant_snapshot(&runtime.manifest) else {
        return Ok(None);
    };
    if grant.generation != runtime.generation {
        return Err(PublicPluginManagementError::Unavailable);
    }
    for key in super::state::manifest_credentials(&runtime.manifest)
        .iter()
        .map(|credential| credential.setting_key.as_str())
        .collect::<BTreeSet<_>>()
    {
        if secrets.read_bearer(&config.plugin_id, key).is_err() {
            return Ok(None);
        }
    }
    PluginNetworkAuthoritySnapshot::new(
        &grant.plugin_id,
        grant.generation,
        activation_id,
        admission_epoch,
        grant.https_hosts,
    )
    .map(|mut snapshot| {
        snapshot.credentials = grant.credentials;
        Some(snapshot)
    })
    .map_err(|_| PublicPluginManagementError::Unavailable)
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
    let response = parse_data_response(context, value)?;
    Ok(PublicWindowResponse {
        request_id: response.request_id,
        data: response.data,
    })
}

pub(crate) fn parse_panel_response(
    context: &PluginRequestContext,
    value: Value,
) -> Result<PublicPanelResponse, PluginRuntimeError> {
    if value
        .as_object()
        .is_some_and(|object| object.contains_key("results"))
    {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    let response = parse_data_response(context, value)?;
    Ok(PublicPanelResponse {
        request_id: response.request_id,
        data: response.data,
    })
}

fn parse_data_response(
    context: &PluginRequestContext,
    value: Value,
) -> Result<WindowResponseWire, PluginRuntimeError> {
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
    Ok(response)
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
        network::{PluginNetworkErrorCode, PluginNetworkRequest, PluginNetworkRequestMethod},
        scheduler::{
            PluginContextStatus, PluginRequestCandidate, PluginScheduleOutcome,
            PluginSubmissionOwner,
        },
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

    fn change_permission(
        manager: &PublicPluginManager,
        permission: PublicPermission,
        granted: bool,
    ) -> ManagementEffects {
        let mut effects = ManagementEffects::default();
        manager
            .set_permission_access_with_effects(
                "com.example.timer",
                permission,
                granted,
                &manager.inventory().unwrap().revision,
                &mut effects,
            )
            .unwrap();
        effects
    }

    #[test]
    fn permission_access_revocation_expires_timer_and_window_authority_without_other_grants() {
        for permission in [
            PublicPermission::UiWindow,
            PublicPermission::NotificationsPublish,
            PublicPermission::TimerControl,
        ] {
            let dir = TestDir::new("permission-access-timer");
            write_timer_package(&dir.source(), "1.0.0");
            let clock = Arc::new(TestClock::default());
            let manager = timer_manager(&dir, clock.clone(), Arc::new(TestPublisher::successful()));
            let generation = install_timer(&manager, &dir.source());
            let (key, ticket) = start_due_timer(&manager, &clock, generation);
            let before = manager.state.config("com.example.timer").unwrap().unwrap();
            assert!(change_permission(&manager, permission, false).changed);
            let after = manager.state.config("com.example.timer").unwrap().unwrap();
            let mut expected = before.permission_grants.clone();
            expected.remove(&permission);
            assert_eq!(after.permission_grants, expected);
            assert_eq!(after.enabled, before.enabled);
            assert!(manager
                .window_timer_get_state("com.example.timer", generation, key.activation_id)
                .is_err());
            let dispatch = manager.commit_timer_claim(ticket);
            assert!(dispatch.effect.is_none());
            if permission == PublicPermission::UiWindow {
                assert!(manager.route("/timer").unwrap().is_none());
                assert!(manager.window_identity("com.example.timer").is_err());
                assert!(manager
                    .window_asset("com.example.timer", "/dist/window.html")
                    .is_none());
            }
            change_permission(&manager, permission, true);
            assert_eq!(
                manager
                    .state
                    .config("com.example.timer")
                    .unwrap()
                    .unwrap()
                    .permission_grants,
                before.permission_grants
            );
        }
    }

    #[test]
    fn permission_access_rejects_stale_undeclared_and_unsupported_and_manages_disabled() {
        let dir = TestDir::new("permission-access-validation");
        write_timer_package(&dir.source(), "1.0.0");
        let manager = manager(&dir);
        install_timer(&manager, &dir.source());
        let revision = manager.inventory().unwrap().revision;
        for (permission, revision) in [
            (PublicPermission::ClipboardWrite, revision.as_str()),
            (PublicPermission::ClipboardRead, revision.as_str()),
            (PublicPermission::TimerControl, "0"),
        ] {
            assert!(manager
                .set_permission_access_with_effects(
                    "com.example.timer",
                    permission,
                    false,
                    revision,
                    &mut ManagementEffects::default()
                )
                .is_err());
        }
        manager
            .set_enabled_with_readiness("com.example.timer", false, |_| true)
            .unwrap();
        change_permission(&manager, PublicPermission::TimerControl, false);
        change_permission(&manager, PublicPermission::TimerControl, true);
        assert!(
            !manager
                .state
                .config("com.example.timer")
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn permission_access_network_requires_revision_for_revoke_and_retires_old_requests() {
        let dir = TestDir::new("permission-access-network");
        let manager = installed_credential_manager(&dir);
        let running = dispatch_network_scheduled(&manager);
        let revision = manager.inventory().unwrap().revision;
        let mut effects = ManagementEffects::default();
        assert!(manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::NetworkHttps,
                false,
                "0",
                &mut effects
            )
            .is_err());
        assert!(!effects.changed);
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::NetworkHttps,
                false,
                &revision,
                &mut effects,
            )
            .unwrap();
        assert!(effects.changed);
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Expired
        );
        let config = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(config.network_https_hosts_grant.is_empty());
        assert!(config.network_credentials_grant.is_empty());
    }

    #[test]
    fn permission_access_popped_notification_is_not_revived_by_regrant() {
        let dir = TestDir::new("permission-access-popped-message");
        write_notification_package(&dir.source(), "1.0.0");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let installed = manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                prepared.permissions.iter().copied().collect(),
                now,
                |_| true,
            )
            .unwrap();
        schedule_test_message(&manager, installed.runtime.generation, "old", now);
        let old = manager
            .delayed_messages
            .claim_due(now + Duration::from_secs(2))
            .unwrap()
            .unwrap();
        for granted in [false, true] {
            manager
                .set_permission_access_with_effects(
                    "com.example.activation",
                    PublicPermission::NotificationsPublish,
                    granted,
                    &manager.inventory().unwrap().revision,
                    &mut ManagementEffects::default(),
                )
                .unwrap();
        }
        assert!(manager.commit_scheduled_message(old).is_none());
        schedule_test_message(&manager, installed.runtime.generation, "new", now);
        let fresh = manager
            .delayed_messages
            .claim_due(now + Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert!(manager.commit_scheduled_message(fresh).is_some());
    }

    #[test]
    fn permission_access_concurrent_plugins_reject_stale_without_terminal_publication() {
        let dir = TestDir::new("permission-access-concurrent");
        write_timer_package(&dir.source(), "1.0.0");
        let manager = Arc::new(manager(&dir));
        install_timer(&manager, &dir.source());
        let network = dir.path().join("network-source");
        write_network_package(&network, "1.0.0", &["example.com"]);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&network), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                prepared.permissions.iter().copied().collect(),
                now,
                |_| true,
            )
            .unwrap();
        let revision = manager.inventory().unwrap().revision;
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let once = std::sync::atomic::AtomicBool::new(false);
        *manager.after_state_durable.lock().unwrap() = Some(Arc::new(move || {
            if !once.swap(true, Ordering::SeqCst) {
                entered_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
        }));
        let first = std::thread::spawn({
            let manager = manager.clone();
            let revision = revision.clone();
            move || {
                manager.set_permission_access_with_effects(
                    "com.example.timer",
                    PublicPermission::TimerControl,
                    false,
                    &revision,
                    &mut ManagementEffects::default(),
                )
            }
        });
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let (started_tx, started) = std::sync::mpsc::channel();
        let second = std::thread::spawn({
            let manager = manager.clone();
            move || {
                started_tx.send(()).unwrap();
                manager.set_permission_access_with_effects(
                    "com.example.network",
                    PublicPermission::NetworkHttps,
                    false,
                    &revision,
                    &mut ManagementEffects::default(),
                )
            }
        });
        started.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        release.send(()).unwrap();
        assert!(first.join().unwrap().is_ok());
        assert!(second.join().unwrap().is_err());
        assert!(
            manager.inventory().is_ok(),
            "stale concurrent toggles must not make management terminal"
        );
    }

    #[test]
    fn permission_access_timer_revoke_waits_for_admitted_publish() {
        struct BlockingPublisher {
            entered: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl MessagePublisher for BlockingPublisher {
            fn is_available(&self) -> bool {
                true
            }
            fn commit_publish(&self, _: MessagePublishRequest) -> MessagePublishOutcome {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                MessagePublishOutcome::Unavailable
            }
        }
        let dir = TestDir::new("permission-access-publish-fence");
        write_timer_package(&dir.source(), "1.0.0");
        let clock = Arc::new(TestClock::default());
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let manager = Arc::new(
            PublicPluginManager::load_with_timer_dependencies(
                dir.path(),
                PublicPluginHost::current(PublicPlatform::Windows),
                [],
                Arc::new(MessageCenterService::load(dir.path())),
                clock.clone(),
                Arc::new(BlockingPublisher {
                    entered: entered_tx,
                    release: Mutex::new(release_rx),
                }),
            )
            .unwrap(),
        );
        let generation = install_timer(&manager, &dir.source());
        let (_, ticket) = start_due_timer(&manager, &clock, generation);
        let publishing = std::thread::spawn({
            let manager = manager.clone();
            move || manager.commit_timer_claim(ticket)
        });
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let (finished_tx, finished) = std::sync::mpsc::channel();
        let revoking = std::thread::spawn({
            let manager = manager.clone();
            move || {
                change_permission(&manager, PublicPermission::NotificationsPublish, false);
                finished_tx.send(()).unwrap();
            }
        });
        let returned_before_publish = finished.recv_timeout(Duration::from_millis(100)).is_ok();
        release.send(()).unwrap();
        publishing.join().unwrap();
        revoking.join().unwrap();
        assert!(
            !returned_before_publish,
            "revocation returned while an admitted notification was still publishing"
        );
    }

    #[test]
    fn permission_access_restart_refresh_update_preserve_revocation_but_require_new_consent() {
        let dir = TestDir::new("permission-access-update");
        write_timer_package(&dir.source(), "1.0.0");
        let manager = manager(&dir);
        install_timer(&manager, &dir.source());
        change_permission(&manager, PublicPermission::TimerControl, false);
        drop(manager);
        let manager = self::manager(&dir);
        assert!(!manager
            .state
            .config("com.example.timer")
            .unwrap()
            .unwrap()
            .permission_grants
            .contains(&PublicPermission::TimerControl));
        // Development refresh retains version and must not reset revoked grants.
        fs::write(
            dir.source().join("dist/runtime.js"),
            "export async function onCommand() { return { data: { refreshed: true } }; }",
        )
        .unwrap();
        let now = Instant::now();
        let refreshed = manager
            .prepare_targeted(
                "main",
                source(&dir.source()),
                Some(PublicPluginRefreshTarget {
                    plugin_id: "com.example.timer".into(),
                    expected_inventory_revision: manager.inventory().unwrap().revision,
                }),
                now,
            )
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &refreshed.token,
                refreshed.permissions.iter().copied().collect(),
                now,
                |_| true,
            )
            .unwrap();
        assert!(!manager
            .state
            .config("com.example.timer")
            .unwrap()
            .unwrap()
            .permission_grants
            .contains(&PublicPermission::TimerControl));
        write_timer_package(&dir.source(), "1.1.0");
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("clipboard.write"));
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert!(manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiWindow,
                    PublicPermission::NotificationsPublish,
                    PublicPermission::TimerControl
                ]),
                now,
                |_| true
            )
            .is_err());
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                prepared.permissions.iter().copied().collect(),
                now,
                |_| true,
            )
            .unwrap();
        let config = manager.state.config("com.example.timer").unwrap().unwrap();
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::TimerControl));
        assert!(config
            .permission_grants
            .contains(&PublicPermission::ClipboardWrite));
    }

    #[test]
    fn permission_access_failure_rolls_back_or_retires_authority() {
        for failure in ["candidate", "unknown", "publication"] {
            let dir = TestDir::new("permission-access-failure");
            write_timer_package(&dir.source(), "1.0.0");
            let manager = Arc::new(manager(&dir));
            let generation = install_timer(&manager, &dir.source());
            let revision = manager.inventory().unwrap().revision;
            let owner = dir.path().join("public-plugins/state/com.example.timer");
            if failure == "candidate" {
                fs::create_dir(owner.join("state.json.backup")).unwrap();
            } else if failure == "unknown" {
                fs::remove_file(owner.join("state.json")).unwrap();
                fs::create_dir(owner.join("state.json")).unwrap();
            } else {
                *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
                    let weak = Arc::downgrade(&manager);
                    move || {
                        let manager = weak.upgrade().unwrap();
                        let _ = std::thread::spawn(move || {
                            let _guard = manager.mutation.lock().unwrap();
                            panic!("synthetic permission publication failure");
                        })
                        .join();
                    }
                }));
            }
            let mut effects = ManagementEffects::default();
            assert!(manager
                .set_permission_access_with_effects(
                    "com.example.timer",
                    PublicPermission::TimerControl,
                    false,
                    &revision,
                    &mut effects
                )
                .is_err());
            if failure == "candidate" {
                assert!(!effects.changed);
                assert!(manager
                    .state
                    .config("com.example.timer")
                    .unwrap()
                    .unwrap()
                    .permission_grants
                    .contains(&PublicPermission::TimerControl));
                assert!(manager.route("/timer").unwrap().is_some());
            } else {
                assert!(effects.changed);
                assert!(manager
                    .window_timer_get_state("com.example.timer", generation, 1)
                    .is_err());
                assert!(manager.route("/timer").is_err());
            }
        }
    }

    #[test]
    fn permission_access_failed_history_revoke_stops_capture_without_deleting_history() {
        for failure in ["unknown", "publication"] {
            let dir = TestDir::new("permission-access-history-failure");
            write_clipboard_history_package_with_permissions(
                &dir.source(),
                "1.0.0",
                vec!["ui.panel", "clipboard.history.read"],
            );
            let manager = Arc::new(manager(&dir));
            let now = Instant::now();
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    prepared.permissions.iter().copied().collect(),
                    now,
                    |_| true,
                )
                .unwrap();
            let plugin_id = "com.example.clipboard-history";
            manager
                .clipboard_history_capture_for_test(ClipboardCapture::text(
                    "retained",
                    "2026-10-30T01:00:00Z",
                ))
                .unwrap();
            let before = manager
                .clipboard_history_snapshot_for_test(plugin_id)
                .unwrap();
            assert!(manager
                .clipboard_history_authorized_plugins_for_test()
                .contains(&plugin_id.into()));
            let revision = manager.inventory().unwrap().revision;
            if failure == "unknown" {
                let current = dir
                    .path()
                    .join("public-plugins/state")
                    .join(plugin_id)
                    .join("state.json");
                fs::remove_file(&current).unwrap();
                fs::create_dir(&current).unwrap();
            } else {
                *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
                    let weak = Arc::downgrade(&manager);
                    move || {
                        let manager = weak.upgrade().unwrap();
                        let _ = std::thread::spawn(move || {
                            let _guard = manager.mutation.lock().unwrap();
                            panic!("synthetic history permission publication failure");
                        })
                        .join();
                    }
                }));
            }
            assert!(manager
                .set_permission_access_with_effects(
                    plugin_id,
                    PublicPermission::ClipboardHistoryRead,
                    false,
                    &revision,
                    &mut ManagementEffects::default()
                )
                .is_err());
            assert!(!manager
                .clipboard_history_authorized_plugins_for_test()
                .contains(&plugin_id.into()));
            manager
                .clipboard_history_capture_for_test(ClipboardCapture::text(
                    "must-not-capture",
                    "2026-10-30T02:00:00Z",
                ))
                .unwrap();
            assert_eq!(
                manager
                    .clipboard_history_snapshot_for_test(plugin_id)
                    .unwrap(),
                before
            );
        }
    }

    fn manager_with_network_authority(
        dir: &TestDir,
        network_authority: Arc<PluginNetworkAuthorityGate>,
    ) -> PublicPluginManager {
        PublicPluginManager::load_with_timer_and_network_dependencies(
            dir.path(),
            PublicPluginHost::current(PublicPlatform::Windows),
            ["find".into(), "math".into()],
            Arc::new(MessageCenterService::load(dir.path())),
            Arc::new(TestClock::default()),
            Arc::new(TestPublisher::successful()),
            network_authority,
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

    fn write_network_package(root: &Path, version: &str, hosts: &[&str]) {
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pluginId": "com.example.network",
                "version": version,
                "apiVersion": 1,
                "minimumHostVersion": "0.3.2",
                "name": "Network",
                "supportedPlatforms": ["windows"],
                "command": {
                    "defaultName": "network",
                    "activationMode": "live",
                    "outputMode": "mainResult",
                    "inputRequired": false
                },
                "runtime": { "entry": "dist/runtime.js" },
                "network": { "httpsHosts": hosts },
                "permissions": ["network.https"]
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

    fn write_clipboard_history_package(root: &Path, version: &str, include_history_read: bool) {
        fs::create_dir_all(root.join("dist")).unwrap();
        let permissions = if include_history_read {
            vec!["ui.panel", "clipboard.history.read"]
        } else {
            vec!["ui.panel"]
        };
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pluginId": "com.example.clipboard-history",
                "version": version,
                "apiVersion": 1,
                "minimumHostVersion": "0.3.3",
                "name": "Clipboard History",
                "supportedPlatforms": ["windows"],
                "command": {
                    "defaultName": "cliphist",
                    "activationMode": "submit",
                    "outputMode": "panel",
                    "inputRequired": false
                },
                "runtime": { "entry": "dist/runtime.js" },
                "panel": { "entry": "dist/panel.html" },
                "permissions": permissions
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("dist/runtime.js"),
            "export async function onCommand() { return { requestId: 'r', data: {} }; }",
        )
        .unwrap();
        fs::write(root.join("dist/panel.html"), "<!doctype html>").unwrap();
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

    fn write_clipboard_history_package_with_permissions(
        root: &Path,
        version: &str,
        permissions: Vec<&str>,
    ) {
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pluginId": "com.example.clipboard-history",
                "version": version,
                "apiVersion": 1,
                "minimumHostVersion": if permissions.contains(&"clipboard.history.copy") { "0.17.0" } else { "0.3.3" },
                "name": "Clipboard History",
                "supportedPlatforms": ["windows"],
                "command": {
                    "defaultName": "cliphist",
                    "activationMode": "submit",
                    "outputMode": "panel",
                    "inputRequired": false
                },
                "runtime": { "entry": "dist/runtime.js" },
                "panel": { "entry": "dist/panel.html" },
                "permissions": permissions
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("dist/runtime.js"),
            "export async function onCommand() { return { requestId: 'r', data: {} }; }",
        )
        .unwrap();
        fs::write(root.join("dist/panel.html"), "<!doctype html>").unwrap();
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

    fn dispatch_network_scheduled(manager: &PublicPluginManager) -> ScheduledPluginRequest {
        let bundle = manager
            .bundles
            .bundle("com.example.network")
            .unwrap()
            .unwrap();
        let outcome = manager
            .scheduler
            .enqueue(
                PluginRequestCandidate {
                    command_id: "default".into(),
                    plugin_id: "com.example.network".into(),
                    plugin_generation: bundle.runtime.generation,
                    activation_id: bundle.activation_id,
                    admission_epoch: bundle.admission_epoch,
                    activation_mode: PublicActivationMode::Live,
                    input: String::new(),
                    owner: crate::public_plugins::PluginRequestOwner::Command(
                        PluginSubmissionOwner {
                            ui_intent_epoch: 1,
                            control_value: String::new(),
                            submission_token: format!("network-{}", bundle.admission_epoch),
                        },
                    ),
                },
                Instant::now(),
            )
            .unwrap();
        let PluginScheduleOutcome::Dispatched(request) = outcome else {
            panic!("network request must dispatch");
        };
        request
    }

    fn dispatch_network_request(manager: &PublicPluginManager) -> PluginRequestContext {
        dispatch_network_scheduled(manager).context
    }

    fn network_request() -> PluginNetworkRequest {
        PluginNetworkRequest {
            url: "https://api.example.com/data".into(),
            method: PluginNetworkRequestMethod::Get,
            headers: None,
            body: None,
            credential: None,
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
    fn plugin_network_management_prepare_update_revoke_and_regrant_are_authoritative() {
        let dir = TestDir::new("network-grant-flow");
        let manager = manager(&dir);
        let now = Instant::now();
        let grants = BTreeSet::from([PublicPermission::NetworkHttps]);

        write_network_package(
            &dir.source(),
            "1.0.0",
            &["auth.example.com", "api.example.com"],
        );
        let cancelled = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            cancelled.network,
            Some(PublicPluginPrepareNetwork {
                credentials: vec![],
                added_credential_ids: vec![],
                https_hosts: vec!["api.example.com".into(), "auth.example.com".into()],
                added_https_hosts: vec!["api.example.com".into(), "auth.example.com".into()],
                requires_network_consent: true,
            })
        );
        manager.cancel("main", &cancelled.token, now).unwrap();
        assert!(manager
            .state
            .config("com.example.network")
            .unwrap()
            .is_none());

        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, grants.clone(), now, |_| true)
            .unwrap();
        let installed = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert_eq!(
            installed.network_https_hosts_grant,
            BTreeSet::from(["api.example.com".into(), "auth.example.com".into()])
        );

        write_network_package(
            &dir.source(),
            "1.0.1",
            &["api.example.com", "auth.example.com"],
        );
        let stale = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .set_network_access("com.example.network", false)
            .unwrap();
        assert_eq!(
            manager.commit_with_readiness("main", &stale.token, grants.clone(), now, |_| true,),
            Err(PublicPluginManagementError::Unavailable)
        );
        assert_eq!(
            manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );
        write_network_package(&dir.source(), "1.1.0", &["api.example.com"]);
        let narrowed = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            narrowed.network,
            Some(PublicPluginPrepareNetwork {
                credentials: vec![],
                added_credential_ids: vec![],
                https_hosts: vec!["api.example.com".into()],
                added_https_hosts: Vec::new(),
                requires_network_consent: false,
            })
        );
        manager
            .commit_with_readiness("main", &narrowed.token, grants.clone(), now, |_| true)
            .unwrap();
        let still_revoked = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert_eq!(still_revoked.version, "1.1.0");
        assert!(!still_revoked
            .permission_grants
            .contains(&PublicPermission::NetworkHttps));
        assert!(still_revoked.network_https_hosts_grant.is_empty());

        manager
            .set_network_access("com.example.network", true)
            .unwrap();
        let regranted = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert_eq!(
            regranted.network_https_hosts_grant,
            BTreeSet::from(["api.example.com".into()])
        );
        let inventory = manager.inventory().unwrap();
        assert_eq!(
            inventory.items[0].network,
            Some(PublicPluginInventoryNetwork {
                credentials: vec![],
                https_hosts: vec!["api.example.com".into()]
            })
        );

        write_network_package(
            &dir.source(),
            "1.2.0",
            &["new.example.com", "api.example.com"],
        );
        let expanded = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            expanded.network.as_ref().unwrap().added_https_hosts,
            vec!["new.example.com"]
        );
        assert!(expanded.network.as_ref().unwrap().requires_network_consent);
        manager.cancel("main", &expanded.token, now).unwrap();
        assert_eq!(
            manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap(),
            regranted
        );

        let failed = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            manager.commit_with_readiness("main", &failed.token, grants, now, |_| false),
            Err(PublicPluginManagementError::RuntimeNotReady)
        );
        assert_eq!(
            manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap(),
            regranted
        );
    }

    #[test]
    fn plugin_network_lifecycle_revoke_expires_old_context_and_regrant_admits_new_context() {
        let dir = TestDir::new("network-lifecycle-revoke");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();

        let old_context = dispatch_network_request(&manager);
        let old_call = manager
            .network_authority
            .admit(
                "com.example.network",
                old_context.plugin_generation,
                &old_context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();

        manager
            .set_network_access("com.example.network", false)
            .unwrap();
        assert!(matches!(
            manager.network_authority.admit(
                "com.example.network",
                old_context.plugin_generation,
                &old_context,
                &manager.scheduler,
                network_request(),
            ),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        ));
        drop(old_call);

        manager
            .set_network_access("com.example.network", true)
            .unwrap();
        assert!(
            !manager
                .scheduler
                .complete(&old_context, now)
                .unwrap()
                .accepted
        );
        let new_context = dispatch_network_request(&manager);
        let admitted = manager.network_authority.admit(
            "com.example.network",
            new_context.plugin_generation,
            &new_context,
            &manager.scheduler,
            network_request(),
        );
        assert!(admitted.is_ok());
    }

    #[test]
    fn clipboard_history_lifecycle_tracks_install_update_disable_and_fault() {
        let dir = TestDir::new("clipboard-history-lifecycle");
        write_clipboard_history_package(&dir.source(), "1.0.0", true);
        let manager = manager(&dir);
        let now = Instant::now();
        let grants = BTreeSet::from([
            PublicPermission::UiPanel,
            PublicPermission::ClipboardHistoryRead,
        ]);
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, grants, now, |_| true)
            .unwrap();
        assert_eq!(
            manager.clipboard_history_authorized_plugins_for_test(),
            vec!["com.example.clipboard-history"]
        );
        manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "stored",
                "2026-08-30T01:00:00Z",
            ))
            .unwrap();
        assert_eq!(
            manager
                .clipboard_history_snapshot_for_test("com.example.clipboard-history")
                .unwrap()
                .entries
                .len(),
            1
        );

        write_clipboard_history_package(&dir.source(), "1.0.1", false);
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::UiPanel]),
                now,
                |_| true,
            )
            .unwrap();
        assert!(manager
            .clipboard_history_authorized_plugins_for_test()
            .is_empty());
        manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "not-authorized",
                "2026-08-30T01:00:01Z",
            ))
            .unwrap();
        assert_eq!(
            manager
                .clipboard_history_snapshot_for_test("com.example.clipboard-history")
                .unwrap()
                .entries
                .len(),
            1
        );

        write_clipboard_history_package(&dir.source(), "1.0.2", true);
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        manager
            .set_enabled_with_readiness("com.example.clipboard-history", false, |_| true)
            .unwrap();
        assert!(manager
            .clipboard_history_authorized_plugins_for_test()
            .is_empty());
        manager
            .set_enabled_with_readiness("com.example.clipboard-history", true, |_| true)
            .unwrap();
        assert_eq!(
            manager.clipboard_history_authorized_plugins_for_test(),
            vec!["com.example.clipboard-history"]
        );
        manager
            .disable_plugin_for_fault(
                "com.example.clipboard-history",
                PublicPluginFault::RuntimeUnavailable,
            )
            .unwrap();
        assert!(manager
            .clipboard_history_authorized_plugins_for_test()
            .is_empty());
    }

    #[test]
    fn clipboard_history_uninstall_retains_or_deletes_plugin_history_data() {
        let dir = TestDir::new("clipboard-history-uninstall");
        write_clipboard_history_package(&dir.source(), "1.0.0", true);
        let retained_manager = manager(&dir);
        let now = Instant::now();
        let grants = BTreeSet::from([
            PublicPermission::UiPanel,
            PublicPermission::ClipboardHistoryRead,
        ]);
        let prepared = retained_manager
            .prepare("main", source(&dir.source()), now)
            .unwrap();
        retained_manager
            .commit_with_readiness("main", &prepared.token, grants.clone(), now, |_| true)
            .unwrap();
        retained_manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "kept",
                "2026-08-30T01:00:00Z",
            ))
            .unwrap();
        let store_path =
            retained_manager.clipboard_history_store_path_for_test("com.example.clipboard-history");
        assert!(store_path.join("index.json").exists());

        retained_manager
            .uninstall("com.example.clipboard-history", true)
            .unwrap();
        assert!(retained_manager
            .clipboard_history_authorized_plugins_for_test()
            .is_empty());
        assert!(store_path.join("index.json").exists());

        let delete_dir = TestDir::new("clipboard-history-complete-uninstall");
        write_clipboard_history_package(&delete_dir.source(), "1.0.0", true);
        let delete_manager = manager(&delete_dir);
        let prepared = delete_manager
            .prepare("main", source(&delete_dir.source()), now)
            .unwrap();
        delete_manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        delete_manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "deleted",
                "2026-08-30T01:00:00Z",
            ))
            .unwrap();
        let deleted_store_path =
            delete_manager.clipboard_history_store_path_for_test("com.example.clipboard-history");
        assert!(deleted_store_path.join("index.json").exists());
        delete_manager
            .uninstall("com.example.clipboard-history", false)
            .unwrap();
        assert!(!deleted_store_path.exists());
    }

    #[test]
    fn clipboard_history_panel_bridge_revalidates_identity_and_permission() {
        let dir = TestDir::new("clipboard-history-panel-bridge");
        write_clipboard_history_package(&dir.source(), "1.0.0", true);
        let manager = manager(&dir);
        let now = Instant::now();
        manager
            .commit_with_readiness(
                "main",
                &manager
                    .prepare("main", source(&dir.source()), now)
                    .unwrap()
                    .token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        let plugin_id = "com.example.clipboard-history";
        let generation = 1;
        let activation_id = manager.activation_id(plugin_id).unwrap();
        let admission_epoch = manager.admission_epoch(plugin_id).unwrap();
        manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "panel-visible-preview",
                "2026-08-30T01:00:00Z",
            ))
            .unwrap();

        assert_eq!(
            manager
                .clipboard_history_snapshot_for_panel(
                    plugin_id,
                    generation,
                    activation_id,
                    admission_epoch
                )
                .unwrap()
                .entries
                .len(),
            1
        );
        assert_eq!(
            manager.clipboard_history_snapshot_for_panel(
                plugin_id,
                generation + 1,
                activation_id,
                admission_epoch
            ),
            Err(ClipboardHistoryBridgeError::ExpiredPanelSession)
        );

        write_clipboard_history_package(&dir.source(), "1.0.1", false);
        manager
            .commit_with_readiness(
                "main",
                &manager
                    .prepare("main", source(&dir.source()), now)
                    .unwrap()
                    .token,
                BTreeSet::from([PublicPermission::UiPanel]),
                now,
                |_| true,
            )
            .unwrap();
        let generation = 2;
        let activation_id = manager.activation_id(plugin_id).unwrap();
        let admission_epoch = manager.admission_epoch(plugin_id).unwrap();
        assert_eq!(
            manager.clipboard_history_snapshot_for_panel(
                plugin_id,
                generation,
                activation_id,
                admission_epoch
            ),
            Err(ClipboardHistoryBridgeError::PermissionDenied)
        );
    }

    #[test]
    fn permission_access_queued_history_paste_rechecks_each_grant_before_effect() {
        for permission in [
            PublicPermission::UiPanel,
            PublicPermission::ClipboardHistoryRead,
            PublicPermission::ClipboardHistoryPaste,
        ] {
            let dir = TestDir::new("permission-access-queued-history-paste");
            write_clipboard_history_package_with_permissions(
                &dir.source(),
                "1.0.0",
                vec![
                    "ui.panel",
                    "clipboard.history.read",
                    "clipboard.history.paste",
                ],
            );
            let manager = manager(&dir);
            let now = Instant::now();
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([
                        PublicPermission::UiPanel,
                        PublicPermission::ClipboardHistoryRead,
                        PublicPermission::ClipboardHistoryPaste,
                    ]),
                    now,
                    |_| true,
                )
                .unwrap();
            let plugin_id = "com.example.clipboard-history";
            let activation_id = manager.activation_id(plugin_id).unwrap();
            let admission_epoch = manager.admission_epoch(plugin_id).unwrap();
            let effects = std::cell::Cell::new(0);
            let revision = manager
                .with_clipboard_history_paste_authorization(
                    plugin_id,
                    1,
                    activation_id,
                    admission_epoch,
                    None,
                    Ok,
                )
                .unwrap();
            let execute = || {
                manager.with_clipboard_history_paste_authorization(
                    plugin_id,
                    1,
                    activation_id,
                    admission_epoch,
                    Some(revision),
                    |_| {
                        effects.set(effects.get() + 1);
                        Ok(())
                    },
                )
            };
            execute().unwrap();
            manager
                .set_permission_access_with_effects(
                    plugin_id,
                    permission,
                    false,
                    &manager.inventory().unwrap().revision,
                    &mut ManagementEffects::default(),
                )
                .unwrap();
            assert!(
                execute().is_err(),
                "queued effect must reject revoked {permission:?}"
            );
            assert_eq!(
                effects.get(),
                1,
                "revoked grant must prevent the native effect"
            );
            let grants = manager
                .state
                .config(plugin_id)
                .unwrap()
                .unwrap()
                .permission_grants;
            assert_eq!(grants.len(), 2, "unrelated grants remain present");
            manager
                .set_permission_access_with_effects(
                    plugin_id,
                    permission,
                    true,
                    &manager.inventory().unwrap().revision,
                    &mut ManagementEffects::default(),
                )
                .unwrap();
            assert!(
                execute().is_err(),
                "regrant must not revive the queued native effect"
            );
            assert_eq!(effects.get(), 1);
            assert!(manager
                .with_clipboard_history_paste_authorization(
                    plugin_id,
                    1,
                    activation_id,
                    admission_epoch,
                    None,
                    |_| Ok(())
                )
                .is_ok());
        }
    }

    #[test]
    fn panel_text_effects_check_independent_grants_and_revoke_authorization() {
        use crate::panel_effects::EffectError as E;
        let dir = TestDir::new("panel-text-authorization");
        write_clipboard_history_package_with_permissions(
            &dir.source(),
            "1.0.0",
            vec!["ui.panel", "clipboard.write", "speech.speak"],
        );
        let path = dir.source().join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["minimumHostVersion"] = json!("0.18.0");
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::UiPanel, PublicPermission::ClipboardWrite]),
                now,
                |_| true,
            )
            .unwrap();
        let id = "com.example.clipboard-history";
        let identity = crate::plugin_panel::PanelSessionIdentity {
            session_epoch: 1,
            plugin_id: id.into(),
            generation: 1,
            activation_id: manager.activation_id(id).unwrap(),
            admission_epoch: manager.admission_epoch(id).unwrap(),
            command_label: "cliphist".into(),
            content_label: "content".into(),
            host_keys: vec![],
            host_key_focus: PanelHostKeyFocus::Content,
        };
        assert_eq!(
            manager.with_panel_text_authorization(
                &identity,
                PublicPermission::SpeechSpeak,
                |_| Ok(())
            ),
            Err(E::PermissionDenied)
        );
        assert_eq!(
            manager.with_panel_text_authorization(
                &identity,
                PublicPermission::ClipboardWrite,
                |_| Err::<(), _>(E::ClipboardWriteFailed)
            ),
            Err(E::ClipboardWriteFailed)
        );
        let change = |permission, granted| {
            manager
                .set_permission_access_with_effects(
                    id,
                    permission,
                    granted,
                    &manager.inventory().unwrap().revision,
                    &mut ManagementEffects::default(),
                )
                .unwrap()
        };
        change(PublicPermission::SpeechSpeak, true);
        let lease = manager
            .with_panel_text_authorization(&identity, PublicPermission::SpeechSpeak, Ok)
            .unwrap();
        assert!(lease.valid());
        // No-op edits preserve current authorization; real revocation does not.
        change(PublicPermission::SpeechSpeak, true);
        assert!(lease.valid());
        change(PublicPermission::SpeechSpeak, false);
        assert!(!lease.valid());
        assert_eq!(
            manager.with_panel_text_authorization(
                &identity,
                PublicPermission::SpeechSpeak,
                |_| Ok(())
            ),
            Err(E::PermissionDenied)
        );
        change(PublicPermission::ClipboardWrite, false);
        assert_eq!(
            manager.with_panel_text_authorization(
                &identity,
                PublicPermission::ClipboardWrite,
                |_| Ok(())
            ),
            Err(E::PermissionDenied)
        );
    }

    #[test]
    fn clipboard_history_copy_requires_explicit_grant_and_rechecks_revocation() {
        let dir = TestDir::new("clipboard-copy-authorization");
        write_clipboard_history_package_with_permissions(
            &dir.source(),
            "1.0.0",
            vec![
                "ui.panel",
                "clipboard.history.read",
                "clipboard.history.copy",
            ],
        );
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        let plugin_id = "com.example.clipboard-history";
        let activation = manager.activation_id(plugin_id).unwrap();
        let admission = manager.admission_epoch(plugin_id).unwrap();
        let effects = std::cell::Cell::new(0);
        let execute = |generation, activation, admission| {
            manager.with_clipboard_history_copy_authorization(
                plugin_id,
                generation,
                activation,
                admission,
                || {
                    effects.set(effects.get() + 1);
                    Ok(())
                },
            )
        };
        assert_eq!(
            execute(1, activation, admission),
            Err(ClipboardHistoryPasteError::PermissionDenied)
        );
        manager
            .set_permission_access_with_effects(
                plugin_id,
                PublicPermission::ClipboardHistoryCopy,
                true,
                &manager.inventory().unwrap().revision,
                &mut ManagementEffects::default(),
            )
            .unwrap();
        execute(1, activation, admission).unwrap();
        assert_eq!(
            execute(2, activation, admission),
            Err(ClipboardHistoryPasteError::ExpiredPanelSession)
        );
        manager
            .set_permission_access_with_effects(
                plugin_id,
                PublicPermission::ClipboardHistoryCopy,
                false,
                &manager.inventory().unwrap().revision,
                &mut ManagementEffects::default(),
            )
            .unwrap();
        assert_eq!(
            execute(1, activation, admission),
            Err(ClipboardHistoryPasteError::PermissionDenied)
        );
        assert_eq!(effects.get(), 1);
    }

    #[test]
    fn clipboard_history_copy_is_not_automatically_granted_on_update() {
        let dir = TestDir::new("clipboard-copy-upgrade");
        write_clipboard_history_package_with_permissions(
            &dir.source(),
            "1.0.0",
            vec![
                "ui.panel",
                "clipboard.history.read",
                "clipboard.history.paste",
            ],
        );
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                    PublicPermission::ClipboardHistoryPaste,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        write_clipboard_history_package_with_permissions(
            &dir.source(),
            "1.1.0",
            vec![
                "ui.panel",
                "clipboard.history.read",
                "clipboard.history.copy",
            ],
        );
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        let config = manager
            .state
            .config("com.example.clipboard-history")
            .unwrap()
            .unwrap();
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::ClipboardHistoryCopy));
        assert!(config
            .permission_grants
            .contains(&PublicPermission::ClipboardHistoryRead));
    }

    #[test]
    fn clipboard_history_paste_panel_bridge_requires_read_and_paste_permissions() {
        let dir = TestDir::new("clipboard-history-paste-panel-bridge");
        write_clipboard_history_package_with_permissions(
            &dir.source(),
            "1.0.0",
            vec![
                "ui.panel",
                "clipboard.history.read",
                "clipboard.history.paste",
            ],
        );
        let full_manager = manager(&dir);
        let now = Instant::now();
        full_manager
            .commit_with_readiness(
                "main",
                &full_manager
                    .prepare("main", source(&dir.source()), now)
                    .unwrap()
                    .token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                    PublicPermission::ClipboardHistoryPaste,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        let plugin_id = "com.example.clipboard-history";
        let generation = 1;
        let activation_id = full_manager.activation_id(plugin_id).unwrap();
        let admission_epoch = full_manager.admission_epoch(plugin_id).unwrap();
        full_manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "panel paste text",
                "2026-08-30T01:00:00Z",
            ))
            .unwrap();
        let id = full_manager
            .clipboard_history_snapshot_for_panel(
                plugin_id,
                generation,
                activation_id,
                admission_epoch,
            )
            .unwrap()
            .entries[0]
            .id();

        assert_eq!(
            full_manager
                .clipboard_history_record_for_paste_panel(
                    plugin_id,
                    generation,
                    activation_id,
                    admission_epoch,
                    &id
                )
                .unwrap()
                .payload,
            crate::clipboard_history::ClipboardHistoryRecordPayload::Text {
                text: "panel paste text".into()
            }
        );
        assert_eq!(
            full_manager.clipboard_history_record_for_paste_panel(
                plugin_id,
                generation + 1,
                activation_id,
                admission_epoch,
                &id,
            ),
            Err(crate::clipboard_history::ClipboardHistoryPasteError::ExpiredPanelSession)
        );

        let read_only_dir = TestDir::new("clipboard-history-paste-read-only-panel-bridge");
        write_clipboard_history_package_with_permissions(
            &read_only_dir.source(),
            "1.0.0",
            vec!["ui.panel", "clipboard.history.read"],
        );
        let read_only_manager = manager(&read_only_dir);
        read_only_manager
            .commit_with_readiness(
                "main",
                &read_only_manager
                    .prepare("main", source(&read_only_dir.source()), now)
                    .unwrap()
                    .token,
                BTreeSet::from([
                    PublicPermission::UiPanel,
                    PublicPermission::ClipboardHistoryRead,
                ]),
                now,
                |_| true,
            )
            .unwrap();
        read_only_manager
            .clipboard_history_capture_for_test(ClipboardCapture::text(
                "read only",
                "2026-08-30T01:00:01Z",
            ))
            .unwrap();
        let activation_id = read_only_manager.activation_id(plugin_id).unwrap();
        let admission_epoch = read_only_manager.admission_epoch(plugin_id).unwrap();
        let read_only_id = read_only_manager
            .clipboard_history_snapshot_for_panel(plugin_id, 1, activation_id, admission_epoch)
            .unwrap()
            .entries[0]
            .id();
        assert_eq!(
            read_only_manager.clipboard_history_record_for_paste_panel(
                plugin_id,
                1,
                activation_id,
                admission_epoch,
                &read_only_id,
            ),
            Err(crate::clipboard_history::ClipboardHistoryPasteError::PermissionDenied)
        );
    }

    #[derive(Clone, Copy, Debug)]
    enum TestedNetworkLifecycleTransition {
        Completion,
        Failure,
        Update,
        Disable,
        FaultDisable,
        Uninstall,
    }

    #[tokio::test]
    async fn plugin_network_lifecycle_terminal_manager_paths_cancel_registered_call() {
        for transition in [
            TestedNetworkLifecycleTransition::Completion,
            TestedNetworkLifecycleTransition::Failure,
            TestedNetworkLifecycleTransition::Update,
            TestedNetworkLifecycleTransition::Disable,
            TestedNetworkLifecycleTransition::FaultDisable,
            TestedNetworkLifecycleTransition::Uninstall,
        ] {
            let dir = TestDir::new(&format!("network-lifecycle-{transition:?}"));
            write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
            let (network_authority, transport_started) =
                PluginNetworkAuthorityGate::blocking_for_test();
            let manager = manager_with_network_authority(&dir, network_authority);
            let now = Instant::now();
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            let commit = manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
            let context = dispatch_network_request(&manager);
            let call = manager
                .network_authority
                .admit(
                    "com.example.network",
                    context.plugin_generation,
                    &context,
                    &manager.scheduler,
                    network_request(),
                )
                .unwrap();
            assert_eq!(manager.network_authority.global_active_for_test(), 1);
            let cancellation = call.cancellation_witness_for_test();
            let pending = tokio::spawn({
                let network_authority = Arc::clone(&manager.network_authority);
                let scheduler = Arc::clone(&manager.scheduler);
                async move { network_authority.execute_for_test(call, scheduler).await }
            });
            tokio::time::timeout(Duration::from_millis(500), transport_started.notified())
                .await
                .unwrap();

            match transition {
                TestedNetworkLifecycleTransition::Completion
                | TestedNetworkLifecycleTransition::Failure => {
                    manager
                        .complete(
                            &commit.runtime.label,
                            &PluginCommandCompletion {
                                context,
                                response: None,
                                failed: matches!(
                                    transition,
                                    TestedNetworkLifecycleTransition::Failure
                                ),
                            },
                            now,
                        )
                        .unwrap();
                }
                TestedNetworkLifecycleTransition::Update => {
                    write_network_package(&dir.source(), "1.1.0", &["api.example.com"]);
                    let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
                    manager
                        .commit_with_readiness(
                            "main",
                            &prepared.token,
                            BTreeSet::from([PublicPermission::NetworkHttps]),
                            now,
                            |_| true,
                        )
                        .unwrap();
                }
                TestedNetworkLifecycleTransition::Disable => {
                    manager
                        .set_enabled_with_readiness("com.example.network", false, |_| true)
                        .unwrap();
                }
                TestedNetworkLifecycleTransition::FaultDisable => {
                    manager
                        .disable_plugin_for_fault(
                            "com.example.network",
                            PublicPluginFault::RuntimeUnavailable,
                        )
                        .unwrap();
                }
                TestedNetworkLifecycleTransition::Uninstall => {
                    manager.uninstall("com.example.network", false).unwrap();
                }
            }
            // The current-thread runtime has not polled the blocked transport again:
            // management must signal cancellation and return before that future settles.
            assert!(
                cancellation.is_cancelled(),
                "{transition:?} must cancel the call"
            );
            assert!(
                !pending.is_finished(),
                "management must return before transport settlement"
            );

            assert_eq!(
                tokio::time::timeout(Duration::from_millis(500), pending)
                    .await
                    .unwrap()
                    .unwrap(),
                Err(PluginNetworkErrorCode::ExpiredRequest),
                "{transition:?} must expire the pending Broker future"
            );
            assert_eq!(manager.network_authority.global_active_for_test(), 0);
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum TestedNetworkRequestTermination {
        Cancel,
        Timeout,
        WebviewTeardown,
        Shutdown,
    }

    #[tokio::test]
    async fn plugin_network_runtime_lifecycle_request_and_host_termination_cancel_registered_call()
    {
        for termination in [
            TestedNetworkRequestTermination::Cancel,
            TestedNetworkRequestTermination::Timeout,
            TestedNetworkRequestTermination::WebviewTeardown,
            TestedNetworkRequestTermination::Shutdown,
        ] {
            let dir = TestDir::new(&format!("network-termination-{termination:?}"));
            write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
            let (network_authority, transport_started) =
                PluginNetworkAuthorityGate::blocking_for_test();
            let manager = Arc::new(manager_with_network_authority(&dir, network_authority));
            let now = Instant::now();
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
            let scheduled = dispatch_network_scheduled(&manager);
            let pending = tokio::spawn({
                let manager = Arc::clone(&manager);
                let caller = PluginRuntimeIdentity {
                    plugin_id: scheduled.context.plugin_id.clone(),
                    generation: scheduled.context.plugin_generation,
                };
                let input = PluginNetworkCommandInput {
                    context: scheduled.context.clone(),
                    request: network_request(),
                };
                async move { manager.request_network(caller, input).await }
            });
            tokio::time::timeout(Duration::from_millis(500), transport_started.notified())
                .await
                .unwrap();

            let termination_started = Instant::now();
            match termination {
                TestedNetworkRequestTermination::Cancel => {
                    manager
                        .cancel_scheduled_request(&scheduled.context, now)
                        .unwrap();
                }
                TestedNetworkRequestTermination::Timeout => {
                    let replacements = manager.expire_network_timeouts(scheduled.deadline).unwrap();
                    assert_eq!(replacements.len(), 1);
                }
                TestedNetworkRequestTermination::WebviewTeardown => {
                    let service = crate::public_plugins::PublicPluginService::default();
                    assert!(service.manager.set(Arc::clone(&manager)).is_ok());
                    service.invalidate_runtime_label(
                        &runtime_label("com.example.network", scheduled.context.plugin_generation)
                            .unwrap(),
                    );
                }
                TestedNetworkRequestTermination::Shutdown => {
                    let service = crate::public_plugins::PublicPluginService::default();
                    assert!(service.manager.set(Arc::clone(&manager)).is_ok());
                    service.shutdown();
                }
            }
            assert!(
                termination_started.elapsed() < Duration::from_millis(500),
                "{termination:?} must not wait for the network deadline"
            );

            assert_eq!(
                tokio::time::timeout(Duration::from_millis(500), pending)
                    .await
                    .unwrap()
                    .unwrap(),
                Err(PluginNetworkErrorCode::ExpiredRequest),
                "{termination:?} must expire the pending Broker future"
            );
            assert_eq!(manager.network_authority.global_active_for_test(), 0);
        }
    }

    #[test]
    fn plugin_network_lifecycle_timeout_replacement_publishes_only_new_runtime_identity() {
        let dir = TestDir::new("network-lifecycle-replacement");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let old = dispatch_network_scheduled(&manager);
        let old_call = manager
            .network_authority
            .admit(
                "com.example.network",
                old.context.plugin_generation,
                &old.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        let replacement = manager
            .expire_network_timeouts(old.deadline)
            .unwrap()
            .pop()
            .unwrap();
        assert!(old_call.is_cancelled_for_test());

        let candidate = manager
            .replace_runtime_generation(
                &replacement.plugin_id,
                replacement.previous_generation,
                replacement.new_generation,
            )
            .unwrap();
        manager
            .finish_runtime_replacement(
                &candidate.plugin_id,
                candidate.generation,
                candidate.activation_id,
                candidate.admission_epoch,
                now,
            )
            .unwrap();
        let current = dispatch_network_request(&manager);
        assert_eq!(current.plugin_generation, replacement.new_generation);
        assert!(manager
            .network_authority
            .admit(
                "com.example.network",
                current.plugin_generation,
                &current,
                &manager.scheduler,
                network_request(),
            )
            .is_ok());
    }

    #[tokio::test]
    async fn plugin_network_lifecycle_waiting_command_replacement_cancels_running_call() {
        let dir = TestDir::new("network-lifecycle-command-replacement");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let (network_authority, transport_started) =
            PluginNetworkAuthorityGate::blocking_for_test();
        let manager = Arc::new(manager_with_network_authority(&dir, network_authority));
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let running = dispatch_network_scheduled(&manager);
        let call = manager
            .network_authority
            .admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        let replacement = manager
            .scheduler
            .reserve(PluginRequestCandidate {
                command_id: "default".into(),
                plugin_id: running.candidate.plugin_id.clone(),
                plugin_generation: running.candidate.plugin_generation,
                activation_id: running.candidate.activation_id,
                admission_epoch: running.candidate.admission_epoch,
                activation_mode: running.candidate.activation_mode,
                input: "replacement".into(),
                owner: crate::public_plugins::PluginRequestOwner::Command(PluginSubmissionOwner {
                    ui_intent_epoch: 2,
                    control_value: "replacement".into(),
                    submission_token: "network-replacement".into(),
                }),
            })
            .unwrap();
        let pending = tokio::spawn({
            let network_authority = Arc::clone(&manager.network_authority);
            let scheduler = Arc::clone(&manager.scheduler);
            async move { network_authority.execute_for_test(call, scheduler).await }
        });
        tokio::time::timeout(Duration::from_millis(500), transport_started.notified())
            .await
            .unwrap();

        let replacement_started = Instant::now();
        assert!(matches!(
            manager.enqueue_reserved(replacement, now).unwrap(),
            PluginScheduleOutcome::Waiting { .. }
        ));
        assert!(replacement_started.elapsed() < Duration::from_millis(500));
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), pending)
                .await
                .unwrap()
                .unwrap(),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
        assert_eq!(manager.network_authority.global_active_for_test(), 0);
    }

    #[tokio::test]
    async fn plugin_network_lifecycle_uncertain_durable_close_survives_poisoned_mutation_lock() {
        let dir = TestDir::new("network-lifecycle-uncertain-close");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let (network_authority, transport_started) =
            PluginNetworkAuthorityGate::blocking_for_test();
        let manager = Arc::new(manager_with_network_authority(&dir, network_authority));
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let running = dispatch_network_scheduled(&manager);
        let call = manager
            .network_authority
            .admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        let pending = tokio::spawn({
            let network_authority = Arc::clone(&manager.network_authority);
            let scheduler = Arc::clone(&manager.scheduler);
            async move { network_authority.execute_for_test(call, scheduler).await }
        });
        tokio::time::timeout(Duration::from_millis(500), transport_started.notified())
            .await
            .unwrap();
        thread::scope(|scope| {
            let mutation = &manager.mutation;
            let _ = scope
                .spawn(move || {
                    let _guard = mutation.lock().unwrap();
                    panic!("poison mutation lock");
                })
                .join();
        });

        let close_started = Instant::now();
        manager.close_network_authority("com.example.network");
        assert!(close_started.elapsed() < Duration::from_millis(500));

        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), pending)
                .await
                .unwrap()
                .unwrap(),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
        assert_eq!(manager.network_authority.global_active_for_test(), 0);
        assert!(matches!(
            manager.network_authority.admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            ),
            Err(PluginNetworkErrorCode::PermissionDenied)
        ));
    }

    #[tokio::test]
    async fn plugin_network_lifecycle_post_commit_publication_failure_keeps_authority_closed() {
        let dir = TestDir::new("network-lifecycle-post-commit-failure");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let (network_authority, transport_started) =
            PluginNetworkAuthorityGate::blocking_for_test();
        let manager = Arc::new(manager_with_network_authority(&dir, network_authority));
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let running = dispatch_network_scheduled(&manager);
        let call = manager
            .network_authority
            .admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        write_network_package(&dir.source(), "1.1.0", &["api.example.com"]);
        let update = manager.prepare("main", source(&dir.source()), now).unwrap();
        let cancellation = call.cancellation_witness_for_test();
        let pending = tokio::spawn({
            let network_authority = Arc::clone(&manager.network_authority);
            let scheduler = Arc::clone(&manager.scheduler);
            async move { network_authority.execute_for_test(call, scheduler).await }
        });
        tokio::time::timeout(Duration::from_millis(500), transport_started.notified())
            .await
            .unwrap();
        thread::scope(|scope| {
            let runtime_faults = &manager.runtime_faults;
            let _ = scope
                .spawn(move || {
                    let _guard = runtime_faults.lock().unwrap();
                    panic!("poison runtime fault publication lock");
                })
                .join();
        });

        assert!(matches!(
            manager.commit_with_readiness(
                "main",
                &update.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            ),
            Err(PublicPluginManagementError::Unavailable)
        ));
        // Disk persistence has no artificial wall-clock budget. Cancellation must
        // already be signalled while the current-thread transport is still pending.
        assert!(cancellation.is_cancelled());
        assert!(!pending.is_finished());
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), pending)
                .await
                .unwrap()
                .unwrap(),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
        assert_eq!(manager.network_authority.global_active_for_test(), 0);
        assert!(matches!(
            manager.network_authority.admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            ),
            Err(PluginNetworkErrorCode::ExpiredRequest | PluginNetworkErrorCode::PermissionDenied)
        ));
    }

    #[test]
    fn plugin_network_lifecycle_uninstall_abort_regrants_only_after_runtime_recovery() {
        let dir = TestDir::new("network-lifecycle-recovery");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let old = dispatch_network_request(&manager);
        let old_call = manager
            .network_authority
            .admit(
                "com.example.network",
                old.plugin_generation,
                &old,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        let transaction = manager
            .begin_uninstall("com.example.network", false)
            .unwrap()
            .unwrap();
        assert!(old_call.is_cancelled_for_test());
        let recovery = manager
            .abort_uninstall_before_commit(transaction)
            .unwrap()
            .unwrap();

        manager.finish_runtime_recovery(&recovery).unwrap();
        let current = dispatch_network_request(&manager);
        assert!(manager
            .network_authority
            .admit(
                "com.example.network",
                current.plugin_generation,
                &current,
                &manager.scheduler,
                network_request(),
            )
            .is_ok());
    }

    #[test]
    fn plugin_network_grant_manifest_mismatch_on_restart_fails_closed() {
        let dir = TestDir::new("network-grant-restart");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        let now = Instant::now();
        {
            let manager = manager(&dir);
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
        }

        let state_path = dir
            .path()
            .join("public-plugins/state/com.example.network/state.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        document["networkHttpsHostsGrant"] = json!(["other.example.com"]);
        fs::write(&state_path, serde_json::to_vec(&document).unwrap()).unwrap();

        let reloaded = manager(&dir);
        let config = reloaded
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::NetworkHttps));
        assert!(config.network_https_hosts_grant.is_empty());
    }

    #[test]
    fn recovery_target_binds_identity_version_revision_and_full_config() {
        let dir = TestDir::new("recovery-target");
        write_package(&dir.source(), "1.0.0", "A");
        let manager = manager(&dir);
        let now = Instant::now();
        let first = manager.prepare("main", source(&dir.source()), now).unwrap();
        let observation = manager
            .diagnostics("com.example.activation")
            .unwrap()
            .last_operation
            .unwrap();
        assert_eq!(observation.phase, Phase::Complete);
        assert_eq!(
            observation.state_commit_outcome,
            CommitOutcome::NotAttempted
        );
        manager
            .commit_with_readiness("main", &first.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        manager
            .set_enabled_with_readiness("com.example.activation", true, |_| {
                panic!("healthy enable must not probe")
            })
            .unwrap();
        assert_eq!(
            manager
                .diagnostics("com.example.activation")
                .unwrap()
                .last_operation
                .unwrap()
                .state_commit_outcome,
            CommitOutcome::NotAttempted
        );
        let target = || PublicPluginRefreshTarget {
            plugin_id: "com.example.activation".into(),
            expected_inventory_revision: manager.state.inventory_revision().unwrap().to_string(),
        };
        let mut stale = target();
        stale.expected_inventory_revision = "0".into();
        assert_eq!(
            manager.prepare_targeted("main", source(&dir.source()), Some(stale), now),
            Err(PublicPluginManagementError::StaleInventory)
        );
        assert_eq!(
            manager.prepare_targeted("plugin-runtime", source(&dir.source()), Some(target()), now),
            Err(PublicPluginManagementError::InvalidCaller)
        );
        let mut wrong = target();
        wrong.plugin_id = "com.example.other".into();
        assert_eq!(
            manager.prepare_targeted("main", source(&dir.source()), Some(wrong), now),
            Err(PublicPluginManagementError::RefreshTargetMismatch)
        );
        write_package(&dir.source(), "1.1.0", "wrong-version");
        assert_eq!(
            manager.prepare_targeted("main", source(&dir.source()), Some(target()), now),
            Err(PublicPluginManagementError::RefreshVersionMismatch)
        );
        write_package(&dir.source(), "1.0.0", "B");
        let prepared = manager
            .prepare_targeted("main", source(&dir.source()), Some(target()), now)
            .unwrap();
        assert_eq!(prepared.operation, PrepareOperation::DevelopmentRefresh);
        assert_eq!(prepared.previous_version.as_deref(), Some("1.0.0"));
        // A changed target cannot be silently adopted by this already issued token.
        manager
            .rename("com.example.activation", Some("changed"))
            .unwrap();
        assert_eq!(
            manager.commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true),
            Err(PublicPluginManagementError::Unavailable)
        );
        assert_eq!(
            manager.commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true),
            Err(PublicPluginManagementError::InvalidToken)
        );
        let prepared = manager
            .prepare_targeted("main", source(&dir.source()), Some(target()), now)
            .unwrap();
        // An unrelated durable owner change after prepare must not invalidate the captured target.
        let other = dir.path().join("other-source");
        write_package(&other, "1.0.0", "other");
        let manifest = other.join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["pluginId"] = "com.example.other".into();
        value["command"]["defaultName"] = "other".into();
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let unrelated = manager.prepare("main", source(&other), now).unwrap();
        manager
            .commit_with_readiness("main", &unrelated.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let report = manager.diagnostics("com.example.activation").unwrap();
        assert_eq!(
            report.last_operation.unwrap().operation,
            Operation::DevelopmentRefresh
        );
        assert!(
            matches!(report.state, Some(DiagnosticState::Installed { package_version, .. }) if package_version == "1.0.0")
        );
    }

    #[test]
    fn recovery_refresh_disabled_retains_inventory_and_can_explicitly_enable() {
        for fault in [None, Some(PublicPluginFault::RuntimeUnavailable)] {
            let dir = TestDir::new("recovery-refresh-disabled");
            write_package(&dir.source(), "1.0.0", "original");
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let icon_bytes = fs::read(
                workspace.join("examples/public-plugins/com.uipilot.demo-win/package/icon.png"),
            )
            .unwrap();
            fs::write(dir.source().join(ICON_PATH), &icon_bytes).unwrap();
            let manager = manager(&dir);
            let now = Instant::now();
            let plugin_id = "com.example.activation";
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            let installed = manager
                .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
                .unwrap();
            manager.rename(plugin_id, Some("retained-name")).unwrap();
            manager.set_favorite(plugin_id, true).unwrap();
            manager
                .save_settings(
                    plugin_id,
                    &BTreeMap::from([("compact".into(), json!(true))]),
                )
                .unwrap();
            let scope = super::super::PluginDataScope::new(plugin_id).unwrap();
            let saved = json!({"schemaVersion": 1, "count": 7});
            manager
                .storage
                .set(&scope, plugin_id, "snapshot", saved.clone())
                .unwrap();
            match fault {
                Some(fault) => {
                    manager.disable_plugin_for_fault(plugin_id, fault).unwrap();
                }
                None => {
                    manager
                        .set_enabled_with_readiness(plugin_id, false, |_| {
                            panic!("disable must not probe")
                        })
                        .unwrap();
                }
            }
            let before = manager.state.config(plugin_id).unwrap().unwrap();
            write_package(&dir.source(), "1.0.0", "repaired");
            let target = PublicPluginRefreshTarget {
                plugin_id: plugin_id.into(),
                expected_inventory_revision: manager.inventory().unwrap().revision,
            };
            let prepared = manager
                .prepare_targeted("main", source(&dir.source()), Some(target), now)
                .unwrap();
            let refreshed = manager
                .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
                .unwrap();
            let inventory = manager.inventory().unwrap();
            let item = inventory
                .items
                .iter()
                .find(|item| item.plugin_id == plugin_id)
                .expect("disabled refreshed plugin must remain manageable in inventory");
            assert!(!item.enabled);
            assert_eq!(item.fault, fault);
            assert_eq!(item.version, "1.0.0");
            assert_eq!(item.generation, refreshed.mutation.generation);
            assert_eq!(item.commands[0].effective_name, "retained-name");
            assert!(item.commands[0].favorite);
            assert_eq!(item.settings[0].value, Some(json!(true)));
            let icon_path = tauri::Url::parse(item.icon_url.as_ref().unwrap())
                .unwrap()
                .path()
                .to_owned();
            assert_eq!(
                manager
                    .icon_asset("main", &icon_path, now)
                    .expect("disabled refreshed icon must remain available to management UI")
                    .bytes,
                icon_bytes
            );
            let shell = crate::plugin_window::plugin_shell_label(plugin_id).unwrap();
            assert!(manager.icon_asset(&shell, &icon_path, now).is_none());
            assert!(manager
                .icon_asset(&refreshed.runtime.label, &icon_path, now)
                .is_none());
            let after = manager.state.config(plugin_id).unwrap().unwrap();
            assert_eq!(after.commands, before.commands);
            assert_eq!(after.settings, before.settings);
            assert_eq!(after.permission_grants, before.permission_grants);
            assert_eq!(after.fault, before.fault);
            assert!(!after.enabled);
            assert_eq!(
                manager.storage.get(&scope, plugin_id, "snapshot").unwrap(),
                Some(saved.clone())
            );
            // Reading management metadata must not publish either the old or probe Runtime.
            assert!(manager.bundles.bundle(plugin_id).unwrap().is_none());
            assert!(manager.route("/retained-name").unwrap().is_none());
            assert!(manager
                .asset(&installed.runtime.label, "/dist/runtime.js")
                .is_none());
            assert!(manager
                .asset(&refreshed.runtime.label, "/dist/runtime.js")
                .is_none());
            assert!(manager
                .manifest_for_label(&refreshed.runtime.label)
                .is_none());
            assert!(staging_is_empty(&manager));
            assert!(!manager
                .data
                .lock()
                .unwrap()
                .active_by_plugin
                .contains_key(plugin_id));
            let enabled = manager
                .set_enabled_with_readiness(plugin_id, true, |runtime| {
                    assert!(manager
                        .asset(&runtime.label, "/dist/runtime.js")
                        .is_some_and(|asset| String::from_utf8(asset.bytes)
                            .unwrap()
                            .contains("repaired")));
                    true
                })
                .unwrap();
            assert!(enabled.mutation.enabled);
            let item = manager.inventory().unwrap().items.remove(0);
            assert!(item.enabled);
            assert_eq!(item.fault, None);
            assert_eq!(item.commands[0].effective_name, "retained-name");
            assert!(item.commands[0].favorite);
            assert_eq!(
                manager.storage.get(&scope, plugin_id, "snapshot").unwrap(),
                Some(saved)
            );
            assert!(manager.route("/retained-name").unwrap().is_some());
        }
    }

    #[test]
    fn recovery_diagnostics_capture_real_durable_outcomes_and_authoritative_absence() {
        for failure in [
            "readiness",
            "notCommitted",
            "revalidate",
            "unknown",
            "publication",
        ] {
            let dir = TestDir::new("recovery-diagnostic-outcome");
            write_package(&dir.source(), "1.0.0", "A");
            let manager = manager(&dir);
            let now = Instant::now();
            assert!(matches!(
                manager.diagnostics("com.example.absent").unwrap().state,
                Some(DiagnosticState::NotInstalled { .. })
            ));
            let first = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness("main", &first.token, BTreeSet::new(), now, |_| true)
                .unwrap();
            write_package(&dir.source(), "1.0.0", "B");
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            let current = dir
                .path()
                .join("public-plugins/state/com.example.activation/state.json");
            if failure == "notCommitted" || failure == "revalidate" {
                let backup = current.with_file_name("state.json.backup");
                if backup.exists() {
                    fs::remove_file(&backup).unwrap();
                }
                fs::create_dir(&backup).unwrap();
            }
            if failure == "revalidate" {
                fs::write(&current, b"different authoritative bytes").unwrap();
            }
            if failure == "unknown" {
                fs::remove_file(&current).unwrap();
                fs::create_dir(&current).unwrap();
            }
            if failure == "publication" {
                std::thread::scope(|scope| {
                    let _ = scope
                        .spawn(|| {
                            let _guard = manager.runtime_faults.lock().unwrap();
                            panic!("synthetic diagnostic publication failure");
                        })
                        .join();
                });
            }
            assert!(manager
                .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| failure
                    != "readiness")
                .is_err());
            let report = manager.diagnostics("com.example.activation").unwrap();
            let observed = report.last_operation.unwrap();
            assert_eq!(observed.operation, Operation::Replace);
            assert_eq!(
                observed.phase,
                match failure {
                    "readiness" => Phase::Readiness,
                    "publication" => Phase::Publication,
                    _ => Phase::Persistence,
                }
            );
            assert_eq!(
                observed.state_commit_outcome,
                match failure {
                    "readiness" => CommitOutcome::NotAttempted,
                    "notCommitted" => CommitOutcome::NotCommitted,
                    "unknown" | "revalidate" => CommitOutcome::Unknown,
                    _ => CommitOutcome::Committed,
                }
            );
            assert_eq!(
                report.state.is_some(),
                failure == "readiness" || failure == "notCommitted"
            );
        }
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
        assert_eq!(
            value["items"][0]["commands"][0]["effectiveName"],
            "activation"
        );
        assert_eq!(value["items"][0]["source"], "localPackage");
        let installation = value["items"][0]["installationDirectory"].as_str().unwrap();
        assert!(std::path::Path::new(installation)
            .join("plugin.json")
            .is_file());
        assert!(value["items"][0].get("runtime").is_none());
        assert!(value["items"][0].get("outputMode").is_none());
        assert!(value.to_string().find("runtime.js").is_none());
    }
    fn multi_manager(dir: &TestDir) -> PublicPluginManager {
        PublicPluginManager::load_with_timer_dependencies(
            dir.path(),
            PublicPluginHost {
                version: [0, 5, 0],
                ..PublicPluginHost::current(PublicPlatform::Windows)
            },
            ["find".into(), "math".into()],
            Arc::new(MessageCenterService::load(dir.path())),
            Arc::new(TestClock::default()),
            Arc::new(TestPublisher::successful()),
        )
        .unwrap()
    }

    fn write_multi_package(root: &Path, version: &str, second_name: &str, reverse: bool) {
        write_package(root, version, version);
        let path = root.join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest.as_object_mut().unwrap().remove("command");
        manifest["minimumHostVersion"] = json!("0.5.0");
        manifest["permissions"] = json!(["clipboard.write"]);
        let mut commands = vec![
            json!({"id":"upper", "defaultName":"upper", "summary":"Uppercase", "activationMode":"live", "outputMode":"mainResult", "inputRequired":false}),
            json!({"id":"constructor", "defaultName":second_name, "summary":"Lowercase", "activationMode":"submit", "outputMode":"mainResult", "inputRequired":true, "inputPlaceholder":"Text"}),
        ];
        if reverse {
            commands.reverse();
        }
        manifest["commands"] = json!(commands);
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }

    fn install_multi(manager: &PublicPluginManager, path: &Path) {
        let now = Instant::now();
        let prepared = manager.prepare("main", source(path), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::ClipboardWrite]),
                now,
                |_| true,
            )
            .unwrap();
    }

    #[test]
    fn command_hotkey_id_route_survives_rename_label_reuse_and_closes_on_fault() {
        let dir = TestDir::new("command-hotkey-id-route");
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        let manager = multi_manager(&dir);
        install_multi(&manager, &dir.source());
        let before = manager
            .route_by_id("com.example.activation", "upper", "hello")
            .unwrap()
            .unwrap();
        manager
            .rename_command("com.example.activation", "upper", Some("renamed"))
            .unwrap();
        manager
            .rename_command("com.example.activation", "constructor", Some("upper"))
            .unwrap();
        assert_eq!(
            manager.route("/upper hello").unwrap().unwrap().command_id,
            "constructor"
        );
        let stable = manager
            .route_by_id("com.example.activation", "upper", "hello")
            .unwrap()
            .unwrap();
        assert_eq!(stable.command_id, "upper");
        assert_eq!(stable.command_label, "renamed");
        let settings = SettingsStore::load(dir.path()).unwrap();
        manager
            .attach_command_hotkeys(settings.clone(), Arc::new(|| {}))
            .unwrap();
        let authority = manager
            .command_hotkey_authority("com.example.activation", "upper")
            .unwrap();
        manager
            .persist_command_hotkey(
                &settings,
                "com.example.activation",
                "upper",
                Some("control+KeyN".into()),
                authority,
            )
            .unwrap();
        assert!(manager
            .disable_plugin_for_fault_if_current(
                "com.example.activation",
                before.generation,
                stable.activation_id,
                PublicPluginFault::RuntimeUnavailable
            )
            .unwrap());
        assert!(manager
            .route_by_id("com.example.activation", "upper", "hello")
            .unwrap()
            .is_none());
        assert_eq!(
            settings.snapshot().public_plugin_command_hotkeys["com.example.activation"]["upper"],
            "control+KeyN"
        );
        assert!(manager
            .persist_command_hotkey(
                &settings,
                "com.example.activation",
                "upper",
                None,
                authority
            )
            .is_err());
        manager
            .set_enabled_with_readiness("com.example.activation", true, |_| true)
            .unwrap();
        assert!(manager
            .route_by_id("com.example.activation", "upper", "hello")
            .unwrap()
            .is_some());
    }

    #[test]
    fn command_hotkey_uninstall_revalidation_and_cleanup_ignore_retain_data() {
        for retain in [true, false] {
            let dir = TestDir::new("command-hotkey-uninstall");
            write_package(&dir.source(), "1.0.0", "one");
            let manager = manager(&dir);
            install_discovery_package(&manager, &dir.source(), Instant::now());
            let settings = SettingsStore::load(dir.path()).unwrap();
            manager
                .attach_command_hotkeys(settings.clone(), Arc::new(|| {}))
                .unwrap();
            let authority = manager
                .command_hotkey_authority("com.example.activation", "default")
                .unwrap();
            manager
                .persist_command_hotkey(
                    &settings,
                    "com.example.activation",
                    "default",
                    Some("control+KeyN".into()),
                    authority,
                )
                .unwrap();
            let mut transaction = manager
                .begin_uninstall("com.example.activation", retain)
                .unwrap()
                .unwrap();
            manager.drain_uninstall_data(&mut transaction).unwrap();
            let committed = manager.commit_uninstall(transaction).unwrap();
            assert!(manager
                .persist_command_hotkey(
                    &settings,
                    "com.example.activation",
                    "default",
                    Some("control+KeyM".into()),
                    authority
                )
                .is_err());
            manager
                .finish_uninstall_cleanup(&committed, &settings)
                .unwrap();
            assert!(settings.snapshot().public_plugin_command_hotkeys.is_empty());
            install_discovery_package(&manager, &dir.source(), Instant::now());
            assert!(settings.snapshot().public_plugin_command_hotkeys.is_empty());
        }
    }

    #[test]
    fn command_hotkey_update_prunes_removed_ids_and_precommit_prune_blocks_resurrection() {
        let dir = TestDir::new("command-hotkey-prune");
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        let manager = multi_manager(&dir);
        install_multi(&manager, &dir.source());
        let settings = SettingsStore::load(dir.path()).unwrap();
        manager
            .attach_command_hotkeys(settings.clone(), Arc::new(|| {}))
            .unwrap();
        for id in ["upper", "constructor"] {
            settings
                .set_command_hotkey("com.example.activation", id, Some("control+KeyN".into()))
                .unwrap();
        }
        write_multi_package(&dir.source(), "2.0.0", "lower", false);
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["commands"]
            .as_array_mut()
            .unwrap()
            .retain(|c| c["id"] == "upper");
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        install_multi(&manager, &dir.source());
        let saved = settings.snapshot().public_plugin_command_hotkeys;
        assert!(saved["com.example.activation"].contains_key("upper"));
        assert!(!saved["com.example.activation"].contains_key("constructor"));
        // Simulate a prior postcommit cleanup failure/crash leaving a stale preference.
        settings
            .set_command_hotkey(
                "com.example.activation",
                "constructor",
                Some("control+KeyM".into()),
            )
            .unwrap();
        write_multi_package(&dir.source(), "3.0.0", "lower", false);
        install_multi(&manager, &dir.source());
        assert!(
            !settings.snapshot().public_plugin_command_hotkeys["com.example.activation"]
                .contains_key("constructor")
        );
    }

    #[test]
    fn multi_command_conflicts_reject_the_entire_install_or_upgrade() {
        for upgrade in [false, true] {
            for conflict in ["find", "quick"] {
                let dir = TestDir::new("multi-atomic-conflicts");
                let manager = multi_manager(&dir);
                let other = dir.path().join("other");
                write_discovery_package(&other, "com.example.other", "Other", "occupied", "Other");
                install_discovery_package(&manager, &other, Instant::now());
                manager
                    .replace_external_reserved_names(["quick".into()])
                    .unwrap();
                if upgrade || conflict == "custom" {
                    write_multi_package(&dir.source(), "1.0.0", "lower", false);
                    install_multi(&manager, &dir.source());
                    manager
                        .rename_command("com.example.activation", "upper", Some("custom"))
                        .unwrap();
                    manager
                        .set_command_favorite("com.example.activation", "upper", true)
                        .unwrap();
                    manager
                        .set_command_favorite("com.example.activation", "constructor", true)
                        .unwrap();
                    if !upgrade {
                        manager.uninstall("com.example.activation", true).unwrap();
                    }
                }
                let before = manager.state.config("com.example.activation").unwrap();
                let inventory_before = manager.inventory().unwrap();
                let state_path = dir
                    .path()
                    .join("public-plugins/state/com.example.activation/state.json");
                let bytes_before = fs::read(&state_path).ok();
                write_multi_package(&dir.source(), "2.0.0", conflict, true);
                let now = Instant::now();
                let prepared = manager
                    .prepare("main", source(&dir.source()), now)
                    .expect("candidate manifest must be valid before full-set registration");
                let outcome = manager.commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::ClipboardWrite]),
                    now,
                    |_| true,
                );
                assert!(
                    matches!(outcome, Err(PublicPluginManagementError::NameConflict)),
                    "upgrade={upgrade} conflict={conflict}"
                );
                assert!(
                    manager.lock_data().unwrap().staged_by_label.is_empty(),
                    "rejected candidate must be unstaged"
                );
                assert_eq!(
                    manager.state.config("com.example.activation").unwrap(),
                    before
                );
                assert_eq!(fs::read(&state_path).ok(), bytes_before);
                assert_eq!(manager.inventory().unwrap(), inventory_before);
                assert!(manager.route("/occupied input").unwrap().is_some());
                if upgrade {
                    assert_eq!(
                        manager
                            .route("/custom original")
                            .unwrap()
                            .unwrap()
                            .command_id,
                        "upper"
                    );
                    assert_eq!(
                        manager
                            .route("/lower original")
                            .unwrap()
                            .unwrap()
                            .command_id,
                        "constructor"
                    );
                    assert_eq!(manager.runtime_candidates().unwrap().len(), 2);
                } else {
                    assert!(manager.route("/upper input").unwrap().is_none());
                }
            }
        }
    }

    #[test]
    fn multi_command_not_committed_keeps_old_set_and_success_publishes_full_reordered_set() {
        let dir = TestDir::new("multi-atomic-durable");
        let manager = multi_manager(&dir);
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        install_multi(&manager, &dir.source());
        manager
            .rename_command("com.example.activation", "upper", Some("custom"))
            .unwrap();
        manager
            .set_command_favorite("com.example.activation", "constructor", true)
            .unwrap();
        let before = manager
            .state
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        let inventory_before = manager.inventory().unwrap();
        let current = dir
            .path()
            .join("public-plugins/state/com.example.activation/state.json");
        let raw = fs::read(&current).unwrap();
        let backup = current.with_file_name("state.json.backup");
        if backup.exists() {
            fs::remove_file(&backup).unwrap();
        }
        fs::create_dir(&backup).unwrap();
        write_multi_package(&dir.source(), "2.0.0", "lower-v2", true);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        let readiness = AtomicU64::new(0);
        assert!(manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::ClipboardWrite]),
                now,
                |_| {
                    readiness.fetch_add(1, Ordering::Relaxed);
                    true
                }
            )
            .is_err());
        assert_eq!(readiness.load(Ordering::Relaxed), 1);
        assert_eq!(fs::read(&current).unwrap(), raw);
        assert_eq!(
            manager
                .state
                .config("com.example.activation")
                .unwrap()
                .unwrap(),
            before
        );
        assert_eq!(manager.inventory().unwrap(), inventory_before);
        assert!(manager
            .bundles
            .confirmed_bundle("com.example.activation")
            .unwrap()
            .is_some());
        assert!(manager.route("/lower-v2").unwrap().is_none());
        assert_eq!(
            manager.route("/custom").unwrap().unwrap().command_id,
            "upper"
        );
        assert_eq!(
            manager.route("/lower").unwrap().unwrap().command_id,
            "constructor"
        );
        let owner_packages = manager.packages_root.join("com.example.activation");
        assert_eq!(fs::read_dir(&owner_packages).unwrap().count(), 1);
        fs::remove_dir(&backup).unwrap();
        install_multi(&manager, &dir.source());
        let after = manager
            .state
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        assert_eq!(after.version, "2.0.0");
        assert_eq!(after.commands.len(), 2);
        assert_eq!(
            after.command("upper").unwrap().name_override.as_deref(),
            Some("custom")
        );
        assert!(after.command("constructor").unwrap().favorite);
        assert!(manager.route("/lower").unwrap().is_none());
        assert_eq!(
            manager
                .route("/lower-v2 kept  input")
                .unwrap()
                .unwrap()
                .input,
            "kept  input"
        );
        assert!(manager.can_copy_text(
            "com.example.activation",
            "constructor",
            after.active_generation
        ));
        assert!(!manager.can_copy_text(
            "com.example.activation",
            "missing",
            after.active_generation
        ));
        assert!(!manager.can_copy_text(
            "com.example.activation",
            "constructor",
            before.active_generation
        ));
    }

    #[test]
    fn copy_authority_remains_bound_to_id_after_name_takeover_and_reorder() {
        use crate::result_registry::{QueryDomain, ResultAction, ResultRegistry};
        let dir = TestDir::new("multi-copy-name-takeover");
        let manager = multi_manager(&dir);
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        install_multi(&manager, &dir.source());
        let route = manager.route("/lower text").unwrap().unwrap();
        let registry = ResultRegistry::default();
        registry.on_show("copy-owner".into());
        let token = registry
            .begin_query(QueryDomain::Plugin, "copy-owner", 1)
            .unwrap();
        let action = ResultAction::CopyText {
            plugin_id: route.plugin_id.clone(),
            command_id: Some(route.command_id.clone()),
            generation: route.generation,
            text: "original private".into(),
        };
        let (request, results) = registry
            .publish_if_latest(
                token,
                vec![((), action.clone())],
                || true,
                |request, results| (request, results),
            )
            .unwrap();
        manager
            .rename_command("com.example.activation", "constructor", Some("renamed"))
            .unwrap();
        let other = dir.path().join("other");
        write_discovery_package(&other, "com.example.other", "Other", "lower", "New owner");
        install_discovery_package(&manager, &other, Instant::now());
        assert_eq!(
            manager.route("/lower text").unwrap().unwrap().plugin_id,
            "com.example.other"
        );
        assert_eq!(registry.resolve(&request, &results[0].0).unwrap(), action);
        // Name takeover cannot rewrite the opaque action's original owner or command.
        assert!(manager.can_copy_text(&route.plugin_id, &route.command_id, route.generation));
        write_multi_package(&dir.source(), "2.0.0", "lower-v2", true);
        install_multi(&manager, &dir.source());
        let current = manager.route("/renamed text").unwrap().unwrap();
        assert_eq!(current.command_id, "constructor");
        assert!(!manager.can_copy_text(&route.plugin_id, &route.command_id, route.generation));
        assert!(manager.can_copy_text(&current.plugin_id, &current.command_id, current.generation));
        // Even with the current generation, a removed/unknown ID has no authority.
        assert!(!manager.can_copy_text(&current.plugin_id, "lower", current.generation));
    }

    #[test]
    fn copy_authority_rejects_every_unconfirmed_reservation_phase() {
        for phase in 0..3 {
            let dir = TestDir::new("copy-unconfirmed");
            let manager = multi_manager(&dir);
            write_multi_package(&dir.source(), "1.0.0", "lower", false);
            install_multi(&manager, &dir.source());
            let generation = manager.route("/upper").unwrap().unwrap().generation;
            assert!(manager.can_copy_text("com.example.activation", "upper", generation));
            let reservation = manager.bundles.reserve("com.example.activation").unwrap();
            if phase >= 1 {
                reservation.begin_committing().unwrap();
            }
            if phase >= 2 {
                reservation.mark_durable().unwrap();
            }
            assert!(
                !manager.can_copy_text("com.example.activation", "upper", generation),
                "phase {phase}"
            );
        }
    }

    #[test]
    fn multi_command_routes_and_inventory_cover_the_full_set_without_default() {
        let dir = TestDir::new("multi-routes");
        write_package(&dir.source(), "1.0.0", "multi");
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest.as_object_mut().unwrap().remove("command");
        manifest["minimumHostVersion"] = json!("0.5.0");
        manifest["commands"] = json!([
            {"id":"upper", "defaultName":"upper", "summary":"Uppercase", "activationMode":"live", "outputMode":"mainResult", "inputRequired":false},
            {"id":"constructor", "defaultName":"lower", "summary":"Lowercase", "activationMode":"submit", "outputMode":"mainResult", "inputRequired":true, "inputPlaceholder":"Text"}
        ]);
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let manager = PublicPluginManager::load_with_timer_dependencies(
            dir.path(),
            PublicPluginHost {
                version: [0, 5, 0],
                ..PublicPluginHost::current(PublicPlatform::Windows)
            },
            ["find".into(), "math".into()],
            Arc::new(MessageCenterService::load(dir.path())),
            Arc::new(TestClock::default()),
            Arc::new(TestPublisher::successful()),
        )
        .unwrap();
        install_discovery_package(&manager, &dir.source(), Instant::now());
        assert_eq!(
            manager.route("/upper A  B").unwrap().unwrap().command_id,
            "upper"
        );
        assert_eq!(
            manager.route("/lower C  D").unwrap().unwrap().command_id,
            "constructor"
        );
        let inventory = serde_json::to_value(manager.inventory().unwrap()).unwrap();
        assert_eq!(
            inventory["items"][0]["commands"].as_array().unwrap().len(),
            2
        );
        assert_eq!(manager.command_suggestions("").unwrap().len(), 2);
        assert_eq!(manager.runtime_candidates().unwrap().len(), 1);
        assert!(manager
            .panel_route("com.example.activation", "")
            .unwrap()
            .is_none());
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
                result_publication: None,
                command_id: "default".into(),
                plugin_id: "com.example.activation".into(),
                generation: 1,
                activation_id: manager.activation_id("com.example.activation").unwrap(),
                admission_epoch: manager.admission_epoch("com.example.activation").unwrap(),
                runtime_recovery_needed: false,
                runtime_label: runtime_label("com.example.activation", 1).unwrap(),
                activation_mode: PublicActivationMode::Live,
                output_mode: PublicOutputMode::MainResult,
                command_label: "activation".into(),
                input: "I am  Jack".into(),
                input_required: false,
                input_placeholder: None,
                window_entry: None,
                panel_entry: None,
                host_keys: Vec::new(),
                host_key_focus: PanelHostKeyFocus::Content,
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
                    command_id: "default".into(),
                    plugin_id: "com.example.demo-return".into(),
                    effective_name: "demo-return".into(),
                    display_name: "Public Plugin Demo Return".into(),
                    summary: Some("返回示例文本到主界面".into()),
                    icon_url: None,
                    favorite: false,
                    input_required: true,
                    output_mode: PublicOutputMode::MainResult,
                },
                PublicCommandSuggestion {
                    command_id: "default".into(),
                    plugin_id: "com.example.demo-win".into(),
                    effective_name: "demo-win".into(),
                    display_name: "Public Plugin Demo Window".into(),
                    summary: Some("打开演示子窗口".into()),
                    icon_url: None,
                    favorite: false,
                    input_required: true,
                    output_mode: PublicOutputMode::MainResult,
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
                command_id: "default".into(),
                plugin_id: "com.example.demo-win".into(),
                effective_name: "demo-win".into(),
                display_name: "Public Plugin Demo Window".into(),
                summary: Some("打开演示子窗口".into()),
                icon_url: None,
                favorite: false,
                input_required: true,
                output_mode: PublicOutputMode::MainResult,
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
        assert!(!before.config.command("default").unwrap().favorite);
        let before_revision = before.config.inventory_revision;
        let before_activation = before.activation_id;
        let before_admission = before.admission_epoch;

        let mutation = manager.set_favorite("com.example.favorite", true).unwrap();
        let favorited = manager
            .bundles
            .bundle("com.example.favorite")
            .unwrap()
            .unwrap();
        assert!(favorited.config.command("default").unwrap().favorite);
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
    fn slash_command_suggestions_sort_favorites_before_name_order() {
        let dir = TestDir::new("slash-favorite-order");
        let manager = manager(&dir);
        let now = Instant::now();
        let panel = dir.path().join("source-panel");
        let pomodoro = dir.path().join("source-pomodoro");
        write_discovery_package(
            &panel,
            "com.example.panel",
            "Panel Tool",
            "panel",
            "Panel summary",
        );
        write_discovery_package(
            &pomodoro,
            "com.example.pomodoro",
            "Pomodoro Tool",
            "pomodoro",
            "Pomodoro summary",
        );
        install_discovery_package(&manager, &panel, now);
        install_discovery_package(&manager, &pomodoro, now);
        manager.set_favorite("com.example.pomodoro", true).unwrap();

        let matching = manager.command_suggestions("p").unwrap();
        assert_eq!(
            matching
                .first()
                .map(|suggestion| (suggestion.effective_name.as_str(), suggestion.favorite)),
            Some(("pomodoro", true))
        );
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
    fn panel_response_accepts_data_payload_and_rejects_main_result_shape() {
        let context = PluginRequestContext {
            plugin_id: "com.example.panel".into(),
            plugin_generation: 1,
            request_id: "public-request-1".into(),
        };
        assert_eq!(
            parse_panel_response(
                &context,
                json!({
                    "requestId": "public-request-1",
                    "data": { "label": "ready" }
                })
            )
            .unwrap(),
            PublicPanelResponse {
                request_id: "public-request-1".into(),
                data: json!({ "label": "ready" }),
            }
        );
        assert!(parse_panel_response(
            &context,
            json!({
                "requestId": "public-request-1",
                "results": [{ "id": "one", "title": "Nope" }]
            })
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

        let before_reservation = manager.route("/renamed").unwrap().unwrap();
        assert_eq!(before_reservation.generation, 1);
        assert_eq!(before_reservation.activation_id, first_id);
        assert_eq!(before_reservation.admission_epoch, first_epoch);
        let reservation = manager.bundles.reserve("com.example.activation").unwrap();
        // Routing requires a confirmed owner even while the old bundle remains
        // readable: a reservation can begin committing outside the mutation lock.
        assert_eq!(
            manager.route("/renamed"),
            Err(PublicPluginManagementError::Unavailable)
        );
        assert_eq!(
            manager.rename("com.example.activation", Some("blocked")),
            Err(PublicPluginManagementError::Unavailable)
        );
        drop(reservation);
        assert_eq!(
            manager.route("/renamed").unwrap().unwrap(),
            before_reservation
        );

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
                    command_id: "default".into(),
                    plugin_id: route.plugin_id.clone(),
                    plugin_generation: route.generation,
                    activation_id: route.activation_id,
                    admission_epoch: route.admission_epoch,
                    activation_mode: route.activation_mode,
                    input: route.input,
                    owner: crate::public_plugins::PluginRequestOwner::Command(
                        PluginSubmissionOwner {
                            ui_intent_epoch: 1,
                            control_value: "/activation value".into(),
                            submission_token: "runtime-data-lease".into(),
                        },
                    ),
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
    fn sole_eligible_duplicate_alias_routes_by_captured_identity_during_recovery() {
        let dir = TestDir::new("duplicate-recovery-route");
        write_package(&dir.source(), "1.0.0", "one");
        let manager = manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
            .unwrap();
        let other = dir.path().join("other");
        write_discovery_package(&other, "com.example.other", "Other", "activation", "Other");
        install_discovery_package(&manager, &other, Instant::now());
        let transaction = manager
            .begin_uninstall("com.example.activation", false)
            .unwrap()
            .unwrap();
        let recovery = manager
            .abort_uninstall_before_commit(transaction)
            .unwrap()
            .unwrap();
        let suggestions = manager
            .command_suggestions("activation")
            .unwrap()
            .into_iter()
            .filter(|item| item.effective_name == "activation")
            .collect::<Vec<_>>();
        let (plugin, command, argument) =
            crate::commands::sole_public_match(&suggestions, "hello  world").unwrap();
        assert_eq!(plugin, "com.example.other");
        let selected = manager
            .route_by_id(&plugin, &command, &argument)
            .unwrap()
            .unwrap();
        assert_eq!(selected.plugin_id, "com.example.other");
        assert_eq!(selected.input, "hello  world");
        assert!(!selected.runtime_recovery_needed);
        // Both bundles own this alias; name lookup has no stable iteration order.
        // Inspect the excluded recovering owner through its captured identity.
        let recovering = manager
            .route_by_id(&recovery.plugin_id, "default", &argument)
            .unwrap()
            .unwrap();
        assert_eq!(recovering.plugin_id, "com.example.activation");
        assert_eq!(recovering.activation_id, recovery.activation_id);
        assert_eq!(recovering.admission_epoch, recovery.admission_epoch);
        assert!(recovering.runtime_recovery_needed);
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
                    command_id: "default".into(),
                    plugin_id: plugin_id.into(),
                    plugin_generation: generation,
                    activation_id,
                    admission_epoch,
                    activation_mode: PublicActivationMode::Live,
                    input: "storage".into(),
                    owner: crate::public_plugins::PluginRequestOwner::Command(
                        crate::public_plugins::PluginSubmissionOwner {
                            ui_intent_epoch: 1,
                            control_value: "/activation storage".into(),
                            submission_token: "window-storage".into(),
                        },
                    ),
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

    fn credential_manager(dir: &TestDir) -> PublicPluginManager {
        PublicPluginManager::load_with_timer_dependencies(
            dir.path(),
            PublicPluginHost {
                version: [0, 4, 0],
                ..PublicPluginHost::current(PublicPlatform::Windows)
            },
            ["find".into(), "math".into()],
            Arc::new(MessageCenterService::load(dir.path())),
            Arc::new(TestClock::default()),
            Arc::new(TestPublisher::successful()),
        )
        .unwrap()
    }

    fn write_credential_package(root: &Path, version: &str, credentials: Value) {
        write_network_package(root, version, &["api.example.com", "auth.example.com"]);
        let path = root.join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["minimumHostVersion"] = json!("0.4.0");
        manifest["settings"] = json!([
            {"key":"token", "label":"Token", "type":"secret"},
            {"key":"other", "label":"Other", "type":"secret"}
        ]);
        if credentials != Value::Null {
            manifest["network"]["credentials"] = credentials;
        }
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }

    fn installed_credential_manager(dir: &TestDir) -> PublicPluginManager {
        write_credential_package(
            &dir.source(),
            "1.0.0",
            json!([
                {"id":"service", "settingKey":"token", "host":"api.example.com", "type":"bearer"}
            ]),
        );
        let manager = credential_manager(dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        manager
    }

    fn mutate_network_for_barrier(manager: &PublicPluginManager, dir: &TestDir, mutation: &str) {
        match mutation {
            "replace" => manager
                .save_secret("com.example.network", "token", Some("synthetic-next"))
                .unwrap(),
            "clear" => manager
                .save_secret("com.example.network", "token", None)
                .unwrap(),
            "rename" => {
                manager
                    .rename("com.example.network", Some("renamed"))
                    .unwrap();
            }
            "revoke" => {
                manager
                    .set_network_access("com.example.network", false)
                    .unwrap();
            }
            "disable" => {
                manager
                    .set_enabled_with_readiness("com.example.network", false, |_| true)
                    .unwrap();
            }
            "upgrade" => {
                write_credential_package(
                    &dir.source(),
                    "1.1.0",
                    json!([{"id":"service","settingKey":"token","host":"api.example.com","type":"bearer"}]),
                );
                let now = Instant::now();
                let update = manager.prepare("main", source(&dir.source()), now).unwrap();
                manager
                    .commit_with_readiness(
                        "main",
                        &update.token,
                        BTreeSet::from([PublicPermission::NetworkHttps]),
                        now,
                        |_| true,
                    )
                    .unwrap();
            }
            "uninstall" => {
                manager.uninstall("com.example.network", true).unwrap();
            }
            "shutdown" => {
                manager.shutdown_network();
            }
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn credential_lifecycle_barriers_cancel_transport_redirect_and_delivery() {
        for stage in ["transport", "redirect", "deliver"] {
            for mutation in [
                "replace",
                "rename",
                "clear",
                "revoke",
                "disable",
                "upgrade",
                "uninstall",
                "shutdown",
            ] {
                let dir = TestDir::new("credential-lifecycle-barrier");
                write_credential_package(
                    &dir.source(),
                    "1.0.0",
                    json!([{"id":"service","settingKey":"token","host":"api.example.com","type":"bearer"}]),
                );
                let (gate, barrier) =
                    PluginNetworkAuthorityGate::paused_response_for_test(if stage == "redirect" {
                        302
                    } else {
                        200
                    });
                let manager = Arc::new(
                    PublicPluginManager::load_with_timer_and_network_dependencies(
                        dir.path(),
                        PublicPluginHost {
                            version: [0, 4, 0],
                            ..PublicPluginHost::current(PublicPlatform::Windows)
                        },
                        [],
                        Arc::new(MessageCenterService::load(dir.path())),
                        Arc::new(TestClock::default()),
                        Arc::new(TestPublisher::successful()),
                        gate.clone(),
                    )
                    .unwrap(),
                );
                let now = Instant::now();
                let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
                manager
                    .commit_with_readiness(
                        "main",
                        &prepared.token,
                        BTreeSet::from([PublicPermission::NetworkHttps]),
                        now,
                        |_| true,
                    )
                    .unwrap();
                manager
                    .secrets
                    .write("com.example.network", "token", "synthetic-admitted")
                    .unwrap();
                let running = dispatch_network_scheduled(&manager);
                let call = gate
                    .admit_with_secrets(
                        "com.example.network",
                        running.context.plugin_generation,
                        &running.context,
                        &manager.scheduler,
                        PluginNetworkRequest {
                            credential: Some("service".into()),
                            ..network_request()
                        },
                        &manager.secrets,
                    )
                    .unwrap();
                if stage == "deliver" {
                    barrier.release.notify_one();
                    let result = gate
                        .execute_before_delivery_for_test(call, manager.scheduler.clone(), || {
                            // The full response is now pending; deliver has not acquired the gate.
                            mutate_network_for_barrier(&manager, &dir, mutation);
                        })
                        .await;
                    assert!(
                        matches!(result, Err(PluginNetworkErrorCode::ExpiredRequest)),
                        "{stage}/{mutation}: {result:?}"
                    );
                } else {
                    let pending = tokio::spawn({
                        let gate = gate.clone();
                        let scheduler = manager.scheduler.clone();
                        async move { gate.execute_for_test(call, scheduler).await }
                    });
                    tokio::time::timeout(Duration::from_secs(2), barrier.reached.notified())
                        .await
                        .unwrap();
                    // Fake transport has started and holds its response before redirect processing.
                    mutate_network_for_barrier(&manager, &dir, mutation);
                    barrier.release.notify_one();
                    let result = tokio::time::timeout(Duration::from_secs(2), pending)
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(
                        matches!(result, Err(PluginNetworkErrorCode::ExpiredRequest)),
                        "{stage}/{mutation}: {result:?}"
                    );
                }
                assert_eq!(
                    barrier.sends.load(Ordering::SeqCst),
                    1,
                    "{stage}/{mutation}: no redirect send"
                );
                assert_eq!(gate.global_active_for_test(), 0);
            }
        }
    }

    #[test]
    fn credential_uninstall_retained_data_abort_rebuilds_with_new_identity() {
        let dir = TestDir::new("credential-retained-abort");
        let manager = installed_credential_manager(&dir);
        let old = dispatch_network_scheduled(&manager);
        let transaction = manager
            .begin_uninstall("com.example.network", true)
            .unwrap()
            .unwrap();
        let recovery = manager
            .abort_uninstall_before_commit(transaction)
            .expect("retained-data abort must recover")
            .unwrap();
        assert_ne!(recovery.activation_id, old.candidate.activation_id);
        assert_ne!(recovery.admission_epoch, old.candidate.admission_epoch);
        assert_eq!(recovery.generation, old.context.plugin_generation);
        assert!(manager.finish_runtime_recovery(&recovery).unwrap());
        assert_eq!(
            manager.scheduler.context_status(&old.context),
            PluginContextStatus::Expired
        );
    }

    #[test]
    fn credential_service_second_key_commit_failure_keeps_first_write_and_settles_waiter() {
        let dir = TestDir::new("credential-second-commit-failure");
        let manager = Arc::new(installed_credential_manager(&dir));
        let service = super::super::PublicPluginService::default();
        assert!(service.manager.set(manager.clone()).is_ok());
        let now = Instant::now();
        let first = service
            .schedule_command(
                manager.route("/network first").unwrap().unwrap(),
                1,
                "/network first".into(),
                now,
            )
            .unwrap();
        let running = first.dispatch.unwrap();
        let waiting = service
            .schedule_command(
                manager.route("/network waiting").unwrap().unwrap(),
                2,
                "/network waiting".into(),
                now,
            )
            .unwrap();
        // A directory at the second record path forces atomic replace to fail after preparation.
        fs::create_dir_all(
            manager
                .secrets
                .record_path_for_test("com.example.network", "token")
                .unwrap(),
        )
        .unwrap();
        let outcome = service.save_settings(
            "com.example.network",
            &BTreeMap::new(),
            &BTreeMap::from([
                ("other".into(), Some("synthetic-first".into())),
                ("token".into(), Some("synthetic-second".into())),
            ]),
        );
        assert!(outcome.result.is_err());
        assert!(outcome.changed);
        assert_eq!(
            manager
                .secrets
                .read_bearer("com.example.network", "other")
                .unwrap()
                .unwrap()
                .as_bytes(),
            b"synthetic-first"
        );
        assert_eq!(
            waiting.receiver.recv_timeout(Duration::from_secs(1)),
            Ok(None)
        );
        let next = service
            .schedule_command(
                manager.route("/network next").unwrap().unwrap(),
                3,
                "/network next".into(),
                now,
            )
            .unwrap();
        assert!(next.dispatch.is_none());
        let completed = manager.scheduler.complete(&running.context, now).unwrap();
        assert!(!completed.accepted);
        let next = completed.next.unwrap();
        assert!(matches!(
            manager.network_authority.admit(
                "com.example.network",
                next.context.plugin_generation,
                &next.context,
                &manager.scheduler,
                network_request()
            ),
            Err(PluginNetworkErrorCode::PermissionDenied)
        ));
    }

    #[test]
    fn credential_successful_rotation_and_clear_reopen_only_for_fresh_requests() {
        let dir = TestDir::new("credential-successful-rotation");
        let manager = installed_credential_manager(&dir);
        manager
            .save_secret("com.example.network", "token", Some("synthetic-old"))
            .unwrap();
        for replacement in [Some("synthetic-new"), None] {
            let old = dispatch_network_scheduled(&manager);
            let call = manager
                .network_authority
                .admit_with_secrets(
                    "com.example.network",
                    old.context.plugin_generation,
                    &old.context,
                    &manager.scheduler,
                    PluginNetworkRequest {
                        credential: Some("service".into()),
                        ..network_request()
                    },
                    &manager.secrets,
                )
                .unwrap();
            manager
                .save_secret("com.example.network", "token", replacement)
                .unwrap();
            assert!(call.is_cancelled_for_test());
            assert!(
                !manager
                    .scheduler
                    .complete(&old.context, Instant::now())
                    .unwrap()
                    .accepted
            );
            let current = dispatch_network_scheduled(&manager);
            assert!(manager
                .network_authority
                .admit(
                    "com.example.network",
                    current.context.plugin_generation,
                    &current.context,
                    &manager.scheduler,
                    network_request()
                )
                .is_ok());
            let credential = manager.network_authority.admit_with_secrets(
                "com.example.network",
                current.context.plugin_generation,
                &current.context,
                &manager.scheduler,
                PluginNetworkRequest {
                    credential: Some("service".into()),
                    ..network_request()
                },
                &manager.secrets,
            );
            if let Some(value) = replacement {
                assert!(credential.is_ok());
                assert_eq!(
                    manager
                        .secrets
                        .read_bearer("com.example.network", "token")
                        .unwrap()
                        .unwrap()
                        .as_bytes(),
                    value.as_bytes()
                );
            } else {
                assert!(matches!(
                    credential,
                    Err(PluginNetworkErrorCode::CredentialUnavailable)
                ));
                assert!(manager
                    .secrets
                    .read_bearer("com.example.network", "token")
                    .unwrap()
                    .is_none());
            }
            manager
                .scheduler
                .complete(&current.context, Instant::now())
                .unwrap();
        }
    }

    #[test]
    fn credential_prepare_failure_preserves_current_request_and_record() {
        let dir = TestDir::new("credential-protect-failure");
        let manager = installed_credential_manager(&dir);
        manager
            .secrets
            .write("com.example.network", "token", "synthetic-old")
            .unwrap();
        let running = dispatch_network_scheduled(&manager);
        let call = manager
            .network_authority
            .admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        manager
            .secrets
            .fail_next_for_test(super::super::secrets::SecretTestFault::Protect);
        let mut effects = ManagementEffects::default();
        assert!(manager
            .save_secret_with_effects(
                "com.example.network",
                "token",
                Some("synthetic-new"),
                &mut effects
            )
            .is_err());
        assert!(!effects.changed);
        assert!(effects.retired.is_empty());
        assert!(!call.is_cancelled_for_test());
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Current
        );
        assert_eq!(
            manager
                .secrets
                .read_bearer("com.example.network", "token")
                .unwrap()
                .unwrap()
                .as_bytes(),
            b"synthetic-old"
        );
    }

    #[test]
    fn credential_failed_commit_recovery_checks_disk_on_every_publication_entry() {
        use super::super::secrets::SecretTestFault;
        for entry in [
            "startup",
            "rename",
            "install",
            "upgrade",
            "enable",
            "grant",
            "uninstall-abort",
            "timeout",
        ] {
            for disk in ["old", "new", "missing", "bad"] {
                let dir = TestDir::new("credential-recovery-matrix");
                let mut manager = installed_credential_manager(&dir);
                manager
                    .secrets
                    .write("com.example.network", "token", "synthetic-old")
                    .unwrap();
                let old = dispatch_network_scheduled(&manager);
                let call = manager
                    .network_authority
                    .admit(
                        "com.example.network",
                        old.context.plugin_generation,
                        &old.context,
                        &manager.scheduler,
                        network_request(),
                    )
                    .unwrap();
                manager.secrets.fail_next_for_test(if disk == "new" {
                    SecretTestFault::AfterCommit
                } else {
                    SecretTestFault::BeforeCommit
                });
                let mut effects = ManagementEffects::default();
                assert!(manager
                    .save_secret_with_effects(
                        "com.example.network",
                        "token",
                        Some("synthetic-new"),
                        &mut effects
                    )
                    .is_err());
                assert!(effects.changed);
                assert_eq!(effects.retired[0].0, old.context);
                assert!(call.is_cancelled_for_test());
                if entry != "timeout" {
                    assert!(
                        !manager
                            .scheduler
                            .complete(&old.context, Instant::now())
                            .unwrap()
                            .accepted
                    );
                }
                match disk {
                    "missing" => manager
                        .secrets
                        .remove("com.example.network", "token")
                        .unwrap(),
                    "bad" => fs::write(
                        manager
                            .secrets
                            .record_path_for_test("com.example.network", "token")
                            .unwrap(),
                        b"broken-record",
                    )
                    .unwrap(),
                    _ => assert_eq!(
                        manager
                            .secrets
                            .read_bearer("com.example.network", "token")
                            .unwrap()
                            .unwrap()
                            .as_bytes(),
                        if disk == "old" {
                            b"synthetic-old"
                        } else {
                            b"synthetic-new"
                        }
                    ),
                }
                // The failed operation never reopens, even when the disk holds a valid value.
                if entry != "timeout" {
                    let closed = dispatch_network_scheduled(&manager);
                    assert!(matches!(
                        manager.network_authority.admit(
                            "com.example.network",
                            closed.context.plugin_generation,
                            &closed.context,
                            &manager.scheduler,
                            network_request()
                        ),
                        Err(PluginNetworkErrorCode::PermissionDenied)
                    ));
                    manager
                        .scheduler
                        .complete(&closed.context, Instant::now())
                        .unwrap();
                }
                match entry {
                    "rename" => {
                        manager
                            .rename("com.example.network", Some("renamed"))
                            .unwrap();
                    }
                    "startup" => {
                        drop(manager);
                        manager = credential_manager(&dir);
                    }
                    "install" | "upgrade" => {
                        if entry == "install" {
                            manager.uninstall("com.example.network", true).unwrap();
                        }
                        write_credential_package(
                            &dir.source(),
                            "1.1.0",
                            json!([{"id":"service","settingKey":"token","host":"api.example.com","type":"bearer"}]),
                        );
                        let now = Instant::now();
                        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
                        manager
                            .commit_with_readiness(
                                "main",
                                &prepared.token,
                                BTreeSet::from([PublicPermission::NetworkHttps]),
                                now,
                                |_| true,
                            )
                            .unwrap();
                    }
                    "enable" => {
                        manager
                            .set_enabled_with_readiness("com.example.network", false, |_| true)
                            .unwrap();
                        manager
                            .set_enabled_with_readiness("com.example.network", true, |_| true)
                            .unwrap();
                    }
                    "grant" => {
                        manager
                            .set_network_access("com.example.network", false)
                            .unwrap();
                        let revision = manager.inventory().unwrap().revision;
                        manager
                            .set_network_access_with_revision(
                                "com.example.network",
                                true,
                                Some(&revision),
                            )
                            .unwrap();
                    }
                    "uninstall-abort" => {
                        let transaction = manager
                            .begin_uninstall("com.example.network", true)
                            .unwrap()
                            .unwrap();
                        let recovery = manager
                            .abort_uninstall_before_commit(transaction)
                            .unwrap_or_else(|error| panic!("{entry}/{disk} abort: {error:?}"))
                            .unwrap();
                        assert!(manager
                            .finish_runtime_recovery(&recovery)
                            .unwrap_or_else(|error| panic!("{entry}/{disk} finish: {error:?}")));
                    }
                    "timeout" => {
                        let replacement = manager
                            .expire_network_timeouts(old.deadline)
                            .unwrap()
                            .pop()
                            .unwrap();
                        assert!(!replacement.counts_as_fault);
                        let candidate = manager
                            .replace_runtime_generation(
                                &replacement.plugin_id,
                                replacement.previous_generation,
                                replacement.new_generation,
                            )
                            .unwrap();
                        manager
                            .finish_runtime_replacement(
                                &candidate.plugin_id,
                                candidate.generation,
                                candidate.activation_id,
                                candidate.admission_epoch,
                                Instant::now(),
                            )
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
                let current = dispatch_network_scheduled(&manager);
                // A fresh process owns a fresh scheduler namespace; old IPC cannot survive startup.
                if entry != "startup" {
                    assert_ne!(current.context, old.context);
                }
                let result = manager.network_authority.admit(
                    "com.example.network",
                    current.context.plugin_generation,
                    &current.context,
                    &manager.scheduler,
                    network_request(),
                );
                assert_eq!(result.is_ok(), disk != "bad", "{entry}/{disk}");
                if entry != "startup" {
                    assert!(!matches!(
                        manager.scheduler.context_status(&old.context),
                        PluginContextStatus::Current
                    ));
                }
            }
        }
    }

    #[test]
    fn credential_unknown_close_barrier_refuses_secret_and_timeout_reopening_before_reservation_drop(
    ) {
        let dir = TestDir::new("credential-unknown-barrier");
        let manager = Arc::new(installed_credential_manager(&dir));
        let old = dispatch_network_scheduled(&manager);
        let waiting = manager.scheduler.reserve(old.candidate.clone()).unwrap();
        manager
            .scheduler
            .enqueue_reserved(waiting, Instant::now())
            .unwrap();
        let replacement = manager
            .expire_network_timeouts(old.deadline)
            .unwrap()
            .pop()
            .unwrap();
        let candidate = manager
            .replace_runtime_generation(
                &replacement.plugin_id,
                replacement.previous_generation,
                replacement.new_generation,
            )
            .unwrap();
        let current = dir
            .path()
            .join("public-plugins/state/com.example.network/state.json");
        fs::remove_file(&current).unwrap();
        fs::create_dir(&current).unwrap();
        let closed = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        *manager.after_uncertain_close.lock().unwrap() = Some(Arc::new({
            let closed = closed.clone();
            let release = release.clone();
            move || {
                closed.wait();
                release.wait();
            }
        }));
        let worker = thread::spawn({
            let manager = manager.clone();
            move || {
                let mut effects = ManagementEffects::default();
                let result = manager.set_network_access_with_effects(
                    "com.example.network",
                    false,
                    None,
                    &mut effects,
                );
                (result, effects)
            }
        });
        closed.wait(); // Exact boundary: Unknown closed the gate; reservation owner has not returned/dropped.
        assert!(!manager.bundles.is_terminal());
        assert!(manager
            .bundles
            .bundle("com.example.network")
            .unwrap()
            .is_some());
        assert!(manager
            .bundles
            .confirmed_bundle("com.example.network")
            .is_err());
        assert!(manager
            .save_secret("com.example.network", "token", Some("synthetic-blocked"))
            .is_err());
        assert!(manager
            .secrets
            .read_bearer("com.example.network", "token")
            .unwrap()
            .is_none());
        assert!(manager
            .finish_runtime_replacement(
                &candidate.plugin_id,
                candidate.generation,
                candidate.activation_id,
                candidate.admission_epoch,
                Instant::now()
            )
            .is_err());
        release.wait();
        let (result, effects) = worker.join().unwrap();
        assert!(result.is_err());
        assert!(effects.changed);
        assert!(manager.bundles.is_terminal());
        assert!(manager
            .finish_runtime_replacement(
                &candidate.plugin_id,
                candidate.generation,
                candidate.activation_id,
                candidate.admission_epoch,
                Instant::now()
            )
            .is_err());
    }

    #[test]
    fn destructive_lifecycle_effects_settle_exact_tokens_but_verified_failures_and_noops_do_not() {
        for operation in ["upgrade", "disable", "uninstall", "enable-noop"] {
            for failure in ["success", "not-committed", "unknown", "post-durable"] {
                if matches!(operation, "uninstall" | "enable-noop") && failure != "success" {
                    continue;
                }
                let dir = TestDir::new("lifecycle-service-effects");
                let manager = Arc::new(installed_credential_manager(&dir));
                let service = crate::public_plugins::PublicPluginService::default();
                assert!(service.manager.set(manager.clone()).is_ok());
                let now = Instant::now();
                let update = if operation == "upgrade" {
                    write_credential_package(
                        &dir.source(),
                        "1.1.0",
                        json!([{"id":"service","settingKey":"token","host":"api.example.com","type":"bearer"}]),
                    );
                    Some(manager.prepare("main", source(&dir.source()), now).unwrap())
                } else {
                    None
                };
                let a = service
                    .schedule_command(
                        manager.route("/network A").unwrap().unwrap(),
                        1,
                        "/network A".into(),
                        now,
                    )
                    .unwrap();
                let waiting = service
                    .schedule_command(
                        manager.route("/network old").unwrap().unwrap(),
                        2,
                        "/network old".into(),
                        now,
                    )
                    .unwrap();
                let untouched = service
                    .prepare_command(
                        manager.route("/network later").unwrap().unwrap(),
                        3,
                        "/network later".into(),
                    )
                    .unwrap();
                let current = dir
                    .path()
                    .join("public-plugins/state/com.example.network/state.json");
                let before = fs::read(&current).unwrap();
                match failure {
                    "not-committed" => {
                        let backup = current.with_file_name("state.json.backup");
                        if backup.exists() {
                            fs::remove_file(&backup).unwrap();
                        }
                        fs::create_dir(&backup).unwrap();
                    }
                    "unknown" => {
                        fs::remove_file(&current).unwrap();
                        fs::create_dir(&current).unwrap();
                    }
                    "post-durable" if operation == "upgrade" => {
                        thread::scope(|scope| {
                            let _ = scope
                                .spawn(|| {
                                    let _guard = manager.runtime_faults.lock().unwrap();
                                    panic!("synthetic publication failure");
                                })
                                .join();
                        });
                    }
                    "post-durable" => {
                        *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
                            let weak = Arc::downgrade(&manager);
                            move || {
                                let manager = weak.upgrade().unwrap();
                                let _ = thread::spawn(move || {
                                    let _guard = manager.mutation.lock().unwrap();
                                    panic!("synthetic durable failure");
                                })
                                .join();
                            }
                        }));
                    }
                    _ => (),
                }
                let outcome = service.manage_with_effects(|manager, effects| match operation {
                    "upgrade" => manager
                        .commit_with_readiness_with_effects(
                            "main",
                            &update.as_ref().unwrap().token,
                            BTreeSet::from([PublicPermission::NetworkHttps]),
                            now,
                            |_| true,
                            effects,
                        )
                        .map(|_| ()),
                    "disable" | "enable-noop" => manager
                        .set_enabled_with_readiness_with_effects(
                            "com.example.network",
                            operation == "enable-noop",
                            |_| true,
                            effects,
                        )
                        .map(|_| ()),
                    "uninstall" => manager
                        .begin_uninstall_with_effects("com.example.network", true, effects)
                        .map(|_| ()),
                    _ => unreachable!(),
                });
                let changed = operation != "enable-noop" && failure != "not-committed";
                assert_eq!(outcome.changed, changed, "{operation}/{failure}");
                assert_eq!(
                    outcome.result.is_ok(),
                    failure == "success",
                    "{operation}/{failure}"
                );
                if changed {
                    assert_eq!(a.receiver.try_recv(), Ok(None));
                    assert_eq!(waiting.receiver.try_recv(), Ok(None));
                } else {
                    // Scheduling the waiter already superseded A before management began.
                    assert_eq!(a.receiver.try_recv(), Ok(None));
                    assert!(matches!(
                        waiting.receiver.try_recv(),
                        Err(std::sync::mpsc::TryRecvError::Empty)
                    ));
                    assert_eq!(fs::read(current).unwrap(), before);
                }
                assert!(service
                    .lock_submissions()
                    .unwrap()
                    .by_token
                    .contains_key(&untouched.token));
            }
        }
    }

    #[test]
    fn command_mutation_errors_settle_exact_service_tokens_and_close_authority() {
        for rename in [false, true] {
            for failure in ["unknown", "revalidate", "post-durable"] {
                for with_waiting in [false, true] {
                    let dir = TestDir::new("command-mutation-error-effects");
                    let (manager, current, _) = manager_with_schema_one_primary(&dir, true);
                    let manager = Arc::new(manager);
                    let service = Arc::new(crate::public_plugins::PublicPluginService::default());
                    assert!(service.manager.set(manager.clone()).is_ok());
                    let now = Instant::now();
                    let a = service
                        .schedule_command(
                            manager.route("/network A").unwrap().unwrap(),
                            1,
                            "/network A".into(),
                            now,
                        )
                        .unwrap();
                    let running = a.dispatch.as_ref().unwrap();
                    let call = manager
                        .network_authority
                        .admit(
                            "com.example.network",
                            running.context.plugin_generation,
                            &running.context,
                            &manager.scheduler,
                            network_request(),
                        )
                        .unwrap();
                    let waiting = with_waiting.then(|| {
                        service
                            .schedule_command(
                                manager.route("/network old").unwrap().unwrap(),
                                2,
                                "/network old".into(),
                                now,
                            )
                            .unwrap()
                    });
                    let untouched = service
                        .prepare_command(
                            manager.route("/network later").unwrap().unwrap(),
                            3,
                            "/network later".into(),
                        )
                        .unwrap();
                    let late = Arc::new(Mutex::new(None));
                    *manager.after_uncertain_close.lock().unwrap() = Some(Arc::new({
                        let service = Arc::downgrade(&service);
                        let late = late.clone();
                        let route = manager.route("/network after-close").unwrap().unwrap();
                        move || {
                            // This new pending owner is created after the captured retirement,
                            // while the failed mutation has not returned to its service caller.
                            *late.lock().unwrap() = Some(
                                service
                                    .upgrade()
                                    .unwrap()
                                    .prepare_command(
                                        route.clone(),
                                        4,
                                        "/network after-close".into(),
                                    )
                                    .unwrap(),
                            );
                        }
                    }));
                    match failure {
                        "unknown" => {
                            fs::remove_file(&current).unwrap();
                            fs::create_dir(&current).unwrap();
                        }
                        "revalidate" => {
                            let backup = current.with_file_name("state.json.backup");
                            if backup.exists() {
                                fs::remove_file(&backup).unwrap();
                            }
                            fs::create_dir(&backup).unwrap();
                            fs::write(&current, b"different authoritative bytes").unwrap();
                        }
                        "post-durable" => {
                            *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
                                let weak = Arc::downgrade(&manager);
                                move || {
                                    let manager = weak.upgrade().unwrap();
                                    let _ = thread::spawn(move || {
                                        let _guard = manager.mutation.lock().unwrap();
                                        panic!("synthetic command mutation poison");
                                    })
                                    .join();
                                }
                            }));
                        }
                        _ => unreachable!(),
                    }
                    let outcome = if rename {
                        service.rename_command("com.example.network", "default", Some("renamed"))
                    } else {
                        service.set_command_favorite("com.example.network", "default", true)
                    };
                    assert!(outcome.result.is_err(), "{rename}/{failure}");
                    assert!(outcome.changed, "{rename}/{failure}");
                    assert_eq!(a.receiver.try_recv(), Ok(None), "{rename}/{failure}");
                    if let Some(waiting) = waiting {
                        assert_eq!(waiting.receiver.try_recv(), Ok(None), "{rename}/{failure}");
                    }
                    let late = late.lock().unwrap().take().unwrap();
                    assert!(service
                        .lock_submissions()
                        .unwrap()
                        .by_token
                        .contains_key(&late.token));
                    assert!(matches!(
                        late.receiver.try_recv(),
                        Err(std::sync::mpsc::TryRecvError::Empty)
                    ));
                    assert!(service
                        .lock_submissions()
                        .unwrap()
                        .by_token
                        .contains_key(&untouched.token));
                    assert!(call.is_cancelled_for_test());
                    assert_eq!(
                        manager.scheduler.context_status(&running.context),
                        PluginContextStatus::Expired
                    );
                    assert!(manager
                        .network_authority
                        .admit(
                            "com.example.network",
                            running.context.plugin_generation,
                            &running.context,
                            &manager.scheduler,
                            network_request()
                        )
                        .is_err());
                    // Even error closure retains the old execution slot; no late sweep targets unrelated tokens.
                    assert!(matches!(
                        manager
                            .scheduler
                            .enqueue(running.candidate.clone(), now)
                            .unwrap(),
                        PluginScheduleOutcome::Waiting { .. }
                    ));
                    assert!(
                        !manager
                            .scheduler
                            .complete(&running.context, now)
                            .unwrap()
                            .accepted
                    );
                }
            }
        }
    }

    #[test]
    fn credential_post_durable_lock_failure_still_retires_and_closes_authority() {
        let dir = TestDir::new("credential-durable-lock-failure");
        let manager = Arc::new(installed_credential_manager(&dir));
        let running = dispatch_network_scheduled(&manager);
        let call = manager
            .network_authority
            .admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
            let weak = Arc::downgrade(&manager);
            move || {
                let manager = weak.upgrade().unwrap();
                let _ = thread::spawn(move || {
                    let _guard = manager.mutation.lock().unwrap();
                    panic!("synthetic mutation poison");
                })
                .join();
            }
        }));
        let mut effects = ManagementEffects::default();
        assert!(manager
            .set_network_access_with_effects("com.example.network", false, None, &mut effects)
            .is_err());
        assert!(call.is_cancelled_for_test());
        assert!(effects.changed);
        assert_eq!(effects.retired[0].0, running.context);
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Expired
        );
    }

    #[test]
    fn credential_unknown_state_commit_carries_retirement_effects() {
        let dir = TestDir::new("credential-unknown-effects");
        let manager = installed_credential_manager(&dir);
        let running = dispatch_network_scheduled(&manager);
        let current = dir
            .path()
            .join("public-plugins/state/com.example.network/state.json");
        fs::remove_file(&current).unwrap();
        fs::create_dir(&current).unwrap(); // Atomic current replace fails after candidate/backup write: Unknown.
        let mut effects = ManagementEffects::default();
        assert!(manager
            .set_network_access_with_effects("com.example.network", false, None, &mut effects)
            .is_err());
        assert!(effects.changed);
        assert_eq!(effects.retired[0].0, running.context);
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Expired
        );
    }

    #[test]
    fn credential_save_cancels_old_request_and_keeps_running_slot() {
        let dir = TestDir::new("credential-secret-retire");
        let manager = installed_credential_manager(&dir);
        let running = dispatch_network_scheduled(&manager);
        manager
            .save_secret("com.example.network", "token", Some("synthetic-next"))
            .unwrap();
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Expired
        );
        assert!(matches!(
            manager
                .scheduler
                .enqueue(running.candidate.clone(), Instant::now())
                .unwrap(),
            PluginScheduleOutcome::Waiting { .. }
        ));
        let completion = manager
            .scheduler
            .complete(&running.context, Instant::now())
            .unwrap();
        assert!(!completion.accepted);
        assert!(completion.next.is_some());
    }

    #[test]
    fn checked_authority_and_command_mutations_reject_all_reservation_phases() {
        for phase in 0..3 {
            let dir = TestDir::new("command-checked-phase");
            let manager = installed_credential_manager(&dir);
            let bundle = manager
                .bundles
                .confirmed_bundle("com.example.network")
                .unwrap()
                .unwrap();
            let reservation = manager.bundles.reserve("com.example.network").unwrap();
            if phase >= 1 {
                reservation.begin_committing().unwrap();
            }
            if phase >= 2 {
                reservation.mark_durable().unwrap();
            }
            assert!(manager.rename("com.example.network", None).is_err());
            assert!(manager.set_favorite("com.example.network", false).is_err());
            let _mutation = manager.lock_mutation().unwrap();
            let _transition = manager.network_authority.begin_transition().unwrap();
            assert_eq!(
                manager
                    .state
                    .config("com.example.network")
                    .unwrap()
                    .unwrap(),
                bundle.config
            );
            assert!(
                checked_network_authority_snapshot(
                    &manager.bundles,
                    &manager.state,
                    &manager.secrets,
                    &bundle.config,
                    &bundle.runtime,
                    bundle.activation_id,
                    bundle.admission_epoch
                )
                .is_err(),
                "phase {phase}"
            );
        }
    }

    #[test]
    fn rename_reopens_only_after_durable_state_and_bundle_publication() {
        let dir = TestDir::new("rename-publish-order");
        let manager = Arc::new(installed_credential_manager(&dir));
        let old = dispatch_network_scheduled(&manager);
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
            let weak = Arc::downgrade(&manager);
            let observed = observed.clone();
            move || {
                let manager = weak.upgrade().unwrap();
                let _mutation = manager.lock_mutation().unwrap();
                let _transition = manager.network_authority.begin_transition().unwrap();
                let old_bundle = manager
                    .bundles
                    .bundle("com.example.network")
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    old_bundle.config.command("default").unwrap().name_override,
                    None
                );
                assert!(checked_network_authority_snapshot(
                    &manager.bundles,
                    &manager.state,
                    &manager.secrets,
                    &old_bundle.config,
                    &old_bundle.runtime,
                    old_bundle.activation_id,
                    old_bundle.admission_epoch
                )
                .is_err());
                observed.store(true, Ordering::SeqCst);
            }
        }));
        manager
            .rename("com.example.network", Some("renamed"))
            .unwrap();
        assert!(observed.load(Ordering::SeqCst));
        let bundle = manager
            .bundles
            .confirmed_bundle("com.example.network")
            .unwrap()
            .unwrap();
        assert_eq!(
            bundle.config,
            manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            bundle
                .config
                .command("default")
                .unwrap()
                .name_override
                .as_deref(),
            Some("renamed")
        );
        assert!(manager
            .network_authority
            .admit(
                "com.example.network",
                old.context.plugin_generation,
                &old.context,
                &manager.scheduler,
                network_request()
            )
            .is_err());
        assert!(
            !manager
                .scheduler
                .complete(&old.context, Instant::now())
                .unwrap()
                .accepted
        );
        let fresh = dispatch_network_scheduled(&manager);
        assert!(manager
            .network_authority
            .admit(
                "com.example.network",
                fresh.context.plugin_generation,
                &fresh.context,
                &manager.scheduler,
                network_request()
            )
            .is_ok());
    }

    #[test]
    fn uncertain_command_close_denies_copy_before_reservation_owner_returns() {
        for rename in [false, true] {
            let dir = TestDir::new("command-copy-close-barrier");
            let manager = Arc::new(multi_manager(&dir));
            write_multi_package(&dir.source(), "1.0.0", "lower", false);
            install_multi(&manager, &dir.source());
            let generation = manager.route("/upper").unwrap().unwrap().generation;
            assert!(manager.can_copy_text("com.example.activation", "upper", generation));
            let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            *manager.after_uncertain_close.lock().unwrap() = Some(Arc::new({
                let weak = Arc::downgrade(&manager);
                let observed = observed.clone();
                move || {
                    let manager = weak.upgrade().unwrap();
                    assert!(!manager.bundles.is_terminal());
                    assert!(manager
                        .bundles
                        .bundle("com.example.activation")
                        .unwrap()
                        .is_some());
                    assert!(!manager.can_copy_text("com.example.activation", "upper", generation));
                    observed.store(true, Ordering::SeqCst);
                }
            }));
            let current = dir
                .path()
                .join("public-plugins/state/com.example.activation/state.json");
            fs::remove_file(&current).unwrap();
            fs::create_dir(&current).unwrap();
            let result = if rename {
                manager.rename_command("com.example.activation", "upper", Some("renamed"))
            } else {
                manager.set_command_favorite("com.example.activation", "upper", true)
            };
            assert!(result.is_err());
            assert!(observed.load(Ordering::SeqCst));
            assert!(manager.bundles.is_terminal());
        }
    }

    #[tokio::test]
    async fn shutdown_first_delayed_management_migration_recovery_rollback_and_repair_cannot_reopen(
    ) {
        for operation in [
            "rename",
            "favorite",
            "migration-rename",
            "migration-favorite",
            "recovery",
            "rollback",
            "repair",
        ] {
            let dir = TestDir::new("shutdown-delayed-mutation");
            write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
            let (gate, barrier) = PluginNetworkAuthorityGate::paused_response_for_test(200);
            let mut manager = manager_with_network_authority(&dir, gate.clone());
            let now = Instant::now();
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
            let current = dir
                .path()
                .join("public-plugins/state/com.example.network/state.json");
            if operation.starts_with("migration") {
                let mut wire: Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
                let command =
                    wire.as_object_mut().unwrap().remove("commands").unwrap()["default"].clone();
                wire["schema"] = json!(1);
                wire["defaultName"] = command["defaultName"].clone();
                wire["nameOverride"] = command["nameOverride"].clone();
                wire["favorite"] = json!(false);
                fs::write(&current, serde_json::to_vec(&wire).unwrap()).unwrap();
                manager.state = Arc::new(
                    PluginStateStore::load(
                        &dir.path().join("public-plugins/state"),
                        ["find".into(), "math".into()],
                    )
                    .unwrap(),
                );
            }
            let manager = Arc::new(manager);
            let old = dispatch_network_scheduled(&manager);
            let call = gate
                .admit(
                    "com.example.network",
                    old.context.plugin_generation,
                    &old.context,
                    &manager.scheduler,
                    network_request(),
                )
                .unwrap();
            barrier.release.notify_one();
            let delivered = gate
                .execute_before_delivery_for_test(call, manager.scheduler.clone(), || {
                    match operation {
                        "rename" | "favorite" | "migration-rename" | "migration-favorite" => {
                            *manager.after_state_durable.lock().unwrap() = Some(Arc::new({
                                let gate = gate.clone();
                                move || {
                                    gate.shutdown();
                                }
                            }));
                            let result = if operation.ends_with("rename") {
                                manager.rename("com.example.network", Some("renamed"))
                            } else {
                                manager.set_favorite("com.example.network", true)
                            };
                            assert_eq!(result.is_ok(), operation.ends_with("favorite"));
                            let wire: Value =
                                serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
                            assert_eq!(wire["schema"], 2);
                        }
                        "recovery" => {
                            let transaction = manager
                                .begin_uninstall("com.example.network", true)
                                .unwrap()
                                .unwrap();
                            let candidate = manager
                                .abort_uninstall_before_commit(transaction)
                                .unwrap()
                                .unwrap();
                            gate.shutdown();
                            assert!(manager.finish_runtime_recovery(&candidate).is_err());
                        }
                        "rollback" => {
                            write_network_package(&dir.source(), "0.9.0", &["api.example.com"]);
                            let prepared =
                                manager.prepare("main", source(&dir.source()), now).unwrap();
                            assert!(manager
                                .commit_with_readiness(
                                    "main",
                                    &prepared.token,
                                    BTreeSet::from([PublicPermission::NetworkHttps]),
                                    now,
                                    |_| {
                                        gate.shutdown();
                                        true
                                    }
                                )
                                .is_err());
                        }
                        "repair" => {
                            let backup = current.with_file_name("state.json.backup");
                            fs::copy(&current, &backup).unwrap();
                            fs::write(&current, b"broken primary").unwrap();
                            gate.shutdown();
                            assert!(
                                PublicPluginManager::load_with_timer_and_network_dependencies(
                                    dir.path(),
                                    PublicPluginHost::current(PublicPlatform::Windows),
                                    [],
                                    Arc::new(MessageCenterService::load(dir.path())),
                                    Arc::new(TestClock::default()),
                                    Arc::new(TestPublisher::successful()),
                                    gate.clone()
                                )
                                .is_err()
                            );
                            let wire: Value =
                                serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
                            assert_eq!(wire["permissionGrants"], json!([]));
                            assert_eq!(wire["networkHttpsHostsGrant"], json!([]));
                        }
                        _ => unreachable!(),
                    }
                    assert!(gate.begin_transition().is_err());
                    assert!(gate
                        .admit(
                            "com.example.network",
                            old.context.plugin_generation,
                            &old.context,
                            &manager.scheduler,
                            network_request()
                        )
                        .is_err());
                })
                .await;
            assert_eq!(
                delivered,
                Err(PluginNetworkErrorCode::ExpiredRequest),
                "{operation}"
            );
            assert_eq!(gate.global_active_for_test(), 0);
        }
    }

    #[test]
    fn credential_save_rejects_every_unconfirmed_reservation_phase_before_mutation() {
        for phase in 0..3 {
            let dir = TestDir::new("credential-save-unconfirmed");
            let manager = installed_credential_manager(&dir);
            let running = dispatch_network_scheduled(&manager);
            let reservation = manager.bundles.reserve("com.example.network").unwrap();
            if phase >= 1 {
                reservation.begin_committing().unwrap();
            }
            if phase >= 2 {
                reservation.mark_durable().unwrap();
            }
            assert!(manager
                .save_secret("com.example.network", "token", Some("synthetic-next"))
                .is_err());
            assert!(manager
                .secrets
                .read_bearer("com.example.network", "token")
                .unwrap()
                .is_none());
            assert_eq!(
                manager.scheduler.context_status(&running.context),
                PluginContextStatus::Current
            );
        }
    }

    #[test]
    fn credential_restart_checks_actual_secret_record_before_authority_publication() {
        for value in [
            None,
            Some("synthetic-valid"),
            Some("invalid bearer with spaces"),
        ] {
            let dir = TestDir::new("credential-restart-check");
            let manager = installed_credential_manager(&dir);
            if let Some(value) = value {
                manager
                    .secrets
                    .write("com.example.network", "token", value)
                    .unwrap();
            }
            drop(manager);
            let restarted = credential_manager(&dir);
            let running = dispatch_network_scheduled(&restarted);
            let result = restarted.network_authority.admit(
                "com.example.network",
                running.context.plugin_generation,
                &running.context,
                &restarted.scheduler,
                network_request(),
            );
            assert_eq!(result.is_ok(), value != Some("invalid bearer with spaces"));
        }
    }

    #[test]
    fn credential_save_retires_without_releasing_running_and_empty_settings_are_noop() {
        let dir = TestDir::new("credential-save-retire");
        write_credential_package(
            &dir.source(),
            "1.0.0",
            json!([
                {"id":"service", "settingKey":"token", "host":"api.example.com", "type":"bearer"}
            ]),
        );
        let manager = credential_manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let running = dispatch_network_scheduled(&manager);
        let revision = manager.inventory().unwrap().revision;
        manager
            .save_settings("com.example.network", &BTreeMap::new())
            .unwrap();
        assert_eq!(manager.inventory().unwrap().revision, revision);
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Current
        );
        manager
            .save_secret("com.example.network", "token", Some("synthetic-next"))
            .unwrap();
        assert_eq!(
            manager.scheduler.context_status(&running.context),
            PluginContextStatus::Expired
        );
        assert!(matches!(
            manager
                .scheduler
                .enqueue(running.candidate.clone(), now)
                .unwrap(),
            PluginScheduleOutcome::Waiting { .. }
        ));
        let completion = manager.scheduler.complete(&running.context, now).unwrap();
        assert!(!completion.accepted);
        assert!(completion.next.is_some());
    }

    #[test]
    fn credential_install_consent_revision_and_unchanged_upgrade_preserve_revocation() {
        let dir = TestDir::new("credential-consent");
        let manager = credential_manager(&dir);
        let now = Instant::now();
        let credentials = json!([{"id":"service", "settingKey":"token", "host":"api.example.com", "type":"bearer"}]);
        write_credential_package(&dir.source(), "1.0.0", credentials.clone());
        let cancelled = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert_eq!(
            cancelled.network.as_ref().unwrap().added_credential_ids,
            ["service"]
        );
        manager.cancel("main", &cancelled.token, now).unwrap();
        assert!(manager.inventory().unwrap().items.is_empty());
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let inventory = manager.inventory().unwrap();
        assert_eq!(
            inventory.items[0].network.as_ref().unwrap().credentials[0].setting_key,
            "token"
        );
        // Revision is mandatory even for an already-granted no-op.
        assert!(manager
            .set_network_access("com.example.network", true)
            .is_err());
        manager
            .set_network_access("com.example.network", false)
            .unwrap();
        assert!(manager
            .set_network_access_with_revision(
                "com.example.network",
                true,
                Some(&inventory.revision)
            )
            .is_err());
        let revision = manager.inventory().unwrap().revision;
        manager
            .set_network_access_with_revision("com.example.network", true, Some(&revision))
            .unwrap();
        manager
            .set_network_access("com.example.network", false)
            .unwrap();
        write_credential_package(&dir.source(), "1.1.0", credentials);
        let unchanged = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert!(!unchanged.network.as_ref().unwrap().requires_network_consent);
        manager
            .commit_with_readiness(
                "main",
                &unchanged.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let config = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(config.network_credentials_grant.is_empty());
        assert!(config.network_https_hosts_grant.is_empty());
        let revision = manager.inventory().unwrap().revision;
        manager
            .set_network_access_with_revision("com.example.network", true, Some(&revision))
            .unwrap();
        assert_eq!(
            manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap()
                .network_credentials_grant
                .len(),
            1
        );
        manager.uninstall("com.example.network", true).unwrap();
        let reinstalled = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert!(!reinstalled.is_update);
        assert_eq!(
            reinstalled.network.as_ref().unwrap().added_credential_ids,
            ["service"]
        );
        manager
            .commit_with_readiness(
                "main",
                &reinstalled.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        assert_eq!(
            manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap()
                .network_credentials_grant
                .len(),
            1
        );
    }

    #[test]
    fn credential_updates_compare_full_tuples_and_current_declarations_and_bind_previous_config() {
        for change in ["settingKey", "host", "id", "reorder", "remove"] {
            let dir = TestDir::new(&format!("credential-update-{change}"));
            let manager = credential_manager(&dir);
            let now = Instant::now();
            let initial = json!([
                {"id":"a", "settingKey":"token", "host":"api.example.com", "type":"bearer"},
                {"id":"b", "settingKey":"token", "host":"api.example.com", "type":"bearer"}
            ]);
            write_credential_package(&dir.source(), "1.0.0", initial.clone());
            let first = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &first.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
            let mut next = initial.clone();
            match change {
                "settingKey" => next[0]["settingKey"] = json!("other"),
                "host" => next[0]["host"] = json!("auth.example.com"),
                "id" => next[0]["id"] = json!("c"),
                "reorder" => next.as_array_mut().unwrap().reverse(),
                "remove" => {
                    next.as_array_mut().unwrap().remove(0);
                }
                _ => unreachable!(),
            }
            write_credential_package(&dir.source(), "1.1.0", next);
            let update = manager.prepare("main", source(&dir.source()), now).unwrap();
            let network = update.network.as_ref().unwrap();
            assert_eq!(
                network.requires_network_consent,
                !matches!(change, "reorder" | "remove")
            );
            assert_eq!(
                network.added_credential_ids.len(),
                usize::from(!matches!(change, "reorder" | "remove"))
            );
            if change == "remove" {
                manager
                    .commit_with_readiness(
                        "main",
                        &update.token,
                        BTreeSet::from([PublicPermission::NetworkHttps]),
                        now,
                        |_| true,
                    )
                    .unwrap();
                assert_eq!(
                    manager
                        .state
                        .config("com.example.network")
                        .unwrap()
                        .unwrap()
                        .network_credentials_grant
                        .len(),
                    1
                );
                write_credential_package(&dir.source(), "1.2.0", initial);
                let readded = manager.prepare("main", source(&dir.source()), now).unwrap();
                assert_eq!(
                    readded.network.as_ref().unwrap().added_credential_ids,
                    ["a"]
                );
                manager.cancel("main", &readded.token, now).unwrap();
            } else {
                manager
                    .set_network_access("com.example.network", false)
                    .unwrap();
                assert!(matches!(
                    manager.commit_with_readiness(
                        "main",
                        &update.token,
                        BTreeSet::from([PublicPermission::NetworkHttps]),
                        now,
                        |_| true
                    ),
                    Err(PublicPluginManagementError::Unavailable)
                ));
                assert_eq!(
                    manager
                        .state
                        .config("com.example.network")
                        .unwrap()
                        .unwrap()
                        .version,
                    "1.0.0"
                );
            }
        }
    }

    #[test]
    fn credential_successful_tuple_updates_persist_exact_grants_across_restart() {
        for change in ["settingKey", "host", "id", "reorder"] {
            let dir = TestDir::new(&format!("credential-successful-update-{change}"));
            let manager = credential_manager(&dir);
            let now = Instant::now();
            let mut next = json!([
                {"id":"a", "settingKey":"token", "host":"api.example.com", "type":"bearer"},
                {"id":"b", "settingKey":"token", "host":"api.example.com", "type":"bearer"}
            ]);
            write_credential_package(&dir.source(), "1.0.0", next.clone());
            let initial = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &initial.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
            let initial_generation = manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap()
                .active_generation;
            match change {
                "settingKey" => next[0]["settingKey"] = json!("other"),
                "host" => next[0]["host"] = json!("auth.example.com"),
                "id" => next[0]["id"] = json!("c"),
                "reorder" => next.as_array_mut().unwrap().reverse(),
                _ => unreachable!(),
            }
            // Build expected order independently of manifest/state canonicalization.
            let mut expected_json = next.as_array().unwrap().clone();
            expected_json.sort_by(|a, b| a["id"].as_str().unwrap().cmp(b["id"].as_str().unwrap()));
            let expected: Vec<super::super::manifest::PluginNetworkCredential> =
                serde_json::from_value(json!(expected_json)).unwrap();
            let hosts =
                BTreeSet::from(["api.example.com".to_owned(), "auth.example.com".to_owned()]);
            write_credential_package(&dir.source(), "1.1.0", next);
            let update = manager.prepare("main", source(&dir.source()), now).unwrap();
            let network = update.network.as_ref().unwrap();
            assert_eq!(network.requires_network_consent, change != "reorder");
            assert_eq!(
                network.added_credential_ids,
                match change {
                    "id" => vec!["c".to_owned()],
                    "reorder" => vec![],
                    _ => vec!["a".to_owned()],
                }
            );
            manager
                .commit_with_readiness(
                    "main",
                    &update.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    now,
                    |_| true,
                )
                .unwrap();
            let check = |current: &PublicPluginManager| {
                let config = current
                    .state
                    .config("com.example.network")
                    .unwrap()
                    .unwrap();
                assert_eq!(config.version, "1.1.0", "{change}");
                assert_eq!(
                    config.permission_grants,
                    BTreeSet::from([PublicPermission::NetworkHttps])
                );
                assert_eq!(config.network_https_hosts_grant, hosts);
                assert_eq!(config.network_credentials_grant, expected);
                let manifest = current.installed_manifest(&config).unwrap();
                let grant = config.network_grant_snapshot(&manifest).unwrap();
                assert_eq!(grant.credentials, expected);
                assert_eq!(grant.https_hosts, hosts);
                let running = dispatch_network_scheduled(current);
                // Missing records permit ordinary HTTPS admission; no secret or transport I/O.
                assert!(current
                    .network_authority
                    .admit(
                        "com.example.network",
                        running.context.plugin_generation,
                        &running.context,
                        &current.scheduler,
                        network_request()
                    )
                    .is_ok());
                config
            };
            let committed = check(&manager);
            assert!(committed.active_generation > initial_generation);
            let disk: Value = serde_json::from_slice(
                &fs::read(
                    dir.path()
                        .join("public-plugins/state/com.example.network/state.json"),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(disk["version"], "1.1.0");
            assert_eq!(disk["permissionGrants"], json!(["network.https"]));
            assert_eq!(
                disk["networkHttpsHostsGrant"],
                json!(["api.example.com", "auth.example.com"])
            );
            assert_eq!(disk["networkCredentialsGrant"], json!(expected_json));
            drop(manager);
            let restarted = credential_manager(&dir);
            let reloaded = check(&restarted);
            assert_eq!(reloaded.version, committed.version);
            assert_eq!(reloaded.permission_grants, committed.permission_grants);
            assert_eq!(
                reloaded.network_https_hosts_grant,
                committed.network_https_hosts_grant
            );
            assert_eq!(
                reloaded.network_credentials_grant,
                committed.network_credentials_grant
            );
        }
    }

    #[test]
    fn credential_manifest_semantic_mismatch_on_restart_is_durably_revoked() {
        let dir = TestDir::new("credential-semantic-restart");
        write_credential_package(
            &dir.source(),
            "1.0.0",
            json!([
                {"id":"service", "settingKey":"token", "host":"api.example.com", "type":"bearer"}
            ]),
        );
        let manager = credential_manager(&dir);
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        drop(manager);
        let path = dir
            .path()
            .join("public-plugins/state/com.example.network/state.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document["networkCredentialsGrant"][0]["settingKey"] = json!("other");
        fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        let restarted = credential_manager(&dir);
        let config = restarted
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(config.network_credentials_grant.is_empty());
        assert!(config.network_https_hosts_grant.is_empty());
        let disk: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(disk.get("networkCredentialsGrant").is_none());
        assert_eq!(disk["permissionGrants"], json!([]));
    }

    #[tokio::test]
    async fn credential_manager_network_request_uses_host_secret_store_before_fake_transport() {
        let dir = TestDir::new("credential-network-read");
        write_credential_package(
            &dir.source(),
            "1.0.0",
            json!([
                {"id":"service", "settingKey":"token", "host":"api.example.com", "type":"bearer"}
            ]),
        );
        let (gate, started) = PluginNetworkAuthorityGate::blocking_for_test();
        let manager = Arc::new(
            PublicPluginManager::load_with_timer_and_network_dependencies(
                dir.path(),
                PublicPluginHost {
                    version: [0, 4, 0],
                    ..PublicPluginHost::current(PublicPlatform::Windows)
                },
                ["find".into(), "math".into()],
                Arc::new(MessageCenterService::load(dir.path())),
                Arc::new(TestClock::default()),
                Arc::new(TestPublisher::successful()),
                gate.clone(),
            )
            .unwrap(),
        );
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let scheduled = dispatch_network_scheduled(&manager);
        let caller = PluginRuntimeIdentity {
            plugin_id: scheduled.context.plugin_id.clone(),
            generation: scheduled.context.plugin_generation,
        };
        let input = PluginNetworkCommandInput {
            context: scheduled.context.clone(),
            request: PluginNetworkRequest {
                credential: Some("service".into()),
                ..network_request()
            },
        };
        assert_eq!(
            manager.request_network(caller.clone(), input.clone()).await,
            Err(PluginNetworkErrorCode::CredentialUnavailable)
        );
        manager
            .secrets
            .write("com.example.network", "token", "synthetic-manager-token")
            .unwrap();
        let pending = tokio::spawn({
            let manager = manager.clone();
            async move { manager.request_network(caller, input).await }
        });
        tokio::time::timeout(Duration::from_millis(500), started.notified())
            .await
            .unwrap();
        assert_eq!(gate.close_plugin("com.example.network"), 1);
        assert_eq!(
            pending.await.unwrap(),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
    }

    fn manager_with_schema_one_primary(
        dir: &TestDir,
        include_favorite: bool,
    ) -> (PublicPluginManager, PathBuf, Vec<u8>) {
        let mut manager = installed_credential_manager(dir);
        let root = dir.path().join("public-plugins/state");
        let current = root.join("com.example.network/state.json");
        let mut wire: Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
        let command = wire.as_object_mut().unwrap().remove("commands").unwrap()["default"].clone();
        wire["schema"] = json!(1);
        wire["defaultName"] = command["defaultName"].clone();
        wire["nameOverride"] = command["nameOverride"].clone();
        if include_favorite {
            wire["favorite"] = json!(false);
        }
        let raw = format!("  {}\n", serde_json::to_string_pretty(&wire).unwrap()).into_bytes();
        fs::write(&current, &raw).unwrap();
        // Isolate pure state read from the normal startup activation transaction.
        manager.state =
            Arc::new(PluginStateStore::load(&root, ["find".into(), "math".into()]).unwrap());
        (manager, current, raw)
    }

    #[test]
    fn schema_one_inventory_route_noops_and_first_mutation_retry_use_authoritative_bytes() {
        for include_favorite in [false, true] {
            for rename in [false, true] {
                let dir = TestDir::new("schema-one-manager-first-write");
                let (manager, current, raw) =
                    manager_with_schema_one_primary(&dir, include_favorite);
                let before = manager
                    .state
                    .config("com.example.network")
                    .unwrap()
                    .unwrap();
                let running = dispatch_network_scheduled(&manager);
                assert_eq!(
                    manager.inventory().unwrap().items[0]
                        .commands
                        .iter()
                        .find(|command| command.command_id == "default")
                        .unwrap()
                        .effective_name,
                    "network"
                );
                assert_eq!(
                    manager.route("/network sample").unwrap().unwrap().input,
                    "sample"
                );
                manager.rename("com.example.network", None).unwrap();
                manager.set_favorite("com.example.network", false).unwrap();
                assert_eq!(
                    manager.scheduler.context_status(&running.context),
                    PluginContextStatus::Current
                );
                assert_eq!(fs::read(&current).unwrap(), raw);
                assert_eq!(
                    manager
                        .state
                        .config("com.example.network")
                        .unwrap()
                        .unwrap(),
                    before
                );
                let backup = current.with_file_name("state.json.backup");
                if backup.exists() {
                    fs::remove_file(&backup).unwrap();
                }
                fs::create_dir(&backup).unwrap();
                let mutate = || {
                    if rename {
                        manager.rename("com.example.network", Some("renamed"))
                    } else {
                        manager.set_favorite("com.example.network", true)
                    }
                };
                assert!(mutate().is_err());
                assert!(!manager.bundles.is_terminal());
                assert_eq!(
                    manager.scheduler.context_status(&running.context),
                    PluginContextStatus::Current
                );
                assert_eq!(
                    manager
                        .state
                        .config("com.example.network")
                        .unwrap()
                        .unwrap(),
                    before
                );
                assert_eq!(fs::read(&current).unwrap(), raw);
                assert_eq!(
                    manager.inventory().unwrap().revision,
                    before.inventory_revision.to_string()
                );
                fs::remove_dir(&backup).unwrap();
                let committed = mutate().unwrap();
                assert_eq!(committed.inventory_revision, before.inventory_revision + 1);
                assert_eq!(fs::read(&backup).unwrap(), raw);
                let wire: Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
                assert_eq!(wire["schema"], 2);
                if !rename {
                    assert_eq!(
                        manager.scheduler.context_status(&running.context),
                        PluginContextStatus::Current
                    );
                }
            }
        }
    }

    #[test]
    fn schema_one_first_rename_and_favorite_unknown_or_failed_revalidation_fail_closed() {
        for rename in [false, true] {
            for unknown in [false, true] {
                let dir = TestDir::new("schema-one-manager-uncertain");
                let (manager, current, _) = manager_with_schema_one_primary(&dir, true);
                let before = manager
                    .state
                    .config("com.example.network")
                    .unwrap()
                    .unwrap();
                let running = dispatch_network_scheduled(&manager);
                let admitted = manager
                    .network_authority
                    .admit(
                        "com.example.network",
                        running.context.plugin_generation,
                        &running.context,
                        &manager.scheduler,
                        network_request(),
                    )
                    .unwrap();
                if unknown {
                    fs::remove_file(&current).unwrap();
                    fs::create_dir(&current).unwrap();
                } else {
                    let backup = current.with_file_name("state.json.backup");
                    if backup.exists() {
                        fs::remove_file(&backup).unwrap();
                    }
                    fs::create_dir(&backup).unwrap();
                    fs::write(&current, b"different authoritative bytes").unwrap();
                }
                let result = if rename {
                    manager.rename("com.example.network", Some("renamed"))
                } else {
                    manager.set_favorite("com.example.network", true)
                };
                assert!(result.is_err());
                assert!(manager.bundles.is_terminal());
                assert_eq!(
                    manager
                        .state
                        .config("com.example.network")
                        .unwrap()
                        .unwrap(),
                    before
                );
                assert_eq!(
                    manager.scheduler.context_status(&running.context),
                    PluginContextStatus::Expired
                );
                assert!(admitted.is_cancelled_for_test());
                assert!(manager
                    .network_authority
                    .admit(
                        "com.example.network",
                        running.context.plugin_generation,
                        &running.context,
                        &manager.scheduler,
                        network_request()
                    )
                    .is_err());
                assert!(manager
                    .bundles
                    .confirmed_bundle("com.example.network")
                    .is_err());
            }
        }
    }

    #[test]
    fn valid_schema_one_startup_reads_inventory_and_route_without_a_migration_transaction() {
        for include_favorite in [false, true] {
            let dir = TestDir::new("schema-one-startup");
            let (manager, current, raw) = manager_with_schema_one_primary(&dir, include_favorite);
            let before = manager
                .state
                .config("com.example.network")
                .unwrap()
                .unwrap();
            drop(manager);
            let reloaded = credential_manager(&dir);
            assert_eq!(
                reloaded
                    .state
                    .config("com.example.network")
                    .unwrap()
                    .unwrap(),
                before
            );
            assert_eq!(
                reloaded.inventory().unwrap().items[0]
                    .commands
                    .iter()
                    .find(|command| command.command_id == "default")
                    .unwrap()
                    .effective_name,
                "network"
            );
            assert!(reloaded.route("/network").unwrap().is_some());
            assert_eq!(fs::read(current).unwrap(), raw);
        }
    }

    #[cfg(windows)]
    #[test]
    fn startup_semantic_repair_write_failure_publishes_no_network_authority() {
        use std::os::windows::fs::OpenOptionsExt;
        for schema_one in [false, true] {
            let dir = TestDir::new("startup-repair-write-failure");
            let (manager, current, _) = manager_with_schema_one_primary(&dir, true);
            let running = dispatch_network_scheduled(&manager);
            let mut wire: Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
            if !schema_one {
                let command = json!({"defaultName":wire["defaultName"],"nameOverride":wire["nameOverride"],"favorite":wire["favorite"]});
                for field in ["defaultName", "nameOverride", "favorite"] {
                    wire.as_object_mut().unwrap().remove(field);
                }
                wire["schema"] = json!(2);
                wire["commands"] = json!({"default":command});
            }
            wire["networkHttpsHostsGrant"] = json!(["invalid host"]);
            let source = serde_json::to_vec(&wire).unwrap();
            fs::write(&current, &source).unwrap();
            drop(manager);
            // Allow reading, but prevent the repair from replacing the authoritative file.
            let held = fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&current)
                .unwrap();
            let (gate, _) = PluginNetworkAuthorityGate::blocking_for_test();
            let result = PublicPluginManager::load_with_timer_and_network_dependencies(
                dir.path(),
                PublicPluginHost::current(PublicPlatform::Windows),
                [],
                Arc::new(MessageCenterService::load(dir.path())),
                Arc::new(TestClock::default()),
                Arc::new(TestPublisher::successful()),
                Arc::clone(&gate),
            );
            assert!(result.is_err());
            assert!(gate
                .admit(
                    "com.example.network",
                    running.context.plugin_generation,
                    &running.context,
                    &PluginRequestScheduler::default(),
                    network_request()
                )
                .is_err());
            assert_eq!(fs::read(&current).unwrap(), source);
            drop(held);
        }
    }
    fn result_action_fixture(
        manager: &PublicPluginManager,
        route: PublicPluginRoute,
        copy: bool,
    ) -> (
        crate::result_registry::ResultRegistry,
        super::super::result_action::ResultActionOrigin,
    ) {
        use super::super::result_action::*;
        let registry = crate::result_registry::ResultRegistry::default();
        registry.on_show("result-action".into());
        let token = registry
            .begin_query(
                crate::result_registry::QueryDomain::Plugin,
                "result-action",
                1,
            )
            .unwrap();
        let rows = vec![PublicActionResult {
            id: "same".into(),
            title: "Row".into(),
            subtitle: None,
            detail: None,
            actions: vec![if copy {
                PublicResultAction::CopyText {
                    id: "copy".into(),
                    title: "Copy".into(),
                    text: "test-only-copy".into(),
                }
            } else {
                PublicResultAction::Runtime {
                    id: "save".into(),
                    title: "Save".into(),
                    payload: Value::Null,
                }
            }],
            default_action_id: None,
        }];
        let prepared = registry
            .prepare_result_actions(route.clone(), rows)
            .unwrap();
        let response = manager
            .publish_result_actions(&route, copy.into(), || {
                registry.publish_prepared_result_actions(token, prepared)
            })
            .unwrap()
            .unwrap();
        let row = &response.items[0];
        let action = &row.result_actions.as_ref().unwrap().actions[0];
        let origin = registry
            .resolve_result_action(&ExecuteResultActionRequest {
                request_id: response.request_id,
                result_id: row.result_id.clone(),
                action_id: action.action_id.clone(),
            })
            .unwrap();
        (registry, origin)
    }
    fn opt_in_result_package(path: &Path) {
        let file = path.join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["runtime"]["resultActions"] = json!(true);
        value["minimumHostVersion"] = json!("0.10.0");
        fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn url_result_rows() -> Vec<super::super::result_action::PublicActionResult> {
        use super::super::result_action::*;
        vec![PublicActionResult {
            id: "url".into(),
            title: "Link".into(),
            subtitle: None,
            detail: None,
            actions: vec![
                PublicResultAction::OpenUrl {
                    id: "open".into(),
                    title: "Open".into(),
                    url: "https://example.com/Case?private=1".into(),
                },
                PublicResultAction::CopyText {
                    id: "copy".into(),
                    title: "Copy".into(),
                    text: "private-copy".into(),
                },
                PublicResultAction::Runtime {
                    id: "save".into(),
                    title: "Save".into(),
                    payload: Value::Null,
                },
            ],
            default_action_id: Some("copy".into()),
        }]
    }
    fn install_url_result_package(manager: &PublicPluginManager, path: &Path) {
        write_multi_package(path, "1.1.0", "lower", false);
        opt_in_result_package(path);
        let file = path.join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["minimumHostVersion"] = json!("0.12.0");
        value["permissions"] = json!(["clipboard.write", "url.open"]);
        fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
        let now = Instant::now();
        let prepared = manager.prepare("main", source(path), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::ClipboardWrite, PublicPermission::UrlOpen]),
                now,
                |_| true,
            )
            .unwrap();
    }
    // This publishes through the real registry, optionally bypassing owner validation
    // so denial tests can present an otherwise valid private source to admission.
    fn url_result_source(
        manager: &PublicPluginManager,
        checked: bool,
    ) -> (
        crate::result_registry::ResultRegistry,
        Vec<super::super::result_action::ResultActionOrigin>,
    ) {
        use super::super::result_action::*;
        let registry = crate::result_registry::ResultRegistry::default();
        registry.on_show("url-action".into());
        let token = registry
            .begin_query(crate::result_registry::QueryDomain::Plugin, "url-action", 1)
            .unwrap();
        let route = manager.route("/upper").unwrap().unwrap();
        let rows = url_result_rows();
        let requirements = ResultActionRequirements::rows(&rows);
        let prepared = registry
            .prepare_result_actions(route.clone(), rows)
            .unwrap();
        let publish = || registry.publish_prepared_result_actions(token, prepared);
        let response = if checked {
            manager
                .publish_result_actions(&route, requirements, publish)
                .unwrap()
        } else {
            publish()
        }
        .unwrap();
        let wire = serde_json::to_string(&response).unwrap();
        for private in ["example.com", "private=1", "openUrl", "private-copy"] {
            assert!(!wire.contains(private));
        }
        let row = &response.items[0];
        let origins = row
            .result_actions
            .as_ref()
            .unwrap()
            .actions
            .iter()
            .map(|action| {
                registry
                    .resolve_result_action(&ExecuteResultActionRequest {
                        request_id: response.request_id.clone(),
                        result_id: row.result_id.clone(),
                        action_id: action.action_id.clone(),
                    })
                    .unwrap()
            })
            .collect();
        (registry, origins)
    }
    fn paste_result_rows() -> Vec<super::super::result_action::PublicActionResult> {
        use super::super::result_action::*;
        vec![PublicActionResult {
            id: "url".into(),
            title: "Link".into(),
            subtitle: None,
            detail: None,
            actions: vec![
                PublicResultAction::PasteText {
                    id: "paste".into(),
                    title: "Paste".into(),
                    text: "private-paste".into(),
                },
                PublicResultAction::CopyText {
                    id: "copy".into(),
                    title: "Copy".into(),
                    text: "private-copy".into(),
                },
                PublicResultAction::Runtime {
                    id: "save".into(),
                    title: "Save".into(),
                    payload: Value::Null,
                },
            ],
            default_action_id: Some("copy".into()),
        }]
    }
    fn install_paste_result_package(manager: &PublicPluginManager, path: &Path) {
        write_multi_package(path, "1.1.0", "lower", false);
        opt_in_result_package(path);
        let file = path.join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["minimumHostVersion"] = json!("0.14.0");
        value["permissions"] = json!(["clipboard.write", "clipboard.paste"]);
        fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
        let now = Instant::now();
        let prepared = manager.prepare("main", source(path), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([
                    PublicPermission::ClipboardWrite,
                    PublicPermission::ClipboardPaste,
                ]),
                now,
                |_| true,
            )
            .unwrap();
    }
    // This publishes through the real registry, optionally bypassing owner validation
    // so denial tests can present an otherwise valid private source to admission.
    fn paste_result_source(
        manager: &PublicPluginManager,
        checked: bool,
    ) -> (
        crate::result_registry::ResultRegistry,
        Vec<super::super::result_action::ResultActionOrigin>,
    ) {
        use super::super::result_action::*;
        let registry = crate::result_registry::ResultRegistry::default();
        registry.on_show("url-action".into());
        let token = registry
            .begin_query(crate::result_registry::QueryDomain::Plugin, "url-action", 1)
            .unwrap();
        let route = manager.route("/upper").unwrap().unwrap();
        let rows = paste_result_rows();
        let requirements = ResultActionRequirements::rows(&rows);
        let prepared = registry
            .prepare_result_actions(route.clone(), rows)
            .unwrap();
        let publish = || registry.publish_prepared_result_actions(token, prepared);
        let response = if checked {
            manager
                .publish_result_actions(&route, requirements, publish)
                .unwrap()
        } else {
            publish()
        }
        .unwrap();
        let wire = serde_json::to_string(&response).unwrap();
        for private in ["private-paste", "pasteText", "private-copy"] {
            assert!(!wire.contains(private));
        }
        let row = &response.items[0];
        let origins = row
            .result_actions
            .as_ref()
            .unwrap()
            .actions
            .iter()
            .map(|action| {
                registry
                    .resolve_result_action(&ExecuteResultActionRequest {
                        request_id: response.request_id.clone(),
                        result_id: row.result_id.clone(),
                        action_id: action.action_id.clone(),
                    })
                    .unwrap()
            })
            .collect();
        (registry, origins)
    }
    fn revoke_result_grant(manager: &PublicPluginManager, permission: PublicPermission) {
        let bundle = manager
            .bundles
            .confirmed_bundle("com.example.activation")
            .unwrap()
            .unwrap();
        let mut config = bundle.config.clone();
        config.permission_grants.remove(&permission);
        let reservation = manager.bundles.reserve("com.example.activation").unwrap();
        reservation.begin_committing().unwrap();
        reservation.mark_durable().unwrap();
        reservation
            .publish(Some(bundle.with_config(config)))
            .unwrap();
    }

    #[test]
    fn permission_access_regrant_never_revives_old_result_actions() {
        for permission in [PublicPermission::UrlOpen, PublicPermission::ClipboardWrite] {
            let dir = TestDir::new("permission-access-old-results");
            let manager = manager(&dir);
            install_url_result_package(&manager, &dir.source());
            let (registry, origins) = url_result_source(&manager, true);
            for granted in [false, true] {
                manager
                    .set_permission_access_with_effects(
                        "com.example.activation",
                        permission,
                        granted,
                        &manager.inventory().unwrap().revision,
                        &mut ManagementEffects::default(),
                    )
                    .unwrap();
                assert!(matches!(
                    manager.open_url_result_action(&registry, &origins[0], |_| panic!(
                        "retired URL effect"
                    )),
                    super::super::result_action::ResultActionOutcome::Failed { .. }
                ));
            }
            let (fresh_registry, fresh_origins) = url_result_source(&manager, true);
            assert!(matches!(
                manager.open_url_result_action(&fresh_registry, &fresh_origins[0], |_| Ok(())),
                super::super::result_action::ResultActionOutcome::UrlOpened
            ));
        }
    }
    #[test]
    fn result_action_paste_consumes_once_and_releases_mutation_before_hide() {
        use super::super::result_action::*;
        for fail in [false, true] {
            let dir = TestDir::new("paste-claim");
            let manager = manager(&dir);
            install_paste_result_package(&manager, &dir.source());
            let (registry, origins) = paste_result_source(&manager, true);
            assert_eq!(
                origins[0].requirements,
                ResultActionRequirements {
                    copy: true,
                    url: false,
                    paste: true
                }
            );
            let mut calls = 0;
            assert!(matches!(
                manager.write_paste_result_action(
                    &registry,
                    &origins[0],
                    || Err(ResultActionErrorCode::PasteTargetUnavailable),
                    |_| panic!("lease changed before claim")
                ),
                Err(ResultActionOutcome::Failed {
                    source_consumed: false,
                    ..
                })
            ));
            let result = manager.write_paste_result_action(
                &registry,
                &origins[0],
                || Ok(()),
                |text| {
                    assert_eq!(text, "private-paste");
                    calls += 1;
                    assert!(manager.mutation.try_lock().is_err());
                    assert!(registry.claim_result_action(&origins[0], None).is_err());
                    if fail {
                        Err(())
                    } else {
                        Ok(42)
                    }
                },
            );
            assert_eq!(result.is_ok(), !fail);
            assert_eq!(calls, 1);
            assert!(manager.mutation.try_lock().is_ok());
            if let Err(ResultActionOutcome::Failed {
                source_consumed, ..
            }) = result
            {
                assert!(source_consumed);
            }
            assert!(matches!(
                manager.copy_result_action(&registry, &origins[1], |_| panic!("sibling replay")),
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::ActionAlreadyExecuted,
                    source_consumed: true
                }
            ));
            registry.hide_and_clear(); // Reentrant clear is safe after every manager guard has left.
            assert!(manager
                .write_paste_result_action(
                    &registry,
                    &origins[0],
                    || Ok(()),
                    |_| panic!("hidden replay")
                )
                .is_err());
        }
    }
    #[test]
    fn result_action_paste_snapshot_grants_and_stale_query_prevent_every_write() {
        use super::super::result_action::*;
        for permission in [
            PublicPermission::ClipboardPaste,
            PublicPermission::ClipboardWrite,
        ] {
            let dir = TestDir::new("paste-grant");
            let manager = manager(&dir);
            install_paste_result_package(&manager, &dir.source());
            let (registry, origins) = paste_result_source(&manager, true);
            revoke_result_grant(&manager, permission);
            assert!(matches!(
                manager.write_paste_result_action(
                    &registry,
                    &origins[0],
                    || Ok(()),
                    |_| panic!("revoked write")
                ),
                Err(ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::PluginPermissionDenied,
                    source_consumed: false
                })
            ));
            assert!(matches!(
                manager.copy_result_action(&registry, &origins[1], |_| panic!("weakened copy")),
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::PluginPermissionDenied,
                    source_consumed: false
                }
            ));
            assert!(manager
                .publish_result_actions(&origins[0].route, origins[0].requirements, || panic!(
                    "revoked publication"
                ))
                .is_err());
        }
        let dir = TestDir::new("paste-stale-query");
        let manager = manager(&dir);
        install_paste_result_package(&manager, &dir.source());
        let (registry, origins) = paste_result_source(&manager, true);
        registry
            .begin_query(crate::result_registry::QueryDomain::Plugin, "url-action", 2)
            .unwrap();
        assert!(manager
            .write_paste_result_action(
                &registry,
                &origins[0],
                || Ok(()),
                |_| panic!("stale query write")
            )
            .is_err());
    }
    #[test]
    fn result_action_paste_concurrent_source_has_one_claim() {
        let dir = TestDir::new("paste-concurrent");
        let manager = Arc::new(manager(&dir));
        install_paste_result_package(&manager, &dir.source());
        let (registry, origins) = paste_result_source(&manager, true);
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let manager = manager.clone();
                let registry = registry.clone();
                let origin = origins[0].clone();
                let writes = writes.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    manager
                        .write_paste_result_action(
                            &registry,
                            &origin,
                            || Ok(()),
                            |_| {
                                writes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                Ok(42)
                            },
                        )
                        .is_ok()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|success| *success)
                .count(),
            1
        );
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn result_action_url_opens_once_and_failure_consumes_every_sibling() {
        use super::super::result_action::*;
        for fail in [false, true] {
            let dir = TestDir::new("url-single-claim");
            let manager = manager(&dir);
            install_url_result_package(&manager, &dir.source());
            let (registry, origins) = url_result_source(&manager, true);
            assert_eq!(
                origins[0].requirements,
                ResultActionRequirements {
                    copy: true,
                    url: true,
                    paste: false
                }
            );
            let mut calls = 0;
            let outcome = manager.open_url_result_action(&registry, &origins[0], |url| {
                calls += 1;
                assert_eq!(url.as_str(), "https://example.com/Case?private=1");
                assert!(manager.mutation.try_lock().is_err());
                // The registry and scheduler locks are released before external effect.
                assert!(registry
                    .resolve_result_action(&ExecuteResultActionRequest {
                        request_id: origins[0].ticket.request_id().into(),
                        result_id: "absent".into(),
                        action_id: "absent".into()
                    })
                    .is_err());
                assert!(manager.scheduler.result_publication_current(
                    &origins[0].route.plugin_id,
                    origins[0].route.result_publication.as_ref().unwrap()
                ));
                if fail {
                    Err(())
                } else {
                    Ok(())
                }
            });
            assert_eq!(calls, 1);
            if fail {
                assert!(matches!(
                    outcome,
                    ResultActionOutcome::Failed {
                        code: ResultActionErrorCode::UrlOpenFailed,
                        source_consumed: true
                    }
                ));
            } else {
                assert!(matches!(outcome, ResultActionOutcome::UrlOpened));
            }
            assert!(matches!(
                manager.open_url_result_action(&registry, &origins[0], |_| panic!("URL replay")),
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::ActionAlreadyExecuted,
                    source_consumed: true
                }
            ));
            assert!(matches!(
                manager.copy_result_action(&registry, &origins[1], |_| panic!("copy sibling")),
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::ActionAlreadyExecuted,
                    source_consumed: true
                }
            ));
            let reserved = manager.reserve_result_action(&origins[2]).unwrap();
            assert!(manager
                .admit_result_action(&registry, &origins[2], reserved, Instant::now())
                .is_err());
        }
    }
    #[test]
    fn result_action_url_denied_stale_disabled_and_invalid_sources_have_zero_effects() {
        use super::super::result_action::*;
        for case in 0..8 {
            let dir = TestDir::new("url-denials");
            let manager = manager(&dir);
            if case == 0 {
                write_multi_package(&dir.source(), "1.0.0", "lower", false);
                opt_in_result_package(&dir.source());
                install_multi(&manager, &dir.source());
            } else {
                install_url_result_package(&manager, &dir.source());
            }
            let (registry, mut origins) = url_result_source(&manager, case != 0);
            match case {
                0 => {} // undeclared
                1 => revoke_result_grant(&manager, PublicPermission::UrlOpen),
                2 => revoke_result_grant(&manager, PublicPermission::ClipboardWrite),
                3 => {
                    manager
                        .set_enabled_with_readiness("com.example.activation", false, |_| true)
                        .unwrap();
                }
                4 => {
                    origins[0].route.generation += 1;
                }
                5 => {
                    registry
                        .begin_query(crate::result_registry::QueryDomain::Plugin, "url-action", 2)
                        .unwrap();
                }
                6 => registry.hide_and_clear(),
                _ => {
                    let PublicResultAction::OpenUrl { url, .. } = &mut origins[0].action else {
                        panic!()
                    };
                    *url = "https://@example.com".into();
                }
            }
            assert!(
                matches!(
                    manager.open_url_result_action(&registry, &origins[0], |_| panic!(
                        "denied URL effect"
                    )),
                    ResultActionOutcome::Failed {
                        source_consumed: false,
                        ..
                    }
                ),
                "case {case}"
            );
            if case <= 2 {
                assert!(manager
                    .publish_result_actions(&origins[0].route, origins[0].requirements, || panic!(
                        "ungranted publication"
                    ))
                    .is_err());
                assert!(matches!(
                    manager.copy_result_action(&registry, &origins[1], |_| panic!(
                        "mixed snapshot copy"
                    )),
                    ResultActionOutcome::Failed {
                        code: ResultActionErrorCode::PluginPermissionDenied,
                        source_consumed: false
                    }
                ));
                let reserved = manager.reserve_result_action(&origins[2]).unwrap();
                assert!(matches!(
                    manager.admit_result_action(&registry, &origins[2], reserved, Instant::now()),
                    Err(ResultActionErrorCode::PluginPermissionDenied)
                ));
            }
        }
    }
    #[test]
    fn result_action_url_admitted_effect_can_retire_source_without_holding_registry_lock() {
        use super::super::result_action::*;
        let dir = TestDir::new("url-retire-effect");
        let manager = manager(&dir);
        install_url_result_package(&manager, &dir.source());
        let (registry, origins) = url_result_source(&manager, true);
        assert!(matches!(
            manager.open_url_result_action(&registry, &origins[0], |_| {
                registry
                    .begin_query(crate::result_registry::QueryDomain::Plugin, "url-action", 2)
                    .unwrap();
                Ok(())
            }),
            ResultActionOutcome::UrlOpened
        ));
        assert!(!origins[0].lease.live());
        assert!(matches!(
            manager.open_url_result_action(&registry, &origins[0], |_| panic!("retired replay")),
            ResultActionOutcome::Failed {
                code: ResultActionErrorCode::StaleRequest,
                ..
            }
        ));
    }
    #[test]
    fn result_action_runtime_replacement_preserves_full_url_and_copy_requirements() {
        use super::super::result_action::*;
        for declared in [false, true] {
            let dir = TestDir::new("url-replacement");
            let manager = manager(&dir);
            if declared {
                install_url_result_package(&manager, &dir.source());
            } else {
                write_multi_package(&dir.source(), "1.0.0", "lower", false);
                opt_in_result_package(&dir.source());
                install_multi(&manager, &dir.source());
            }
            let route = manager.route("/upper").unwrap().unwrap();
            let (registry, origin) = result_action_fixture(&manager, route.clone(), false);
            let now = Instant::now();
            let reserved = manager.reserve_result_action(&origin).unwrap();
            let dispatch = manager
                .admit_result_action(&registry, &origin, reserved, now)
                .unwrap();
            assert!(
                manager
                    .scheduler
                    .complete(&dispatch.context, now)
                    .unwrap()
                    .accepted
            );
            let rows = url_result_rows();
            let requirements = ResultActionRequirements::rows(&rows);
            let prepared = registry.prepare_result_actions(route, rows).unwrap();
            let result = manager.finish_result_action(
                &registry,
                &origin,
                &dispatch.context,
                prepared,
                requirements,
            );
            if !declared {
                assert!(matches!(
                    result,
                    Err(ResultActionErrorCode::PluginPermissionDenied)
                ));
                continue;
            }
            let response = result.unwrap();
            let row = &response.items[0];
            let copy = registry
                .resolve_result_action(&ExecuteResultActionRequest {
                    request_id: response.request_id.clone(),
                    result_id: row.result_id.clone(),
                    action_id: row.result_actions.as_ref().unwrap().actions[1]
                        .action_id
                        .clone(),
                })
                .unwrap();
            assert_eq!(copy.requirements, requirements);
            revoke_result_grant(&manager, PublicPermission::UrlOpen);
            assert!(matches!(
                manager.copy_result_action(&registry, &copy, |_| panic!("replacement weakened")),
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::PluginPermissionDenied,
                    ..
                }
            ));
        }
    }
    #[test]
    fn result_action_url_update_denied_consent_preserves_installed_config_and_data() {
        let dir = TestDir::new("url-update-consent");
        let manager = manager(&dir);
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        opt_in_result_package(&dir.source());
        install_multi(&manager, &dir.source());
        manager
            .rename_command("com.example.activation", "upper", Some("retained"))
            .unwrap();
        let scope = PluginDataScope::new("com.example.activation").unwrap();
        manager
            .storage
            .set(
                &scope,
                "com.example.activation",
                "snippet.latest",
                json!({"schemaVersion":1,"revision":3,"text":"saved"}),
            )
            .unwrap();
        let before = manager
            .state
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        write_multi_package(&dir.source(), "1.1.0", "lower", false);
        opt_in_result_package(&dir.source());
        let file = dir.source().join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["minimumHostVersion"] = json!("0.12.0");
        value["permissions"] = json!(["clipboard.write", "url.open"]);
        fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert!(prepared.is_update);
        assert!(prepared.permissions.contains(&PublicPermission::UrlOpen));
        assert_eq!(
            manager.commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::ClipboardWrite]),
                now,
                |_| panic!("denied readiness")
            ),
            Err(PublicPluginManagementError::PermissionDenied)
        );
        assert_eq!(
            manager
                .state
                .config("com.example.activation")
                .unwrap()
                .unwrap(),
            before
        );
        assert_eq!(
            manager
                .storage
                .get(&scope, "com.example.activation", "snippet.latest")
                .unwrap(),
            Some(json!({"schemaVersion":1,"revision":3,"text":"saved"}))
        );
        assert!(manager.route("/retained").unwrap().is_some());
    }
    #[test]
    fn result_action_runtime_replacement_preserves_full_paste_and_copy_requirements() {
        use super::super::result_action::*;
        for declared in [false, true] {
            let dir = TestDir::new("paste-replacement");
            let manager = manager(&dir);
            if declared {
                install_paste_result_package(&manager, &dir.source());
            } else {
                write_multi_package(&dir.source(), "1.0.0", "lower", false);
                opt_in_result_package(&dir.source());
                install_multi(&manager, &dir.source());
            }
            let route = manager.route("/upper").unwrap().unwrap();
            let (registry, origin) = result_action_fixture(&manager, route.clone(), false);
            let now = Instant::now();
            let reserved = manager.reserve_result_action(&origin).unwrap();
            let dispatch = manager
                .admit_result_action(&registry, &origin, reserved, now)
                .unwrap();
            assert!(
                manager
                    .scheduler
                    .complete(&dispatch.context, now)
                    .unwrap()
                    .accepted
            );
            let rows = paste_result_rows();
            let requirements = ResultActionRequirements::rows(&rows);
            let prepared = registry.prepare_result_actions(route, rows).unwrap();
            let result = manager.finish_result_action(
                &registry,
                &origin,
                &dispatch.context,
                prepared,
                requirements,
            );
            if !declared {
                assert!(matches!(
                    result,
                    Err(ResultActionErrorCode::PluginPermissionDenied)
                ));
                continue;
            }
            let response = result.unwrap();
            let row = &response.items[0];
            let copy = registry
                .resolve_result_action(&ExecuteResultActionRequest {
                    request_id: response.request_id.clone(),
                    result_id: row.result_id.clone(),
                    action_id: row.result_actions.as_ref().unwrap().actions[1]
                        .action_id
                        .clone(),
                })
                .unwrap();
            assert_eq!(copy.requirements, requirements);
            revoke_result_grant(&manager, PublicPermission::ClipboardPaste);
            assert!(matches!(
                manager.copy_result_action(&registry, &copy, |_| panic!("replacement weakened")),
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::PluginPermissionDenied,
                    ..
                }
            ));
        }
    }
    #[test]
    fn result_action_paste_update_denied_consent_preserves_installed_config_and_data() {
        let dir = TestDir::new("paste-update-consent");
        let manager = manager(&dir);
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        opt_in_result_package(&dir.source());
        install_multi(&manager, &dir.source());
        manager
            .rename_command("com.example.activation", "upper", Some("retained"))
            .unwrap();
        let scope = PluginDataScope::new("com.example.activation").unwrap();
        manager
            .storage
            .set(
                &scope,
                "com.example.activation",
                "snippet.latest",
                json!({"schemaVersion":1,"revision":3,"text":"saved"}),
            )
            .unwrap();
        let before = manager
            .state
            .config("com.example.activation")
            .unwrap()
            .unwrap();
        write_multi_package(&dir.source(), "1.1.0", "lower", false);
        opt_in_result_package(&dir.source());
        let file = dir.source().join("plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["minimumHostVersion"] = json!("0.14.0");
        value["permissions"] = json!(["clipboard.write", "clipboard.paste"]);
        fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        assert!(prepared.is_update);
        assert!(prepared
            .permissions
            .contains(&PublicPermission::ClipboardPaste));
        assert_eq!(
            manager.commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::ClipboardWrite]),
                now,
                |_| panic!("denied readiness")
            ),
            Err(PublicPluginManagementError::PermissionDenied)
        );
        assert_eq!(
            manager
                .state
                .config("com.example.activation")
                .unwrap()
                .unwrap(),
            before
        );
        assert_eq!(
            manager
                .storage
                .get(&scope, "com.example.activation", "snippet.latest")
                .unwrap(),
            Some(json!({"schemaVersion":1,"revision":3,"text":"saved"}))
        );
        assert!(manager.route("/retained").unwrap().is_some());
    }
    #[test]
    fn result_action_initial_publication_and_copy_hold_actual_owner_mutation_barrier() {
        use super::super::result_action::*;
        for copy in [false, true] {
            let dir = TestDir::new("result-action-owner-barrier");
            write_multi_package(&dir.source(), "1.0.0", "lower", false);
            opt_in_result_package(&dir.source());
            let manager = Arc::new(manager(&dir));
            install_multi(&manager, &dir.source());
            let route = manager.route("/upper saved").unwrap().unwrap();
            let (registry, origin) = result_action_fixture(&manager, route.clone(), copy);
            let (entered, observed) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let worker = {
                let manager = manager.clone();
                let origin = origin.clone();
                let registry = registry.clone();
                thread::spawn(move || {
                    if copy {
                        assert!(matches!(
                            manager.copy_result_action(&registry, &origin, |text| {
                                assert_eq!(text, "test-only-copy");
                                entered.send(()).unwrap();
                                wait.recv().unwrap();
                                Ok(())
                            }),
                            ResultActionOutcome::Copied
                        ));
                    } else {
                        manager
                            .publish_result_actions(&origin.route, false.into(), || {
                                entered.send(()).unwrap();
                                wait.recv().unwrap();
                            })
                            .unwrap();
                    }
                })
            };
            observed.recv().unwrap();
            let (done, finished) = mpsc::channel();
            let mutation = {
                let manager = manager.clone();
                thread::spawn(move || {
                    let result =
                        manager.rename_command("com.example.activation", "upper", Some("changed"));
                    done.send(result.is_ok()).unwrap();
                })
            };
            assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
            release.send(()).unwrap();
            worker.join().unwrap();
            assert!(finished.recv_timeout(Duration::from_secs(2)).unwrap());
            mutation.join().unwrap();
            assert!(manager
                .publish_result_actions(&route, copy.into(), || panic!("stale owner published"))
                .is_err());
        }
    }
    #[test]
    fn result_action_copy_failure_consumes_and_denied_owner_never_executes_fake_effect() {
        use super::super::result_action::*;
        let dir = TestDir::new("result-action-copy-consume");
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        opt_in_result_package(&dir.source());
        let manager = manager(&dir);
        install_multi(&manager, &dir.source());
        let route = manager.route("/upper").unwrap().unwrap();
        let (registry, origin) = result_action_fixture(&manager, route, true);
        assert!(matches!(
            manager.copy_result_action(&registry, &origin, |_| Err(())),
            ResultActionOutcome::Failed {
                code: ResultActionErrorCode::ClipboardWriteFailed,
                source_consumed: true
            }
        ));
        assert!(matches!(
            manager.copy_result_action(&registry, &origin, |_| panic!("copy replay")),
            ResultActionOutcome::Failed {
                code: ResultActionErrorCode::ActionAlreadyExecuted,
                source_consumed: true
            }
        ));
        manager
            .set_enabled_with_readiness("com.example.activation", false, |_| true)
            .unwrap();
        assert!(matches!(
            manager.copy_result_action(&registry, &origin, |_| panic!("denied effect")),
            ResultActionOutcome::Failed {
                source_consumed: false,
                ..
            }
        ));
    }
    #[tokio::test]
    async fn result_action_retired_lease_blocks_actual_apis_and_network_delivery_without_cleanup() {
        use super::super::runtime::{PluginApiOperation, PluginApiRequest};
        let dir = TestDir::new("result-action-api-lease");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        opt_in_result_package(&dir.source());
        let (authority, barrier) = PluginNetworkAuthorityGate::paused_response_for_test(200);
        let manager = Arc::new(manager_with_network_authority(&dir, authority.clone()));
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let route = manager.route("/network").unwrap().unwrap();
        let (registry, origin) = result_action_fixture(&manager, route, false);
        let reserved = manager.reserve_result_action(&origin).unwrap();
        let request = manager
            .admit_result_action(&registry, &origin, reserved, now)
            .unwrap();
        let api = |operation| PluginApiRequest {
            context: request.context.clone(),
            operation,
            key: Some("draft".into()),
            value: Some(json!("test")),
            notification: None,
        };
        assert!(manager
            .execute_api(
                &origin.route.runtime_label,
                api(PluginApiOperation::StorageSet)
            )
            .result
            .is_ok());
        let call = authority
            .admit(
                "com.example.network",
                request.context.plugin_generation,
                &request.context,
                &manager.scheduler,
                network_request(),
            )
            .unwrap();
        let pending = tokio::spawn({
            let authority = authority.clone();
            let scheduler = manager.scheduler.clone();
            async move {
                authority
                    .execute_before_delivery_for_test(call, scheduler, || {})
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), barrier.reached.notified())
            .await
            .unwrap();
        // Revoke under registry only. Do not call service cleanup or scheduler retirement.
        registry.retire_result_actions(&request_source_id(&registry, &origin));
        assert!(!origin.lease.live());
        for operation in [
            PluginApiOperation::StorageGet,
            PluginApiOperation::StorageSet,
            PluginApiOperation::StorageRemove,
            PluginApiOperation::SettingGet,
            PluginApiOperation::SecretConfigured,
            PluginApiOperation::NotificationPublish,
            PluginApiOperation::NotificationSchedule,
        ] {
            assert_eq!(
                manager
                    .execute_api(&origin.route.runtime_label, api(operation))
                    .result,
                Err(PluginRuntimeError::ExpiredRequest)
            );
        }
        assert_eq!(
            authority
                .admit(
                    "com.example.network",
                    request.context.plugin_generation,
                    &request.context,
                    &manager.scheduler,
                    network_request()
                )
                .err(),
            Some(PluginNetworkErrorCode::ExpiredRequest)
        );
        barrier.release.notify_one();
        assert_eq!(
            pending.await.unwrap(),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
        assert!(
            !manager
                .scheduler
                .complete(&request.context, now)
                .unwrap()
                .accepted
        );
    }
    fn request_source_id(
        _registry: &crate::result_registry::ResultRegistry,
        origin: &super::super::result_action::ResultActionOrigin,
    ) -> String {
        origin.ticket.request_id().to_owned()
    }

    #[test]
    fn result_action_publication_token_tracks_business_changes_but_preserves_favorites_and_noops() {
        for setting in [false, true] {
            let dir = TestDir::new("result-action-business-token");
            write_multi_package(&dir.source(), "1.0.0", "lower", false);
            opt_in_result_package(&dir.source());
            let manager = manager(&dir);
            install_multi(&manager, &dir.source());
            let route = manager.route("/upper saved").unwrap().unwrap();
            let (registry, origin) = result_action_fixture(&manager, route.clone(), true);
            manager
                .set_command_favorite("com.example.activation", "upper", true)
                .unwrap();
            manager
                .rename_command("com.example.activation", "upper", None)
                .unwrap();
            manager
                .save_settings(
                    "com.example.activation",
                    &BTreeMap::from([("compact".into(), json!(false))]),
                )
                .unwrap();
            assert!(manager
                .publish_result_actions(&route, true.into(), || ())
                .is_ok());
            if setting {
                manager
                    .save_settings(
                        "com.example.activation",
                        &BTreeMap::from([("compact".into(), json!(true))]),
                    )
                    .unwrap();
            } else {
                manager
                    .rename_command("com.example.activation", "upper", Some("renamed"))
                    .unwrap();
            }
            assert!(manager
                .publish_result_actions(&route, true.into(), || panic!("stale publication"))
                .is_err());
            assert!(matches!(
                manager.copy_result_action(&registry, &origin, |_| panic!("stale copy")),
                super::super::result_action::ResultActionOutcome::Failed {
                    source_consumed: false,
                    ..
                }
            ));
            let mut runtime = origin.clone();
            runtime.action = super::super::result_action::PublicResultAction::Runtime {
                id: "save".into(),
                title: "Save".into(),
                payload: Value::Null,
            };
            let reserved = manager.reserve_result_action(&runtime);
            assert!(
                reserved.is_err()
                    || manager
                        .admit_result_action(&registry, &runtime, reserved.unwrap(), Instant::now())
                        .is_err()
            );
            let fresh = manager
                .route(if setting {
                    "/upper saved"
                } else {
                    "/renamed saved"
                })
                .unwrap()
                .unwrap();
            assert!(manager
                .publish_result_actions(&fresh, true.into(), || ())
                .is_ok());
        }
    }
    #[test]
    fn result_action_route_capture_cannot_pair_old_bundle_with_post_rename_token() {
        let dir = TestDir::new("result-action-route-capture");
        write_multi_package(&dir.source(), "1.0.0", "lower", false);
        opt_in_result_package(&dir.source());
        let manager = Arc::new(manager(&dir));
        install_multi(&manager, &dir.source());
        let (entered, observed) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Arc::new(std::sync::Mutex::new(wait));
        *manager.before_result_route_token.lock().unwrap() = Some(Arc::new(move || {
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
        }));
        let route_worker = {
            let manager = manager.clone();
            thread::spawn(move || manager.route("/upper saved").unwrap().unwrap())
        };
        observed.recv().unwrap();
        let (done, finished) = mpsc::channel();
        let rename = {
            let manager = manager.clone();
            thread::spawn(move || {
                done.send(manager.rename_command(
                    "com.example.activation",
                    "upper",
                    Some("renamed"),
                ))
                .unwrap();
            })
        };
        assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
        release.send(()).unwrap();
        let captured = route_worker.join().unwrap();
        finished
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        rename.join().unwrap();
        *manager.before_result_route_token.lock().unwrap() = None;
        assert_eq!(captured.command_label, "upper");
        assert!(!captured.result_publication.as_ref().unwrap().live());
        let fresh = manager.route("/renamed saved").unwrap().unwrap();
        assert_eq!(fresh.command_label, "renamed");
        assert!(fresh.result_publication.as_ref().unwrap().live());
        assert_ne!(captured.result_publication, fresh.result_publication);
    }
    #[tokio::test]
    async fn result_action_review_plugin_domain_invalidation_cancels_pending_network_without_deadline(
    ) {
        use super::super::result_action::*;
        let dir = TestDir::new("result-action-review-network-cancel");
        write_network_package(&dir.source(), "1.0.0", &["api.example.com"]);
        opt_in_result_package(&dir.source());
        let (authority, started) = PluginNetworkAuthorityGate::blocking_for_test();
        let manager = Arc::new(manager_with_network_authority(&dir, authority));
        let now = Instant::now();
        let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                now,
                |_| true,
            )
            .unwrap();
        let route = manager.route("/network").unwrap().unwrap();
        let (registry, origin) = result_action_fixture(&manager, route.clone(), false);
        let service = Arc::new(super::super::PublicPluginService::default());
        assert!(service.manager.set(manager.clone()).is_ok());
        let legacy = Arc::new(crate::plugins::PluginManager::new());
        let (arm, _watch) = mpsc::channel();
        let (action, receiver) = service
            .prepare_result_action_with_observer(legacy.clone(), registry.clone(), origin, || {
                Ok(arm)
            })
            .unwrap();
        let pending = tokio::spawn({
            let manager = manager.clone();
            let context = action.context.clone();
            let caller = super::super::parse_runtime_label(&route.runtime_label).unwrap();
            async move {
                manager
                    .request_network(
                        caller,
                        super::super::PluginNetworkCommandInput {
                            context,
                            request: network_request(),
                        },
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        let outcome = super::super::ManagementOutcome {
            result: Err::<(), _>(PublicPluginManagementError::Unavailable),
            changed: true,
        };
        assert!(crate::commands::finish_public_plugin_management(
            &service,
            outcome,
            &registry,
            &legacy,
            || {}
        )
        .is_err());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), pending)
                .await
                .unwrap()
                .unwrap(),
            Err(PluginNetworkErrorCode::ExpiredRequest)
        );
        assert!(matches!(
            receiver.try_recv(),
            Ok(ResultActionOutcome::Failed {
                code: ResultActionErrorCode::ActionCancelled,
                source_consumed: true
            })
        ));
        assert!(
            !service.settle_result_action(&action.context, ResultActionErrorCode::ActionTimedOut)
        );
        let next = service
            .schedule_command(
                manager.route("/network next").unwrap().unwrap(),
                2,
                "/network next".into(),
                Instant::now(),
            )
            .unwrap();
        assert!(next.dispatch.is_none());
        assert!(manager
            .expire_network_timeouts(action.deadline - Duration::from_millis(1))
            .unwrap()
            .is_empty());
    }
    #[test]
    fn file_open_manager_rejects_revoked_grants_and_runtime_recovery_without_reserving_runtime() {
        use super::super::file_open::FileOpenError as E;
        for mode in ["ui-grant", "file-grant", "recovery"] {
            let dir = TestDir::new(&format!("file-open-{mode}"));
            write_package(&dir.source(), "1.0.0", "fixture");
            let path = dir.source().join("plugin.json");
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["minimumHostVersion"] = json!("0.13.0");
            value["command"]["activationMode"] = json!("submit");
            value["command"]["outputMode"] = json!("window");
            value["window"] = json!({"entry":"dist/window.html"});
            value["permissions"] = json!(["ui.window", "files.openText"]);
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            fs::write(dir.source().join("dist/window.html"), "<html></html>").unwrap();
            let manager = manager(&dir);
            let now = Instant::now();
            let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::UiWindow, PublicPermission::FilesOpenText]),
                    now,
                    |_| true,
                )
                .unwrap();
            let bundle = manager
                .bundles
                .confirmed_bundle("com.example.activation")
                .unwrap()
                .unwrap();
            let owner = crate::plugin_window::PluginWindowOwner {
                ui_intent_epoch: 1,
                submission_token: "test".into(),
                plugin_id: "com.example.activation".into(),
                plugin_generation: bundle.runtime.generation,
                activation_id: bundle.activation_id,
                admission_epoch: bundle.admission_epoch,
                request_id: "test".into(),
                control_value: "/activation".into(),
            };
            assert_eq!(manager.authorize_file_open(&owner), Ok(()));
            let replacement = if mode == "recovery" {
                bundle.with_runtime_recovery(true)
            } else {
                let mut config = bundle.config.clone();
                config.permission_grants.remove(&if mode == "ui-grant" {
                    PublicPermission::UiWindow
                } else {
                    PublicPermission::FilesOpenText
                });
                bundle.with_config(config)
            };
            let reservation = manager.bundles.reserve("com.example.activation").unwrap();
            reservation.begin_committing().unwrap();
            reservation.mark_durable().unwrap();
            reservation.publish(Some(replacement)).unwrap();
            assert_eq!(
                manager.authorize_file_open(&owner),
                Err(if mode == "recovery" {
                    E::ExpiredWindowSessionError
                } else {
                    E::FileOpenPermissionError
                })
            );
        }
    }
    fn write_host_service_package(dir: &TestDir) {
        write_network_package(&dir.source(), "1.0.0", &["openapi.youdao.com"]);
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["minimumHostVersion"] = json!("0.15.0");
        manifest["permissions"] = json!([
            "network.https",
            "credentials.youdaoTranslation",
            "credentials.baiduOcr",
            "credentials.deepseek"
        ]);
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }
    #[tokio::test]
    async fn host_services_independent_grants_caller_and_config_mutation_retire_pending_request() {
        let dir = TestDir::new("host-service-revocation");
        write_host_service_package(&dir);
        let (gate, started) = PluginNetworkAuthorityGate::blocking_for_test();
        let manager = Arc::new(manager_with_network_authority(&dir, gate));
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        let config = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::CredentialsYoudaoTranslation));
        let store = Arc::new(crate::credentials::CredentialStore::new(
            dir.path().join("synthetic-host-credentials"),
        ));
        store.synthetic_youdao_for_test();
        let context = dispatch_network_request(&manager);
        let caller = PluginRuntimeIdentity {
            plugin_id: context.plugin_id.clone(),
            generation: context.plugin_generation,
        };
        let command = || {
            serde_json::from_value::<super::super::host_services::YoudaoCommandInput>(
                json!({"context":context,"input":{"text":"hello","from":"en","to":"zh-CHS"}}),
            )
            .unwrap()
        };
        assert_eq!(
            manager
                .youdao_translate(caller.clone(), command(), &store)
                .await
                .unwrap_err()
                .code,
            "servicePermissionDenied"
        );
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::CredentialsBaiduOcr,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        manager
            .scheduler
            .complete(&context, Instant::now())
            .unwrap();
        // Granting OCR cannot authorize translation.
        let context = dispatch_network_request(&manager);
        let command: super::super::host_services::YoudaoCommandInput = serde_json::from_value(
            json!({"context":context,"input":{"text":"hello","from":"en","to":"zh-CHS"}}),
        )
        .unwrap();
        assert_eq!(
            manager
                .youdao_translate(caller.clone(), command, &store)
                .await
                .unwrap_err()
                .code,
            "servicePermissionDenied"
        );
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::CredentialsYoudaoTranslation,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        manager
            .scheduler
            .complete(&context, Instant::now())
            .unwrap();
        let context = dispatch_network_request(&manager);
        let command = serde_json::from_value(
            json!({"context":context,"input":{"text":"hello","from":"en","to":"zh-CHS"}}),
        )
        .unwrap();
        let task = {
            let manager = manager.clone();
            let store = store.clone();
            tokio::spawn(async move { manager.youdao_translate(caller, command, &store).await })
        };
        started.notified().await;
        store.disable_youdao_for_test();
        assert_eq!(task.await.unwrap().unwrap_err().code, "expiredRequest");
        assert_eq!(
            manager.scheduler.context_status(&context),
            super::super::PluginContextStatus::Expired
        );
    }

    #[tokio::test]
    async fn host_services_result_already_returned_to_js_cannot_complete_after_configuration_mutation(
    ) {
        let dir = TestDir::new("host-service-js-late");
        write_host_service_package(&dir);
        let (gate, barrier) = PluginNetworkAuthorityGate::paused_body_for_test(
            200,
            br#"{"errorCode":"0","translation":["hello"]}"#.to_vec(),
        );
        let manager = Arc::new(manager_with_network_authority(&dir, gate));
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::CredentialsYoudaoTranslation,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        let store = Arc::new(crate::credentials::CredentialStore::new(
            dir.path().join("synthetic-host"),
        ));
        store.synthetic_youdao_for_test();
        let context = dispatch_network_request(&manager);
        let caller = PluginRuntimeIdentity {
            plugin_id: context.plugin_id.clone(),
            generation: context.plugin_generation,
        };
        let command = serde_json::from_value(
            json!({"context":context,"input":{"text":"你好","from":"zh-CHS","to":"en"}}),
        )
        .unwrap();
        let task = {
            let manager = manager.clone();
            let store = store.clone();
            tokio::spawn(async move { manager.youdao_translate(caller, command, &store).await })
        };
        barrier.reached.notified().await;
        barrier.release.notify_one();
        assert_eq!(task.await.unwrap().unwrap().translations, vec!["hello"]);
        let rejected = serde_json::from_value(json!({"context":{"pluginId":context.plugin_id,"pluginGeneration":context.plugin_generation,"requestId":"forged-expired-request"},"input":{"text":"你好","from":"zh-CHS","to":"en"}})).unwrap();
        let caller = PluginRuntimeIdentity {
            plugin_id: context.plugin_id.clone(),
            generation: context.plugin_generation,
        };
        assert_eq!(
            manager
                .youdao_translate(caller, rejected, &store)
                .await
                .unwrap_err()
                .code,
            "expiredRequest"
        );

        assert_eq!(
            manager.scheduler.context_status(&context),
            super::super::PluginContextStatus::Current
        );
        let (revision, completion) = store
            .with_completion(&context, |revision| {
                (
                    revision,
                    manager
                        .scheduler
                        .complete(&context, Instant::now())
                        .unwrap(),
                )
            })
            .unwrap();
        assert!(completion.accepted);
        let (revision, publication) =
            revision.expect("native service revision survives completion");
        store.disable_youdao_for_test();
        // The already-settled legacy response is still fenced at actual registry publication.
        let published = std::cell::Cell::new(false);
        assert_eq!(
            store.with_revision(revision, || {
                published.set(true);
                Ok(())
            }),
            Err("expiredRequest")
        );
        assert!(!published.get());
        assert!(manager
            .scheduler
            .with_result_publication(&context.plugin_id, &publication, || ())
            .is_ok());
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::CredentialsYoudaoTranslation,
                false,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        assert!(manager
            .scheduler
            .with_result_publication(&context.plugin_id, &publication, || panic!(
                "revoked response cannot publish"
            ))
            .is_err());
    }
    #[test]
    fn selection_permission_defaults_off_and_preserves_explicit_choice_on_upgrade() {
        let dir = TestDir::new("selection-grant-upgrade");
        write_host_service_package(&dir);
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["minimumHostVersion"] = json!("0.16.0");
        manifest["permissions"] = json!(["network.https", "selection.read"]);
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let manager = manager(&dir);
        let permission: PublicPermission = serde_json::from_value(json!("selection.read")).unwrap();
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                prepared.permissions.iter().copied().collect(),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        assert!(!manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap()
            .permission_grants
            .contains(&permission));
        for (index, granted) in [true, false].into_iter().enumerate() {
            manager
                .set_permission_access_with_effects(
                    "com.example.network",
                    permission,
                    granted,
                    &manager.state.inventory_revision().unwrap().to_string(),
                    &mut ManagementEffects::default(),
                )
                .unwrap();
            manifest["version"] = json!(format!("1.0.{}", index + 1));
            fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let prepared = manager
                .prepare("main", source(&dir.source()), Instant::now())
                .unwrap();
            manager
                .commit_with_readiness(
                    "main",
                    &prepared.token,
                    BTreeSet::from([PublicPermission::NetworkHttps]),
                    Instant::now(),
                    |_| true,
                )
                .unwrap();
            assert_eq!(
                manager
                    .state
                    .config("com.example.network")
                    .unwrap()
                    .unwrap()
                    .permission_grants
                    .contains(&permission),
                granted
            );
        }
    }

    #[test]
    fn host_service_permissions_default_off_and_updates_preserve_only_explicit_grants() {
        let dir = TestDir::new("host-service-update");
        write_host_service_package(&dir);
        let manager = manager(&dir);
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                prepared.permissions.iter().copied().collect(),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        let config = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(config
            .permission_grants
            .iter()
            .all(|p| !p.is_host_service()));
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::CredentialsYoudaoTranslation,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        let manifest_path = dir.source().join("plugin.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["version"] = json!("1.0.1");
        fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        let config = manager
            .state
            .config("com.example.network")
            .unwrap()
            .unwrap();
        assert!(config
            .permission_grants
            .contains(&PublicPermission::CredentialsYoudaoTranslation));
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::CredentialsBaiduOcr));
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::CredentialsDeepseek));
    }
    #[tokio::test]
    async fn host_service_rejects_result_action_runtime_before_network() {
        let dir = TestDir::new("host-service-output-boundary");
        write_host_service_package(&dir);
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["runtime"]["resultActions"] = json!(true);
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let manager = manager(&dir);
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps]),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::CredentialsYoudaoTranslation,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        let context = dispatch_network_request(&manager);
        let caller = PluginRuntimeIdentity {
            plugin_id: context.plugin_id.clone(),
            generation: context.plugin_generation,
        };
        let command = serde_json::from_value(
            json!({"context":context,"input":{"text":"hello","from":"en","to":"zh-CHS"}}),
        )
        .unwrap();
        let store = crate::credentials::CredentialStore::new(dir.path().join("synthetic"));
        store.synthetic_youdao_for_test();
        assert_eq!(
            manager
                .youdao_translate(caller, command, &store)
                .await
                .unwrap_err()
                .code,
            "invalidServiceRequest"
        );
    }
    fn installed_translation_panel(
        dir: &TestDir,
        gate: Arc<PluginNetworkAuthorityGate>,
        plugin_id: &str,
    ) -> (
        PublicPluginManager,
        crate::plugin_panel::PanelSessionIdentity,
    ) {
        write_host_service_package(dir);
        let path = dir.source().join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["pluginId"] = json!(plugin_id);
        manifest["minimumHostVersion"] = json!("0.18.0");
        manifest["command"]["activationMode"] = json!("submit");
        manifest["command"]["outputMode"] = json!("panel");
        manifest["panel"] = json!({"entry":"dist/panel.html","runtimeActions":true});
        manifest["permissions"] = json!([
            "network.https",
            "credentials.youdaoTranslation",
            "ui.panel",
            "selection.read"
        ]);
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        fs::write(dir.source().join("dist/panel.html"), "<!doctype html>").unwrap();
        let mut manager = manager_with_network_authority(dir, gate);
        manager.host.version = [0, 18, 0];
        let prepared = manager
            .prepare("main", source(&dir.source()), Instant::now())
            .unwrap();
        manager
            .commit_with_readiness(
                "main",
                &prepared.token,
                BTreeSet::from([PublicPermission::NetworkHttps, PublicPermission::UiPanel]),
                Instant::now(),
                |_| true,
            )
            .unwrap();
        manager
            .set_permission_access_with_effects(
                plugin_id,
                PublicPermission::CredentialsYoudaoTranslation,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        let b = manager
            .bundles
            .confirmed_bundle(plugin_id)
            .unwrap()
            .unwrap();
        let identity = crate::plugin_panel::PanelSessionIdentity {
            session_epoch: 1,
            plugin_id: plugin_id.into(),
            generation: b.runtime.generation,
            activation_id: b.activation_id,
            admission_epoch: b.admission_epoch,
            command_label: "translate".into(),
            content_label: "translation-panel".into(),
            host_keys: vec![],
            host_key_focus: PanelHostKeyFocus::Content,
        };
        (manager, identity)
    }
    #[tokio::test]
    async fn host_services_panel_local_response_keeps_credentials_and_completion_fenced() {
        let dir = TestDir::new("translation-panel-local");
        let (gate, _started) = PluginNetworkAuthorityGate::blocking_for_test();
        let (manager, identity) = installed_translation_panel(&dir, gate, "com.example.network");
        let action = manager
            .admit_panel_action(
                &identity,
                super::super::panel_action::PanelActionRequest {
                    action: "translate".into(),
                    payload: Value::Null,
                },
                Instant::now(),
            )
            .unwrap();
        let context = action.context;
        let caller = PluginRuntimeIdentity {
            plugin_id: context.plugin_id.clone(),
            generation: context.plugin_generation,
        };
        let command = || {
            serde_json::from_value(
                json!({"context":context,"input":{"text":" hello ","from":"en","to":"en"}}),
            )
            .unwrap()
        };
        let store = crate::credentials::CredentialStore::new(dir.path().join("synthetic-panel"));
        assert_eq!(
            manager
                .youdao_translate(caller.clone(), command(), &store)
                .await
                .unwrap_err()
                .code,
            "credentialNotConfigured"
        );
        store.synthetic_youdao_for_test();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            manager.youdao_translate(caller.clone(), command(), &store),
        )
        .await
        .expect("same language must never reach blocking transport")
        .unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({"translations":[" hello "],"sourceLanguage":"en","targetLanguage":"en","unchanged":true})
        );
        assert_eq!(manager.network_authority.global_active_for_test(), 0);
        let completion = store
            .with_completion(&context, |_| {
                manager
                    .scheduler
                    .complete(&context, Instant::now())
                    .unwrap()
            })
            .unwrap();
        assert!(completion.accepted);
        store.disable_youdao_for_test();
        assert_eq!(
            manager.claim_panel_completion(&identity, &context),
            Err(super::super::panel_action::PanelActionError::ExpiredPanelSession)
        );
        assert_eq!(
            manager
                .youdao_translate(caller, command(), &store)
                .await
                .unwrap_err()
                .code,
            "expiredRequest"
        );
    }
    #[tokio::test]
    async fn host_services_panel_pending_provider_cannot_survive_revocation() {
        let dir = TestDir::new("translation-panel-revoked");
        let (gate, started) = PluginNetworkAuthorityGate::blocking_for_test();
        let (manager, identity) = installed_translation_panel(&dir, gate, "com.example.network");
        let manager = Arc::new(manager);
        let action = manager
            .admit_panel_action(
                &identity,
                super::super::panel_action::PanelActionRequest {
                    action: "translate".into(),
                    payload: Value::Null,
                },
                Instant::now(),
            )
            .unwrap();
        let context = action.context;
        let caller = PluginRuntimeIdentity {
            plugin_id: context.plugin_id.clone(),
            generation: context.plugin_generation,
        };
        let command = serde_json::from_value(
            json!({"context":context,"input":{"text":"hello","from":"auto","to":"zh-CHS"}}),
        )
        .unwrap();
        let store = Arc::new(crate::credentials::CredentialStore::new(
            dir.path().join("synthetic-panel"),
        ));
        store.synthetic_youdao_for_test();
        let task = {
            let manager = manager.clone();
            let store = store.clone();
            tokio::spawn(async move { manager.youdao_translate(caller, command, &store).await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
            .await
            .expect("authorized panel must reach fake provider");
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                PublicPermission::UiPanel,
                false,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        assert_eq!(task.await.unwrap().unwrap_err().code, "expiredRequest");
        assert!(
            !manager
                .scheduler
                .complete(&context, Instant::now())
                .unwrap()
                .accepted
        );
        assert!(manager.claim_panel_completion(&identity, &context).is_err());
    }

    #[test]
    fn selection_translation_accepts_authorized_panel_and_rechecks_ui_grant() {
        let dir = TestDir::new("selection-translation-panel");
        let (gate, _) = PluginNetworkAuthorityGate::blocking_for_test();
        let (manager, identity) = installed_translation_panel(&dir, gate, "com.uipilot.translate");
        assert_eq!(manager.selection_translation_authority(), None);
        manager
            .set_permission_access_with_effects(
                "com.uipilot.translate",
                PublicPermission::SelectionRead,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        assert_eq!(
            manager.selection_translation_authority(),
            Some(identity.generation)
        );
        manager
            .set_permission_access_with_effects(
                "com.uipilot.translate",
                PublicPermission::UiPanel,
                false,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
        assert_eq!(manager.selection_translation_authority(), None);
    }
}
