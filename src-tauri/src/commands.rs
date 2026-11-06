use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

use crate::{
    apps::{self, AppCache, Application},
    clipboard_history::{
        paste_clipboard_history_record, send_ctrl_v_to_foreground_target,
        ClipboardHistoryBridgeError, ClipboardHistoryCopyOutcome, ClipboardHistoryCopyStatus,
        ClipboardHistoryPasteError, ClipboardHistoryPasteOutcome, ClipboardHistoryPasteStatus,
        ClipboardHistorySnapshot,
    },
    file_index::{FileIndex, OpenIndexedPath},
    file_search::{
        everything::{EverythingSearchError, EverythingSearchState},
        windows::path_auth,
        FileCategory, FileExecutionAction, FileExecutionError, FileExecutionOutcome,
        FileIndexStatus, FileResultItem, FileSearchResponse, PublishedFileBatch,
        PublishedFileDraft,
    },
    find_window::{
        ExecutionHideAdmission, FindReadyStatus, FindWindowController, HideFinish,
        OpenFindCompletion,
    },
    hotkey::HotkeyKind,
    lifecycle::{
        self, ClipboardPasteTarget, CriticalReservation, LifecycleCoordinator, ReservationError,
        SettingsSaveError,
    },
    message_center::{
        MessageCenterError, MessageCenterService, MessageCenterSnapshot, MessageSummary,
    },
    model::{
        LauncherResultActivation, MainResultCommandContext, ResultFavorite, ResultIconKind,
        SearchResponse,
    },
    plugin_panel::{
        self, HostInputFocusAdvance, HostInputFocusIdentity, HostInputFocusOutcome,
        HostKeyEnqueueInput, HostKeyEnqueueOutcome, PanelAdmissionError, PanelBounds,
        PanelBoundsError, PanelCallError, PanelHideTicketIdentity, PanelHideTicketPhase,
        PanelOwner, PanelSessionIdentity, PanelSettlementError, PendingDispatch,
        PluginPanelController, PluginPanelHostKey, PluginPanelUpdate, QueueDispatchOutcome,
        HOST_INPUT_FOCUS_TIMEOUT,
    },
    plugin_window::{
        self, PluginWindowCallError, PluginWindowController, PluginWindowOwner,
        PluginWindowPinState, PluginWindowUpdate,
    },
    plugins::{
        PluginCopyError, PluginInventorySnapshot, PluginManagementError, PluginManager,
        PluginMutationOutcome, PluginQueryError, PluginQueryStart,
    },
    public_plugins::{
        validate_plugin_network_caller, PluginApiRequest, PluginCommandCompletion,
        PluginInvocationTheme, PluginNetworkCommandError, PluginNetworkCommandInput,
        PluginNetworkErrorCode, PluginNetworkResponse, PluginRequestContext, PluginRuntimeError,
        PluginTimerStartInput, PluginTimerState, PreparedPublicSubmission, PublicActivationMode,
        PublicMainResult, PublicOutputMode, PublicPermission, PublicPluginInstallSource,
        PublicPluginInventory, PublicPluginManagementError, PublicPluginManager,
        PublicPluginMutation, PublicPluginPrepareSummary, PublicPluginResponse, PublicPluginRoute,
        PublicPluginService, PublicPluginWindowIdentity, TimerError, WindowStorageError,
    },
    quicklinks::{
        expand_template, QuicklinkErrorCode, QuicklinkIconCandidate, QuicklinkListResponse,
        QuicklinkSaveInput, QuicklinkView, QuicklinksStore,
    },
    result_registry::{
        QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistries, ResultRegistry,
    },
    settings::{
        BuiltinFeature, ManagedBuiltinFeature, Settings, SettingsError, SettingsStore,
        SettingsUpdate, ThemePreference, WebSearchEngine, WindowPosition,
    },
    window_transfer::MainWindowTransferCoordinator,
};

const ACTIVATION_REFUSED_MESSAGE: &str = "Windows 拒绝了前台切换，已发送启动请求";

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct YoudaoValidationInput {
    app_id: String,
    app_key: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct YoudaoValidationResult {
    valid: bool,
    message: String,
}

#[tauri::command]
pub(crate) async fn validate_youdao_translation(
    window: WebviewWindow,
    input: YoudaoValidationInput,
) -> Result<YoudaoValidationResult, CommandError> {
    require_main_window(&window)?;
    let app_id = input.app_id.trim().to_owned();
    let app_key = input.app_key.trim().to_owned();
    if app_id.is_empty() || app_key.is_empty() {
        return Ok(YoudaoValidationResult {
            valid: false,
            message: "App ID 和 App Key 不能为空".into(),
        });
    }
    let q = "uipilot";
    let salt = format!(
        "{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    );
    let curtime = chrono::Utc::now().timestamp().to_string();
    let mut digest = Sha256::new();
    digest.update(format!("{app_id}{q}{salt}{curtime}{app_key}"));
    let sign = format!("{:x}", digest.finalize());
    let response = reqwest::Client::new()
        .post("https://openapi.youdao.com/api")
        .form(&[
            ("q", q),
            ("from", "auto"),
            ("to", "zh"),
            ("appKey", app_id.as_str()),
            ("salt", salt.as_str()),
            ("signType", "v3"),
            ("curtime", curtime.as_str()),
            ("sign", sign.as_str()),
        ])
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| CommandError::window_failed())?;
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| CommandError::window_failed())?;
    let valid = body.get("errorCode").and_then(serde_json::Value::as_str) == Some("0");
    Ok(YoudaoValidationResult {
        valid,
        message: if valid {
            "验证成功".into()
        } else {
            "App ID 或 App Key 无效".into()
        },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HideReason {
    ExplicitReturn,
    Blur,
    LaunchHandoff,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsView {
    hotkey: String,
    screenshot_hotkey: Option<String>,
    screenshot_hotkey_status: &'static str,
    screenshot_save_directory: Option<PathBuf>,
    screenshot_save_directory_path: Option<PathBuf>,
    autostart: bool,
    file_preview_enabled: bool,
    theme: ThemePreference,
    web_search_engine: WebSearchEngine,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct UserSettingsUpdate {
    hotkey: String,
    autostart: bool,
    theme: ThemePreference,
    web_search_engine: WebSearchEngine,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HotkeySettingsUpdate {
    hotkey: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HotkeySettingsView {
    hotkey: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct FilePreviewPreferenceUpdate {
    enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ThemePreferenceUpdate {
    theme: ThemePreference,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WebSearchEngineUpdate {
    engine: WebSearchEngine,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct BuiltinFeatureFavoriteUpdate {
    feature: BuiltinFeature,
    favorite: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct OpenFindInput {
    query: String,
    invocation_id: String,
    query_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum OpenFindOutcome {
    Forwarded,
    Superseded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindInitializationPrepared {
    initialization_token: String,
    theme_revision: String,
    theme: ThemePreference,
    file_preview_revision: String,
    file_preview_enabled: bool,
    pinned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum FindReadyOutcome {
    Prepared {
        initialization: FindInitializationPrepared,
    },
    Ready {
        initialization_token: String,
    },
    Superseded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct FindInitializationInput {
    initialization_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct FindPinUpdate {
    invocation_id: String,
    pinned: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindPinResult {
    pinned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindPreviewPreferenceResult {
    file_preview_revision: String,
    file_preview_enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindThemeChanged {
    theme_revision: String,
    theme: ThemePreference,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct FindHideInput {
    invocation_id: String,
    force: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSummaryDto {
    revision: String,
    unread_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageViewDto {
    id: String,
    plugin_id: String,
    plugin_name_snapshot: String,
    plugin_icon_url: Option<String>,
    created_at: String,
    content: String,
    read_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageCenterSnapshotDto {
    revision: String,
    unread_count: usize,
    messages: Vec<MessageViewDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageHostCommandErrorDto {
    code: &'static str,
    store_status: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum MessageCommandError {
    Caller(CommandError),
    Host(MessageHostCommandErrorDto),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
#[allow(clippy::enum_variant_names)]
pub(crate) enum ExecuteOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested { message: &'static str },
    TextCopied,
    FileRevealRequested,
    FolderOpenRequested,
}

impl CommandError {
    fn invalid_caller() -> Self {
        Self {
            code: "invalidCaller",
            message: "command caller is invalid",
        }
    }

    fn settings_failed() -> Self {
        Self {
            code: "settingsFailed",
            message: "settings operation failed",
        }
    }

    fn hotkey_conflict() -> Self {
        Self {
            code: "hotkeyConflict",
            message: "launcher hotkey conflicts with a command binding",
        }
    }

    fn stale_request() -> Self {
        Self {
            code: "staleRequest",
            message: "result request is stale",
        }
    }

    fn unknown_result() -> Self {
        Self {
            code: "unknownResult",
            message: "result is unknown",
        }
    }

    fn application_entry_unavailable() -> Self {
        Self {
            code: "applicationEntryUnavailable",
            message: "application entry is unavailable; rescan applications",
        }
    }

    fn window_failed() -> Self {
        Self {
            code: "windowFailed",
            message: "launcher window operation failed",
        }
    }

    fn invalid_file_query() -> Self {
        Self {
            code: "invalidFileQuery",
            message: "file query is invalid",
        }
    }

    fn file_search_worker_failed() -> Self {
        Self {
            code: "fileSearchWorkerFailed",
            message: "file search worker failed",
        }
    }

    fn search_unavailable() -> Self {
        Self {
            code: "searchUnavailable",
            message: "file search is unavailable",
        }
    }

    fn file_search_starting() -> Self {
        Self {
            code: "fileSearchStarting",
            message: "file search is starting",
        }
    }

    fn plugin_query_failed() -> Self {
        Self {
            code: "pluginQueryFailed",
            message: "plugin query failed",
        }
    }

    fn plugin_list_failed() -> Self {
        Self {
            code: "pluginListFailed",
            message: "plugin list failed",
        }
    }

    fn plugin_install_failed() -> Self {
        Self {
            code: "pluginInstallFailed",
            message: "plugin install failed",
        }
    }

    fn plugin_reload_failed() -> Self {
        Self {
            code: "pluginReloadFailed",
            message: "plugin reload failed",
        }
    }

    fn plugin_delete_failed() -> Self {
        Self {
            code: "pluginDeleteFailed",
            message: "plugin delete failed",
        }
    }

    fn clipboard_write_failed() -> Self {
        Self {
            code: "clipboardWriteFailed",
            message: "clipboard write failed",
        }
    }

    fn plugin_permission_denied() -> Self {
        Self {
            code: "pluginPermissionDenied",
            message: "plugin permission denied",
        }
    }

    fn file_not_found() -> Self {
        Self {
            code: "fileNotFound",
            message: "indexed file no longer exists",
        }
    }

    fn file_open_failed() -> Self {
        Self {
            code: "fileOpenFailed",
            message: "indexed file could not be opened",
        }
    }

    fn web_search_failed() -> Self {
        Self {
            code: "webSearchFailed",
            message: "browser search could not be opened",
        }
    }

    fn quicklink(error: QuicklinkErrorCode) -> Self {
        match error {
            QuicklinkErrorCode::LoadFailed => Self {
                code: "quicklinkLoadFailed",
                message: "quicklinks could not be loaded",
            },
            QuicklinkErrorCode::SaveFailed => Self {
                code: "quicklinkSaveFailed",
                message: "quicklink could not be saved",
            },
            QuicklinkErrorCode::DeleteFailed => Self {
                code: "quicklinkDeleteFailed",
                message: "quicklink could not be deleted",
            },
            QuicklinkErrorCode::CommandConflict => Self {
                code: "quicklinkCommandConflict",
                message: "quicklink command conflicts with another command",
            },
            QuicklinkErrorCode::InvalidCommand => Self {
                code: "quicklinkInvalidCommand",
                message: "quicklink command is invalid",
            },
            QuicklinkErrorCode::InvalidTemplate => Self {
                code: "quicklinkInvalidTemplate",
                message: "quicklink template is invalid",
            },
            QuicklinkErrorCode::InvalidIcon => Self {
                code: "quicklinkInvalidIcon",
                message: "quicklink icon is invalid",
            },
            QuicklinkErrorCode::OpenFailed => Self {
                code: "quicklinkOpenFailed",
                message: "quicklink URL could not be opened",
            },
        }
    }
}

impl From<SettingsError> for CommandError {
    fn from(_: SettingsError) -> Self {
        Self::settings_failed()
    }
}

impl From<ReservationError> for CommandError {
    fn from(_: ReservationError) -> Self {
        Self::settings_failed()
    }
}

impl From<PublicPluginManagementError> for CommandError {
    fn from(error: PublicPluginManagementError) -> Self {
        Self {
            code: error.code(),
            message: "public plugin operation failed",
        }
    }
}

impl From<PluginRuntimeError> for CommandError {
    fn from(error: PluginRuntimeError) -> Self {
        let code = match error {
            PluginRuntimeError::InvalidContext => "invalidContext",
            PluginRuntimeError::ExpiredRequest => "expiredRequest",
            PluginRuntimeError::InvalidCaller => "invalidCaller",
            PluginRuntimeError::InvalidOperation => "invalidOperation",
            PluginRuntimeError::PermissionDenied => "permissionDenied",
            PluginRuntimeError::InvalidNotification => "invalidNotification",
            PluginRuntimeError::InvalidDelay => "invalidDelay",
            PluginRuntimeError::ScheduleLimitExceeded => "scheduleLimitExceeded",
            PluginRuntimeError::AlreadyPublished => "alreadyPublished",
            PluginRuntimeError::MessageStoreUnavailable => "messageStoreUnavailable",
            PluginRuntimeError::Storage => "storageFailed",
            PluginRuntimeError::Unavailable => "runtimeUnavailable",
        };
        Self {
            code,
            message: "public plugin runtime operation failed",
        }
    }
}

impl From<TimerError> for CommandError {
    fn from(error: TimerError) -> Self {
        Self {
            code: error.code(),
            message: "public plugin timer operation failed",
        }
    }
}

impl From<WindowStorageError> for CommandError {
    fn from(error: WindowStorageError) -> Self {
        Self {
            code: error.code(),
            message: "public plugin window storage operation failed",
        }
    }
}

impl From<ClipboardHistoryBridgeError> for CommandError {
    fn from(error: ClipboardHistoryBridgeError) -> Self {
        Self {
            code: error.code(),
            message: "public plugin clipboard history operation failed",
        }
    }
}

impl From<ClipboardHistoryPasteError> for CommandError {
    fn from(error: ClipboardHistoryPasteError) -> Self {
        Self {
            code: error.code(),
            message: "public plugin clipboard history paste operation failed",
        }
    }
}

impl From<crate::panel_effects::EffectError> for CommandError {
    fn from(error: crate::panel_effects::EffectError) -> Self { Self { code: error.code(), message: error.code() } }
}

fn parse_canonical_nonzero_u64(value: &str) -> Option<u64> {
    if value == "0"
        || value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn parse_timer_session_generation(value: &str) -> Result<u64, TimerError> {
    parse_canonical_nonzero_u64(value).ok_or(TimerError::ExpiredWindowSessionError)
}

fn parse_window_storage_session_generation(value: &str) -> Result<u64, WindowStorageError> {
    parse_canonical_nonzero_u64(value).ok_or(WindowStorageError::ExpiredWindowSessionError)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CommitPublicPluginInstallInput {
    token: String,
    permission_grants: BTreeSet<PublicPermission>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SavePublicPluginSettingsInput {
    plugin_id: String,
    settings: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    secrets: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CompletionOriginPhase {
    Preview,
    Commit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompletionOriginInput {
    phase: CompletionOriginPhase,
    plugin_id: String,
    #[serde(default)]
    command_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginCommandCompletionResult {
    accepted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenPluginPanelInput {
    plugin_id: String,
    #[serde(default)]
    command_id: Option<String>,
    argument: String,
    #[serde(default)]
    stable_command_target: Option<crate::command_hotkeys::CommandEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SubmitPluginPanelInput {
    session_epoch: String,
    argument: String,
    ui_intent_epoch: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClosePluginPanelInput {
    session_epoch: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginPanelBoundsInput {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetPluginPanelBoundsInput {
    session_epoch: String,
    bounds: PluginPanelBoundsInput,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginPanelHostKeyEnqueueInput {
    session_epoch: String,
    client_sequence: String,
    declaration: crate::public_plugins::PanelHostKeyDeclaration,
    key: PluginPanelHostKey,
    ctrl_key: bool,
    meta_key: bool,
    shift_key: bool,
    alt_key: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PluginPanelHostKeyEnqueueResult {
    Enqueued { route_sequence: String },
    DroppedQueueFull,
    Noop,
    ProtocolViolation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PanelHideAdmitResult {
    Admitted { hide_ticket_id: String },
    Noop,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPanelCommandResult {
    session_epoch: String,
    plugin_id: String,
    command_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_placeholder: Option<String>,
    host_keys: Vec<crate::public_plugins::PanelHostKeyDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPanelErrorEvent {
    session_epoch: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginPanelFocusHostInputEvent {
    session_epoch: String,
    focus_request_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostInputFocusStart {
    AwaitingAck,
    Noop,
    Failed,
}

const PLUGIN_PANEL_FOCUS_HOST_INPUT_EVENT: &str = "uipilot-plugin-panel-focus-host-input";

fn reset_plugin_panel(app: &AppHandle, controller: &PluginPanelController, plugin_id: &str) {
    let Some(identity) = plugin_panel::teardown_plugin(app, controller, plugin_id) else {
        return;
    };
    let _ = app.emit_to(
        "main",
        "uipilot-plugin-panel-reset",
        PluginPanelErrorEvent {
            session_epoch: identity.session_epoch.to_string(),
        },
    );
}

fn reset_plugin_panel_checked(
    app: &AppHandle,
    controller: &PluginPanelController,
    plugin_id: &str,
) -> Result<(), CommandError> {
    let (identity, closed) = plugin_panel::teardown_plugin_checked(app, controller, plugin_id);
    let reset = if let Some(identity) = identity {
        app.emit_to(
            "main",
            "uipilot-plugin-panel-reset",
            PluginPanelErrorEvent {
                session_epoch: identity.session_epoch.to_string(),
            },
        )
        .map_err(|_| CommandError::from(PublicPluginManagementError::Unavailable))
    } else {
        Ok(())
    };
    closed.map_err(CommandError::from).and(reset)
}

fn require_main_label(label: &str) -> Result<(), CommandError> {
    (label == "main")
        .then_some(())
        .ok_or_else(CommandError::invalid_caller)
}

fn require_main_window(window: &WebviewWindow) -> Result<(), CommandError> {
    require_main_label(window.label())
}

fn require_find_label(label: &str) -> Result<(), CommandError> {
    (label == "find")
        .then_some(())
        .ok_or_else(CommandError::invalid_caller)
}

fn require_find_window(window: &WebviewWindow) -> Result<(), CommandError> {
    require_find_label(window.label())
}

#[tauri::command]
pub(crate) fn set_settings_pinned(
    window: WebviewWindow,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    active: bool,
    pinned: bool,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    window
        .set_always_on_top(!active || pinned)
        .map_err(|_| CommandError::window_failed())?;
    coordinator.set_settings_pinned(active && pinned);
    Ok(())
}

#[tauri::command]
pub(crate) fn get_message_summary(
    window: WebviewWindow,
    messages: State<'_, Arc<MessageCenterService>>,
) -> Result<MessageSummaryDto, MessageCommandError> {
    require_main_window(&window)?;
    messages
        .summary()
        .map(message_summary_dto)
        .map_err(message_command_error)
}

#[tauri::command]
pub(crate) fn open_message_center(
    window: WebviewWindow,
    app: AppHandle,
    messages: State<'_, Arc<MessageCenterService>>,
    public_plugins: State<'_, Arc<PublicPluginService>>,
) -> Result<MessageCenterSnapshotDto, MessageCommandError> {
    require_main_window(&window)?;
    let execution = messages.open_and_mark_read();
    messages.dispatch_post_guard(&app, execution.post_guard_effect);
    let snapshot = execution.result.map_err(message_command_error)?;
    Ok(message_snapshot_dto(
        snapshot,
        public_plugins.manager().ok().map(Arc::as_ref),
    ))
}

#[tauri::command]
pub(crate) fn read_message_center(
    window: WebviewWindow,
    messages: State<'_, Arc<MessageCenterService>>,
    public_plugins: State<'_, Arc<PublicPluginService>>,
) -> Result<MessageCenterSnapshotDto, MessageCommandError> {
    require_main_window(&window)?;
    let snapshot = messages.read_snapshot().map_err(message_command_error)?;
    Ok(message_snapshot_dto(
        snapshot,
        public_plugins.manager().ok().map(Arc::as_ref),
    ))
}

#[tauri::command]
pub(crate) fn clear_messages(
    window: WebviewWindow,
    app: AppHandle,
    messages: State<'_, Arc<MessageCenterService>>,
    public_plugins: State<'_, Arc<PublicPluginService>>,
) -> Result<MessageCenterSnapshotDto, MessageCommandError> {
    require_main_window(&window)?;
    let execution = messages.clear();
    messages.dispatch_post_guard(&app, execution.post_guard_effect);
    let snapshot = execution.result.map_err(message_command_error)?;
    Ok(message_snapshot_dto(
        snapshot,
        public_plugins.manager().ok().map(Arc::as_ref),
    ))
}

fn message_summary_dto(summary: MessageSummary) -> MessageSummaryDto {
    MessageSummaryDto {
        revision: summary.revision,
        unread_count: summary.unread_count,
    }
}

fn message_snapshot_dto(
    snapshot: MessageCenterSnapshot,
    plugins: Option<&PublicPluginManager>,
) -> MessageCenterSnapshotDto {
    MessageCenterSnapshotDto {
        revision: snapshot.revision,
        unread_count: snapshot.unread_count,
        messages: snapshot
            .messages
            .into_iter()
            .map(|message| MessageViewDto {
                plugin_icon_url: plugins
                    .and_then(|plugins| plugins.message_icon_url(&message.plugin_id)),
                id: message.id,
                plugin_id: message.plugin_id,
                plugin_name_snapshot: message.plugin_name_snapshot,
                created_at: message.created_at,
                content: message.content,
                read_at: message.read_at,
            })
            .collect(),
    }
}

fn message_command_error(error: MessageCenterError) -> MessageCommandError {
    MessageCommandError::Host(match error {
        MessageCenterError::OperationFailed => MessageHostCommandErrorDto {
            code: "MessageOperationFailed",
            store_status: "ready",
        },
        MessageCenterError::Unavailable => MessageHostCommandErrorDto {
            code: "MessageStoreUnavailable",
            store_status: "unavailable",
        },
    })
}

impl From<CommandError> for MessageCommandError {
    fn from(error: CommandError) -> Self {
        Self::Caller(error)
    }
}

#[tauri::command]
pub(crate) async fn open_find_window(
    window: WebviewWindow,
    input: OpenFindInput,
    app: AppHandle,
    registries: State<'_, ResultRegistries>,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<OpenFindOutcome, CommandError> {
    require_main_window(&window)?;
    if input.invocation_id.is_empty()
        || input.query_sequence == 0
        || input.query.len() > 1_024
        || input.query.contains('\0')
    {
        return Err(CommandError::invalid_file_query());
    }
    let retirement = match registries
        .main()
        .prepare_application_query_retirement(&input.invocation_id, input.query_sequence)
    {
        Ok(Some(retirement)) => retirement,
        Ok(None) => {
            return Err(CommandError::stale_request());
        }
        Err(_) => {
            return Err(CommandError::stale_request());
        }
    };
    let submission = controller
        .submit_open(input.query, retirement, Instant::now())
        .map_err(|_| CommandError::window_failed())?;
    if submission.snapshot_required
        && lifecycle::start_find_transfer(&app, controller.inner().as_ref(), &registries).is_err()
    {
        return Err(CommandError::window_failed());
    }
    let wait_controller = Arc::clone(controller.inner());
    let completion = tauri::async_runtime::spawn_blocking(move || loop {
        match submission
            .completion
            .recv_timeout(Duration::from_millis(50))
        {
            Ok(outcome) => break Some(outcome),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                wait_controller.expire(Instant::now());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break None,
        }
    })
    .await
    .map_err(|_| CommandError::window_failed())?;
    match completion {
        Some(OpenFindCompletion::Forwarded) => Ok(OpenFindOutcome::Forwarded),
        Some(OpenFindCompletion::Superseded) => Ok(OpenFindOutcome::Superseded),
        Some(OpenFindCompletion::Unavailable) | None => {
            controller.expire(Instant::now());
            Err(CommandError::window_failed())
        }
    }
}

#[tauri::command]
pub(crate) fn prepare_find_initialization(
    window: WebviewWindow,
    settings: State<'_, SettingsStore>,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<FindReadyOutcome, CommandError> {
    require_find_window(&window)?;
    let preferences = settings.find_preference_snapshot();
    let prepared = controller
        .prepare_initialization(Instant::now())
        .map_err(|_| CommandError::window_failed())?;
    Ok(FindReadyOutcome::Prepared {
        initialization: FindInitializationPrepared {
            initialization_token: prepared.token,
            theme_revision: preferences.theme_revision.to_string(),
            theme: preferences.theme,
            file_preview_revision: preferences.file_preview_revision.to_string(),
            file_preview_enabled: preferences.file_preview_enabled,
            pinned: controller.pinned(),
        },
    })
}

#[tauri::command]
pub(crate) fn commit_find_ready(
    window: WebviewWindow,
    input: FindInitializationInput,
    app: AppHandle,
    registries: State<'_, ResultRegistries>,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<FindReadyOutcome, CommandError> {
    require_find_window(&window)?;
    let commit = controller.commit_ready(&input.initialization_token, Instant::now());
    if commit.snapshot_required {
        lifecycle::start_find_transfer(&app, controller.inner().as_ref(), &registries)
            .map_err(|_| CommandError::window_failed())?;
    }
    Ok(match commit.outcome {
        FindReadyStatus::Ready => FindReadyOutcome::Ready {
            initialization_token: input.initialization_token,
        },
        FindReadyStatus::Prepared => FindReadyOutcome::Superseded,
        FindReadyStatus::Superseded => FindReadyOutcome::Superseded,
    })
}

#[tauri::command]
pub(crate) fn get_find_ready_status(
    window: WebviewWindow,
    input: FindInitializationInput,
    settings: State<'_, SettingsStore>,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<FindReadyOutcome, CommandError> {
    require_find_window(&window)?;
    Ok(
        match controller.ready_status(&input.initialization_token, Instant::now()) {
            FindReadyStatus::Ready => FindReadyOutcome::Ready {
                initialization_token: input.initialization_token,
            },
            FindReadyStatus::Prepared => {
                let preferences = settings.find_preference_snapshot();
                FindReadyOutcome::Prepared {
                    initialization: FindInitializationPrepared {
                        initialization_token: input.initialization_token,
                        theme_revision: preferences.theme_revision.to_string(),
                        theme: preferences.theme,
                        file_preview_revision: preferences.file_preview_revision.to_string(),
                        file_preview_enabled: preferences.file_preview_enabled,
                        pinned: controller.pinned(),
                    },
                }
            }
            FindReadyStatus::Superseded => FindReadyOutcome::Superseded,
        },
    )
}

#[tauri::command]
pub(crate) fn set_find_pinned(
    window: WebviewWindow,
    input: FindPinUpdate,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<FindPinResult, CommandError> {
    require_find_window(&window)?;
    controller
        .set_pin(&input.invocation_id, input.pinned)
        .then_some(FindPinResult {
            pinned: input.pinned,
        })
        .ok_or_else(CommandError::stale_request)
}

#[tauri::command]
pub(crate) fn hide_find_window(
    window: WebviewWindow,
    input: FindHideInput,
    app: AppHandle,
    registries: State<'_, ResultRegistries>,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<(), CommandError> {
    require_find_window(&window)?;
    if !controller.request_explicit_hide(&input.invocation_id, input.force) {
        if controller.pinned() && !input.force {
            return Ok(());
        }
        return Err(CommandError::stale_request());
    }
    let hidden = window.hide().is_ok();
    controller.finish_explicit_hide(&input.invocation_id, hidden, &registries);
    if !hidden {
        return Err(CommandError::window_failed());
    }
    if controller.queued_query().is_some() {
        lifecycle::start_find_transfer(&app, controller.inner().as_ref(), &registries)
            .map_err(|_| CommandError::window_failed())?;
    }
    Ok(())
}

fn list_plugins_with_label<L>(label: &str, list: L) -> Result<PluginInventorySnapshot, CommandError>
where
    L: FnOnce() -> Result<PluginInventorySnapshot, PluginManagementError>,
{
    require_main_label(label)?;
    list().map_err(|_| CommandError::plugin_list_failed())
}

#[tauri::command]
pub(crate) fn list_plugins(
    window: WebviewWindow,
    plugins: State<'_, Arc<PluginManager>>,
) -> Result<PluginInventorySnapshot, CommandError> {
    list_plugins_with_label(window.label(), || plugins.list_inventory())
}

fn install_plugin_with_label<I>(
    label: &str,
    coordinator: &Arc<LifecycleCoordinator>,
    install: I,
) -> Result<PluginMutationOutcome, CommandError>
where
    I: FnOnce() -> Result<PluginMutationOutcome, PluginManagementError>,
{
    require_main_label(label)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_install_failed())?;
    install().map_err(|_| CommandError::plugin_install_failed())
}

#[tauri::command]
pub(crate) async fn install_plugin(
    window: WebviewWindow,
    app: AppHandle,
    plugins: State<'_, Arc<PluginManager>>,
    registries: State<'_, ResultRegistries>,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    plugin_id: String,
) -> Result<PluginMutationOutcome, CommandError> {
    install_plugin_with_label(window.label(), &coordinator, || {
        plugins.install_plugin(&app, registries.main(), &plugin_id)
    })
}

fn reload_plugin_with_label<R>(
    label: &str,
    coordinator: &Arc<LifecycleCoordinator>,
    reload: R,
) -> Result<PluginMutationOutcome, CommandError>
where
    R: FnOnce() -> Result<PluginMutationOutcome, PluginManagementError>,
{
    require_main_label(label)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_reload_failed())?;
    reload().map_err(|_| CommandError::plugin_reload_failed())
}

#[tauri::command]
pub(crate) async fn reload_plugin(
    window: WebviewWindow,
    app: AppHandle,
    plugins: State<'_, Arc<PluginManager>>,
    registries: State<'_, ResultRegistries>,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    plugin_id: String,
) -> Result<PluginMutationOutcome, CommandError> {
    reload_plugin_with_label(window.label(), &coordinator, || {
        plugins.reload_plugin(&app, registries.main(), &plugin_id)
    })
}

fn delete_plugin_with_label<D>(
    label: &str,
    coordinator: &Arc<LifecycleCoordinator>,
    delete: D,
) -> Result<PluginMutationOutcome, CommandError>
where
    D: FnOnce() -> Result<PluginMutationOutcome, PluginManagementError>,
{
    require_main_label(label)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_delete_failed())?;
    delete().map_err(|_| CommandError::plugin_delete_failed())
}

#[tauri::command]
pub(crate) async fn delete_plugin(
    window: WebviewWindow,
    app: AppHandle,
    plugins: State<'_, Arc<PluginManager>>,
    registries: State<'_, ResultRegistries>,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    plugin_id: String,
) -> Result<PluginMutationOutcome, CommandError> {
    delete_plugin_with_label(window.label(), &coordinator, || {
        plugins.delete_plugin(&app, registries.main(), &plugin_id)
    })
}

fn select_public_plugin_source_with<S, F>(
    label: &str,
    coordinator: &Arc<LifecycleCoordinator>,
    select: S,
    restore_focus: F,
) -> Result<Option<PathBuf>, CommandError>
where
    S: FnOnce() -> Option<PathBuf>,
    F: FnOnce() -> Result<(), ()>,
{
    require_main_label(label)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_install_failed())?;
    let selected = select();
    restore_focus().map_err(|_| CommandError::plugin_install_failed())?;
    Ok(selected)
}

#[tauri::command]
pub(crate) async fn select_public_plugin_directory(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
) -> Result<Option<PathBuf>, CommandError> {
    select_public_plugin_source_with(
        window.label(),
        coordinator.inner(),
        || {
            app.dialog()
                .file()
                .set_parent(&window)
                .set_title("选择插件开发目录")
                .blocking_pick_folder()
                .and_then(|path| path.into_path().ok())
        },
        || {
            window.set_focus().map_err(|_| ())?;
            window.as_ref().set_focus().map_err(|_| ())
        },
    )
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandHotkeyInput {
    plugin_id: String,
    command_id: String,
    #[serde(deserialize_with = "deserialize_nullable_hotkey")]
    hotkey: Option<String>,
}
fn deserialize_nullable_hotkey<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandEntryResolution {
    plugin_id: String,
    command_id: String,
    command_label: String,
    output_mode: PublicOutputMode,
    activation_mode: PublicActivationMode,
    input_required: bool,
    input_placeholder: Option<String>,
}

fn command_hotkey_access<T>(label: &str, protected: impl FnOnce() -> T) -> Result<T, CommandError> {
    require_main_label(label)?;
    Ok(protected())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ScreenshotHotkeyInput {
    #[serde(deserialize_with = "deserialize_nullable_hotkey")]
    hotkey: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ScreenshotSaveDirectoryInput {
    #[serde(deserialize_with = "deserialize_nullable_hotkey")]
    directory: Option<String>,
}

fn screenshot_directory_access<T>(
    label: &str,
    access: impl FnOnce() -> T,
) -> Result<T, CommandError> {
    require_main_label(label)?;
    Ok(access())
}

fn select_screenshot_directory_with<T>(
    label: &str,
    select: impl FnOnce() -> Option<PathBuf>,
    save: impl FnOnce(PathBuf) -> Result<T, CommandError>,
) -> Result<Option<T>, CommandError> {
    screenshot_directory_access(label, || select().map(save).transpose())?
}

#[tauri::command]
pub(crate) async fn select_screenshot_save_directory(
    window: WebviewWindow,
) -> Result<Option<ScreenshotSaveDirectoryView>, CommandError> {
    let app = screenshot_directory_access(window.label(), || window.app_handle().clone())?;
    tauri::async_runtime::spawn_blocking(move || {
        let coordinator = app.state::<Arc<LifecycleCoordinator>>();
        let _critical = coordinator
            .reserve_critical()
            .map_err(|_| CommandError::settings_failed())?;
        let _focus = coordinator
            .suppress_transient_focus_loss()
            .map_err(|_| CommandError::settings_failed())?;
        let selected = select_screenshot_directory_with(
            window.label(),
            || {
                app.dialog()
                    .file()
                    .set_parent(&window)
                    .set_title("选择截图保存文件夹")
                    .blocking_pick_folder()
                    .and_then(|path| path.into_path().ok())
            },
            |path| {
                app.state::<SettingsStore>()
                    .set_screenshot_save_directory(Some(path))
                    .map_err(|_| CommandError::settings_failed())?;
                Ok(load_screenshot_directory_settings(&app))
            },
        );
        // The saved result is authoritative even if native focus restoration fails.
        let _ = window.set_focus();
        let _ = window.as_ref().set_focus();
        selected
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
}

#[tauri::command]
pub(crate) async fn set_screenshot_save_directory(
    window: WebviewWindow,
    input: ScreenshotSaveDirectoryInput,
) -> Result<ScreenshotSaveDirectoryView, CommandError> {
    let app = screenshot_directory_access(window.label(), || window.app_handle().clone())?;
    tauri::async_runtime::spawn_blocking(move || {
        let coordinator = app.state::<Arc<LifecycleCoordinator>>();
        let _critical = coordinator
            .reserve_critical()
            .map_err(|_| CommandError::settings_failed())?;
        app.state::<SettingsStore>()
            .set_screenshot_save_directory(input.directory.map(PathBuf::from))
            .map_err(|_| CommandError::settings_failed())?;
        Ok(load_screenshot_directory_settings(&app))
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenshotSaveDirectoryView {
    directory: Option<PathBuf>,
    path: Option<PathBuf>,
}

fn load_screenshot_directory_settings(app: &AppHandle) -> ScreenshotSaveDirectoryView {
    let directory = app
        .state::<SettingsStore>()
        .snapshot()
        .screenshot_save_directory;
    let path = directory.clone().or_else(|| app.path().desktop_dir().ok());
    ScreenshotSaveDirectoryView { directory, path }
}

fn edit_screenshot_hotkey_with(
    settings: &SettingsStore,
    keys: &crate::screenshot::hotkey::ScreenshotHotkeys,
    replacement: Option<tauri_plugin_global_shortcut::Shortcut>,
    register: &mut impl FnMut(tauri_plugin_global_shortcut::Shortcut) -> Result<(), ()>,
    unregister: &mut impl FnMut(tauri_plugin_global_shortcut::Shortcut) -> Result<(), ()>,
) -> Result<(), CommandError> {
    let canonical = replacement.map(|s| s.to_string());
    if canonical
        .as_deref()
        .is_some_and(|s| crate::screenshot::hotkey::conflicts(&settings.snapshot(), s))
    {
        return Err(CommandError::hotkey_conflict());
    }
    let desired = replacement.filter(|_| {
        settings
            .snapshot()
            .builtin_feature_enabled(ManagedBuiltinFeature::Screenshot)
    });
    keys.edit(desired, register, unregister, || {
        settings.set_screenshot_hotkey(canonical).map_err(|_| ())
    })
    .map_err(|_| CommandError::settings_failed())
}

#[tauri::command]
pub(crate) async fn set_screenshot_hotkey(
    window: WebviewWindow,
    input: ScreenshotHotkeyInput,
) -> Result<(), CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    let replacement = input
        .hotkey
        .as_deref()
        .map(crate::hotkey::command_chord)
        .transpose()
        .map_err(|_| CommandError::settings_failed())?;
    tauri::async_runtime::spawn_blocking(move || {
        let coordinator = app.state::<Arc<LifecycleCoordinator>>();
        let _critical = coordinator
            .reserve_critical()
            .map_err(|_| CommandError::settings_failed())?;
        let _operation = coordinator
            .lock_hotkey_operation()
            .map_err(|_| CommandError::settings_failed())?;
        let settings = app.state::<SettingsStore>();
        let native = app.global_shortcut();
        edit_screenshot_hotkey_with(
            &settings,
            &coordinator.screenshot_hotkeys,
            replacement,
            &mut |s| native.register(s).map_err(|_| ()),
            &mut |s| native.unregister(s).map_err(|_| ()),
        )
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
}

#[tauri::command]
pub(crate) async fn set_hotkey_recording(
    window: WebviewWindow,
    recording: bool,
) -> Result<(), CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Arc<LifecycleCoordinator>>()
            .set_hotkey_recording(recording)
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
    .map_err(|_| CommandError::settings_failed())
}

#[tauri::command]
pub(crate) fn cancel_translation_selection(window: WebviewWindow) -> Result<(), CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    app.state::<Arc<LifecycleCoordinator>>()
        .cancel_selection_capture();
    Ok(())
}
#[tauri::command]
pub(crate) fn get_translation_selection_settings(
    window: WebviewWindow,
) -> Result<crate::selection_translation::TranslationSelectionSettings, CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    Ok(app
        .state::<Arc<LifecycleCoordinator>>()
        .translation_selection_settings(&app))
}
#[tauri::command]
pub(crate) async fn set_translation_selection_hotkey(
    window: WebviewWindow,
    hotkey: Option<String>,
) -> Result<crate::selection_translation::TranslationSelectionSettings, CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Arc<LifecycleCoordinator>>()
            .edit_selection_hotkey(&app, hotkey)
            .map_err(|_| CommandError::settings_failed())
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
}
#[tauri::command]
pub(crate) async fn set_public_plugin_command_hotkey(
    window: WebviewWindow,
    input: CommandHotkeyInput,
) -> Result<(), CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    let replacement = input
        .hotkey
        .as_deref()
        .map(crate::hotkey::command_chord)
        .transpose()
        .map_err(|_| CommandError::plugin_query_failed())?;
    tauri::async_runtime::spawn_blocking(move || {
        let coordinator = Arc::clone(app.state::<Arc<LifecycleCoordinator>>().inner());
        let _critical = coordinator
            .reserve_critical()
            .map_err(|_| CommandError::plugin_query_failed())?;
        let _operation = coordinator
            .lock_hotkey_operation()
            .map_err(|_| CommandError::plugin_query_failed())?;
        let service = app.state::<Arc<PublicPluginService>>();
        let manager = service.manager()?;
        let authority = manager.command_hotkey_authority(&input.plugin_id, &input.command_id)?;
        let settings = app.state::<SettingsStore>();
        let canonical = replacement.map(|s| s.to_string());
        if canonical.as_ref().is_some_and(|chord| {
            crate::command_hotkeys::conflicts(
                &settings.snapshot(),
                &input.plugin_id,
                &input.command_id,
                chord,
            )
        }) {
            return Err(CommandError::plugin_query_failed());
        }
        let native = app.global_shortcut();
        coordinator
            .command_hotkeys
            .edit(
                &input.plugin_id,
                &input.command_id,
                replacement,
                authority.0,
                &mut |s| native.register(s).map_err(|_| ()),
                &mut |s| native.unregister(s).map_err(|_| ()),
                || {
                    manager
                        .persist_command_hotkey(
                            &settings,
                            &input.plugin_id,
                            &input.command_id,
                            canonical,
                            authority,
                        )
                        .map_err(|_| ())
                },
            )
            .map_err(|_| CommandError::plugin_query_failed())
    })
    .await
    .map_err(|_| CommandError::plugin_query_failed())?
}

#[tauri::command]
pub(crate) fn resolve_public_plugin_command_entry(
    window: WebviewWindow,
    entry: crate::command_hotkeys::CommandEntry,
) -> Result<CommandEntryResolution, CommandError> {
    let app = command_hotkey_access(window.label(), || window.app_handle().clone())?;
    let coordinator = app.state::<Arc<LifecycleCoordinator>>();
    if !coordinator.command_entry_current(&app, &entry) {
        return Err(CommandError::plugin_query_failed());
    }
    let service = app.state::<Arc<PublicPluginService>>();
    let route = service
        .manager()?
        .route_by_id(&entry.plugin_id, &entry.command_id, "")?
        .ok_or_else(CommandError::plugin_query_failed)?;
    if !coordinator.command_entry_current(&app, &entry) {
        return Err(CommandError::plugin_query_failed());
    }
    Ok(CommandEntryResolution {
        plugin_id: route.plugin_id,
        command_id: route.command_id,
        command_label: route.command_label,
        output_mode: route.output_mode,
        activation_mode: route.activation_mode,
        input_required: route.input_required,
        input_placeholder: route.input_placeholder,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuiltinFeatureView {
    feature: ManagedBuiltinFeature,
    name: &'static str,
    command: String,
    default_command: String,
    favorite: bool,
    hotkey: Option<String>,
    hotkey_status: &'static str,
    directory: Option<String>,
    description: &'static str,
    examples: Vec<String>,
    version: &'static str,
    enabled: bool,
}

fn builtin_feature_views(settings: &Settings) -> Vec<BuiltinFeatureView> {
    ManagedBuiltinFeature::ALL
        .into_iter()
        .map(|feature| {
            let (name, command, description, _examples) = match feature {
                ManagedBuiltinFeature::Screenshot => (
                    "截图",
                    "/screenshot",
                    "截取屏幕区域，编辑、复制、保存或贴图。",
                    vec!["/screenshot"],
                ),
                ManagedBuiltinFeature::Quicklinks => (
                    "快捷链接",
                    "/quicklinks",
                    "管理快捷链接，使用自定义命令打开链接。",
                    vec!["/quicklinks"],
                ),
                ManagedBuiltinFeature::WebSearch => (
                    "网页搜索",
                    "/web-search",
                    "使用默认搜索引擎搜索网页。",
                    vec!["/web-search UiPilot"],
                ),
            };
            BuiltinFeatureView {
                feature,
                name,
                command: settings.builtin_command(feature),
                default_command: command.into(),
                favorite: settings
                    .favorite_builtin_features
                    .contains(&feature.favorite_feature()),
                hotkey: settings.builtin_hotkey(feature).map(str::to_owned),
                hotkey_status: if settings.builtin_hotkey(feature).is_none() {
                    "unset"
                } else if !settings.builtin_feature_enabled(feature) {
                    "suspended"
                } else {
                    "unavailable"
                },
                directory: None,
                description,
                examples: if feature == ManagedBuiltinFeature::WebSearch {
                    vec![format!("{} UiPilot", settings.builtin_command(feature))]
                } else {
                    vec![settings.builtin_command(feature)]
                },
                version: env!("CARGO_PKG_VERSION"),
                enabled: settings.builtin_feature_enabled(feature),
            }
        })
        .collect()
}

fn with_managed_builtin<T>(
    label: &str,
    feature: &str,
    operation: impl FnOnce(ManagedBuiltinFeature) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    require_main_label(label)?;
    let feature = ManagedBuiltinFeature::parse(feature).ok_or(CommandError {
        code: "invalidBuiltinFeature",
        message: "builtin feature is invalid",
    })?;
    operation(feature)
}

fn normalized_builtin_name(value: &str, reserved: &[String]) -> Result<String, CommandError> {
    let value = value.trim().strip_prefix('/').unwrap_or(value.trim());
    if !(1..=32).contains(&value.len())
        || !value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || matches!(value, "find" | "math")
        || reserved.iter().any(|name| name.eq_ignore_ascii_case(value))
    {
        return Err(CommandError::settings_failed());
    }
    Ok(value.to_owned())
}

#[tauri::command]
pub(crate) fn set_builtin_feature_name(
    window: WebviewWindow,
    feature: String,
    name_override: Option<String>,
) -> Result<(), CommandError> {
    let feature = with_managed_builtin(window.label(), &feature, Ok)?;
    let app = window.app_handle();
    let name = name_override
        .as_deref()
        .map(|value| normalized_builtin_name(value, &[]))
        .transpose()?;
    app.state::<SettingsStore>()
        .set_builtin_feature_name_checked(feature, name, |value| {
            !app.state::<Arc<QuicklinksStore>>()
                .commands()
                .iter()
                .any(|command| command.eq_ignore_ascii_case(value))
        })
        .map_err(|_| CommandError::settings_failed())?;
    let _ = app.emit_to("main", "builtin-features://changed", ());
    Ok(())
}
#[tauri::command]
pub(crate) async fn set_builtin_feature_hotkey(
    window: WebviewWindow,
    feature: String,
    hotkey: Option<String>,
) -> Result<(), CommandError> {
    let feature = with_managed_builtin(window.label(), &feature, Ok)?;
    let app = window.app_handle().clone();
    let replacement = hotkey
        .as_deref()
        .map(crate::hotkey::command_chord)
        .transpose()
        .map_err(|_| CommandError::settings_failed())?;
    tauri::async_runtime::spawn_blocking(move || {
        let coordinator = app.state::<Arc<LifecycleCoordinator>>();
        let _critical = coordinator
            .reserve_critical()
            .map_err(|_| CommandError::settings_failed())?;
        let _operation = coordinator
            .lock_hotkey_operation()
            .map_err(|_| CommandError::settings_failed())?;
        let settings = app.state::<SettingsStore>();
        let saved = settings.snapshot();
        let canonical = replacement.map(|s| s.to_string());
        if canonical
            .as_deref()
            .is_some_and(|s| crate::screenshot::hotkey::builtin_conflicts(&saved, feature, s))
        {
            return Err(CommandError::hotkey_conflict());
        }
        let desired = replacement.filter(|_| saved.builtin_feature_enabled(feature));
        let native = app.global_shortcut();
        coordinator
            .builtin_hotkeys(feature)
            .edit(
                desired,
                &mut |s| native.register(s).map_err(|_| ()),
                &mut |s| native.unregister(s).map_err(|_| ()),
                || {
                    settings
                        .set_builtin_hotkey(feature, canonical)
                        .map_err(|_| ())
                },
            )
            .map_err(|_| CommandError::settings_failed())?;
        let _ = app.emit_to("main", "builtin-features://changed", ());
        Ok(())
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
}
#[tauri::command]
pub(crate) fn list_builtin_features(
    window: WebviewWindow,
) -> Result<Vec<BuiltinFeatureView>, CommandError> {
    require_main_window(&window)?;
    let settings = window.app_handle().state::<SettingsStore>();
    let mut views = builtin_feature_views(&settings.snapshot());
    let coordinator = window.app_handle().state::<Arc<LifecycleCoordinator>>();
    for view in &mut views {
        if view.enabled {
            view.hotkey_status = coordinator
                .builtin_hotkeys(view.feature)
                .status(view.hotkey.as_deref());
        }
    }
    Ok(views)
}

fn set_builtin_enabled_with(
    settings: &SettingsStore,
    keys: &crate::screenshot::hotkey::ScreenshotHotkeys,
    feature: ManagedBuiltinFeature,
    enabled: bool,
    register: &mut impl FnMut(tauri_plugin_global_shortcut::Shortcut) -> Result<(), ()>,
    unregister: &mut impl FnMut(tauri_plugin_global_shortcut::Shortcut) -> Result<(), ()>,
) -> Result<(), CommandError> {
    let saved = settings.snapshot();
    if saved.builtin_feature_enabled(feature) == enabled {
        return Ok(());
    }
    let desired = saved
        .builtin_hotkey(feature)
        .filter(|_| enabled)
        .map(crate::hotkey::command_chord)
        .transpose()
        .map_err(|_| CommandError::settings_failed())?;
    if desired.is_some_and(|s| {
        crate::screenshot::hotkey::builtin_conflicts(&saved, feature, &s.to_string())
    }) {
        return Err(CommandError::hotkey_conflict());
    }
    keys.edit(desired, register, unregister, || {
        settings
            .set_builtin_feature_enabled(feature, enabled)
            .map_err(|_| ())
    })
    .map_err(|_| CommandError::settings_failed())
}

#[tauri::command]
pub(crate) async fn set_builtin_feature_enabled(
    window: WebviewWindow,
    feature: String,
    enabled: bool,
) -> Result<Vec<BuiltinFeatureView>, CommandError> {
    let feature = with_managed_builtin(window.label(), &feature, Ok)?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let coordinator = app.state::<Arc<LifecycleCoordinator>>();
        let _critical = coordinator
            .reserve_critical()
            .map_err(|_| CommandError::settings_failed())?;
        let _operation = coordinator
            .lock_hotkey_operation()
            .map_err(|_| CommandError::settings_failed())?;
        let settings = app.state::<SettingsStore>();
        let native = app.global_shortcut();
        set_builtin_enabled_with(
            &settings,
            coordinator.builtin_hotkeys(feature),
            feature,
            enabled,
            &mut |s| native.register(s).map_err(|_| ()),
            &mut |s| native.unregister(s).map_err(|_| ()),
        )?;
        let mut views = builtin_feature_views(&settings.snapshot());
        for view in &mut views {
            if view.enabled {
                view.hotkey_status = coordinator
                    .builtin_hotkeys(view.feature)
                    .status(view.hotkey.as_deref());
            }
        }
        let _ = app.emit_to("main", "builtin-features://changed", ());
        Ok(views)
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
}

#[tauri::command]
pub(crate) fn list_public_plugins(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
) -> Result<PublicPluginInventory, CommandError> {
    require_main_window(&window)?;
    let mut inventory = service.manager()?.inventory()?;
    let app = window.app_handle();
    let coordinator = app.state::<Arc<LifecycleCoordinator>>();
    let settings = app.state::<SettingsStore>().snapshot();
    inventory.hotkey_revision = coordinator.command_hotkeys.revision();
    for plugin in &mut inventory.items {
        for command in &mut plugin.commands {
            command.hotkey = settings
                .public_plugin_command_hotkeys
                .get(&plugin.plugin_id)
                .and_then(|m| m.get(&command.command_id))
                .cloned();
            command.hotkey_status = coordinator.command_hotkeys.status(
                &plugin.plugin_id,
                &command.command_id,
                command.hotkey.as_deref(),
                plugin.enabled && plugin.fault.is_none(),
            );
        }
    }
    Ok(inventory)
}

#[tauri::command]
pub(crate) fn list_quicklinks(
    window: WebviewWindow,
    store: State<'_, Arc<QuicklinksStore>>,
) -> Result<QuicklinkListResponse, CommandError> {
    require_main_window(&window)?;
    window
        .app_handle()
        .state::<SettingsStore>()
        .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || store.list())
        .map_err(|_| CommandError::plugin_query_failed())
}

fn quicklink_external_names(settings: &Settings, mut public_names: Vec<String>) -> Vec<String> {
    public_names.extend(ManagedBuiltinFeature::ALL.into_iter().map(|feature| {
        settings
            .builtin_command(feature)
            .trim_start_matches('/')
            .to_owned()
    }));
    public_names
}

#[tauri::command]
pub(crate) fn save_quicklink(
    window: WebviewWindow,
    store: State<'_, Arc<QuicklinksStore>>,
    service: State<'_, Arc<PublicPluginService>>,
    input: QuicklinkSaveInput,
) -> Result<QuicklinkView, CommandError> {
    require_main_window(&window)?;
    let manager = service.manager()?;
    let public_commands = manager.installed_command_names()?;
    let saved = window
        .app_handle()
        .state::<SettingsStore>()
        .with_enabled_builtin_settings(ManagedBuiltinFeature::Quicklinks, |settings| {
            let conflicts = quicklink_external_names(settings, public_commands);
            store.save_with_external_conflicts(input, &conflicts)
        })
        .map_err(|_| CommandError::plugin_query_failed())?
        .map_err(CommandError::quicklink)?;
    manager.replace_external_reserved_names(store.commands())?;
    Ok(saved)
}

#[tauri::command]
pub(crate) fn delete_quicklink(
    window: WebviewWindow,
    store: State<'_, Arc<QuicklinksStore>>,
    service: State<'_, Arc<PublicPluginService>>,
    id: String,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    window
        .app_handle()
        .state::<SettingsStore>()
        .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || store.delete(&id))
        .map_err(|_| CommandError::plugin_query_failed())?
        .map_err(CommandError::quicklink)?;
    service
        .manager()?
        .replace_external_reserved_names(store.commands())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn choose_quicklink_icon(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    store: State<'_, Arc<QuicklinksStore>>,
) -> Result<Option<QuicklinkIconCandidate>, CommandError> {
    require_main_window(&window)?;
    app.state::<SettingsStore>()
        .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || ())
        .map_err(|_| CommandError::plugin_query_failed())?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::quicklink(QuicklinkErrorCode::InvalidIcon))?;
    let picker_window = window.clone();
    let picker_store = Arc::clone(store.inner());
    let result = tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_parent(&picker_window)
            .set_title("选择 128x128 PNG 图标")
            .add_filter("PNG 图片", &["png"])
            .blocking_pick_file()
            .and_then(|path| path.into_path().ok());
        selected
            .as_deref()
            .map(|path| {
                app.state::<SettingsStore>()
                    .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || {
                        picker_store.create_icon_candidate_from_path(path)
                    })
                    .map_err(|_| QuicklinkErrorCode::InvalidIcon)?
            })
            .transpose()
    })
    .await
    .map_err(|_| CommandError::quicklink(QuicklinkErrorCode::InvalidIcon))?
    .map_err(CommandError::quicklink)?;
    window
        .set_focus()
        .map_err(|_| CommandError::window_failed())?;
    window
        .as_ref()
        .set_focus()
        .map_err(|_| CommandError::window_failed())?;
    Ok(result)
}

#[tauri::command]
pub(crate) fn prepare_public_plugin_install(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    source: PublicPluginInstallSource,
    refresh_target: Option<crate::public_plugins::PublicPluginRefreshTarget>,
) -> Result<PublicPluginPrepareSummary, CommandError> {
    require_main_window(&window)?;
    Ok(service.manager()?.prepare_targeted(
        window.label(),
        source,
        refresh_target,
        Instant::now(),
    )?)
}

#[tauri::command]
pub(crate) fn get_public_plugin_diagnostics(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    plugin_id: String,
) -> Result<crate::public_plugins::diagnostics::DiagnosticReport, CommandError> {
    require_main_window(&window)?;
    Ok(service.manager()?.diagnostics(&plugin_id)?)
}

#[tauri::command]
pub(crate) fn copy_public_plugin_diagnostics(
    window: WebviewWindow,
    report: crate::public_plugins::diagnostics::DiagnosticReport,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    Ok(crate::public_plugins::diagnostics::copy_report(
        &report,
        |preview| {
            window
                .app_handle()
                .clipboard()
                .write_text(preview)
                .map_err(|_| ())
        },
    )?)
}

#[tauri::command]
pub(crate) fn cancel_public_plugin_install(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    token: String,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    Ok(service
        .manager()?
        .cancel(window.label(), &token, Instant::now())?)
}

#[tauri::command]
pub(crate) async fn commit_public_plugin_install(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    quicklinks: State<'_, Arc<QuicklinksStore>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    panel_controller: State<'_, Arc<PluginPanelController>>,
    input: CommitPublicPluginInstallInput,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_install_failed())?;
    let manager = service.manager()?;
    manager.replace_external_reserved_names(quicklinks.commands())?;
    let mut created_runtime = None;
    let outcome = service.manage_with_effects(|manager, effects| {
        manager.commit_with_readiness_with_effects(
            window.label(),
            &input.token,
            input.permission_grants,
            Instant::now(),
            |candidate| match service.create_runtime(&app, candidate) {
                Ok(_) => {
                    created_runtime = Some(candidate.label.clone());
                    true
                }
                Err(_) => false,
            },
            effects,
        )
    });
    let result = finish_public_plugin_management(
        service.inner().as_ref(),
        outcome,
        registries.main(),
        plugins.inner(),
        || {},
    );
    match result {
        Ok(commit) => {
            PublicPluginService::destroy_runtime(&app, commit.previous_runtime_label.as_deref());
            if !commit.mutation.enabled {
                PublicPluginService::destroy_runtime(&app, Some(&commit.runtime.label));
            }
            plugin_window::teardown_current(
                &app,
                window_controller.inner().as_ref(),
                &commit.mutation.plugin_id,
            );
            reset_plugin_panel(
                &app,
                panel_controller.inner().as_ref(),
                &commit.mutation.plugin_id,
            );
            let _ = focus_main_window_content_now(&app);
            Ok(commit.mutation)
        }
        Err(error) => {
            PublicPluginService::destroy_runtime(&app, created_runtime.as_deref());
            Err(error.into())
        }
    }
}

#[tauri::command]
pub(crate) async fn set_plugin_enabled(
    window: WebviewWindow,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    panel_controller: State<'_, Arc<PluginPanelController>>,
    plugin_id: String,
    enabled: bool,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let app = window.app_handle().clone();
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let mut created_runtime = None;
    let outcome = service.manage_with_effects(|manager, effects| {
        manager.set_enabled_with_readiness_with_effects(
            &plugin_id,
            enabled,
            |candidate| match service.create_runtime(&app, candidate) {
                Ok(_) => {
                    created_runtime = Some(candidate.label.clone());
                    true
                }
                Err(_) => false,
            },
            effects,
        )
    });
    let result = finish_public_plugin_management(
        service.inner().as_ref(),
        outcome,
        registries.main(),
        plugins.inner(),
        || {},
    );
    match result {
        Ok(commit) => {
            PublicPluginService::destroy_runtime(&app, commit.closed_runtime_label.as_deref());
            plugin_window::teardown_current(
                &app,
                window_controller.inner().as_ref(),
                &commit.mutation.plugin_id,
            );
            reset_plugin_panel(
                &app,
                panel_controller.inner().as_ref(),
                &commit.mutation.plugin_id,
            );
            Ok(commit.mutation)
        }
        Err(error) => {
            PublicPluginService::destroy_runtime(&app, created_runtime.as_deref());
            Err(error.into())
        }
    }
}

#[tauri::command]
pub(crate) fn set_public_plugin_permission_access(
    window: WebviewWindow,
    plugin_id: String,
    permission: PublicPermission,
    granted: bool,
    expected_inventory_revision: String,
) -> Result<(), CommandError> {
    // Authorize before resolving any application state, including on malformed
    // or forged calls from a plugin WebView.
    require_main_window(&window)?;
    let app = window.app_handle();
    let service = app.state::<Arc<PublicPluginService>>();
    let registries = app.state::<ResultRegistries>();
    let plugins = app.state::<Arc<PluginManager>>();
    let outcome = service.set_permission_access_outcome(
        &plugin_id,
        permission,
        granted,
        &expected_inventory_revision,
    );
    let failed_closed = outcome.changed && outcome.result.is_err();
    let mut cleanup_error = None;
    let result = finish_public_plugin_management(
        service.inner().as_ref(),
        outcome,
        registries.main(),
        plugins.inner(),
        || {
            if failed_closed || (!granted && permission == PublicPermission::UiWindow) {
                if let Err(error) = plugin_window::teardown_current_checked(
                    app,
                    app.state::<Arc<PluginWindowController>>().inner().as_ref(),
                    &plugin_id,
                ) {
                    cleanup_error = Some(CommandError::from(error));
                }
            }
            if failed_closed || (!granted && permission == PublicPermission::UiPanel) {
                if let Err(error) = reset_plugin_panel_checked(
                    app,
                    app.state::<Arc<PluginPanelController>>().inner().as_ref(),
                    &plugin_id,
                ) {
                    cleanup_error = Some(error);
                }
            }
            if app
                .emit_to("main", "public-plugin-permissions-changed", ())
                .is_err()
            {
                cleanup_error = Some(PublicPluginManagementError::Unavailable.into());
            }
        },
    );
    result?;
    cleanup_error.map_or(Ok(()), Err)
}

#[tauri::command]
pub(crate) fn set_public_plugin_network_access(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    plugin_id: String,
    granted: bool,
    expected_inventory_revision: Option<String>,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    finish_public_plugin_management(
        service.inner().as_ref(),
        service.set_network_access_outcome(
            &plugin_id,
            granted,
            expected_inventory_revision.as_deref(),
        ),
        registries.main(),
        plugins.inner(),
        || {
            let _ = window
                .app_handle()
                .emit_to("main", "public-plugin-permissions-changed", ());
        },
    )
}

// Request injection preserves actual top-level key presence. In particular,
// Tauri's Option<String> CommandArg normalizes both absent and null to None.
// Keep authorization and membership resolution shared with the real-IPC probes.
fn resolve_management_command(
    label: &str,
    request: &tauri::ipc::Request<'_>,
    contains: impl FnOnce(&str) -> Result<bool, CommandError>,
) -> Result<String, CommandError> {
    require_main_label(label)?;
    let tauri::ipc::InvokeBody::Json(serde_json::Value::Object(body)) = request.body() else {
        return Err(PublicPluginManagementError::Unavailable.into());
    };
    let command_id = match body.get("commandId") {
        None => "default",
        Some(serde_json::Value::String(id)) => id.as_str(),
        Some(_) => return Err(PublicPluginManagementError::Unavailable.into()),
    };
    if !contains(command_id)? {
        return Err(PublicPluginManagementError::Unavailable.into());
    }
    Ok(command_id.to_owned())
}

fn contains_management_command(
    service: &PublicPluginService,
    plugin_id: &str,
    command_id: &str,
) -> Result<bool, CommandError> {
    Ok(service.manager()?.inventory()?.items.iter().any(|plugin| {
        plugin.plugin_id == plugin_id
            && plugin
                .commands
                .iter()
                .any(|command| command.command_id == command_id)
    }))
}

#[tauri::command]
pub(crate) fn set_plugin_favorite(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    plugin_id: String,
    request: tauri::ipc::Request<'_>,
    favorite: bool,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let command_id = resolve_management_command(window.label(), &request, |id| {
        contains_management_command(service.inner().as_ref(), &plugin_id, id)
    })?;
    finish_public_plugin_management(
        service.inner().as_ref(),
        service.set_command_favorite(&plugin_id, &command_id, favorite),
        registries.main(),
        plugins.inner(),
        || {},
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_plugin_effective_name(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    quicklinks: State<'_, Arc<QuicklinksStore>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    plugin_id: String,
    request: tauri::ipc::Request<'_>,
    name_override: Option<String>,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let command_id = resolve_management_command(window.label(), &request, |id| {
        contains_management_command(service.inner().as_ref(), &plugin_id, id)
    })?;
    let manager = service.manager()?;
    manager.replace_external_reserved_names(quicklinks.commands())?;
    let normalized = name_override.as_deref().map(|value| {
        let value = value.trim();
        value.strip_prefix('/').unwrap_or(value)
    });
    finish_public_plugin_management(
        service.inner().as_ref(),
        service.rename_command(&plugin_id, &command_id, normalized),
        registries.main(),
        plugins.inner(),
        || {
            plugin_window::teardown_current(&app, window_controller.inner().as_ref(), &plugin_id);
        },
    )
}

#[tauri::command]
pub(crate) fn save_plugin_settings(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    input: SavePublicPluginSettingsInput,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let plugin_id = input.plugin_id.clone();
    save_plugin_settings_with(
        service.inner().as_ref(),
        input,
        registries.main(),
        plugins.inner(),
        || {
            plugin_window::teardown_current(&app, window_controller.inner().as_ref(), &plugin_id);
        },
    )
}

pub(crate) fn finish_public_plugin_management<T>(
    service: &PublicPluginService,
    outcome: crate::public_plugins::ManagementOutcome<T>,
    registry: &ResultRegistry,
    plugins: &PluginManager,
    cleanup: impl FnOnce(),
) -> Result<T, CommandError> {
    if outcome.changed {
        // The owned outcome is settled: public mutation/authority/submission locks
        // have been released before taking legacy admission. The method releases
        // admission before cleanup, which may re-enter other host services.
        plugins.invalidate_plugin_results(registry);
        service.cleanup_retired_result_actions(registry);
        cleanup();
    }
    Ok(outcome.result?)
}

fn save_plugin_settings_with(
    service: &PublicPluginService,
    input: SavePublicPluginSettingsInput,
    registry: &ResultRegistry,
    plugins: &PluginManager,
    teardown: impl FnOnce(),
) -> Result<PublicPluginMutation, CommandError> {
    let outcome = service.save_settings(&input.plugin_id, &input.settings, &input.secrets);
    finish_public_plugin_management(service, outcome, registry, plugins, teardown)
}

#[tauri::command]
pub(crate) fn uninstall_plugin(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    registries: State<'_, ResultRegistries>,
    plugins: State<'_, Arc<PluginManager>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    panel_controller: State<'_, Arc<PluginPanelController>>,
    plugin_id: String,
    retain_data: bool,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let manager = service.manager()?;
    let outcome = service.manage_with_effects(|manager, effects| {
        manager.begin_uninstall_with_effects(&plugin_id, retain_data, effects)
    });
    let Some(mut transaction) = finish_public_plugin_management(
        service.inner().as_ref(),
        outcome,
        registries.main(),
        plugins.inner(),
        || {},
    )?
    else {
        return Ok(());
    };
    reset_plugin_panel(&app, panel_controller.inner().as_ref(), &plugin_id);
    let previous_runtime_label = transaction.runtime_label.clone();
    if !window_controller.close_for_uninstall(&plugin_id) {
        let _ = manager.drain_uninstall_data(&mut transaction);
        PublicPluginService::destroy_runtime(&app, previous_runtime_label.as_deref());
        plugin_window::destroy_current(&app, &plugin_id);
        let _ = manager.abort_uninstall_before_commit(transaction);
        return Err(PublicPluginManagementError::Unavailable.into());
    }
    if let Err(error) = manager.drain_uninstall_data(&mut transaction) {
        PublicPluginService::destroy_runtime(&app, previous_runtime_label.as_deref());
        plugin_window::destroy_current(&app, &plugin_id);
        let _ = manager.abort_uninstall_before_commit(transaction);
        return Err(error.into());
    }
    let committed = match manager.commit_uninstall(transaction) {
        Ok(committed) => committed,
        Err(mut error) => {
            PublicPluginService::destroy_runtime(&app, previous_runtime_label.as_deref());
            plugin_window::destroy_current(&app, &plugin_id);
            if let Some(transaction) = error.transaction.take() {
                let _ = manager.abort_uninstall_before_commit(*transaction);
            }
            return Err(error.error.into());
        }
    };
    PublicPluginService::destroy_runtime(&app, committed.runtime_label.as_deref());
    plugin_window::destroy_current(&app, &plugin_id);
    manager.finish_uninstall_cleanup(&committed, app.state::<SettingsStore>().inner())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn plugin_youdao_translate(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    store: State<'_, crate::credentials::CredentialStore>,
    input: serde_json::Value,
) -> Result<
    crate::public_plugins::host_services::YoudaoResponse,
    crate::public_plugins::host_services::ServiceError,
> {
    use crate::public_plugins::host_services::{ServiceError, YoudaoCommandInput};
    let input: YoudaoCommandInput =
        serde_json::from_value(input).map_err(|_| ServiceError::from("invalidServiceRequest"))?;
    let caller = crate::public_plugins::parse_runtime_label(window.label())
        .ok_or(ServiceError::from("expiredRequest"))?;
    if caller.plugin_id != input.context.plugin_id
        || caller.generation != input.context.plugin_generation
    {
        return Err(ServiceError::from("expiredRequest"));
    }
    let manager = service
        .manager()
        .map_err(|_| ServiceError::from("networkFailure"))?;
    manager.youdao_translate(caller, input, store.inner()).await
}

#[tauri::command]
pub(crate) async fn plugin_network_request(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    input: PluginNetworkCommandInput,
) -> Result<PluginNetworkResponse, PluginNetworkCommandError> {
    let caller = validate_plugin_network_caller(window.label(), &input)
        .map_err(PluginNetworkCommandError::from)?;
    let manager = service
        .manager()
        .map_err(|_| PluginNetworkCommandError::from(PluginNetworkErrorCode::NetworkFailure))?;
    manager
        .request_network(caller, input)
        .await
        .map_err(PluginNetworkCommandError::from)
}

#[tauri::command]
pub(crate) fn plugin_api_call(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    request: PluginApiRequest,
) -> Result<serde_json::Value, CommandError> {
    let manager = service.manager()?;
    let execution = manager.execute_api(window.label(), request);
    manager
        .message_center()
        .dispatch_post_guard(&app, execution.post_guard_effect);
    Ok(execution.result?)
}

#[tauri::command]
pub(crate) fn complete_plugin_command(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    settings: State<'_, SettingsStore>,
    completion: PluginCommandCompletion,
    credentials: State<'_, crate::credentials::CredentialStore>,
) -> Result<PluginCommandCompletionResult, CommandError> {
    let now = Instant::now();
    let outcome = credentials
        .with_completion(&completion.context, |revision| {
            let mut outcome = service
                .manager()?
                .complete(window.label(), &completion, now)?;
            outcome.credential_publication = revision;
            Ok::<_, CommandError>(outcome)
        })
        .map_err(|_| CommandError::plugin_query_failed())??;
    let accepted = outcome.accepted;
    if let Some(next) = service.complete_submission(&app, &completion, outcome, now)? {
        service.dispatch(
            &app,
            &next,
            invocation_theme(&app, settings.snapshot().theme),
            invoked_at_rfc3339(),
        )?;
    }
    Ok(PluginCommandCompletionResult { accepted })
}
#[tauri::command]
pub(crate) fn plugin_window_content_ready(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginWindowController>>,
) -> Result<(), CommandError> {
    plugin_window::content_ready(controller.inner().as_ref(), webview.label())
        .then_some(())
        .ok_or_else(|| PublicPluginManagementError::InvalidCaller.into())
}

#[tauri::command]
pub(crate) fn plugin_window_content_ack(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginWindowController>>,
    request_id: String,
) -> Result<(), CommandError> {
    plugin_window::content_ack(controller.inner().as_ref(), webview.label(), &request_id)
        .then_some(())
        .ok_or_else(|| PublicPluginManagementError::InvalidCaller.into())
}

#[tauri::command]
pub(crate) fn plugin_window_content_toggle_pin(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    session_generation: String,
) -> Result<(), CommandError> {
    let session = parse_canonical_nonzero_u64(&session_generation)
        .ok_or(PublicPluginManagementError::InvalidCaller)?;
    let lease = controller
        .inner()
        .begin_window_call(webview.label(), session, true)
        .map_err(|_| PublicPluginManagementError::InvalidCaller)?;
    let label = plugin_window::plugin_shell_label(&lease.owner().plugin_id)
        .ok_or(PublicPluginManagementError::InvalidCaller)?;
    let shell = app
        .get_webview(&label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    // Route only to this plugin's trusted shell; content never receives native pin authority.
    shell
        .eval("window.dispatchEvent(new Event('uipilot-window-pin-shortcut'));")
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_window_content_close(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
) -> Result<(), CommandError> {
    plugin_window::content_close(&app, controller.inner().as_ref(), webview.label())
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) fn plugin_window_storage_get(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
    key: String,
) -> Result<Option<serde_json::Value>, CommandError> {
    let session_generation = parse_window_storage_session_generation(&session_generation)?;
    let lease = controller
        .inner()
        .begin_window_call(webview.label(), session_generation, false)
        .map_err(map_storage_window_call_error)?;
    let owner = lease.owner().clone();
    let value = service.manager()?.window_storage_get(
        &owner.plugin_id,
        owner.plugin_generation,
        owner.activation_id,
        owner.admission_epoch,
        &key,
    )?;
    drop(lease);
    Ok(value)
}

#[tauri::command]
pub(crate) fn plugin_window_storage_set(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), CommandError> {
    let session_generation = parse_window_storage_session_generation(&session_generation)?;
    let lease = controller
        .inner()
        .begin_window_call(webview.label(), session_generation, true)
        .map_err(map_storage_window_call_error)?;
    let owner = lease.owner().clone();
    service.manager()?.window_storage_set(
        &owner.plugin_id,
        owner.plugin_generation,
        owner.activation_id,
        owner.admission_epoch,
        &key,
        value,
    )?;
    drop(lease);
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_window_storage_remove(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
    key: String,
) -> Result<(), CommandError> {
    let session_generation = parse_window_storage_session_generation(&session_generation)?;
    let lease = controller
        .inner()
        .begin_window_call(webview.label(), session_generation, true)
        .map_err(map_storage_window_call_error)?;
    let owner = lease.owner().clone();
    service.manager()?.window_storage_remove(
        &owner.plugin_id,
        owner.plugin_generation,
        owner.activation_id,
        owner.admission_epoch,
        &key,
    )?;
    drop(lease);
    Ok(())
}

fn map_storage_window_call_error(error: PluginWindowCallError) -> WindowStorageError {
    match error {
        PluginWindowCallError::InvalidCaller => WindowStorageError::InvalidCaller,
        PluginWindowCallError::ExpiredWindowSession => {
            WindowStorageError::ExpiredWindowSessionError
        }
        PluginWindowCallError::Unavailable => WindowStorageError::StorageError,
    }
}

fn parse_panel_storage_session_epoch(value: &str) -> Result<u64, WindowStorageError> {
    parse_canonical_nonzero_u64(value).ok_or(WindowStorageError::ExpiredWindowSessionError)
}

fn map_storage_panel_call_error(error: PanelCallError) -> WindowStorageError {
    match error {
        PanelCallError::InvalidCaller => WindowStorageError::InvalidCaller,
        PanelCallError::ExpiredSession => WindowStorageError::ExpiredWindowSessionError,
        PanelCallError::Unavailable => WindowStorageError::StorageError,
    }
}

fn map_clipboard_history_panel_call_error(error: PanelCallError) -> CommandError {
    match error {
        PanelCallError::InvalidCaller => CommandError::invalid_caller(),
        PanelCallError::ExpiredSession => ClipboardHistoryBridgeError::ExpiredPanelSession.into(),
        PanelCallError::Unavailable => ClipboardHistoryBridgeError::Unavailable.into(),
    }
}

fn begin_clipboard_history_panel_call(
    caller_label: &str,
    controller: &PluginPanelController,
    session_epoch: &str,
    mutable: bool,
) -> Result<PanelSessionIdentity, CommandError> {
    let session_epoch = parse_canonical_nonzero_u64(session_epoch)
        .ok_or(ClipboardHistoryBridgeError::ExpiredPanelSession)?;
    controller
        .begin_content_call(caller_label, session_epoch, mutable)
        .map_err(map_clipboard_history_panel_call_error)
}

#[tauri::command]
pub(crate) fn plugin_panel_content_ready(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    host_key_receiver_registered: bool,
    host_key_registration_violation: bool,
) -> Result<(), CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    plugin_panel::content_ready_with_host_keys(
        controller.inner().as_ref(),
        webview.label(),
        session_epoch,
        host_key_receiver_registered,
        host_key_registration_violation,
    )
    .then_some(())
    .ok_or_else(|| PublicPluginManagementError::InvalidCaller.into())
}

#[tauri::command]
pub(crate) fn plugin_panel_content_ack(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    request_id: String,
) -> Result<(), CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    plugin_panel::content_ack(
        controller.inner().as_ref(),
        webview.label(),
        session_epoch,
        &request_id,
    )
    .then_some(())
    .ok_or_else(|| PublicPluginManagementError::InvalidCaller.into())
}

#[tauri::command]
pub(crate) fn plugin_panel_host_key_enqueue(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    input: PluginPanelHostKeyEnqueueInput,
) -> Result<PluginPanelHostKeyEnqueueResult, CommandError> {
    require_main_label(webview.label())?;
    let session_epoch = parse_panel_session_epoch(&input.session_epoch)?;
    let client_sequence = parse_panel_session_epoch(&input.client_sequence)?;
    let decision = controller
        .enqueue_host_key(
            session_epoch,
            HostKeyEnqueueInput {
                client_sequence,
                declaration: input.declaration,
                key: input.key,
                ctrl_key: input.ctrl_key,
                meta_key: input.meta_key,
                shift_key: input.shift_key,
                alt_key: input.alt_key,
            },
        )
        .map_err(|_| CommandError::window_failed())?;
    let result = match decision.outcome {
        HostKeyEnqueueOutcome::Enqueued { route_sequence } => {
            PluginPanelHostKeyEnqueueResult::Enqueued {
                route_sequence: route_sequence.to_string(),
            }
        }
        HostKeyEnqueueOutcome::DroppedQueueFull => {
            PluginPanelHostKeyEnqueueResult::DroppedQueueFull
        }
        HostKeyEnqueueOutcome::Noop => PluginPanelHostKeyEnqueueResult::Noop,
        HostKeyEnqueueOutcome::ProtocolViolation => {
            PluginPanelHostKeyEnqueueResult::ProtocolViolation
        }
    };
    if decision.terminate_session {
        let registries = app.state::<ResultRegistries>();
        let _ = clear_and_hide_window(
            registries.main(),
            &webview.window(),
            HideReason::ExplicitReturn,
        );
    } else if decision.start_pump {
        plugin_panel::start_host_key_pump(app, Arc::clone(controller.inner()));
    }
    Ok(result)
}

#[tauri::command]
pub(crate) fn plugin_panel_host_key_ack(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    route_sequence: String,
) -> Result<(), CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    let route_sequence = parse_panel_storage_session_epoch(&route_sequence)?;
    let _ = controller.ack_host_key(webview.label(), session_epoch, route_sequence);
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_panel_request_hide_admit(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
) -> Result<PanelHideAdmitResult, CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    let Some(identity) = controller
        .admit_hide(webview.label(), session_epoch)
        .map_err(|_| CommandError::window_failed())?
    else {
        return Ok(PanelHideAdmitResult::Noop);
    };
    plugin_panel::schedule_panel_hide_fallback(
        app,
        Arc::clone(controller.inner()),
        identity,
        PanelHideTicketPhase::Admitted,
        Duration::from_secs(30),
    );
    Ok(PanelHideAdmitResult::Admitted {
        hide_ticket_id: identity.hide_ticket_id.to_string(),
    })
}

fn parse_panel_hide_ticket(
    session_epoch: &str,
    hide_ticket_id: &str,
) -> Result<PanelHideTicketIdentity, CommandError> {
    Ok(PanelHideTicketIdentity {
        session_epoch: parse_panel_session_epoch(session_epoch)?,
        hide_ticket_id: parse_panel_session_epoch(hide_ticket_id)?,
    })
}

#[tauri::command]
pub(crate) fn plugin_panel_request_hide_admit_observed(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    hide_ticket_id: String,
) -> Result<(), CommandError> {
    let identity = parse_panel_hide_ticket(&session_epoch, &hide_ticket_id)?;
    if controller
        .live_identity()
        .is_none_or(|session| session.content_label != webview.label())
    {
        return Err(CommandError::window_failed());
    }
    if controller.observe_hide(identity) {
        plugin_panel::schedule_panel_hide_fallback(
            app,
            Arc::clone(controller.inner()),
            identity,
            PanelHideTicketPhase::Observed,
            Duration::from_millis(500),
        );
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_panel_request_hide_commit(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    hide_ticket_id: String,
) -> Result<(), CommandError> {
    let identity = parse_panel_hide_ticket(&session_epoch, &hide_ticket_id)?;
    if controller
        .live_identity()
        .is_none_or(|session| session.content_label != webview.label())
    {
        return Err(CommandError::window_failed());
    }
    plugin_panel::commit_panel_hide(&app, Arc::clone(controller.inner()), identity)
        .map(|_| ())
        .map_err(|_| CommandError::window_failed())
}

fn host_input_focus_failure(
    controller: &PluginPanelController,
    identity: HostInputFocusIdentity,
) -> Result<(), CommandError> {
    match controller.cancel_host_input_focus(identity) {
        Ok(true) | Err(_) => Err(CommandError::window_failed()),
        Ok(false) => Ok(()),
    }
}

fn start_host_input_focus(
    controller: &PluginPanelController,
    identity: HostInputFocusIdentity,
    native_focus: impl FnOnce() -> bool,
    confirmation_time: impl FnOnce() -> Instant,
    emit_request: impl FnOnce() -> bool,
) -> HostInputFocusStart {
    match controller.claim_host_input_focus(identity) {
        Ok(HostInputFocusAdvance::Noop) => HostInputFocusStart::Noop,
        Ok(HostInputFocusAdvance::Expired) => HostInputFocusStart::Failed,
        Err(_) => {
            let _ = controller.cancel_host_input_focus(identity);
            HostInputFocusStart::Failed
        }
        Ok(HostInputFocusAdvance::Advanced) if !native_focus() => {
            match controller.fail_native_host_input_focus(identity) {
                Ok(false) => HostInputFocusStart::Noop,
                Ok(true) | Err(_) => HostInputFocusStart::Failed,
            }
        }
        Ok(HostInputFocusAdvance::Advanced) => {
            match controller.confirm_native_host_input_focus(identity, confirmation_time()) {
                Ok(HostInputFocusAdvance::Noop) => HostInputFocusStart::Noop,
                Ok(HostInputFocusAdvance::Expired) => HostInputFocusStart::Failed,
                Err(_) => {
                    let _ = controller.cancel_host_input_focus(identity);
                    HostInputFocusStart::Failed
                }
                Ok(HostInputFocusAdvance::Advanced) if !emit_request() => {
                    match controller.cancel_host_input_focus(identity) {
                        Ok(false) => HostInputFocusStart::Noop,
                        Ok(true) | Err(_) => HostInputFocusStart::Failed,
                    }
                }
                Ok(HostInputFocusAdvance::Advanced) => HostInputFocusStart::AwaitingAck,
            }
        }
    }
}

fn focus_main_window_content_now(app: &AppHandle) -> bool {
    let Some(main_window) = app.get_window("main") else {
        return false;
    };
    let Some(main_webview) = app.get_webview("main") else {
        return false;
    };
    main_window.set_focus().is_ok() && main_webview.set_focus().is_ok()
}

fn focus_live_panel_host_content(
    controller: &PluginPanelController,
    session_epoch: u64,
    native_focus: impl FnOnce() -> bool,
) -> bool {
    if controller
        .live_identity()
        .is_none_or(|identity| identity.session_epoch != session_epoch)
    {
        return false;
    }
    native_focus()
        && controller
            .live_identity()
            .is_some_and(|identity| identity.session_epoch == session_epoch)
}

async fn focus_main_window_content(
    app: &AppHandle,
    controller: Arc<PluginPanelController>,
    session_epoch: u64,
) -> bool {
    let (sender, receiver) = mpsc::sync_channel(1);
    let focus_app = app.clone();
    let focus_controller = Arc::clone(&controller);
    if app
        .run_on_main_thread(move || {
            let focused = focus_live_panel_host_content(&focus_controller, session_epoch, || {
                focus_main_window_content_now(&focus_app)
            });
            let _ = sender.send(focused);
        })
        .is_err()
    {
        return false;
    }
    matches!(
        tauri::async_runtime::spawn_blocking(move || {
            receiver.recv_timeout(HOST_INPUT_FOCUS_TIMEOUT)
        })
        .await,
        Ok(Ok(true))
    ) && controller
        .live_identity()
        .is_some_and(|identity| identity.session_epoch == session_epoch)
}

#[tauri::command]
pub(crate) async fn plugin_panel_focus_host_input(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
) -> Result<(), CommandError> {
    let Some(session_epoch) = parse_canonical_nonzero_u64(&session_epoch) else {
        return Ok(());
    };
    let deadline = Instant::now()
        .checked_add(HOST_INPUT_FOCUS_TIMEOUT)
        .ok_or_else(CommandError::window_failed)?;
    let controller = Arc::clone(controller.inner());
    let Some(identity) = controller
        .prepare_host_input_focus(webview.label(), session_epoch, deadline)
        .map_err(|_| CommandError::window_failed())?
    else {
        return Ok(());
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let focus_app = app.clone();
    let focus_controller = Arc::clone(&controller);
    if app
        .run_on_main_thread(move || {
            let outcome = start_host_input_focus(
                &focus_controller,
                identity,
                || focus_main_window_content_now(&focus_app),
                Instant::now,
                || {
                    let payload = PluginPanelFocusHostInputEvent {
                        session_epoch: identity.session_epoch.to_string(),
                        focus_request_id: identity.focus_request_id.to_string(),
                    };
                    focus_app
                        .emit_to("main", PLUGIN_PANEL_FOCUS_HOST_INPUT_EVENT, payload)
                        .is_ok()
                },
            );
            let _ = sender.send(outcome);
        })
        .is_err()
    {
        return host_input_focus_failure(&controller, identity);
    }

    let start = tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    })
    .await;
    match start {
        Ok(Ok(HostInputFocusStart::Noop)) => Ok(()),
        Ok(Ok(HostInputFocusStart::Failed)) => {
            let _ = controller.cancel_host_input_focus(identity);
            Err(CommandError::window_failed())
        }
        Ok(Ok(HostInputFocusStart::AwaitingAck)) => {
            let wait_controller = Arc::clone(&controller);
            let settled = tauri::async_runtime::spawn_blocking(move || {
                wait_controller.wait_host_input_focus(identity)
            })
            .await;
            match settled {
                Ok(Ok(HostInputFocusOutcome::Focused | HostInputFocusOutcome::Noop)) => Ok(()),
                Ok(Ok(HostInputFocusOutcome::Failed)) | Ok(Err(_)) => {
                    Err(CommandError::window_failed())
                }
                Err(_) => host_input_focus_failure(&controller, identity),
            }
        }
        Ok(Err(_)) | Err(_) => host_input_focus_failure(&controller, identity),
    }
}

#[tauri::command]
pub(crate) fn plugin_panel_focus_host_input_ack(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    focus_request_id: String,
    focused: bool,
) -> Result<(), CommandError> {
    require_main_label(webview.label())?;
    let (Some(session_epoch), Some(focus_request_id)) = (
        parse_canonical_nonzero_u64(&session_epoch),
        parse_canonical_nonzero_u64(&focus_request_id),
    ) else {
        return Ok(());
    };
    controller
        .ack_host_input_focus(
            HostInputFocusIdentity {
                session_epoch,
                focus_request_id,
            },
            focused,
        )
        .map_err(|_| CommandError::window_failed())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn plugin_window_open_text(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
    document_generation: String,
) -> Result<
    Option<crate::public_plugins::file_open::OpenedText>,
    crate::public_plugins::file_open::FileOpenError,
> {
    use crate::public_plugins::file_open::FileOpenError as E;
    if plugin_window::plugin_id_from_content_label(webview.label()).is_none() {
        return Err(E::ExpiredWindowSessionError);
    }
    let session =
        parse_canonical_nonzero_u64(&session_generation).ok_or(E::ExpiredWindowSessionError)?;
    let document =
        parse_canonical_nonzero_u64(&document_generation).ok_or(E::ExpiredWindowSessionError)?;
    plugin_window::open_text(
        app,
        controller.inner().clone(),
        service.inner().clone(),
        webview.label().into(),
        session,
        document,
    )
    .await
}

#[tauri::command]
pub(crate) async fn plugin_window_request(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    settings: State<'_, SettingsStore>,
    session_generation: String,
    document_generation: String,
    request: serde_json::Value,
) -> Result<serde_json::Value, crate::public_plugins::window_action::WindowActionError> {
    use crate::public_plugins::window_action::{
        parse_request, parse_session, WindowActionError as E,
    };
    if plugin_window::plugin_id_from_content_label(webview.label()).is_none() {
        return Err(E::ExpiredWindowSessionError);
    }
    let session = parse_session(&session_generation)?;
    let document = parse_session(&document_generation)?;
    let request = parse_request(request)?;
    let (dispatch, receiver) = controller.inner().admit_window_action(
        service.inner(),
        webview.label(),
        session,
        document,
        request,
    )?;
    if service
        .dispatch(
            &app,
            &dispatch,
            invocation_theme(&app, settings.snapshot().theme),
            invoked_at_rfc3339(),
        )
        .is_err()
    {
        // The emit may have delivered; watchdog owns the physical slot until completion/deadline.
        if let Ok(manager) = service.manager() {
            manager.retire_panel_action(&dispatch.context);
        }
        service.settle_window_action(
            &dispatch.context,
            Err(crate::public_plugins::content_action::ContentActionError::Unavailable),
            true,
        );
    }
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv()
            .unwrap_or(Err(E::WindowRequestUnavailableError))
    })
    .await
    .map_err(|_| E::WindowRequestUnavailableError)?
}

#[tauri::command]
pub(crate) async fn plugin_panel_request(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    settings: State<'_, SettingsStore>,
    session_epoch: String,
    request: serde_json::Value,
) -> Result<serde_json::Value, crate::public_plugins::panel_action::PanelActionError> {
    use crate::public_plugins::panel_action::{PanelActionError as E, PanelActionRequest};
    if plugin_panel::plugin_id_from_panel_content_label(webview.label()).is_none() {
        return Err(E::ExpiredPanelSession);
    }
    let epoch = crate::public_plugins::panel_action::parse_epoch(&session_epoch)?;
    let request = PanelActionRequest::from_value(request)?;
    let (dispatch, receiver) =
        controller
            .inner()
            .admit_panel_action(service.inner(), webview.label(), epoch, request)?;
    if service
        .dispatch(
            &app,
            &dispatch,
            invocation_theme(&app, settings.snapshot().theme),
            invoked_at_rfc3339(),
        )
        .is_err()
    {
        // Delivery may have occurred. Revoke authority but retain the physical slot and watchdog.
        if let Ok(manager) = service.manager() {
            manager.retire_panel_action(&dispatch.context);
        }
        service.settle_panel_action(
            &dispatch.context,
            Err(E::PanelRequestUnavailableError),
            true,
        );
    }
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv()
            .unwrap_or(Err(E::PanelRequestUnavailableError))
    })
    .await
    .map_err(|_| E::PanelRequestUnavailableError)?
}

#[tauri::command]
pub(crate) fn plugin_panel_set_status(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    sequence: String,
    status: crate::panel_status::PanelStatus,
) -> Result<(), CommandError> {
    let epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    parse_panel_storage_session_epoch(&sequence)?;
    controller
        .begin_content_call(webview.label(), epoch, true)
        .map_err(map_storage_panel_call_error)?;
    if !status.valid() {
        return Err(CommandError::window_failed());
    }
    app.emit_to(
        "main",
        "uipilot-plugin-panel-status",
        crate::panel_status::PanelStatusEvent {
            session_epoch,
            sequence,
            status,
        },
    )
    .map_err(|_| CommandError::window_failed())
}

#[tauri::command]
pub(crate) fn open_plugin_panel_actions(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    source: String,
    ticket: String,
) -> Result<(), CommandError> {
    require_main_label(webview.label())?;
    let epoch = parse_panel_session_epoch(&session_epoch)?;
    if !matches!(source.as_str(), "hostInput" | "content") || ticket.len() > 64 {
        return Err(CommandError::window_failed());
    }
    let identity = controller
        .claim_bounds_identity(epoch)
        .map_err(map_panel_bounds_error)?;
    let content = app
        .get_webview(&identity.content_label)
        .ok_or_else(CommandError::window_failed)?;
    let payload =
        serde_json::json!({ "sessionEpoch": session_epoch, "source": source, "ticket": ticket });
    controller
        .expect_actions_focus(epoch, true)
        .map_err(map_panel_bounds_error)?;
    if content.set_focus().is_err() {
        let _ = controller.expect_actions_focus(epoch, false);
        return Err(CommandError::window_failed());
    }
    controller
        .claim_bounds_identity(epoch)
        .map_err(map_panel_bounds_error)?;
    content
        .eval(&format!(
            "window.__UIPILOT_PLUGIN_PANEL_ACTIONS__({payload});"
        ))
        .map_err(|_| CommandError::window_failed())
}

#[tauri::command]
pub(crate) fn plugin_panel_actions_submit(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    ticket: String,
) -> Result<(), CommandError> {
    let epoch = parse_panel_session_epoch(&session_epoch)?;
    controller
        .begin_content_call(webview.label(), epoch, true)
        .map_err(map_storage_panel_call_error)?;
    if ticket.is_empty() || ticket.len() > 64 {
        return Err(CommandError::window_failed());
    }
    app.emit_to(
        "main",
        "uipilot-plugin-panel-actions-submit",
        serde_json::json!({ "sessionEpoch": session_epoch, "ticket": ticket }),
    )
    .map_err(|_| CommandError::window_failed())
}

#[tauri::command]
pub(crate) fn plugin_panel_storage_get(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    key: String,
) -> Result<Option<serde_json::Value>, CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    let identity = controller
        .inner()
        .begin_content_call(webview.label(), session_epoch, false)
        .map_err(map_storage_panel_call_error)?;
    let value = service.manager()?.window_storage_get(
        &identity.plugin_id,
        identity.generation,
        identity.activation_id,
        identity.admission_epoch,
        &key,
    )?;
    Ok(value)
}

#[tauri::command]
pub(crate) fn plugin_panel_storage_set(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    let identity = controller
        .inner()
        .begin_content_call(webview.label(), session_epoch, true)
        .map_err(map_storage_panel_call_error)?;
    service.manager()?.window_storage_set(
        &identity.plugin_id,
        identity.generation,
        identity.activation_id,
        identity.admission_epoch,
        &key,
        value,
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_panel_storage_remove(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    key: String,
) -> Result<(), CommandError> {
    let session_epoch = parse_panel_storage_session_epoch(&session_epoch)?;
    let identity = controller
        .inner()
        .begin_content_call(webview.label(), session_epoch, true)
        .map_err(map_storage_panel_call_error)?;
    service.manager()?.window_storage_remove(
        &identity.plugin_id,
        identity.generation,
        identity.activation_id,
        identity.admission_epoch,
        &key,
    )?;
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_panel_clipboard_history_list(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    query: Option<String>,
) -> Result<ClipboardHistorySnapshot, CommandError> {
    let identity = begin_clipboard_history_panel_call(
        webview.label(),
        controller.inner().as_ref(),
        &session_epoch,
        false,
    )?;
    service
        .manager()?
        .clipboard_history_query_for_panel(
            &identity.plugin_id,
            identity.generation,
            identity.activation_id,
            identity.admission_epoch,
            query.as_deref(),
        )
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn plugin_panel_clipboard_history_remove(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    id: String,
) -> Result<(), CommandError> {
    let identity = begin_clipboard_history_panel_call(
        webview.label(),
        controller.inner().as_ref(),
        &session_epoch,
        true,
    )?;
    service
        .manager()?
        .clipboard_history_remove_for_panel(
            &identity.plugin_id,
            identity.generation,
            identity.activation_id,
            identity.admission_epoch,
            &id,
        )
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn plugin_panel_clipboard_history_clear(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
) -> Result<(), CommandError> {
    let identity = begin_clipboard_history_panel_call(
        webview.label(),
        controller.inner().as_ref(),
        &session_epoch,
        true,
    )?;
    service
        .manager()?
        .clipboard_history_clear_for_panel(
            &identity.plugin_id,
            identity.generation,
            identity.activation_id,
            identity.admission_epoch,
        )
        .map_err(CommandError::from)
}

fn begin_panel_text_effect(
    label: &str,
    controller: &PluginPanelController,
    epoch: &str,
) -> Result<PanelSessionIdentity, CommandError> {
    use crate::panel_effects::EffectError as E;
    let epoch = parse_canonical_nonzero_u64(epoch).ok_or(E::ExpiredPanelSession)?;
    controller
        .begin_content_call(label, epoch, true)
        .map_err(|_| E::ExpiredPanelSession.into())
}

#[tauri::command]
pub(crate) fn plugin_panel_clipboard_write_text(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    input: crate::panel_effects::PanelText,
) -> Result<(), CommandError> {
    use crate::panel_effects::{validate_text, EffectError as E};
    let identity = begin_panel_text_effect(webview.label(), &controller, &session_epoch)?;
    service
        .manager()?
        .with_panel_text_authorization(&identity, PublicPermission::ClipboardWrite, |_| {
            controller.with_current_text_effect(
                webview.label(),
                identity.session_epoch,
                |current, _| {
                    if current != &identity {
                        return Err(E::ExpiredPanelSession);
                    }
                    validate_text(&input.text)?;
                    app.clipboard()
                        .write_text(input.text)
                        .map_err(|_| E::ClipboardWriteFailed)
                },
            )
        })
        .map_err(Into::into)
}

fn panel_speech_owner(
    label: &str,
    app: &AppHandle,
    controller: &PluginPanelController,
    manager: &PublicPluginManager,
    epoch: &str,
) -> Result<crate::panel_effects::SpeechOwner, CommandError> {
    let identity = begin_panel_text_effect(label, controller, epoch)?;
    manager
        .with_panel_text_authorization(&identity, PublicPermission::SpeechSpeak, |authorization| {
            controller.with_current_text_effect(
                label,
                identity.session_epoch,
                |current, session| {
                    if current != &identity {
                        return Err(crate::panel_effects::EffectError::ExpiredPanelSession);
                    }
                    let app = app.clone();
                    let label = identity.content_label.clone();
                    let epoch = identity.session_epoch.to_string();
                    Ok(crate::panel_effects::SpeechOwner {
                        key: format!("{}:{}", label, epoch),
                        session,
                        authorization,
                        publish: Arc::new(move |state| {
                            // Only the captured content document receives this event. Its bridge checks epoch again.
                            let event = serde_json::json!({"sessionEpoch":epoch,"state":state});
                            if let Some(content) = app.get_webview(&label) {
                                let _ = content.eval(format!(
                                    "window.__UIPILOT_PLUGIN_PANEL_SPEECH_STATE__({event});"
                                ));
                            }
                        }),
                    })
                },
            )
        })
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn plugin_panel_speech_voices(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
) -> Result<Vec<crate::panel_effects::SpeechVoice>, CommandError> {
    let owner = panel_speech_owner(
        webview.label(),
        &app,
        &controller,
        service.manager()?.as_ref(),
        &session_epoch,
    )?;
    crate::panel_effects::PanelSpeech::global()
        .voices(owner)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn plugin_panel_speech_play(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    input: crate::panel_effects::SpeechPlay,
) -> Result<crate::panel_effects::SpeechState, CommandError> {
    let owner = panel_speech_owner(
        webview.label(),
        &app,
        &controller,
        service.manager()?.as_ref(),
        &session_epoch,
    )?;
    crate::panel_effects::PanelSpeech::global()
        .play(owner, input)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn plugin_panel_speech_stop(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    input: crate::panel_effects::SpeechStop,
) -> Result<crate::panel_effects::SpeechState, CommandError> {
    let owner = panel_speech_owner(
        webview.label(),
        &app,
        &controller,
        service.manager()?.as_ref(),
        &session_epoch,
    )?;
    crate::panel_effects::PanelSpeech::global()
        .stop(owner, input)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn plugin_panel_speech_state(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
) -> Result<crate::panel_effects::SpeechState, CommandError> {
    let owner = panel_speech_owner(
        webview.label(),
        &app,
        &controller,
        service.manager()?.as_ref(),
        &session_epoch,
    )?;
    crate::panel_effects::PanelSpeech::global()
        .state(owner)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) fn plugin_panel_clipboard_history_copy(
    webview: tauri::Webview,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    id: String,
) -> Result<ClipboardHistoryCopyOutcome, CommandError> {
    let identity = begin_clipboard_history_panel_call(
        webview.label(),
        controller.inner().as_ref(),
        &session_epoch,
        true,
    )?;
    let manager = service.manager()?;
    manager.with_clipboard_history_copy_authorization(
        &identity.plugin_id,
        identity.generation,
        identity.activation_id,
        identity.admission_epoch,
        || {
            controller.with_current_clipboard_history_copy(
                webview.label(),
                identity.session_epoch,
                |current| {
                    if current != &identity {
                        return Err(ClipboardHistoryPasteError::ExpiredPanelSession);
                    }
                    manager.clipboard_history_write_copy_for_panel(&identity.plugin_id, &id)
                },
            )
        },
    )?;
    Ok(ClipboardHistoryCopyOutcome {
        outcome: ClipboardHistoryCopyStatus::Copied,
    })
}

#[tauri::command]
pub(crate) fn plugin_panel_clipboard_history_paste(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    id: String,
    route_sequence: String,
) -> Result<ClipboardHistoryPasteOutcome, CommandError> {
    let identity = begin_clipboard_history_panel_call(
        webview.label(),
        controller.inner().as_ref(),
        &session_epoch,
        true,
    )?;
    let route_sequence = parse_canonical_nonzero_u64(&route_sequence)
        .ok_or(ClipboardHistoryPasteError::ExpiredPanelSession)?;
    let manager = service.manager()?;
    let record = manager
        .clipboard_history_record_for_paste_panel(
            &identity.plugin_id,
            identity.generation,
            identity.activation_id,
            identity.admission_epoch,
            &id,
        )
        .map_err(CommandError::from)?;
    let paste_ticket = controller
        .begin_enter_host_key_paste(webview.label(), identity.session_epoch, route_sequence)
        .ok_or(ClipboardHistoryPasteError::ExpiredPanelSession)?;
    let target = match app
        .state::<Arc<LifecycleCoordinator>>()
        .clipboard_paste_target_for_explicit_return()
    {
        Some(target) => target,
        None => {
            let _ = controller.cancel_enter_host_key_paste(paste_ticket);
            return Err(ClipboardHistoryPasteError::PasteTargetUnavailable.into());
        }
    };
    if let Err(error) = manager.clipboard_history_begin_paste_restore_for_panel() {
        let _ = controller.cancel_enter_host_key_paste(paste_ticket);
        return Err(error.into());
    }
    let authority_revision = match manager.with_clipboard_history_paste_authorization(
        &identity.plugin_id,
        identity.generation,
        identity.activation_id,
        identity.admission_epoch,
        None,
        |revision| paste_clipboard_history_record(&record).map(|_| revision),
    ) {
        Ok(revision) => revision,
        Err(error) => {
            manager.clipboard_history_cancel_paste_restore_for_panel();
            let _ = controller.cancel_enter_host_key_paste(paste_ticket);
            return Err(error.into());
        }
    };
    if !controller.commit_enter_host_key_paste(paste_ticket) {
        return Err(ClipboardHistoryPasteError::ExpiredPanelSession.into());
    }
    manager
        .clipboard_history_complete_paste_for_panel(&identity.plugin_id, &id)
        .map_err(CommandError::from)?;
    schedule_clipboard_history_paste_hide_and_send(
        app,
        target,
        identity,
        authority_revision,
        Arc::clone(service.inner()),
    );
    Ok(ClipboardHistoryPasteOutcome {
        outcome: ClipboardHistoryPasteStatus::Admitted,
    })
}

fn schedule_clipboard_history_paste_hide_and_send(
    app: AppHandle,
    target: ClipboardPasteTarget,
    identity: PanelSessionIdentity,
    authority_revision: u64,
    service: Arc<PublicPluginService>,
) {
    std::thread::spawn(move || {
        let dispatch_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            let controller = dispatch_app.state::<Arc<PluginPanelController>>();
            if controller
                .begin_content_call(&identity.content_label, identity.session_epoch, false)
                .is_err()
            {
                return;
            }
            let Ok(manager) = service.manager() else {
                return;
            };
            if manager
                .with_clipboard_history_paste_authorization(
                    &identity.plugin_id,
                    identity.generation,
                    identity.activation_id,
                    identity.admission_epoch,
                    Some(authority_revision),
                    |_| Ok(()),
                )
                .is_err()
            {
                return;
            }
            let Some(window) = dispatch_app.get_window("main") else {
                return;
            };
            let registries = dispatch_app.state::<ResultRegistries>();
            if clear_and_hide_window(registries.main(), &window, HideReason::ExplicitReturn)
                .is_err()
            {
                return;
            }
            let lifecycle = dispatch_app.state::<Arc<LifecycleCoordinator>>();
            // Hiding can retire the panel. Keep the original owner and recheck
            // its grants at the actual native send, without locking across hide.
            let _ = manager.with_clipboard_history_paste_authorization(
                &identity.plugin_id,
                identity.generation,
                identity.activation_id,
                identity.admission_epoch,
                Some(authority_revision),
                |_| {
                    if lifecycle.clipboard_paste_target_is_foreground(target) {
                        let _ = send_ctrl_v_to_foreground_target(target);
                    }
                    Ok(())
                },
            );
        });
    });
}

#[tauri::command]
pub(crate) fn plugin_window_timer_get_state(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
) -> Result<PluginTimerState, CommandError> {
    let session_generation = parse_timer_session_generation(&session_generation)?;
    let lease = controller
        .inner()
        .begin_window_call(webview.label(), session_generation, false)
        .map_err(map_timer_window_call_error)?;
    let owner = lease.owner().clone();
    let state = service.manager()?.window_timer_get_state(
        &owner.plugin_id,
        owner.plugin_generation,
        owner.activation_id,
    )?;
    drop(lease);
    plugin_window::publish_timer_state(
        &app,
        controller.inner().as_ref(),
        &crate::public_plugins::TimerKey::new(
            &owner.plugin_id,
            owner.plugin_generation,
            owner.activation_id,
        )
        .ok_or(TimerError::TimerUnavailable)?,
        &state,
    );
    Ok(state)
}

#[tauri::command]
pub(crate) fn plugin_window_timer_start(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
    input: Option<PluginTimerStartInput>,
) -> Result<PluginTimerState, CommandError> {
    plugin_window_timer_mutation(
        webview.label(),
        &app,
        controller.inner(),
        service.inner(),
        &session_generation,
        |manager, owner| {
            manager.window_timer_start(
                &owner.plugin_id,
                owner.plugin_generation,
                owner.activation_id,
                input,
            )
        },
    )
}

#[tauri::command]
pub(crate) fn plugin_window_timer_stop(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
) -> Result<PluginTimerState, CommandError> {
    plugin_window_timer_mutation(
        webview.label(),
        &app,
        controller.inner(),
        service.inner(),
        &session_generation,
        |manager, owner| {
            manager.window_timer_stop(
                &owner.plugin_id,
                owner.plugin_generation,
                owner.activation_id,
            )
        },
    )
}

#[tauri::command]
pub(crate) fn plugin_window_timer_reset(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    service: State<'_, Arc<PublicPluginService>>,
    session_generation: String,
) -> Result<PluginTimerState, CommandError> {
    plugin_window_timer_mutation(
        webview.label(),
        &app,
        controller.inner(),
        service.inner(),
        &session_generation,
        |manager, owner| {
            manager.window_timer_reset(
                &owner.plugin_id,
                owner.plugin_generation,
                owner.activation_id,
            )
        },
    )
}

fn plugin_window_timer_mutation(
    caller_label: &str,
    app: &AppHandle,
    controller: &Arc<PluginWindowController>,
    service: &Arc<PublicPluginService>,
    session_generation: &str,
    operation: impl FnOnce(
        &PublicPluginManager,
        &PluginWindowOwner,
    ) -> Result<PluginTimerState, TimerError>,
) -> Result<PluginTimerState, CommandError> {
    let session_generation = parse_timer_session_generation(session_generation)?;
    let lease = controller
        .begin_window_call(caller_label, session_generation, true)
        .map_err(map_timer_window_call_error)?;
    let owner = lease.owner().clone();
    let state = operation(service.manager()?.as_ref(), &owner)?;
    drop(lease);
    let key = crate::public_plugins::TimerKey::new(
        &owner.plugin_id,
        owner.plugin_generation,
        owner.activation_id,
    )
    .ok_or(TimerError::TimerUnavailable)?;
    plugin_window::publish_timer_state(app, controller.as_ref(), &key, &state);
    Ok(state)
}

fn map_timer_window_call_error(error: PluginWindowCallError) -> TimerError {
    match error {
        PluginWindowCallError::InvalidCaller => TimerError::InvalidCaller,
        PluginWindowCallError::ExpiredWindowSession => TimerError::ExpiredWindowSessionError,
        PluginWindowCallError::Unavailable => TimerError::TimerUnavailable,
    }
}

#[tauri::command]
pub(crate) async fn commit_plugin_window_transfer(
    window: WebviewWindow,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    transfers: State<'_, Arc<MainWindowTransferCoordinator>>,
    transfer_token: String,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let controller = Arc::clone(controller.inner());
    let transfers = Arc::clone(transfers.inner());
    tauri::async_runtime::spawn_blocking(move || {
        plugin_window::commit(
            &app,
            controller.as_ref(),
            transfers.as_ref(),
            &transfer_token,
        )
    })
    .await
    .map_err(|_| PublicPluginManagementError::Unavailable)??;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_public_plugin_window_identity(
    webview: tauri::Webview,
    service: State<'_, Arc<PublicPluginService>>,
) -> Result<PublicPluginWindowIdentity, CommandError> {
    let plugin_id = plugin_window::plugin_id_from_shell_label(webview.label())
        .ok_or(PublicPluginManagementError::InvalidCaller)?;
    Ok(service.manager()?.window_identity(&plugin_id)?)
}

#[tauri::command]
pub(crate) fn set_plugin_window_pinned(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
    pinned: bool,
) -> Result<PluginWindowPinState, CommandError> {
    Ok(plugin_window::set_pinned(
        &app,
        controller.inner().as_ref(),
        webview.label(),
        pinned,
    )?)
}

#[tauri::command]
pub(crate) fn close_plugin_window(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginWindowController>>,
) -> Result<(), CommandError> {
    plugin_window::close(&app, controller.inner().as_ref(), webview.label())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn open_plugin_panel(
    window: WebviewWindow,
    app: AppHandle,
    public: State<'_, Arc<PublicPluginService>>,
    controller: State<'_, Arc<PluginPanelController>>,
    input: OpenPluginPanelInput,
) -> Result<Option<PluginPanelCommandResult>, CommandError> {
    require_main_window(&window)?;
    open_plugin_panel_impl(app, public, controller, input).await
}

#[tauri::command]
pub(crate) async fn submit_plugin_panel(
    webview: tauri::Webview,
    app: AppHandle,
    public: State<'_, Arc<PublicPluginService>>,
    controller: State<'_, Arc<PluginPanelController>>,
    settings: State<'_, SettingsStore>,
    input: SubmitPluginPanelInput,
) -> Result<Option<PluginPanelCommandResult>, CommandError> {
    require_main_label(webview.label())?;
    submit_plugin_panel_impl(app, public, controller, settings, input).await
}

#[tauri::command]
pub(crate) fn set_plugin_panel_bounds(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    input: SetPluginPanelBoundsInput,
) -> Result<(), CommandError> {
    require_main_label(webview.label())?;
    let session_epoch = parse_panel_session_epoch(&input.session_epoch)?;
    let result = plugin_panel::set_bounds(
        &app,
        controller.inner().as_ref(),
        session_epoch,
        PanelBounds {
            x: input.bounds.x,
            y: input.bounds.y,
            width: input.bounds.width,
            height: input.bounds.height,
        },
    );
    if let Err(error) = &result {
        eprintln!("[plugin-panel-bounds] session={session_epoch} error={error:?}");
    }
    result.map_err(map_panel_bounds_error)
}

#[tauri::command]
pub(crate) fn close_plugin_panel(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    input: ClosePluginPanelInput,
) -> Result<(), CommandError> {
    require_main_label(webview.label())?;
    let session_epoch = parse_panel_session_epoch(&input.session_epoch)?;
    plugin_panel::teardown(&app, controller.inner().as_ref(), Some(session_epoch));
    Ok(())
}

fn parse_panel_session_epoch(value: &str) -> Result<u64, CommandError> {
    parse_canonical_nonzero_u64(value).ok_or_else(CommandError::plugin_query_failed)
}

fn map_panel_bounds_error(error: PanelBoundsError) -> CommandError {
    match error {
        PanelBoundsError::InvalidBounds => CommandError::plugin_query_failed(),
        PanelBoundsError::Stale => CommandError::stale_request(),
        PanelBoundsError::Unavailable => CommandError::window_failed(),
    }
}

fn panel_route_matches_identity(
    route: &PublicPluginRoute,
    identity: &PanelSessionIdentity,
) -> bool {
    route.output_mode == PublicOutputMode::Panel
        && route.plugin_id == identity.plugin_id
        && route.generation == identity.generation
        && route.activation_id == identity.activation_id
        && route.admission_epoch == identity.admission_epoch
        && route.command_label == identity.command_label
        && route.panel_entry.is_some()
        && route.host_keys == identity.host_keys
        && route.host_key_focus == identity.host_key_focus
}

fn panel_command_result(
    identity: &PanelSessionIdentity,
    input_placeholder: Option<&str>,
) -> PluginPanelCommandResult {
    PluginPanelCommandResult {
        session_epoch: identity.session_epoch.to_string(),
        plugin_id: identity.plugin_id.clone(),
        command_label: identity.command_label.clone(),
        input_placeholder: input_placeholder.map(str::to_owned),
        host_keys: identity.host_keys.clone(),
    }
}

fn rollback_panel_submission(
    app: &AppHandle,
    public: &Arc<PublicPluginService>,
    controller: &PluginPanelController,
    identity: &PanelSessionIdentity,
    request_id: &str,
    submission_token: &str,
) -> bool {
    let Some(removed) =
        controller.teardown_current(identity.session_epoch, request_id, submission_token)
    else {
        return false;
    };
    plugin_panel::destroy_content(app, &removed.content_label);
    let mut next = public.abort_submission_request(
        &PluginRequestContext {
            plugin_id: identity.plugin_id.clone(),
            plugin_generation: identity.generation,
            request_id: request_id.to_owned(),
        },
        submission_token,
        Instant::now(),
    );
    let settings = app.state::<SettingsStore>();
    while let Ok(Some(request)) = next {
        if public
            .dispatch(
                app,
                &request,
                invocation_theme(app, settings.snapshot().theme),
                invoked_at_rfc3339(),
            )
            .is_ok()
        {
            break;
        }
        next = public.abort_submission_request(
            &request.context,
            &request
                .candidate
                .owner
                .command()
                .expect("promoted waiter is a command")
                .submission_token,
            Instant::now(),
        );
    }
    true
}

async fn run_panel_submission(
    app: AppHandle,
    public: Arc<PublicPluginService>,
    controller: Arc<PluginPanelController>,
    identity: PanelSessionIdentity,
    prepared: PreparedPublicSubmission,
    pending: PendingDispatch,
    queue_outcome: QueueDispatchOutcome,
) -> Result<(), CommandError> {
    let submission_token = prepared.token.clone();
    let admission_controller = Arc::clone(&controller);
    let admission_public = Arc::clone(&public);
    let admission_pending = pending.clone();
    let admission_token = submission_token.clone();
    let session_epoch = identity.session_epoch;
    let admission_task = tauri::async_runtime::spawn_blocking(move || {
        let mut prepared = Some(prepared);
        let result = match queue_outcome {
            QueueDispatchOutcome::Ready => admission_controller
                .admit_current_dispatch(
                    session_epoch,
                    &admission_pending.request_id,
                    &admission_pending.submission_token,
                    || {
                        admission_public.admit_prepared_command(
                            prepared.take().expect("panel preparation consumed once"),
                            Instant::now(),
                        )
                    },
                )
                .map(|submission| (admission_pending, submission)),
            QueueDispatchOutcome::Buffered => admission_controller
                .wait_until_dispatchable_and_admit(
                    session_epoch,
                    &admission_pending.request_id,
                    &admission_pending.submission_token,
                    || {
                        admission_public.admit_prepared_command(
                            prepared.take().expect("panel preparation consumed once"),
                            Instant::now(),
                        )
                    },
                ),
        };
        if matches!(
            &result,
            Err(PanelAdmissionError::Stale | PanelAdmissionError::Unavailable)
        ) {
            admission_public.fail_submission(&admission_token);
        }
        result
    });
    let admission = match admission_task.await {
        Ok(admission) => admission,
        Err(_) => {
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Err(CommandError::plugin_query_failed());
            }
            return Ok(());
        }
    };
    let (pending, submission) = match admission {
        Ok(value) => value,
        Err(PanelAdmissionError::Stale) => return Ok(()),
        Err(PanelAdmissionError::Unavailable) => {
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Err(CommandError::plugin_query_failed());
            }
            return Ok(());
        }
        Err(PanelAdmissionError::Operation(error)) => {
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Err(error.into());
            }
            return Ok(());
        }
    };
    let dispatch_result = if let Some(recovery) = submission.recovery.clone() {
        let service = Arc::clone(&public);
        let app_handle = app.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            service.recover_runtime(&app_handle, &recovery, Instant::now())
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Err(PublicPluginManagementError::Unavailable),
        }
    } else {
        Ok(submission.dispatch.clone())
    };
    let dispatch = match dispatch_result {
        Ok(dispatch) => dispatch,
        Err(error) => {
            public.fail_submission(&submission.token);
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Err(error.into());
            }
            return Ok(());
        }
    };
    if let Some(dispatch) = dispatch.as_ref() {
        if let Err(error) =
            public.dispatch(&app, dispatch, pending.theme, pending.invoked_at.clone())
        {
            public.fail_submission(&submission.token);
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Err(error.into());
            }
            return Ok(());
        }
    }

    let receiver = submission.receiver;
    let received = match tauri::async_runtime::spawn_blocking(move || receiver.recv()).await {
        Ok(Ok(received)) => received,
        Ok(Err(_)) | Err(_) => {
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Err(CommandError::plugin_query_failed());
            }
            return Ok(());
        }
    };
    let Some(response) = received else {
        return Ok(());
    };
    let response = match response {
        Ok(PublicPluginResponse::Panel(response)) => response,
        Ok(
            PublicPluginResponse::MainResults(_)
            | PublicPluginResponse::ServiceMainResults { .. }
            | PublicPluginResponse::MainActionResults { .. }
            | PublicPluginResponse::Window(_),
        )
        | Err(_) => {
            if !rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &pending.request_id,
                &submission_token,
            ) {
                return Ok(());
            }
            return Err(CommandError::plugin_query_failed());
        }
    };
    let settlement_request_id = response.request_id.clone();
    let update = PluginPanelUpdate {
        request_id: response.request_id,
        input: pending.argument,
        platform: "windows",
        theme: pending.theme,
        invoked_at: pending.invoked_at,
        session_epoch: identity.session_epoch.to_string(),
        data: response.data,
    };
    let delivery_app = app.clone();
    let delivery_controller = Arc::clone(&controller);
    let delivery_identity = identity.clone();
    let delivery_token = submission_token.clone();
    let delivery = tauri::async_runtime::spawn_blocking(move || {
        plugin_panel::deliver_panel_update(
            &delivery_app,
            &delivery_controller,
            &delivery_identity,
            &delivery_token,
            update,
        )
    })
    .await;
    match delivery {
        Err(_) => {
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &settlement_request_id,
                &submission_token,
            ) {
                Err(CommandError::plugin_query_failed())
            } else {
                Ok(())
            }
        }
        Ok(Ok(())) => Ok(()),
        Ok(Err(PanelSettlementError::Stale)) => Ok(()),
        Ok(Err(PanelSettlementError::Unavailable)) => {
            if rollback_panel_submission(
                &app,
                &public,
                &controller,
                &identity,
                &settlement_request_id,
                &submission_token,
            ) {
                Err(CommandError::plugin_query_failed())
            } else {
                Ok(())
            }
        }
    }
}

fn spawn_panel_submission(
    app: AppHandle,
    public: Arc<PublicPluginService>,
    controller: Arc<PluginPanelController>,
    identity: PanelSessionIdentity,
    prepared: PreparedPublicSubmission,
    pending: PendingDispatch,
    queue_outcome: QueueDispatchOutcome,
) {
    let event_app = app.clone();
    let session_epoch = identity.session_epoch.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_panel_submission(
            app,
            public,
            controller,
            identity,
            prepared,
            pending,
            queue_outcome,
        )
        .await
        {
            eprintln!("[plugin-panel-submit] session={session_epoch} error={error:?}");
            let _ = event_app.emit_to(
                "main",
                "uipilot-plugin-panel-error",
                PluginPanelErrorEvent { session_epoch },
            );
        }
    });
}

fn stable_command_argument(query: &str, preserve_selection: bool) -> &str {
    let argument = query.split_once(' ').map_or("", |(_, argument)| argument);
    if preserve_selection {
        argument
    } else {
        argument.trim()
    }
}

async fn open_plugin_panel_impl(
    app: AppHandle,
    public: State<'_, Arc<PublicPluginService>>,
    controller: State<'_, Arc<PluginPanelController>>,
    input: OpenPluginPanelInput,
) -> Result<Option<PluginPanelCommandResult>, CommandError> {
    let route = if let Some(target) = &input.stable_command_target {
        if target.plugin_id != input.plugin_id
            || !app
                .state::<Arc<LifecycleCoordinator>>()
                .command_entry_current(&app, target)
        {
            return Err(CommandError::plugin_query_failed());
        }
        public
            .manager()?
            .route_by_id(&target.plugin_id, &target.command_id, &input.argument)?
            .filter(|route| {
                route.output_mode == PublicOutputMode::Panel
                    && (!route.input_required || !route.input.is_empty())
            })
    } else if let Some(command_id) = &input.command_id {
        public
            .manager()?
            .route_by_id(&input.plugin_id, command_id, &input.argument)?
            .filter(|route| {
                route.output_mode == PublicOutputMode::Panel
                    && (!route.input_required || !route.input.is_empty())
            })
    } else {
        public
            .manager()?
            .panel_route(&input.plugin_id, &input.argument)?
    }
    .ok_or_else(CommandError::plugin_query_failed)?;
    let panel_entry = route
        .panel_entry
        .clone()
        .ok_or_else(CommandError::plugin_query_failed)?;
    let owner = PanelOwner {
        plugin_id: route.plugin_id.clone(),
        plugin_generation: route.generation,
        activation_id: route.activation_id,
        admission_epoch: route.admission_epoch,
        command_label: route.command_label.clone(),
        request_id: String::new(),
        submission_token: String::new(),
        argument: input.argument.clone(),
        host_keys: route.host_keys.clone(),
        host_key_focus: route.host_key_focus,
    };
    let mount_app = app.clone();
    let mount_controller = Arc::clone(controller.inner());
    let identity = tauri::async_runtime::spawn_blocking(move || {
        plugin_panel::mount(&mount_app, mount_controller, owner, &panel_entry)
    })
    .await
    .map_err(|_| CommandError::plugin_query_failed())??;
    if !focus_main_window_content(&app, Arc::clone(controller.inner()), identity.session_epoch)
        .await
    {
        plugin_panel::teardown(
            &app,
            controller.inner().as_ref(),
            Some(identity.session_epoch),
        );
        return Err(CommandError::window_failed());
    }
    Ok(Some(panel_command_result(
        &identity,
        route.input_placeholder.as_deref(),
    )))
}

async fn submit_plugin_panel_impl(
    app: AppHandle,
    public: State<'_, Arc<PublicPluginService>>,
    controller: State<'_, Arc<PluginPanelController>>,
    settings: State<'_, SettingsStore>,
    input: SubmitPluginPanelInput,
) -> Result<Option<PluginPanelCommandResult>, CommandError> {
    let session_epoch = parse_panel_session_epoch(&input.session_epoch)?;
    let identity = controller
        .live_identity()
        .filter(|identity| identity.session_epoch == session_epoch)
        .ok_or_else(CommandError::plugin_query_failed)?;
    let route = public
        .manager()?
        .panel_route(&identity.plugin_id, &input.argument)?
        .filter(|route| panel_route_matches_identity(route, &identity))
        .ok_or_else(CommandError::plugin_query_failed)?;
    let input_placeholder = route.input_placeholder.clone();
    let prepared = public.prepare_command(route, input.ui_intent_epoch, input.argument.clone())?;
    let pending = PendingDispatch {
        request_id: prepared.request_context().request_id.clone(),
        submission_token: prepared.token.clone(),
        argument: input.argument,
        theme: invocation_theme(&app, settings.snapshot().theme),
        invoked_at: invoked_at_rfc3339(),
    };
    let outcome = match plugin_panel::queue_dispatch(&controller, session_epoch, pending.clone()) {
        Ok(outcome) => outcome,
        Err(error) => {
            public.fail_submission(&prepared.token);
            return Err(error.into());
        }
    };
    let result = panel_command_result(&identity, input_placeholder.as_deref());
    spawn_panel_submission(
        app,
        Arc::clone(public.inner()),
        Arc::clone(controller.inner()),
        identity,
        prepared,
        pending,
        outcome,
    );
    Ok(Some(result))
}
#[tauri::command]
pub(crate) async fn search_apps(
    window: WebviewWindow,
    query: String,
    invocation_id: String,
    query_sequence: u64,
    submit: Option<bool>,
    completion_origin: Option<CompletionOriginInput>,
    stable_command_target: Option<crate::command_hotkeys::CommandEntry>,
    stable_builtin_target: Option<ManagedBuiltinFeature>,
) -> Result<Option<SearchResponse>, CommandError> {
    require_main_window(&window)?;
    let submit = submit.unwrap_or(false);
    let app = window.app_handle();
    let registries = app.state::<ResultRegistries>();
    let registry = registries.main();
    let public_cleanup = app.state::<Arc<PublicPluginService>>();
    let _result_action_cleanup = ResultActionCleanup {
        service: public_cleanup.inner(),
        registry,
    };
    let normalized_query = query.trim().to_owned();
    if stable_command_target.is_none()
        && completion_origin.is_none()
        && !normalized_query.starts_with('/')
    {
        match resolve_currency_search(
            registry,
            &normalized_query,
            &invocation_id,
            query_sequence,
            |intent| {
                let exchange_rate_service = Arc::clone(
                    app.state::<Arc<
                        crate::quick_evaluation::currency::service::ExchangeRateService,
                    >>()
                    .inner(),
                );
                async move { exchange_rate_service.resolve(intent).await }
            },
        )
        .await
        {
            CurrencySearchResponse::Continue => {}
            CurrencySearchResponse::Published(response) => return Ok(response),
        }
    }
    let cache = app.state::<Arc<AppCache>>();
    let settings = app.state::<SettingsStore>();
    let public = app.state::<Arc<PublicPluginService>>();
    let quicklinks = app.state::<Arc<QuicklinksStore>>();
    let settings_snapshot = settings.snapshot();
    let web_search_engine = settings_snapshot.web_search_engine;
    let quicklink_items = quicklinks.list().items;
    if let Some(feature) = stable_builtin_target {
        if feature != ManagedBuiltinFeature::WebSearch
            || !settings_snapshot.builtin_feature_enabled(feature)
        {
            return Err(CommandError::settings_failed());
        }
        let alias = settings_snapshot.builtin_command(feature);
        let rest = normalized_query
            .strip_prefix(&alias)
            .ok_or_else(CommandError::settings_failed)?;
        if !rest.is_empty() && !rest.starts_with(' ') {
            return Err(CommandError::settings_failed());
        }
        let synthetic = format!("/web-search{}", rest);
        return Ok(search_apps_with_catalog(
            registry,
            CatalogSearchRequest {
                query: &synthetic,
                invocation_id: &invocation_id,
                query_sequence,
                submit,
                web_search_engine,
                favorite_builtin_features: &settings_snapshot.favorite_builtin_features,
                disabled_builtin_features: &settings_snapshot.disabled_builtin_features,
                quicklinks: &quicklink_items,
            },
            |_| Ok(Vec::new()),
            Vec::new,
            |_| {},
        ));
    }
    let mut matched_public_identity = None;
    if stable_command_target.is_none()
        && completion_origin.is_none()
        && normalized_query.starts_with('/')
        && normalized_query != "/"
    {
        let body = &normalized_query[1..];
        let (name, argument) = split_launcher_alias(body);
        let (public_matches, builtin_matches) = exact_launch_aliases(
            &settings_snapshot,
            name,
            public.manager()?.command_suggestions(name)?,
        );
        if public_matches.len() + builtin_matches.len() > 1 {
            let token =
                registry.begin_query(QueryDomain::Application, &invocation_id, query_sequence);
            return Ok(token.and_then(|token| {
                let mut entries: Vec<_> = public_matches
                    .into_iter()
                    .filter_map(|suggestion| {
                        public_plugin_completion_result(suggestion, Some(argument))
                    })
                    .collect();
                for feature in builtin_matches {
                    let favorite = settings_snapshot
                        .favorite_builtin_features
                        .contains(&feature.favorite_feature());
                    let mut entry = match feature {
                        ManagedBuiltinFeature::Screenshot => screenshot_result(favorite),
                        ManagedBuiltinFeature::Quicklinks => quicklinks_result(favorite),
                        ManagedBuiltinFeature::WebSearch if !argument.is_empty() => {
                            web_search_result(web_search_engine, argument.to_owned(), favorite)
                        }
                        ManagedBuiltinFeature::WebSearch => {
                            let mut entry = builtin_completion_result(
                                name,
                                "网页搜索".into(),
                                ResultIconKind::WebSearch,
                                BuiltinFeature::WebSearch,
                                favorite,
                            )?;
                            entry.0.activation = LauncherResultActivation::BuiltinActivation {
                                feature,
                                initial_argument: String::new(),
                            };
                            entry
                        }
                    };
                    entry.0.title = format!("/{name}");
                    entries.push(entry);
                }
                registry.publish_if_latest(
                    token,
                    entries,
                    || true,
                    |request_id, items| {
                        let mut response = search_response(request_id, items);
                        response.replace_local_results = true;
                        response
                    },
                )
            }));
        }
        if !body.contains(' ')
            && builtin_matches.is_empty()
            && quicklink_query(&normalized_query, &quicklink_items).is_none()
        {
            let token =
                registry.begin_query(QueryDomain::Application, &invocation_id, query_sequence);
            return Ok(token.and_then(|token| {
                let mut entries: Vec<_> = public
                    .manager()
                    .ok()?
                    .command_suggestions(name)
                    .ok()?
                    .into_iter()
                    .filter_map(|suggestion| public_plugin_completion_result(suggestion, None))
                    .collect();
                if "find".starts_with(name) {
                    entries.push(find_result(
                        String::new(),
                        "搜索文件".into(),
                        settings_snapshot
                            .favorite_builtin_features
                            .contains(&BuiltinFeature::Find),
                    ));
                }
                for feature in ManagedBuiltinFeature::ALL {
                    let command = settings_snapshot.builtin_command(feature);
                    if !settings_snapshot.builtin_feature_enabled(feature)
                        || !command[1..].starts_with(name)
                    {
                        continue;
                    }
                    let favorite = settings_snapshot
                        .favorite_builtin_features
                        .contains(&feature.favorite_feature());
                    let mut entry = match feature {
                        ManagedBuiltinFeature::Screenshot => screenshot_result(favorite),
                        ManagedBuiltinFeature::Quicklinks => quicklinks_result(favorite),
                        ManagedBuiltinFeature::WebSearch => {
                            let mut entry = builtin_completion_result(
                                &command[1..],
                                "网页搜索".into(),
                                ResultIconKind::WebSearch,
                                BuiltinFeature::WebSearch,
                                favorite,
                            )?;
                            entry.0.activation = LauncherResultActivation::BuiltinActivation {
                                feature,
                                initial_argument: String::new(),
                            };
                            entry
                        }
                    };
                    entry.0.title = command;
                    entries.push(entry);
                }
                registry.publish_if_latest(
                    token,
                    entries,
                    || true,
                    |request_id, items| {
                        let mut response = search_response(request_id, items);
                        response.replace_local_results = true;
                        response
                    },
                )
            }));
        }
        if public_matches.is_empty()
            && builtin_matches.is_empty()
            && ManagedBuiltinFeature::ALL
                .iter()
                .any(|feature| feature.default_name() == name)
            && quicklink_query(&normalized_query, &quicklink_items).is_none()
        {
            let token =
                registry.begin_query(QueryDomain::Application, &invocation_id, query_sequence);
            return Ok(token.and_then(|token| {
                registry.publish_if_latest(
                    token,
                    Vec::<(crate::model::ResultItem, Option<ResultAction>)>::new(),
                    || true,
                    search_response,
                )
            }));
        }
        matched_public_identity = sole_public_match(&public_matches, argument);
        if public_matches.is_empty() && builtin_matches.len() == 1 {
            let feature = builtin_matches[0];
            let synthetic = format!(
                "/{}{}",
                feature.default_name(),
                if argument.is_empty() {
                    String::new()
                } else {
                    format!(" {argument}")
                }
            );
            let mut response = search_apps_with_catalog(
                registry,
                CatalogSearchRequest {
                    query: &synthetic,
                    invocation_id: &invocation_id,
                    query_sequence,
                    submit,
                    web_search_engine,
                    favorite_builtin_features: &settings_snapshot.favorite_builtin_features,
                    disabled_builtin_features: &settings_snapshot.disabled_builtin_features,
                    quicklinks: &quicklink_items,
                },
                |_| Ok(Vec::new()),
                Vec::new,
                |_| {},
            );
            if let Some(response) = &mut response {
                for item in &mut response.items {
                    if argument.is_empty() {
                        item.title = settings_snapshot.builtin_command(feature);
                        if feature == ManagedBuiltinFeature::WebSearch {
                            item.activation = LauncherResultActivation::BuiltinActivation {
                                feature,
                                initial_argument: String::new(),
                            };
                        }
                    }
                }
            }
            return Ok(response);
        }
    }
    if stable_command_target.is_none()
        && completion_origin.is_none()
        && (!normalized_query.starts_with('/') || normalized_query == "/")
    {
        let mut response = search_apps_with_catalog(
            registry,
            CatalogSearchRequest {
                query: &normalized_query,
                invocation_id: &invocation_id,
                query_sequence,
                submit,
                web_search_engine,
                favorite_builtin_features: &settings_snapshot.favorite_builtin_features,
                disabled_builtin_features: &settings_snapshot.disabled_builtin_features,
                quicklinks: &quicklink_items,
            },
            |plain_query| public.manager()?.launcher_command_suggestions(plain_query),
            || cache.snapshot(),
            |applications| settings.decorate_applications(applications),
        );
        if let Some(response) = &mut response {
            for item in &mut response.items {
                let feature = match item.icon_kind {
                    Some(ResultIconKind::Screenshot) => Some(ManagedBuiltinFeature::Screenshot),
                    Some(ResultIconKind::Quicklinks) => Some(ManagedBuiltinFeature::Quicklinks),
                    Some(ResultIconKind::WebSearch)
                        if matches!(
                            item.activation,
                            LauncherResultActivation::Completion { .. }
                        ) =>
                    {
                        Some(ManagedBuiltinFeature::WebSearch)
                    }
                    _ => None,
                };
                if let Some(feature) = feature {
                    item.title = settings_snapshot.builtin_command(feature);
                    if feature == ManagedBuiltinFeature::WebSearch {
                        item.activation = LauncherResultActivation::BuiltinActivation {
                            feature,
                            initial_argument: String::new(),
                        };
                    }
                }
            }
        }
        return Ok(response);
    }
    if stable_command_target.is_none()
        && completion_origin.is_none()
        && reserved_catalog_query(&normalized_query, &quicklink_items)
    {
        return Ok(search_apps_with_catalog(
            registry,
            CatalogSearchRequest {
                query: &normalized_query,
                invocation_id: &invocation_id,
                query_sequence,
                submit,
                web_search_engine,
                favorite_builtin_features: &settings_snapshot.favorite_builtin_features,
                disabled_builtin_features: &settings_snapshot.disabled_builtin_features,
                quicklinks: &quicklink_items,
            },
            |_| Ok(Vec::new()),
            Vec::new,
            |_| {},
        ));
    }
    if stable_command_target.is_none()
        && completion_origin.is_none()
        && plugin_discovery_prefix(&normalized_query).is_some()
    {
        return Ok(search_apps_with_catalog(
            registry,
            CatalogSearchRequest {
                query: &normalized_query,
                invocation_id: &invocation_id,
                query_sequence,
                submit,
                web_search_engine,
                favorite_builtin_features: &settings_snapshot.favorite_builtin_features,
                disabled_builtin_features: &settings_snapshot.disabled_builtin_features,
                quicklinks: &quicklink_items,
            },
            |prefix| public.manager()?.command_suggestions(prefix),
            Vec::new,
            |_| {},
        ));
    }
    let route_query = completion_origin
        .as_ref()
        .map_or(normalized_query.as_str(), |_| query.as_str());
    let route = if let Some(target) = &stable_command_target {
        if !app
            .state::<Arc<LifecycleCoordinator>>()
            .command_entry_current(app, target)
        {
            return Err(CommandError::plugin_query_failed());
        }
        public
            .manager()?
            .route_by_id(
                &target.plugin_id,
                &target.command_id,
                stable_command_argument(
                    &query,
                    app.state::<Arc<LifecycleCoordinator>>()
                        .selection_entry_current(app, target),
                ),
            )?
            .ok_or_else(CommandError::plugin_query_failed)
            .map(Some)?
    } else if let Some(origin) = &completion_origin {
        if let Some(command_id) = &origin.command_id {
            let argument = route_query
                .split_once(' ')
                .map_or("", |(_, value)| value.trim());
            public
                .manager()?
                .route_by_id(&origin.plugin_id, command_id, argument)?
        } else {
            public.manager()?.route(route_query)?
        }
    } else if let Some((plugin_id, command_id, argument)) = &matched_public_identity {
        Some(
            public
                .manager()?
                .route_by_id(plugin_id, command_id, argument)?
                .ok_or_else(CommandError::plugin_query_failed)?,
        )
    } else {
        public.manager()?.route(route_query)?
    };
    if let Some(route) = route {
        // Hotkey entry prompts before the ordinary Window completion exception.
        if stable_command_target.is_some() && route.input_required && route.input.is_empty() {
            return Ok(public_plugin_prompt(
                registry,
                &invocation_id,
                query_sequence,
                Some(
                    route
                        .input_placeholder
                        .clone()
                        .unwrap_or_else(|| "请输入内容".into()),
                ),
                submitted_main_result_command(&route, submit),
            ));
        }
        match public_plugin_search_decision(&route, submit, completion_origin.as_ref())
            .map_err(|_| CommandError::plugin_query_failed())?
        {
            PublicPluginSearchDecision::Ignore => return Ok(None),
            PublicPluginSearchDecision::Hint(hint) => {
                return Ok(public_plugin_prompt(
                    registry,
                    &invocation_id,
                    query_sequence,
                    Some(hint),
                    submitted_main_result_command(&route, submit),
                ))
            }
            PublicPluginSearchDecision::Dispatch => {}
        }
        if route.output_mode == PublicOutputMode::Panel {
            return Err(CommandError::plugin_query_failed());
        }
        let Some(registry_token) =
            registry.begin_query(QueryDomain::Plugin, &invocation_id, query_sequence)
        else {
            return Ok(None);
        };
        public.cleanup_retired_result_actions(registry);
        let theme = invocation_theme(app, settings.snapshot().theme);
        let invoked_at = invoked_at_rfc3339();
        let submission = public.schedule_command(
            route.clone(),
            query_sequence,
            query.clone(),
            Instant::now(),
        )?;
        let submission_token = submission.token.clone();
        let dispatch = if let Some(recovery) = submission.recovery.clone() {
            let service = Arc::clone(public.inner());
            let app_handle = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                service.recover_runtime(&app_handle, &recovery, Instant::now())
            })
            .await
            .map_err(|_| CommandError::plugin_query_failed())??
        } else {
            submission.dispatch.clone()
        };
        if let Some(dispatch) = dispatch.as_ref() {
            if let Err(error) = public.dispatch(app, dispatch, theme, invoked_at.clone()) {
                public.fail_submission(&submission.token);
                return Err(error.into());
            }
        }
        let receiver = submission.receiver;
        let received = tauri::async_runtime::spawn_blocking(move || receiver.recv())
            .await
            .map_err(|_| CommandError::plugin_query_failed())?
            .map_err(|_| CommandError::plugin_query_failed())?;
        let Some(response) = received else {
            return Ok(None);
        };
        return match response.map_err(CommandError::from)? {
            PublicPluginResponse::MainActionResults { route, results } => {
                let requirements =
                    crate::public_plugins::result_action::ResultActionRequirements::rows(&results);
                let prepared = registry
                    .prepare_result_actions(route.clone(), results)
                    .ok_or_else(CommandError::plugin_query_failed)?;
                let plugins = app.state::<Arc<PluginManager>>();
                let published = plugins
                    .with_public_result_admission(|| {
                        public
                            .manager()
                            .map_err(|_| CommandError::plugin_query_failed())?
                            .publish_result_actions(&route, requirements, || {
                                registry.publish_prepared_result_actions(registry_token, prepared)
                            })
                            .map_err(|_| CommandError::plugin_query_failed())
                    })
                    .ok_or_else(CommandError::plugin_query_failed)??;
                public.cleanup_retired_result_actions(registry);
                Ok(published)
            }
            PublicPluginResponse::ServiceMainResults {
                results,
                credential_revision,
                publication,
            } => {
                if route.output_mode != PublicOutputMode::MainResult {
                    return Err(CommandError::plugin_query_failed());
                }
                app.state::<crate::credentials::CredentialStore>()
                    .with_revision(credential_revision, || {
                        public
                            .manager()
                            .map_err(|_| "expiredRequest")?
                            .scheduler()
                            .with_result_publication(&route.plugin_id, &publication, || {
                                publish_public_main_results(
                                    registry,
                                    registry_token,
                                    &route,
                                    results,
                                    submitted_main_result_command(&route, submit),
                                )
                            })
                            .map_err(|_| "expiredRequest")
                    })
                    .map_err(|_| CommandError::plugin_query_failed())
            }
            PublicPluginResponse::MainResults(results) => {
                if route.output_mode != PublicOutputMode::MainResult {
                    return Err(CommandError::plugin_query_failed());
                }
                Ok(publish_public_main_results(
                    registry,
                    registry_token,
                    &route,
                    results,
                    submitted_main_result_command(&route, submit),
                ))
            }
            PublicPluginResponse::Window(response) => {
                if route.output_mode != PublicOutputMode::Window {
                    return Err(CommandError::plugin_query_failed());
                }
                let window_entry = route
                    .window_entry
                    .clone()
                    .ok_or_else(CommandError::plugin_query_failed)?;
                let controller = Arc::clone(app.state::<Arc<PluginWindowController>>().inner());
                let owner = PluginWindowOwner {
                    ui_intent_epoch: query_sequence,
                    submission_token,
                    plugin_id: route.plugin_id.clone(),
                    plugin_generation: route.generation,
                    activation_id: route.activation_id,
                    admission_epoch: route.admission_epoch,
                    request_id: response.request_id.clone(),
                    control_value: query.clone(),
                };
                let update = PluginWindowUpdate {
                    request_id: response.request_id.clone(),
                    input: route.input.clone(),
                    platform: "windows",
                    theme,
                    invoked_at,
                    instance_number: 1,
                    data: response.data,
                };
                let coordinator = app.state::<Arc<LifecycleCoordinator>>();
                let _focus = coordinator
                    .suppress_transient_focus_loss()
                    .map_err(|_| CommandError::plugin_query_failed())?;
                let app_handle = app.clone();
                let prepared = tauri::async_runtime::spawn_blocking(move || {
                    plugin_window::prepare(&app_handle, controller, owner, update, &window_entry)
                })
                .await
                .map_err(|_| CommandError::plugin_query_failed())??;
                Ok(Some(SearchResponse {
                    request_id: response.request_id,
                    items: Vec::new(),
                    command_hint: None,
                    main_result_command: None,
                    window_transfer_token: Some(prepared.transfer_token),
                    auto_execute_result_id: None,
                    replace_local_results: false,
                    refresh_at_utc: None,
                }))
            }
            PublicPluginResponse::Panel(_) => {
                if route.output_mode != PublicOutputMode::Panel {
                    return Err(CommandError::plugin_query_failed());
                }
                // Panel session open/submit settlement lands in Task 4.
                Ok(None)
            }
        };
    }
    if completion_origin.is_some() {
        return Err(CommandError::plugin_query_failed());
    }
    let plugins = app.state::<Arc<PluginManager>>();
    match plugins.begin_routed_query(&normalized_query, registry, &invocation_id, query_sequence) {
        PluginQueryStart::Started { route, token } => {
            let entries = match plugins.query(window.app_handle(), route.clone()).await {
                Ok(entries) => entries,
                Err(PluginQueryError::Timeout) => Vec::new(),
                Err(_) => return Err(CommandError::plugin_query_failed()),
            };
            return Ok(plugins.publish_results(registry, token, &route, entries));
        }
        PluginQueryStart::Rejected => return Ok(None),
        PluginQueryStart::NoRoute => {}
    }
    Ok(search_apps_with_catalog(
        registry,
        CatalogSearchRequest {
            query: &normalized_query,
            invocation_id: &invocation_id,
            query_sequence,
            submit,
            web_search_engine,
            favorite_builtin_features: &settings_snapshot.favorite_builtin_features,
            disabled_builtin_features: &settings_snapshot.disabled_builtin_features,
            quicklinks: &quicklink_items,
        },
        |_| Ok(Vec::new()),
        || cache.snapshot(),
        |applications| settings.decorate_applications(applications),
    ))
}

pub(crate) fn sole_public_match(
    matches: &[crate::public_plugins::PublicCommandSuggestion],
    argument: &str,
) -> Option<(String, String, String)> {
    let [matched] = matches else {
        return None;
    };
    Some((
        matched.plugin_id.clone(),
        matched.command_id.clone(),
        argument.to_owned(),
    ))
}

fn split_launcher_alias(body: &str) -> (&str, &str) {
    body.split_once(' ')
        .map_or((body, ""), |(name, arg)| (name, arg.trim()))
}

fn exact_launch_aliases(
    settings: &Settings,
    name: &str,
    suggestions: Vec<crate::public_plugins::PublicCommandSuggestion>,
) -> (
    Vec<crate::public_plugins::PublicCommandSuggestion>,
    Vec<ManagedBuiltinFeature>,
) {
    let public = suggestions
        .into_iter()
        .filter(|item| item.effective_name == name)
        .collect();
    let builtins = ManagedBuiltinFeature::ALL
        .into_iter()
        .filter(|feature| {
            settings.builtin_feature_enabled(*feature)
                && settings.builtin_command(*feature) == format!("/{name}")
        })
        .collect();
    (public, builtins)
}

fn reserved_catalog_query(query: &str, quicklinks: &[QuicklinkView]) -> bool {
    query == "/find" || query.starts_with("/find ") || quicklink_query(query, quicklinks).is_some()
}

fn plugin_discovery_prefix(query: &str) -> Option<&str> {
    let command = query.strip_prefix('/')?;
    (!command.is_empty() && !command.contains(' ')).then_some(command)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PublicPluginSearchDecision {
    Ignore,
    Hint(String),
    Dispatch,
}

fn submitted_main_result_command(
    route: &PublicPluginRoute,
    submit: bool,
) -> Option<MainResultCommandContext> {
    (submit && route.output_mode == PublicOutputMode::MainResult).then(|| {
        MainResultCommandContext {
            command_id: route.command_id.clone(),
            plugin_id: route.plugin_id.clone(),
            command_label: route.command_label.clone(),
            argument: route.input.clone(),
        }
    })
}

fn public_plugin_search_decision(
    route: &crate::public_plugins::PublicPluginRoute,
    submit: bool,
    completion_origin: Option<&CompletionOriginInput>,
) -> Result<PublicPluginSearchDecision, ()> {
    if let Some(origin) = completion_origin {
        if !crate::public_plugins::valid_plugin_id(&origin.plugin_id)
            || origin.plugin_id != route.plugin_id
            || origin
                .command_id
                .as_ref()
                .is_some_and(|id| id != &route.command_id)
            || matches!(origin.phase, CompletionOriginPhase::Preview) == submit
        {
            return Err(());
        }
    }
    if matches!(
        (
            completion_origin.map(|origin| origin.phase),
            route.output_mode,
            submit,
        ),
        (
            Some(CompletionOriginPhase::Commit),
            PublicOutputMode::Window,
            true
        )
    ) {
        return Ok(PublicPluginSearchDecision::Dispatch);
    }
    if route.input_required && route.input.is_empty() {
        return Ok(PublicPluginSearchDecision::Hint(
            route
                .input_placeholder
                .clone()
                .unwrap_or_else(|| "请输入内容".into()),
        ));
    }
    if let Some(origin) = completion_origin {
        return Ok(match origin.phase {
            CompletionOriginPhase::Preview => route.input_placeholder.clone().map_or(
                PublicPluginSearchDecision::Ignore,
                PublicPluginSearchDecision::Hint,
            ),
            CompletionOriginPhase::Commit => PublicPluginSearchDecision::Dispatch,
        });
    }
    Ok(match (route.activation_mode, submit) {
        (PublicActivationMode::Submit, true) | (PublicActivationMode::Live, false) => {
            PublicPluginSearchDecision::Dispatch
        }
        (PublicActivationMode::Submit, false) => route.input_placeholder.clone().map_or(
            PublicPluginSearchDecision::Ignore,
            PublicPluginSearchDecision::Hint,
        ),
        (PublicActivationMode::Live, true) => PublicPluginSearchDecision::Ignore,
    })
}

pub(crate) fn invocation_theme(
    app: &AppHandle,
    preference: ThemePreference,
) -> PluginInvocationTheme {
    let system_theme = app
        .get_window("main")
        .and_then(|window| window.theme().ok());
    resolve_invocation_theme(preference, system_theme)
}

fn resolve_invocation_theme(
    preference: ThemePreference,
    system_theme: Option<tauri::Theme>,
) -> PluginInvocationTheme {
    match preference {
        ThemePreference::Dark => PluginInvocationTheme::Dark,
        ThemePreference::Light => PluginInvocationTheme::Light,
        ThemePreference::System if matches!(system_theme, Some(tauri::Theme::Dark)) => {
            PluginInvocationTheme::Dark
        }
        ThemePreference::System => PluginInvocationTheme::Light,
    }
}

pub(crate) fn invoked_at_rfc3339() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn public_plugin_prompt(
    registry: &ResultRegistry,
    invocation_id: &str,
    query_sequence: u64,
    placeholder: Option<String>,
    main_result_command: Option<MainResultCommandContext>,
) -> Option<SearchResponse> {
    let token = registry.begin_query(QueryDomain::Plugin, invocation_id, query_sequence)?;
    registry.publish_if_latest(
        token,
        Vec::<((), Option<ResultAction>)>::new(),
        || true,
        |request_id, _| SearchResponse {
            request_id,
            items: Vec::new(),
            command_hint: Some(placeholder.unwrap_or_else(|| "请输入内容".into())),
            main_result_command,
            window_transfer_token: None,
            auto_execute_result_id: None,
            replace_local_results: false,
            refresh_at_utc: None,
        },
    )
}

#[cfg(test)]
fn publish_public_command_suggestions(
    registry: &ResultRegistry,
    invocation_id: &str,
    query_sequence: u64,
    suggestions: Vec<crate::public_plugins::PublicCommandSuggestion>,
) -> Option<SearchResponse> {
    let token = registry.begin_query(QueryDomain::Plugin, invocation_id, query_sequence)?;
    let entries = suggestions
        .into_iter()
        .filter_map(|suggestion| public_plugin_completion_result(suggestion, None))
        .collect();
    registry.publish_if_latest(token, entries, || true, search_response)
}

fn publish_public_main_results(
    registry: &ResultRegistry,
    token: QueryToken,
    route: &crate::public_plugins::PublicPluginRoute,
    results: Vec<PublicMainResult>,
    main_result_command: Option<MainResultCommandContext>,
) -> Option<SearchResponse> {
    let entries = results
        .into_iter()
        .map(|result| {
            let action = result.copy_text.clone().map(|text| ResultAction::CopyText {
                command_id: Some(route.command_id.clone()),
                plugin_id: route.plugin_id.clone(),
                generation: route.generation,
                text,
            });
            (
                crate::model::ResultItem {
                    result_actions: None,
                    result_id: String::new(),
                    activation: LauncherResultActivation::ExecuteResult,
                    title: result.title,
                    subtitle: result.subtitle,
                    icon: None,
                    plugin_icon_url: route.icon_url.clone(),
                    icon_kind: None,
                    detail: result.detail,
                    favorite: None,
                    has_default_action: action.is_some(),
                },
                action,
            )
        })
        .collect();
    registry.publish_if_latest(
        token,
        entries,
        || true,
        |request_id, items| {
            let mut response = search_response(request_id, items);
            response.main_result_command = main_result_command;
            response
        },
    )
}

fn search_response(
    request_id: String,
    items: Vec<(String, crate::model::ResultItem)>,
) -> SearchResponse {
    SearchResponse {
        request_id,
        items: items
            .into_iter()
            .map(|(result_id, mut item)| {
                item.result_id = result_id;
                item
            })
            .collect(),
        command_hint: None,
        main_result_command: None,
        window_transfer_token: None,
        auto_execute_result_id: None,
        replace_local_results: false,
        refresh_at_utc: None,
    }
}
#[tauri::command]
pub(crate) fn publish_plugin_results(
    window: WebviewWindow,
    plugins: State<'_, Arc<PluginManager>>,
    response: serde_json::Value,
) -> Result<(), CommandError> {
    publish_plugin_results_with_label(window.label(), || {
        plugins.publish_response(window.label(), response)
    })
}

fn publish_plugin_results_with_label<P>(label: &str, publish: P) -> Result<(), CommandError>
where
    P: FnOnce() -> Result<(), PluginQueryError>,
{
    if !label.starts_with("plugin-") {
        return Err(CommandError::invalid_caller());
    }
    publish().map_err(|_| CommandError::plugin_query_failed())
}

#[cfg(test)]
static EMPTY_DISABLED_BUILTINS: BTreeSet<ManagedBuiltinFeature> = BTreeSet::new();

#[derive(Clone, Copy)]
struct CatalogSearchRequest<'a> {
    query: &'a str,
    invocation_id: &'a str,
    query_sequence: u64,
    submit: bool,
    web_search_engine: WebSearchEngine,
    favorite_builtin_features: &'a BTreeSet<BuiltinFeature>,
    disabled_builtin_features: &'a BTreeSet<ManagedBuiltinFeature>,
    quicklinks: &'a [QuicklinkView],
}

#[cfg(test)]
fn search_apps_with<S, D>(
    registry: &ResultRegistry,
    query: &str,
    invocation_id: &str,
    query_sequence: u64,
    snapshot: S,
    decorate: D,
    web_search_engine: WebSearchEngine,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
{
    search_apps_with_local_date(
        registry,
        query,
        invocation_id,
        query_sequence,
        snapshot,
        decorate,
        web_search_engine,
        || time::Date::from_calendar_date(2026, time::Month::September, 3).map_err(|_| ()),
    )
}

#[cfg(test)]
fn search_apps_with_local_date<S, D, L>(
    registry: &ResultRegistry,
    query: &str,
    invocation_id: &str,
    query_sequence: u64,
    snapshot: S,
    decorate: D,
    web_search_engine: WebSearchEngine,
    local_today: L,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
    L: FnOnce() -> Result<time::Date, ()>,
{
    search_apps_with_clocks(
        registry,
        query,
        invocation_id,
        query_sequence,
        snapshot,
        decorate,
        web_search_engine,
        local_today,
        || Ok(time::OffsetDateTime::UNIX_EPOCH),
    )
}

#[cfg(test)]
fn search_apps_with_clocks<S, D, L, U>(
    registry: &ResultRegistry,
    query: &str,
    invocation_id: &str,
    query_sequence: u64,
    snapshot: S,
    decorate: D,
    web_search_engine: WebSearchEngine,
    local_today: L,
    utc_now: U,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
    L: FnOnce() -> Result<time::Date, ()>,
    U: FnOnce() -> Result<time::OffsetDateTime, ()>,
{
    let favorite_builtin_features = BTreeSet::new();
    search_apps_with_catalog_with_clocks(
        registry,
        CatalogSearchRequest {
            query,
            invocation_id,
            query_sequence,
            submit: false,
            web_search_engine,
            favorite_builtin_features: &favorite_builtin_features,
            disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
            quicklinks: &[],
        },
        |_| Ok(Vec::new()),
        snapshot,
        decorate,
        local_today,
        utc_now,
    )
}

fn completion_result(
    command: &str,
    subtitle: String,
    icon_kind: Option<ResultIconKind>,
    plugin_icon_url: Option<String>,
    argument: Option<&str>,
) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    let title = format!("/{command}");
    let completion_text =
        argument.map_or_else(|| format!("{title} "), |value| format!("{title} {value}"));
    Some((
        crate::model::ResultItem {
            result_actions: None,
            result_id: String::new(),
            activation: LauncherResultActivation::completion(completion_text)?,
            title,
            subtitle: Some(subtitle),
            icon: None,
            plugin_icon_url,
            icon_kind,
            detail: None,
            favorite: None,
            has_default_action: false,
        },
        None,
    ))
}

fn builtin_completion_result(
    command: &str,
    subtitle: String,
    icon_kind: ResultIconKind,
    feature: BuiltinFeature,
    favorite: bool,
) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    let (mut item, action) = completion_result(command, subtitle, Some(icon_kind), None, None)?;
    item.favorite = Some(ResultFavorite::builtin(feature, favorite));
    Some((item, action))
}

fn find_result(
    query: String,
    subtitle: String,
    favorite: bool,
) -> (crate::model::ResultItem, Option<ResultAction>) {
    (
        crate::model::ResultItem {
            result_actions: None,
            result_id: String::new(),
            activation: LauncherResultActivation::OpenFind { query },
            title: "/find".into(),
            subtitle: Some(subtitle),
            icon: None,
            plugin_icon_url: None,
            icon_kind: Some(ResultIconKind::Find),
            detail: None,
            favorite: Some(ResultFavorite::builtin(BuiltinFeature::Find, favorite)),
            has_default_action: false,
        },
        None,
    )
}

fn web_search_result(
    engine: WebSearchEngine,
    query: String,
    favorite: bool,
) -> (crate::model::ResultItem, Option<ResultAction>) {
    (
        crate::model::ResultItem {
            result_actions: None,
            result_id: String::new(),
            activation: LauncherResultActivation::ExecuteResult,
            title: crate::web_search::search_result_title(engine).into(),
            subtitle: Some(format!("搜索：{query}")),
            icon: None,
            plugin_icon_url: None,
            icon_kind: Some(ResultIconKind::WebSearch),
            detail: None,
            favorite: Some(ResultFavorite::builtin(BuiltinFeature::WebSearch, favorite)),
            has_default_action: true,
        },
        Some(ResultAction::OpenWebSearch { engine, query }),
    )
}

fn quicklinks_result(favorite: bool) -> (crate::model::ResultItem, Option<ResultAction>) {
    (
        crate::model::ResultItem {
            result_actions: None,
            result_id: String::new(),
            activation: LauncherResultActivation::OpenQuicklinks,
            title: "/quicklinks".into(),
            subtitle: Some("管理快速链接".into()),
            icon: None,
            plugin_icon_url: None,
            icon_kind: Some(ResultIconKind::Quicklinks),
            detail: None,
            favorite: Some(ResultFavorite::builtin(
                BuiltinFeature::Quicklinks,
                favorite,
            )),
            has_default_action: false,
        },
        None,
    )
}

fn screenshot_result(favorite: bool) -> (crate::model::ResultItem, Option<ResultAction>) {
    (
        crate::model::ResultItem {
            result_actions: None,
            result_id: String::new(),
            activation: LauncherResultActivation::OpenScreenshot,
            title: "/screenshot".into(),
            subtitle: Some("截图 · 框选桌面区域或选择窗口".into()),
            icon: None,
            plugin_icon_url: None,
            icon_kind: Some(ResultIconKind::Screenshot),
            detail: None,
            favorite: Some(ResultFavorite::builtin(
                BuiltinFeature::Screenshot,
                favorite,
            )),
            has_default_action: true,
        },
        None,
    )
}

fn builtin_prefix_results(
    prefix: &str,
    find_favorite: bool,
    quicklinks_favorite: bool,
    web_search_favorite: bool,
    screenshot_favorite: bool,
) -> Vec<(crate::model::ResultItem, Option<ResultAction>)> {
    let mut entries = Vec::new();
    if "screenshot".starts_with(prefix) {
        entries.push(screenshot_result(screenshot_favorite));
    }
    if "find".starts_with(prefix) {
        entries.push(find_result(String::new(), "搜索文件".into(), find_favorite));
    }
    if "quicklinks".starts_with(prefix) {
        entries.push(quicklinks_result(quicklinks_favorite));
    }
    if "web-search".starts_with(prefix) {
        entries.extend(builtin_completion_result(
            "web-search",
            "使用默认搜索引擎搜索".into(),
            ResultIconKind::WebSearch,
            BuiltinFeature::WebSearch,
            web_search_favorite,
        ));
    }
    entries
}

fn quicklink_result(
    quicklink: &QuicklinkView,
    argument: Option<&str>,
) -> (crate::model::ResultItem, Option<ResultAction>) {
    let title = format!("/{}", quicklink.command);
    let icon = quicklink.icon_data_url.clone();
    let Some(argument) = argument else {
        return (
            crate::model::ResultItem {
                result_actions: None,
                result_id: String::new(),
                activation: LauncherResultActivation::completion(format!("{title} "))
                    .unwrap_or(LauncherResultActivation::ExecuteResult),
                title,
                subtitle: Some(quicklink.name.clone()),
                icon,
                plugin_icon_url: None,
                icon_kind: None,
                detail: None,
                favorite: None,
                has_default_action: false,
            },
            None,
        );
    };
    match expand_template(&quicklink.template, argument) {
        Ok(url) => (
            crate::model::ResultItem {
                result_actions: None,
                result_id: String::new(),
                activation: LauncherResultActivation::ExecuteResult,
                title,
                subtitle: Some(quicklink.name.clone()),
                icon,
                plugin_icon_url: None,
                icon_kind: None,
                detail: Some(url.to_string()),
                favorite: None,
                has_default_action: true,
            },
            Some(ResultAction::OpenQuicklink {
                id: quicklink.id.clone(),
                url: url.to_string(),
            }),
        ),
        Err(_) => (
            crate::model::ResultItem {
                result_actions: None,
                result_id: String::new(),
                activation: LauncherResultActivation::ExecuteResult,
                title,
                subtitle: Some("快速链接配置无效".into()),
                icon,
                plugin_icon_url: None,
                icon_kind: None,
                detail: None,
                favorite: None,
                has_default_action: false,
            },
            None,
        ),
    }
}

fn quicklink_query<'a, 'q>(
    query: &'q str,
    quicklinks: &'a [QuicklinkView],
) -> Option<(&'a QuicklinkView, Option<&'q str>)> {
    let command = query.strip_prefix('/')?;
    let (command, argument) = command
        .split_once(' ')
        .map_or((command, None), |(command, argument)| {
            (command, Some(argument.trim()))
        });
    if command.is_empty() {
        return None;
    }
    let quicklink = quicklinks
        .iter()
        .find(|quicklink| quicklink.command == command)?;
    Some((quicklink, argument.filter(|argument| !argument.is_empty())))
}

fn public_plugin_completion_result(
    suggestion: crate::public_plugins::PublicCommandSuggestion,
    argument: Option<&str>,
) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    let title = format!("/{}", suggestion.effective_name);
    let identity = format!("{} · {}", suggestion.display_name, suggestion.command_id);
    let subtitle = Some(
        suggestion
            .summary
            .as_ref()
            .map(|summary| format!("{identity} · {summary}"))
            .unwrap_or(identity),
    );
    let activation = match suggestion.output_mode {
        PublicOutputMode::Panel => LauncherResultActivation::panel_activation(
            suggestion.plugin_id.clone(),
            suggestion.command_id.clone(),
            argument.unwrap_or("").to_owned(),
            suggestion.favorite,
        )?,
        PublicOutputMode::MainResult => LauncherResultActivation::main_result_activation(
            suggestion.plugin_id.clone(),
            suggestion.command_id.clone(),
            suggestion.effective_name.clone(),
            argument.unwrap_or("").to_owned(),
            suggestion.favorite,
        )?,
        PublicOutputMode::Window => LauncherResultActivation::window_activation(
            suggestion.plugin_id.clone(),
            suggestion.command_id.clone(),
            suggestion.effective_name.clone(),
            argument.unwrap_or("").to_owned(),
            suggestion.favorite,
        )?,
    };
    let favorite = ResultFavorite::public_plugin(
        suggestion.plugin_id.clone(),
        suggestion.command_id.clone(),
        suggestion.favorite,
    )?;
    Some((
        crate::model::ResultItem {
            result_actions: None,
            result_id: String::new(),
            activation,
            title,
            subtitle,
            icon: None,
            plugin_icon_url: suggestion.icon_url,
            icon_kind: None,
            detail: None,
            favorite: Some(favorite),
            has_default_action: false,
        },
        None,
    ))
}

fn windows_local_today() -> Result<time::Date, ()> {
    time::OffsetDateTime::now_local()
        .map(|now| now.date())
        .map_err(|_| ())
}

fn utc_now() -> Result<time::OffsetDateTime, ()> {
    Ok(time::OffsetDateTime::now_utc())
}

fn search_apps_with_catalog<S, D, P>(
    registry: &ResultRegistry,
    request: CatalogSearchRequest<'_>,
    plugin_catalog: P,
    snapshot: S,
    decorate: D,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
    P: FnOnce(
        &str,
    ) -> Result<
        Vec<crate::public_plugins::PublicCommandSuggestion>,
        PublicPluginManagementError,
    >,
{
    search_apps_with_catalog_with_clocks(
        registry,
        request,
        plugin_catalog,
        snapshot,
        decorate,
        windows_local_today,
        utc_now,
    )
}

fn publish_quick_evaluation<L, U>(
    registry: &ResultRegistry,
    token: QueryToken,
    classification: crate::quick_evaluation::QuickClassification,
    local_today: L,
    utc_now: U,
) -> Option<SearchResponse>
where
    L: FnOnce() -> Result<time::Date, ()>,
    U: FnOnce() -> Result<time::OffsetDateTime, ()>,
{
    let evaluated = match classification {
        crate::quick_evaluation::QuickClassification::NoMatch => return None,
        crate::quick_evaluation::QuickClassification::Invalid(failure) => Err(failure),
        crate::quick_evaluation::QuickClassification::Ready(intent) => {
            crate::quick_evaluation::evaluate(intent, local_today, utc_now)
        }
    };
    publish_quick_evaluation_result(registry, token, evaluated)
}

fn publish_quick_evaluation_result(
    registry: &ResultRegistry,
    token: QueryToken,
    evaluated: Result<crate::quick_evaluation::QuickOutput, crate::quick_evaluation::QuickFailure>,
) -> Option<SearchResponse> {
    let (item, action, refresh_at_utc) = match evaluated {
        Ok(output) => {
            let crate::quick_evaluation::QuickOutput {
                kind,
                title,
                subtitle,
                copy_text,
                refresh_at_utc,
            } = output;
            let refresh_at_utc = match kind {
                crate::quick_evaluation::QuickKind::TimeZone => {
                    refresh_at_utc.and_then(|instant| {
                        instant
                            .format(&time::format_description::well_known::Rfc3339)
                            .ok()
                    })
                }
                _ => None,
            };
            let icon_kind = match kind {
                crate::quick_evaluation::QuickKind::Currency => ResultIconKind::Currency,
                crate::quick_evaluation::QuickKind::Unit => ResultIconKind::Unit,
                crate::quick_evaluation::QuickKind::Arithmetic => ResultIconKind::Calculator,
                crate::quick_evaluation::QuickKind::Date => ResultIconKind::Date,
                crate::quick_evaluation::QuickKind::TimeZone => ResultIconKind::TimeZone,
            };
            (
                crate::model::ResultItem {
                    result_actions: None,
                    result_id: String::new(),
                    activation: LauncherResultActivation::ExecuteResult,
                    title,
                    subtitle,
                    icon: None,
                    plugin_icon_url: None,
                    icon_kind: Some(icon_kind),
                    detail: None,
                    favorite: None,
                    has_default_action: true,
                },
                Some(ResultAction::CopyBuiltInText { text: copy_text }),
                refresh_at_utc,
            )
        }
        Err(failure) => {
            let icon_kind = match failure.kind {
                crate::quick_evaluation::QuickKind::Currency => ResultIconKind::Currency,
                crate::quick_evaluation::QuickKind::Unit => ResultIconKind::Unit,
                crate::quick_evaluation::QuickKind::Arithmetic => ResultIconKind::Calculator,
                crate::quick_evaluation::QuickKind::Date => ResultIconKind::Date,
                crate::quick_evaluation::QuickKind::TimeZone => ResultIconKind::TimeZone,
            };
            (
                crate::model::ResultItem {
                    result_actions: None,
                    result_id: String::new(),
                    activation: LauncherResultActivation::Informational,
                    title: failure.title,
                    subtitle: Some(failure.explanation),
                    icon: None,
                    plugin_icon_url: None,
                    icon_kind: Some(icon_kind),
                    detail: None,
                    favorite: None,
                    has_default_action: false,
                },
                None,
                None,
            )
        }
    };
    registry.publish_if_latest(
        token,
        vec![(item, action)],
        || true,
        |request_id, items| SearchResponse {
            request_id,
            items: items
                .into_iter()
                .map(|(result_id, mut item)| {
                    item.result_id = result_id;
                    item
                })
                .collect(),
            command_hint: None,
            main_result_command: None,
            window_transfer_token: None,
            auto_execute_result_id: None,
            replace_local_results: true,
            refresh_at_utc,
        },
    )
}

#[cfg(test)]
async fn search_apps_with_catalog_and_currency<S, D, P, L, U, R, F>(
    registry: &ResultRegistry,
    request: CatalogSearchRequest<'_>,
    plugin_catalog: P,
    snapshot: S,
    decorate: D,
    local_today: L,
    utc_now: U,
    resolver: R,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
    P: FnOnce(
        &str,
    ) -> Result<
        Vec<crate::public_plugins::PublicCommandSuggestion>,
        PublicPluginManagementError,
    >,
    L: FnOnce() -> Result<time::Date, ()>,
    U: FnOnce() -> Result<time::OffsetDateTime, ()>,
    R: FnOnce(crate::quick_evaluation::currency::CurrencyIntent) -> F,
    F: Future<
        Output = Result<
            crate::quick_evaluation::currency::CurrencyOutput,
            crate::quick_evaluation::currency::service::CurrencyResolveError,
        >,
    >,
{
    match resolve_currency_search(
        registry,
        request.query,
        request.invocation_id,
        request.query_sequence,
        resolver,
    )
    .await
    {
        CurrencySearchResponse::Published(response) => response,
        CurrencySearchResponse::Continue => search_apps_with_catalog_with_clocks(
            registry,
            request,
            plugin_catalog,
            snapshot,
            decorate,
            local_today,
            utc_now,
        ),
    }
}

enum CurrencySearchResponse {
    Continue,
    Published(Option<SearchResponse>),
}

async fn resolve_currency_search<R, F>(
    registry: &ResultRegistry,
    query: &str,
    invocation_id: &str,
    query_sequence: u64,
    resolver: R,
) -> CurrencySearchResponse
where
    R: FnOnce(crate::quick_evaluation::currency::CurrencyIntent) -> F,
    F: Future<
        Output = Result<
            crate::quick_evaluation::currency::CurrencyOutput,
            crate::quick_evaluation::currency::service::CurrencyResolveError,
        >,
    >,
{
    match crate::quick_evaluation::classify(query.trim()) {
        crate::quick_evaluation::QuickClassification::Invalid(failure)
            if failure.kind == crate::quick_evaluation::QuickKind::Currency =>
        {
            let token =
                registry.begin_query(QueryDomain::Application, invocation_id, query_sequence);
            CurrencySearchResponse::Published(
                token.and_then(|token| {
                    publish_quick_evaluation_result(registry, token, Err(failure))
                }),
            )
        }
        crate::quick_evaluation::QuickClassification::Ready(
            crate::quick_evaluation::QuickIntent::Currency(intent),
        ) => {
            let Some(token) =
                registry.begin_query(QueryDomain::Application, invocation_id, query_sequence)
            else {
                return CurrencySearchResponse::Published(None);
            };
            let evaluated = crate::quick_evaluation::evaluate_currency(intent, resolver).await;
            CurrencySearchResponse::Published(publish_quick_evaluation_result(
                registry, token, evaluated,
            ))
        }
        _ => CurrencySearchResponse::Continue,
    }
}

fn search_apps_with_catalog_with_clocks<S, D, P, L, U>(
    registry: &ResultRegistry,
    request: CatalogSearchRequest<'_>,
    plugin_catalog: P,
    snapshot: S,
    decorate: D,
    local_today: L,
    utc_now: U,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
    P: FnOnce(
        &str,
    ) -> Result<
        Vec<crate::public_plugins::PublicCommandSuggestion>,
        PublicPluginManagementError,
    >,
    L: FnOnce() -> Result<time::Date, ()>,
    U: FnOnce() -> Result<time::OffsetDateTime, ()>,
{
    let query = request.query.trim();
    if !query.starts_with('/') {
        let classification = crate::quick_evaluation::classify(query);
        if !matches!(
            classification,
            crate::quick_evaluation::QuickClassification::NoMatch
        ) {
            let token = registry.begin_query(
                QueryDomain::Application,
                request.invocation_id,
                request.query_sequence,
            )?;
            return publish_quick_evaluation(registry, token, classification, local_today, utc_now);
        }
    }
    let token = registry.begin_query(
        QueryDomain::Application,
        request.invocation_id,
        request.query_sequence,
    )?;

    let mut command_hint = None;
    let mut replace_local_results = false;
    let mut auto_execute_first_result = false;
    let mut entries: Vec<(crate::model::ResultItem, Option<ResultAction>)> = Vec::new();
    let catalog_query = if query == "/" { "" } else { query };
    let find_favorite = request
        .favorite_builtin_features
        .contains(&BuiltinFeature::Find);
    let quicklinks_favorite = request
        .favorite_builtin_features
        .contains(&BuiltinFeature::Quicklinks);
    let web_search_favorite = request
        .favorite_builtin_features
        .contains(&BuiltinFeature::WebSearch);
    let screenshot_favorite = request
        .favorite_builtin_features
        .contains(&BuiltinFeature::Screenshot);
    if query == "/find" {
        entries.push(find_result(String::new(), "搜索文件".into(), find_favorite));
        replace_local_results = true;
    } else if query == "/quicklinks" {
        entries.push(quicklinks_result(quicklinks_favorite));
        replace_local_results = true;
    } else if query == "/screenshot" {
        entries.push(screenshot_result(screenshot_favorite));
        replace_local_results = true;
    } else if query == "/web-search" {
        command_hint = Some("请输入搜索内容".into());
        entries.extend(builtin_completion_result(
            "web-search",
            "使用默认搜索引擎搜索".into(),
            ResultIconKind::WebSearch,
            BuiltinFeature::WebSearch,
            web_search_favorite,
        ));
        replace_local_results = true;
    } else if let Some(argument) = query.strip_prefix("/web-search ") {
        let argument = argument.trim();
        if argument.is_empty() {
            command_hint = Some("请输入搜索内容".into());
        } else {
            entries.push(web_search_result(
                request.web_search_engine,
                argument.to_owned(),
                web_search_favorite,
            ));
            auto_execute_first_result = request.submit;
        }
        replace_local_results = true;
    } else if let Some((quicklink, argument)) = quicklink_query(query, request.quicklinks) {
        if argument.is_none() {
            command_hint = Some("请输入参数".into());
        }
        entries.push(quicklink_result(quicklink, argument));
        auto_execute_first_result = request.submit && argument.is_some();
        replace_local_results = true;
    } else if let Some(prefix) = plugin_discovery_prefix(query) {
        entries.extend(builtin_prefix_results(
            prefix,
            find_favorite,
            quicklinks_favorite,
            web_search_favorite,
            screenshot_favorite,
        ));
        entries.extend(
            plugin_catalog(prefix)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|suggestion| public_plugin_completion_result(suggestion, None)),
        );
        replace_local_results = true;
    } else if query.starts_with('/') && query != "/" {
        replace_local_results = true;
    } else {
        let mut suggestions = plugin_catalog(catalog_query).unwrap_or_default();
        suggestions.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| left.effective_name.cmp(&right.effective_name))
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
        });
        if catalog_query.is_empty() {
            entries.push(find_result(String::new(), "搜索文件".into(), find_favorite));
            entries.push(quicklinks_result(quicklinks_favorite));
            entries.push(screenshot_result(screenshot_favorite));
            entries.extend(builtin_completion_result(
                "web-search",
                "使用默认搜索引擎搜索".into(),
                ResultIconKind::WebSearch,
                BuiltinFeature::WebSearch,
                web_search_favorite,
            ));
        } else {
            let mut applications = snapshot();
            let normalized_query = catalog_query.to_lowercase();
            if "screenshot".contains(&normalized_query) || "截图".contains(&normalized_query) {
                entries.push(screenshot_result(screenshot_favorite));
            }
            decorate(&mut applications);
            entries.extend(
                apps::rank(&applications, catalog_query)
                    .iter()
                    .map(apps::registry_entry)
                    .map(|(item, action)| (item, Some(action))),
            );
            for suggestion in suggestions
                .iter()
                .filter(|suggestion| suggestion.favorite)
                .cloned()
            {
                entries.extend(public_plugin_completion_result(
                    suggestion,
                    Some(catalog_query),
                ));
            }
            entries.push(find_result(
                catalog_query.to_owned(),
                format!("搜索文件：{catalog_query}"),
                find_favorite,
            ));
            entries.push(web_search_result(
                request.web_search_engine,
                catalog_query.to_owned(),
                web_search_favorite,
            ));
        }
        for suggestion in suggestions {
            if !catalog_query.is_empty() && suggestion.favorite {
                continue;
            }
            let argument = match suggestion.output_mode {
                PublicOutputMode::Panel
                    if !catalog_query.is_empty() && !catalog_query.starts_with('/') =>
                {
                    Some(catalog_query)
                }
                _ if !catalog_query.is_empty() && suggestion.favorite => Some(catalog_query),
                _ => None,
            };
            entries.extend(public_plugin_completion_result(suggestion, argument));
        }
        if catalog_query.is_empty() {
            entries.sort_by(|(left, _), (right, _)| {
                let left_favorite = left
                    .favorite
                    .as_ref()
                    .is_some_and(|favorite| favorite.favorite);
                let right_favorite = right
                    .favorite
                    .as_ref()
                    .is_some_and(|favorite| favorite.favorite);
                right_favorite.cmp(&left_favorite)
            });
        }
    }

    // Filter before publishing so disabled entries never acquire executable result IDs.
    // Saved child command names still resolve above, keeping their names reserved.
    let disabled = |feature| request.disabled_builtin_features.contains(&feature);
    entries.retain(|(item, _)| match item.icon_kind {
        Some(ResultIconKind::Screenshot) => !disabled(ManagedBuiltinFeature::Screenshot),
        Some(ResultIconKind::Quicklinks) => !disabled(ManagedBuiltinFeature::Quicklinks),
        Some(ResultIconKind::WebSearch) => !disabled(ManagedBuiltinFeature::WebSearch),
        _ => {
            !disabled(ManagedBuiltinFeature::Quicklinks)
                || !request
                    .quicklinks
                    .iter()
                    .any(|link| item.title == format!("/{}", link.command))
        }
    });
    if entries.is_empty() {
        command_hint = None;
        auto_execute_first_result = false;
    }

    registry.publish_if_latest(
        token,
        entries,
        || true,
        |request_id, items| {
            let items = items
                .into_iter()
                .map(|(result_id, mut item)| {
                    item.result_id = result_id;
                    item
                })
                .collect::<Vec<_>>();
            let auto_execute_result_id = auto_execute_first_result
                .then(|| items.first().map(|item| item.result_id.clone()))
                .flatten();
            SearchResponse {
                request_id,
                items,
                command_hint,
                main_result_command: None,
                window_transfer_token: None,
                auto_execute_result_id,
                replace_local_results,
                refresh_at_utc: None,
            }
        },
    )
}

struct PreparedFileQuery {
    query: String,
    category: FileCategory,
    invocation_id: String,
    query_sequence: u64,
}

fn prepare_file_query(
    query: String,
    category: String,
    sort: String,
    invocation_id: String,
    query_sequence: u64,
) -> Result<PreparedFileQuery, CommandError> {
    let category = FileCategory::parse_wire(&category);
    if query.is_empty()
        || query.len() > 1_024
        || query.chars().count() > 255
        || query.contains('\0')
        || category.is_none()
        || sort != "modifiedDesc"
        || invocation_id.is_empty()
        || query_sequence == 0
    {
        return Err(CommandError::invalid_file_query());
    }
    Ok(PreparedFileQuery {
        query,
        category: category.expect("validated file category"),
        invocation_id,
        query_sequence,
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn search_files(
    window: WebviewWindow,
    registries: State<'_, ResultRegistries>,
    controller: State<'_, Arc<FindWindowController>>,
    everything_search: State<'_, Arc<EverythingSearchState>>,
    query: String,
    category: String,
    sort: String,
    invocation_id: String,
    query_sequence: u64,
) -> Result<Option<FileSearchResponse>, CommandError> {
    require_find_window(&window)?;
    let prepared = prepare_file_query(query, category, sort, invocation_id, query_sequence)?;
    let state = Arc::clone(everything_search.inner());
    search_files_with(
        registries.find(),
        controller.inner().as_ref(),
        prepared,
        move |query, category| state.search(&query, category),
    )
    .await
}

async fn search_files_with<S>(
    registry: &ResultRegistry,
    controller: &FindWindowController,
    prepared: PreparedFileQuery,
    search: S,
) -> Result<Option<FileSearchResponse>, CommandError>
where
    S: FnOnce(String, FileCategory) -> Result<PublishedFileBatch, EverythingSearchError>
        + Send
        + 'static,
{
    let invocation_id = prepared.invocation_id.clone();
    let token = match controller.with_visible_admission(&invocation_id, || {
        registry.begin_query(QueryDomain::File, &invocation_id, prepared.query_sequence)
    }) {
        Some(Some(token)) => token,
        None | Some(None) => return Ok(None),
    };
    let batch =
        tauri::async_runtime::spawn_blocking(move || search(prepared.query, prepared.category))
            .await
            .map_err(|_| CommandError::file_search_worker_failed())?
            .map_err(map_everything_search_error)?;
    Ok(controller
        .with_visible_admission(&invocation_id, || {
            publish_everything_search(registry, token, batch)
        })
        .flatten())
}

fn map_everything_search_error(error: EverythingSearchError) -> CommandError {
    match error {
        EverythingSearchError::Starting => CommandError::file_search_starting(),
        EverythingSearchError::Unavailable | EverythingSearchError::RevisionExhausted => {
            CommandError::search_unavailable()
        }
    }
}

fn publish_everything_search(
    registry: &ResultRegistry,
    token: QueryToken,
    batch: PublishedFileBatch,
) -> Option<FileSearchResponse> {
    let total = batch.items.len();
    registry.publish_if_latest(
        token,
        batch
            .items
            .into_iter()
            .map(|item| {
                let action = ResultAction::OpenFile(item.action.clone());
                (item, action)
            })
            .collect(),
        || true,
        |request_id, items| FileSearchResponse {
            request_id,
            index_revision: batch.index_revision.to_string(),
            total: total.to_string(),
            status: FileIndexStatus::Ready,
            items: items.into_iter().map(map_published_file_item).collect(),
        },
    )
}

fn map_published_file_item((result_id, item): (String, PublishedFileDraft)) -> FileResultItem {
    FileResultItem {
        result_id,
        name: item.name,
        kind: item.kind,
        size_bytes: item.size_bytes.map(|value| value.to_string()),
        modified_utc: item.modified_utc,
        full_path: item.full_path,
    }
}

#[tauri::command]
pub(crate) fn load_settings(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    settings: State<'_, SettingsStore>,
    public_plugins: State<'_, Arc<PublicPluginService>>,
) -> Result<SettingsView, CommandError> {
    require_main_window(&window)?;
    crate::dev_startup::stage("收到前端 load_settings，读取设置");
    let mut view = load_settings_ready_with(
        || {
            coordinator
                .mark_frontend_ready(&app)
                .map_err(|_| CommandError::window_failed())
        },
        || load_settings_core(&settings),
    )?;
    crate::dev_startup::stage("调度公开插件运行时");
    public_plugins.inner().start_enabled_runtimes(&app)?;
    view.screenshot_hotkey_status = coordinator
        .screenshot_hotkeys
        .status(view.screenshot_hotkey.as_deref());
    view.screenshot_save_directory_path = view
        .screenshot_save_directory
        .clone()
        .or_else(|| app.path().desktop_dir().ok());
    static READY_MARKER: std::sync::Once = std::sync::Once::new();
    READY_MARKER.call_once(|| eprintln!("[UiPilot] main frontend ready"));
    #[cfg(debug_assertions)]
    {
        static READY_LOG: std::sync::Once = std::sync::Once::new();
        READY_LOG.call_once(|| {
            crate::dev_startup::ready();
            eprintln!("[UiPilot dev] 主程序已就绪：主窗口前端已连接，设置已加载，可使用启动器。");
        });
    }
    Ok(view)
}

fn load_settings_ready_with<R, L, T>(mark_ready: R, load: L) -> Result<T, CommandError>
where
    R: FnOnce() -> Result<(), CommandError>,
    L: FnOnce() -> T,
{
    mark_ready()?;
    Ok(load())
}

fn load_settings_core(settings: &SettingsStore) -> SettingsView {
    let settings = settings.snapshot();
    SettingsView {
        screenshot_hotkey_status: if settings.screenshot_hotkey.is_some() {
            "unavailable"
        } else {
            "unset"
        },
        screenshot_hotkey: settings.screenshot_hotkey,
        screenshot_save_directory_path: settings.screenshot_save_directory.clone(),
        screenshot_save_directory: settings.screenshot_save_directory,
        hotkey: settings.hotkey,
        autostart: settings.autostart,
        file_preview_enabled: settings.file_preview_enabled,
        theme: settings.theme,
        web_search_engine: settings.web_search_engine,
    }
}

fn prepare_settings_save(
    settings: UserSettingsUpdate,
) -> Result<(HotkeyKind, SettingsUpdate), CommandError> {
    let kind = HotkeyKind::parse(&settings.hotkey).map_err(|_| CommandError::settings_failed())?;
    let update = SettingsUpdate {
        hotkey: kind.canonical(),
        autostart: settings.autostart,
        theme: settings.theme,
        web_search_engine: settings.web_search_engine,
    };
    Ok((kind, update))
}

fn prepare_hotkey_save(
    update: HotkeySettingsUpdate,
) -> Result<(HotkeyKind, HotkeySettingsView), CommandError> {
    let kind = HotkeyKind::parse(&update.hotkey).map_err(|_| CommandError::settings_failed())?;
    let hotkey = kind.canonical();
    Ok((kind, HotkeySettingsView { hotkey }))
}

async fn save_settings_with<R, E, W>(
    settings: UserSettingsUpdate,
    reserve: R,
    worker: W,
) -> Result<(), CommandError>
where
    R: FnOnce() -> Result<CriticalReservation, E>,
    W: FnOnce(CriticalReservation, HotkeyKind, SettingsUpdate) -> Result<(), SettingsSaveError>
        + Send
        + 'static,
{
    let (kind, update) = prepare_settings_save(settings)?;
    save_settings_worker_with(reserve, move |reservation| {
        worker(reservation, kind, update)
    })
    .await
}

async fn save_hotkey_with<R, E, W>(
    hotkey: HotkeySettingsUpdate,
    reserve: R,
    worker: W,
) -> Result<HotkeySettingsView, CommandError>
where
    R: FnOnce() -> Result<CriticalReservation, E>,
    W: FnOnce(CriticalReservation, HotkeyKind, String) -> Result<(), SettingsSaveError>
        + Send
        + 'static,
{
    let (kind, view) = prepare_hotkey_save(hotkey)?;
    let persisted = view.hotkey.clone();
    save_settings_worker_with(reserve, move |reservation| {
        worker(reservation, kind, persisted)
    })
    .await?;
    Ok(view)
}

pub(crate) async fn save_settings_worker_with<R, E, W>(
    reserve: R,
    worker: W,
) -> Result<(), CommandError>
where
    R: FnOnce() -> Result<CriticalReservation, E>,
    W: FnOnce(CriticalReservation) -> Result<(), SettingsSaveError> + Send + 'static,
{
    let reservation = reserve().map_err(|_| CommandError::settings_failed())?;
    let result = tauri::async_runtime::spawn_blocking(move || worker(reservation))
        .await
        .map_err(|_| ());
    map_save_worker_result(result)
}

fn map_save_worker_result(
    result: Result<Result<(), SettingsSaveError>, ()>,
) -> Result<(), CommandError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(SettingsSaveError::HotkeyConflict)) => Err(CommandError::hotkey_conflict()),
        Ok(Err(SettingsSaveError::Failed)) | Err(()) => Err(CommandError::settings_failed()),
    }
}

#[tauri::command]
pub(crate) async fn save_settings(
    window: tauri::WebviewWindow,
    settings: UserSettingsUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    save_settings_with(
        settings,
        || {
            let reservation = coordinator.reserve_critical()?;
            Ok::<_, ReservationError>(reservation)
        },
        {
            let app_for_worker = app.clone();
            let coordinator_for_worker = Arc::clone(coordinator.inner());
            move |reservation, kind, update| {
                let _reservation = reservation;
                let settings = app_for_worker.state::<SettingsStore>();
                coordinator_for_worker.save_settings_transaction(
                    &app_for_worker,
                    &settings,
                    kind,
                    update,
                )
            }
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn save_hotkey(
    window: tauri::WebviewWindow,
    hotkey: HotkeySettingsUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<HotkeySettingsView, CommandError> {
    require_main_window(&window)?;
    save_hotkey_with(
        hotkey,
        || {
            let reservation = coordinator.reserve_critical()?;
            Ok::<_, ReservationError>(reservation)
        },
        {
            let app_for_worker = app.clone();
            let coordinator_for_worker = Arc::clone(coordinator.inner());
            move |reservation, kind, hotkey| {
                let _reservation = reservation;
                let settings = app_for_worker.state::<SettingsStore>();
                coordinator_for_worker.save_hotkey_transaction(
                    &app_for_worker,
                    &settings,
                    kind,
                    hotkey,
                )
            }
        },
    )
    .await
}

async fn set_theme_preference_with<R, E, W>(
    preference: ThemePreferenceUpdate,
    reserve: R,
    worker: W,
) -> Result<(), CommandError>
where
    R: FnOnce() -> Result<CriticalReservation, E>,
    W: FnOnce(CriticalReservation, ThemePreferenceUpdate) -> Result<(), ()> + Send + 'static,
{
    let reservation = reserve().map_err(|_| CommandError::settings_failed())?;
    let result = tauri::async_runtime::spawn_blocking(move || worker(reservation, preference))
        .await
        .map_err(|_| ());
    map_theme_preference_worker_result(result)
}

fn map_theme_preference_worker_result(
    result: Result<Result<(), ()>, ()>,
) -> Result<(), CommandError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) | Err(()) => Err(CommandError::settings_failed()),
    }
}

#[tauri::command]
pub(crate) async fn set_theme_preference(
    window: tauri::WebviewWindow,
    preference: ThemePreferenceUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let app_for_worker = app.clone();
    set_theme_preference_with(
        preference,
        || coordinator.reserve_critical(),
        move |reservation, preference| {
            let _reservation = reservation;
            let revision = app_for_worker
                .state::<SettingsStore>()
                .set_theme_preference_with_revision(preference.theme)
                .map_err(|_| ())?;
            if let Some(screenshot) =
                app_for_worker.try_state::<Arc<crate::screenshot::window::ScreenshotHost>>()
            {
                screenshot.emit_theme(&app_for_worker);
            }
            if let Some(find) = app_for_worker.get_webview_window("find") {
                find.emit(
                    "find://theme-changed",
                    FindThemeChanged {
                        theme_revision: revision.to_string(),
                        theme: preference.theme,
                    },
                )
                .map_err(|_| ())?;
            }
            Ok(())
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_web_search_engine(
    window: tauri::WebviewWindow,
    preference: WebSearchEngineUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let reservation = coordinator
        .reserve_critical()
        .map_err(|_| CommandError::settings_failed())?;
    let app_for_worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        app_for_worker
            .state::<SettingsStore>()
            .set_web_search_engine(preference.engine)
            .map_err(|_| ())
    })
    .await
    .map_err(|_| ());
    map_theme_preference_worker_result(result)
}

#[tauri::command]
pub(crate) async fn set_builtin_feature_favorite(
    window: tauri::WebviewWindow,
    input: BuiltinFeatureFavoriteUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let reservation = coordinator
        .reserve_critical()
        .map_err(|_| CommandError::settings_failed())?;
    let app_for_worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        app_for_worker
            .state::<SettingsStore>()
            .set_builtin_feature_favorite(input.feature, input.favorite)
            .map_err(|_| ())
    })
    .await
    .map_err(|_| ());
    map_theme_preference_worker_result(result)
}

async fn set_file_preview_preference_with<R, E, W>(
    preference: FilePreviewPreferenceUpdate,
    reserve: R,
    worker: W,
) -> Result<(), CommandError>
where
    R: FnOnce() -> Result<CriticalReservation, E>,
    W: FnOnce(CriticalReservation, FilePreviewPreferenceUpdate) -> Result<(), ()> + Send + 'static,
{
    let reservation = reserve().map_err(|_| CommandError::settings_failed())?;
    let result = tauri::async_runtime::spawn_blocking(move || worker(reservation, preference))
        .await
        .map_err(|_| ());
    map_file_preview_worker_result(result)
}

fn map_file_preview_worker_result(result: Result<Result<(), ()>, ()>) -> Result<(), CommandError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) | Err(()) => Err(CommandError::settings_failed()),
    }
}

#[tauri::command]
pub(crate) async fn set_file_preview_preference(
    window: tauri::WebviewWindow,
    preference: FilePreviewPreferenceUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let app_for_worker = app.clone();
    set_file_preview_preference_with(
        preference,
        || coordinator.reserve_critical(),
        move |reservation, preference| {
            let _reservation = reservation;
            app_for_worker
                .state::<SettingsStore>()
                .set_file_preview_enabled(preference.enabled)
                .map_err(|_| ())
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_find_preview_preference(
    window: tauri::WebviewWindow,
    preference: FilePreviewPreferenceUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
) -> Result<FindPreviewPreferenceResult, CommandError> {
    require_find_window(&window)?;
    let controller = app.state::<Arc<FindWindowController>>();
    let Some(invocation_id) = controller.current_invocation() else {
        return Err(CommandError::stale_request());
    };
    if !controller.admit_search(&invocation_id) {
        return Err(CommandError::stale_request());
    }
    let app_for_worker = app.clone();
    let reservation = coordinator.reserve_critical()?;
    let enabled = preference.enabled;
    let revision = tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        app_for_worker
            .state::<SettingsStore>()
            .set_file_preview_enabled_with_revision(enabled)
            .map_err(|_| ())
    })
    .await
    .map_err(|_| CommandError::settings_failed())?
    .map_err(|_| CommandError::settings_failed())?;
    Ok(FindPreviewPreferenceResult {
        file_preview_revision: revision.to_string(),
        file_preview_enabled: enabled,
    })
}

#[tauri::command]
pub(crate) async fn load_find_thumbnail(
    window: WebviewWindow,
    request_id: String,
    result_id: String,
    registries: State<'_, ResultRegistries>,
    controller: State<'_, Arc<FindWindowController>>,
) -> Result<Option<String>, CommandError> {
    require_find_window(&window)?;
    let invocation_id = controller
        .current_invocation()
        .ok_or_else(CommandError::stale_request)?;
    let action = controller
        .with_visible_admission(&invocation_id, || {
            registries.find().resolve(&request_id, &result_id)
        })
        .ok_or_else(CommandError::stale_request)?
        .map_err(|error| match error {
            RegistryError::StaleRequest => CommandError::stale_request(),
            RegistryError::UnknownResult => CommandError::unknown_result(),
        })?;
    let identity = match action {
        ResultAction::OpenFile(FileExecutionAction::Everything(action)) => {
            action.identity().clone()
        }
        _ => return Ok(None),
    };
    let thumbnail = tauri::async_runtime::spawn_blocking(move || {
        path_auth::with_authenticated_file(&identity, |path| {
            apps::thumbnail_from_path(&PathBuf::from(path))
        })
        .ok()
        .flatten()
    })
    .await
    .unwrap_or(None);
    let still_current = controller
        .with_visible_admission(&invocation_id, || {
            registries.find().resolve(&request_id, &result_id).is_ok()
        })
        .unwrap_or(false);
    Ok(still_current.then_some(thumbnail).flatten())
}

#[cfg(test)]
fn save_settings_core(
    settings: UserSettingsUpdate,
    store: &SettingsStore,
) -> Result<(), CommandError> {
    store
        .update_user_settings(SettingsUpdate {
            hotkey: settings.hotkey,
            autostart: settings.autostart,
            theme: settings.theme,
            web_search_engine: settings.web_search_engine,
        })
        .map_err(|_| CommandError::settings_failed())
}

struct ResultActionCleanup<'a> {
    service: &'a PublicPluginService,
    registry: &'a ResultRegistry,
}
impl Drop for ResultActionCleanup<'_> {
    fn drop(&mut self) {
        self.service.cleanup_retired_result_actions(self.registry);
    }
}
struct NativeResultPaste<'a> {
    window: &'a WebviewWindow,
    registry: &'a ResultRegistry,
    public: &'a PublicPluginService,
    legacy: &'a PluginManager,
    lifecycle: &'a LifecycleCoordinator,
    origin: &'a crate::public_plugins::result_action::ResultActionOrigin,
    lease: crate::lifecycle::ResultPasteLease,
    main_hwnd: isize,
}
impl crate::result_paste::PasteDriver for NativeResultPaste<'_> {
    fn preflight(
        &mut self,
    ) -> Result<(), crate::public_plugins::result_action::ResultActionErrorCode> {
        use crate::result_paste::native;
        crate::result_paste::preflight(
            self.window.is_visible().unwrap_or(false),
            native::main_is_foreground(self.main_hwnd),
            self.lifecycle.result_paste_lease_current(self.lease),
            native::target_valid(self.lease.target),
            native::keys_released(),
        )
    }
    fn admit_write(
        &mut self,
    ) -> Result<u32, crate::public_plugins::result_action::ResultActionOutcome> {
        use crate::public_plugins::result_action::{
            ResultActionErrorCode as E, ResultActionOutcome as O,
        };
        self.legacy
            .with_public_result_admission(|| {
                self.public.manager().map(|manager| {
                    manager.write_paste_result_action(
                        self.registry,
                        self.origin,
                        || {
                            crate::result_paste::preflight(
                                self.window.is_visible().unwrap_or(false),
                                crate::result_paste::native::main_is_foreground(self.main_hwnd),
                                self.lifecycle.result_paste_lease_current(self.lease),
                                crate::result_paste::native::target_valid(self.lease.target),
                                crate::result_paste::native::keys_released(),
                            )
                        },
                        |text| crate::result_paste::native::write_text(self.main_hwnd, text),
                    )
                })
            })
            .and_then(Result::ok)
            .unwrap_or_else(|| Err(O::failed(E::ActionUnavailable, false)))
    }
    fn hide_restore(
        &mut self,
    ) -> Result<(), crate::public_plugins::result_action::ResultActionErrorCode> {
        clear_and_hide_window(
            self.registry,
            &self.window.as_ref().window(),
            HideReason::ExplicitReturn,
        )
        .map_err(|_| crate::public_plugins::result_action::ResultActionErrorCode::PasteFailed)
    }
    fn send(
        &mut self,
        sequence: u32,
    ) -> Result<(), crate::public_plugins::result_action::ResultActionErrorCode> {
        crate::result_paste::native::send(
            self.lease.target,
            sequence,
            self.lifecycle.result_paste_generation_current(self.lease),
        )
    }
}
async fn execute_paste_result_action(
    window: WebviewWindow,
    registry: ResultRegistry,
    public: Arc<PublicPluginService>,
    legacy: Arc<PluginManager>,
    origin: crate::public_plugins::result_action::ResultActionOrigin,
) -> crate::public_plugins::result_action::ResultActionOutcome {
    use crate::public_plugins::result_action::{
        ResultActionErrorCode as E, ResultActionOutcome as O,
    };
    let lifecycle = Arc::clone(window.state::<Arc<LifecycleCoordinator>>().inner());
    let Some(lease) = lifecycle.result_paste_lease() else {
        return O::failed(E::PasteTargetUnavailable, false);
    };
    let main_hwnd = match window.hwnd() {
        Ok(hwnd) => hwnd.0 as isize,
        Err(_) => return O::failed(E::PasteTargetUnavailable, false),
    };
    match tauri::async_runtime::spawn_blocking(crate::result_paste::wait_for_keys).await {
        Ok(Ok(())) => {}
        Ok(Err(code)) => return O::failed(code, false),
        Err(_) => return O::failed(E::ActionUnavailable, false),
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let app = window.app_handle().clone();
    if app
        .run_on_main_thread(move || {
            let outcome = crate::result_paste::execute(&mut NativeResultPaste {
                window: &window,
                registry: &registry,
                public: &public,
                legacy: &legacy,
                lifecycle: &lifecycle,
                origin: &origin,
                lease,
                main_hwnd,
            });
            let _ = sender.send(outcome);
        })
        .is_err()
    {
        return O::failed(E::ActionUnavailable, false);
    }
    receiver
        .await
        .unwrap_or_else(|_| O::failed(E::ActionUnavailable, true))
}
#[tauri::command]
pub(crate) async fn execute_result_action(
    window: WebviewWindow,
    request: serde_json::Value,
) -> Result<crate::public_plugins::result_action::ResultActionOutcome, CommandError> {
    require_main_label(window.label())?;
    use crate::public_plugins::result_action::{
        ExecuteResultActionRequest, PublicResultAction, ResultActionErrorCode as E,
        ResultActionOutcome as O,
    };
    let request: ExecuteResultActionRequest =
        serde_json::from_value(request).map_err(|_| CommandError::unknown_result())?;
    let app = window.app_handle().clone();
    let registry = app.state::<ResultRegistries>().main().clone();
    let public = Arc::clone(app.state::<Arc<PublicPluginService>>().inner());
    public.cleanup_retired_result_actions(&registry);
    let origin = match registry.resolve_result_action(&request) {
        Ok(origin) => origin,
        Err(code) => return Ok(O::failed(code, false)),
    };
    let legacy = Arc::clone(app.state::<Arc<PluginManager>>().inner());
    if matches!(origin.action, PublicResultAction::PasteText { .. }) {
        return Ok(execute_paste_result_action(window, registry, public, legacy, origin).await);
    }
    if matches!(
        origin.action,
        PublicResultAction::CopyText { .. } | PublicResultAction::OpenUrl { .. }
    ) {
        let result = legacy
            .with_public_result_admission(|| {
                public.manager().map(|m| match &origin.action {
                    PublicResultAction::OpenUrl { .. } => {
                        m.open_url_result_action(&registry, &origin, crate::browser_open::open_url)
                    }
                    _ => m.copy_result_action(&registry, &origin, |text| {
                        app.clipboard().write_text(text.to_owned()).map_err(|_| ())
                    }),
                })
            })
            .and_then(Result::ok)
            .unwrap_or_else(|| O::failed(E::ActionUnavailable, false));
        return Ok(result);
    }
    let (dispatch, receiver) =
        match public.prepare_result_action(&app, legacy, registry.clone(), origin) {
            Ok(prepared) => prepared,
            Err(code) => {
                return Ok(O::failed(
                    code,
                    matches!(code, E::ActionInProgress | E::ActionAlreadyExecuted),
                ))
            }
        };
    let theme = invocation_theme(&app, app.state::<SettingsStore>().snapshot().theme);
    if public
        .dispatch(&app, &dispatch, theme, invoked_at_rfc3339())
        .is_err()
    {
        // Uncertain transport consumes ownership but never frees possibly executing JS.
        if let Ok(manager) = public.manager() {
            manager.retire_panel_action(&dispatch.context);
        }
        public.settle_result_action(&dispatch.context, E::ActionUnavailable);
    }
    Ok(
        tauri::async_runtime::spawn_blocking(move || receiver.recv())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(|| O::failed(E::ActionUnavailable, true)),
    )
}
#[tauri::command]
pub(crate) fn retire_result_actions(
    window: WebviewWindow,
    request: serde_json::Value,
) -> Result<(), CommandError> {
    require_main_label(window.label())?;
    let request: crate::public_plugins::result_action::RetireResultActionsRequest =
        serde_json::from_value(request).map_err(|_| CommandError::unknown_result())?;
    let app = window.app_handle();
    let registries = app.state::<ResultRegistries>();
    registries.main().retire_result_actions(&request.request_id);
    app.state::<Arc<PublicPluginService>>()
        .cleanup_retired_result_actions(registries.main());
    Ok(())
}

#[tauri::command]
pub(crate) async fn execute_result(
    window: WebviewWindow,
    request_id: String,
    result_id: String,
) -> Result<ExecuteOutcome, CommandError> {
    if window.label() != "main" && window.label() != "find" {
        return Err(CommandError::invalid_caller());
    }
    let app = window.app_handle().clone();
    let registries = app.state::<ResultRegistries>();
    let file_index = app.state::<Arc<FileIndex>>();
    if window.label() == "find" {
        let controller = app.state::<Arc<FindWindowController>>();
        let (action, ticket) = registries
            .find()
            .resolve_with_ticket(&request_id, &result_id)
            .map_err(|error| match error {
                RegistryError::StaleRequest => CommandError::stale_request(),
                RegistryError::UnknownResult => CommandError::unknown_result(),
            })?;
        let ResultAction::OpenFile(action) = action else {
            return Err(CommandError::unknown_result());
        };
        let worker_index = Arc::clone(file_index.inner());
        let result = tauri::async_runtime::spawn_blocking(move || {
            execute_file_action_with(
                action,
                |action| worker_index.execute_indexed_path(action),
                |action| path_auth::execute_authenticated_path(action.identity()),
            )
            .map_err(map_file_execution_error)
        })
        .await
        .map_err(|_| CommandError::file_open_failed())??;
        let outcome = match result {
            FileExecutionOutcome::FileRevealRequested => ExecuteOutcome::FileRevealRequested,
            FileExecutionOutcome::FolderOpenRequested => ExecuteOutcome::FolderOpenRequested,
        };
        if controller.begin_execution_hide(&ticket, registries.find())
            == ExecutionHideAdmission::Started
        {
            let hidden = window.hide().is_ok();
            let finish = controller.finish_execution_hide(hidden, &registries);
            if matches!(
                finish,
                HideFinish::Hidden {
                    snapshot_required: true
                } | HideFinish::Visible {
                    snapshot_required: true
                }
            ) {
                lifecycle::start_find_transfer(&app, controller.inner().as_ref(), &registries)
                    .map_err(|_| CommandError::window_failed())?;
            }
            if !hidden {
                return Err(CommandError::window_failed());
            }
        }
        return Ok(outcome);
    }

    let registry = registries.main();
    let plugins = app.state::<Arc<PluginManager>>();
    let public_plugins = app.state::<Arc<PublicPluginService>>();
    let settings = app.state::<SettingsStore>();
    let cache = app.state::<Arc<AppCache>>();
    let worker_index = Arc::clone(file_index.inner());
    execute_result_with_clipboard(
        (&request_id, &result_id),
        ClipboardExecution {
            resolve: |request_id: &str, result_id: &str| registry.resolve(request_id, result_id),
            execute: |action: &ResultAction| apps::execute_application(action).map_err(|_| ()),
            execute_file: move |action| {
                let worker_index = Arc::clone(&worker_index);
                async move {
                    tauri::async_runtime::spawn_blocking(move || {
                        execute_file_action_with(
                            action,
                            |action| worker_index.execute_indexed_path(action),
                            |action| path_auth::execute_authenticated_path(action.identity()),
                        )
                        .map_err(map_file_execution_error)
                    })
                    .await
                    .map_err(|_| CommandError::file_open_failed())?
                }
            },
            open_web_search: |engine: WebSearchEngine, query: &str| {
                open_builtin_with(&settings, ManagedBuiltinFeature::WebSearch, || {
                    crate::web_search::open_search(engine, query)
                })
            },
            open_quicklink: |url| {
                open_builtin_with(&settings, ManagedBuiltinFeature::Quicklinks, || {
                    crate::browser_open::open_url(url)
                })
            },
            copy_builtin: |text: &str| app.clipboard().write_text(text.to_owned()).map_err(|_| ()),
            copy_plugin: |plugin_id: &str,
                          command_id: Option<&str>,
                          generation: u64,
                          text: &str| {
                copy_plugin_text_with(
                    command_id,
                    |command_id| {
                        if !public_plugins.manager().is_ok_and(|manager| {
                            manager.can_copy_text(plugin_id, command_id, generation)
                        }) {
                            return Err(PluginCopyError::PermissionDenied);
                        }
                        app.clipboard()
                            .write_text(text.to_owned())
                            .map_err(|_| PluginCopyError::SideEffectFailed)
                    },
                    || {
                        plugins.copy_text(plugin_id, generation, || {
                            app.clipboard().write_text(text.to_owned()).map_err(|_| ())
                        })
                    },
                )
            },
            clear_and_hide: || clear_and_hide_reason(registry, &window, HideReason::LaunchHandoff),
            increment: |app_id: &str| settings.increment_use_count(app_id, &cache).map_err(|_| ()),
        },
    )
    .await
}

// A public command's denied authority can never be reinterpreted by the internal plugin manager.
fn copy_plugin_text_with<P, L>(
    command_id: Option<&str>,
    public_copy: P,
    legacy_copy: L,
) -> Result<(), PluginCopyError>
where
    P: FnOnce(&str) -> Result<(), PluginCopyError>,
    L: FnOnce() -> Result<(), PluginCopyError>,
{
    match command_id {
        Some(command_id) => public_copy(command_id),
        None => legacy_copy(),
    }
}

struct ClipboardExecution<R, A, F, W, Q, B, P, H, S> {
    resolve: R,
    execute: A,
    execute_file: F,
    open_web_search: W,
    open_quicklink: Q,
    copy_builtin: B,
    copy_plugin: P,
    clear_and_hide: H,
    increment: S,
}

async fn execute_result_with_clipboard<R, A, F, Fut, W, Q, B, P, H, S>(
    ids: (&str, &str),
    execution: ClipboardExecution<R, A, F, W, Q, B, P, H, S>,
) -> Result<ExecuteOutcome, CommandError>
where
    R: FnOnce(&str, &str) -> Result<ResultAction, RegistryError>,
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    F: FnOnce(FileExecutionAction) -> Fut,
    Fut: Future<Output = Result<FileExecutionOutcome, CommandError>>,
    W: FnOnce(WebSearchEngine, &str) -> Result<(), ()>,
    Q: FnOnce(url::Url) -> Result<(), ()>,
    B: FnOnce(&str) -> Result<(), ()>,
    P: FnOnce(&str, Option<&str>, u64, &str) -> Result<(), PluginCopyError>,
    H: FnOnce() -> Result<(), CommandError>,
    S: FnOnce(&str) -> Result<(), ()>,
{
    let (request_id, result_id) = ids;
    let ClipboardExecution {
        resolve,
        execute,
        execute_file,
        open_web_search,
        open_quicklink,
        copy_builtin,
        copy_plugin,
        clear_and_hide,
        increment,
    } = execution;
    let action = resolve(request_id, result_id).map_err(|error| match error {
        RegistryError::StaleRequest => CommandError::stale_request(),
        RegistryError::UnknownResult => CommandError::unknown_result(),
    })?;
    if let ResultAction::OpenWebSearch { engine, query } = &action {
        return execute_web_search_with(*engine, query, open_web_search, clear_and_hide);
    }
    if let ResultAction::OpenQuicklink { url, .. } = &action {
        return execute_quicklink_with(url, open_quicklink, clear_and_hide);
    }
    if let ResultAction::CopyBuiltInText { text } = &action {
        copy_builtin(text).map_err(|_| CommandError::clipboard_write_failed())?;
        clear_and_hide()?;
        return Ok(ExecuteOutcome::TextCopied);
    }
    if let ResultAction::CopyText {
        command_id,
        plugin_id,
        generation,
        text,
    } = &action
    {
        copy_plugin(plugin_id, command_id.as_deref(), *generation, text).map_err(|error| {
            match error {
                PluginCopyError::PermissionDenied => CommandError::plugin_permission_denied(),
                PluginCopyError::SideEffectFailed => CommandError::clipboard_write_failed(),
            }
        })?;
        clear_and_hide()?;
        return Ok(ExecuteOutcome::TextCopied);
    }
    execute_resolved_result_with(
        ids,
        |_, _| Ok(action),
        execute,
        execute_file,
        clear_and_hide,
        increment,
    )
    .await
}

fn map_file_execution_error(error: FileExecutionError) -> CommandError {
    match error {
        FileExecutionError::SearchUnavailable => CommandError::search_unavailable(),
        FileExecutionError::Stale => CommandError::stale_request(),
        FileExecutionError::NotFound => CommandError::file_not_found(),
        FileExecutionError::OpenFailed => CommandError::file_open_failed(),
    }
}

fn execute_file_action_with<I, E>(
    action: FileExecutionAction,
    execute_indexed: I,
    execute_everything: E,
) -> Result<FileExecutionOutcome, FileExecutionError>
where
    I: FnOnce(OpenIndexedPath) -> Result<FileExecutionOutcome, FileExecutionError>,
    E: FnOnce(
        crate::file_search::EverythingPathAction,
    ) -> Result<FileExecutionOutcome, FileExecutionError>,
{
    match action {
        FileExecutionAction::Indexed(action) => execute_indexed(action),
        FileExecutionAction::Everything(action) => execute_everything(action),
    }
}

async fn execute_resolved_result_with<R, A, F, Fut, H, S>(
    ids: (&str, &str),
    resolve: R,
    execute_application: A,
    execute_file: F,
    clear_and_hide: H,
    increment: S,
) -> Result<ExecuteOutcome, CommandError>
where
    R: FnOnce(&str, &str) -> Result<ResultAction, RegistryError>,
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    F: FnOnce(FileExecutionAction) -> Fut,
    Fut: Future<Output = Result<FileExecutionOutcome, CommandError>>,
    H: FnOnce() -> Result<(), CommandError>,
    S: FnOnce(&str) -> Result<(), ()>,
{
    let action = resolve(ids.0, ids.1).map_err(|error| match error {
        RegistryError::StaleRequest => CommandError::stale_request(),
        RegistryError::UnknownResult => CommandError::unknown_result(),
    })?;
    match action {
        ResultAction::LaunchApplication { .. } => {
            execute_application_result_with(&action, execute_application, clear_and_hide, increment)
        }
        ResultAction::OpenFile(action) => {
            let outcome = execute_file(action).await?;
            let response = match outcome {
                FileExecutionOutcome::FileRevealRequested => ExecuteOutcome::FileRevealRequested,
                FileExecutionOutcome::FolderOpenRequested => ExecuteOutcome::FolderOpenRequested,
            };
            clear_and_hide()?;
            Ok(response)
        }
        ResultAction::CopyBuiltInText { .. }
        | ResultAction::OpenWebSearch { .. }
        | ResultAction::OpenQuicklink { .. }
        | ResultAction::CopyText { .. } => Err(CommandError::application_entry_unavailable()),
    }
}

#[cfg(test)]
fn execute_result_with<R, A, H, S>(
    ids: (&str, &str),
    resolve: R,
    execute: A,
    clear_and_hide: H,
    increment: S,
) -> Result<ExecuteOutcome, CommandError>
where
    R: FnOnce(&str, &str) -> Result<ResultAction, RegistryError>,
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    H: FnOnce() -> Result<(), CommandError>,
    S: FnOnce(&str) -> Result<(), ()>,
{
    let (request_id, result_id) = ids;
    let action = resolve(request_id, result_id).map_err(|error| match error {
        RegistryError::StaleRequest => CommandError::stale_request(),
        RegistryError::UnknownResult => CommandError::unknown_result(),
    })?;
    if matches!(action, ResultAction::OpenFile(_)) {
        return Err(CommandError::application_entry_unavailable());
    }
    if matches!(
        action,
        ResultAction::CopyBuiltInText { .. }
            | ResultAction::OpenWebSearch { .. }
            | ResultAction::OpenQuicklink { .. }
            | ResultAction::CopyText { .. }
    ) {
        return Err(CommandError::application_entry_unavailable());
    }
    execute_application_result_with(&action, execute, clear_and_hide, increment)
}

fn execute_application_result_with<A, H, S>(
    action: &ResultAction,
    execute: A,
    clear_and_hide: H,
    increment: S,
) -> Result<ExecuteOutcome, CommandError>
where
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    H: FnOnce() -> Result<(), CommandError>,
    S: FnOnce(&str) -> Result<(), ()>,
{
    let ResultAction::LaunchApplication { app_id, .. } = action else {
        return Err(CommandError::application_entry_unavailable());
    };
    let outcome = execute(action).map_err(|_| CommandError::application_entry_unavailable())?;
    let response = outcome_parts(outcome);

    let window_error = clear_and_hide().err();
    let settings_error = increment(app_id)
        .err()
        .map(|_| CommandError::settings_failed());

    settings_error.or(window_error).map_or(Ok(response), Err)
}

fn outcome_parts(outcome: apps::ApplicationActionOutcome) -> ExecuteOutcome {
    match outcome {
        apps::ApplicationActionOutcome::LaunchRequested => ExecuteOutcome::LaunchRequested,
        apps::ApplicationActionOutcome::ActivationRequested => ExecuteOutcome::ActivationRequested,
        apps::ApplicationActionOutcome::ActivationRefusedLaunchRequested => {
            ExecuteOutcome::ActivationRefusedLaunchRequested {
                message: ACTIVATION_REFUSED_MESSAGE,
            }
        }
    }
}

// This is the final action admission, after resolving the current result handle.
// The admission linearizes against settings commits; native shell IO must run
// outside the settings lock, and an already-admitted open may finish normally.
fn open_builtin_with(
    settings: &SettingsStore,
    feature: ManagedBuiltinFeature,
    open: impl FnOnce() -> Result<(), ()>,
) -> Result<(), ()> {
    settings.with_enabled_builtin(feature, || ())?;
    open()
}

fn execute_web_search_with<O, H>(
    engine: WebSearchEngine,
    query: &str,
    open: O,
    clear_and_hide: H,
) -> Result<ExecuteOutcome, CommandError>
where
    O: FnOnce(WebSearchEngine, &str) -> Result<(), ()>,
    H: FnOnce() -> Result<(), CommandError>,
{
    open(engine, query).map_err(|_| CommandError::web_search_failed())?;
    clear_and_hide()?;
    Ok(ExecuteOutcome::LaunchRequested)
}

fn execute_quicklink_with<O, H>(
    url: &str,
    open: O,
    clear_and_hide: H,
) -> Result<ExecuteOutcome, CommandError>
where
    O: FnOnce(url::Url) -> Result<(), ()>,
    H: FnOnce() -> Result<(), CommandError>,
{
    let url = url::Url::parse(url)
        .map_err(|_| CommandError::quicklink(QuicklinkErrorCode::OpenFailed))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(CommandError::quicklink(QuicklinkErrorCode::OpenFailed));
    }
    open(url).map_err(|_| CommandError::quicklink(QuicklinkErrorCode::OpenFailed))?;
    clear_and_hide()?;
    Ok(ExecuteOutcome::LaunchRequested)
}

#[tauri::command]
pub(crate) fn hide_launcher(
    webview: tauri::Webview,
    registries: State<'_, ResultRegistries>,
) -> Result<(), CommandError> {
    require_main_label(webview.label())?;
    clear_and_hide_window(
        registries.main(),
        &webview.window(),
        HideReason::ExplicitReturn,
    )
}

pub(crate) fn clear_and_hide(
    registry: &ResultRegistry,
    window: &WebviewWindow,
) -> Result<(), CommandError> {
    clear_and_hide_reason(registry, window, HideReason::Other)
}

pub(crate) fn clear_and_hide_reason(
    registry: &ResultRegistry,
    window: &WebviewWindow,
    reason: HideReason,
) -> Result<(), CommandError> {
    clear_and_hide_window(registry, &window.as_ref().window(), reason)
}

pub(crate) fn clear_and_hide_window(
    registry: &ResultRegistry,
    window: &tauri::Window,
    reason: HideReason,
) -> Result<(), CommandError> {
    let settings = window.state::<SettingsStore>();
    window
        .state::<Arc<LifecycleCoordinator>>()
        .cancel_selection_capture();
    let lifecycle = Arc::clone(window.state::<Arc<LifecycleCoordinator>>().inner());
    if lifecycle.settings_pinned() && reason != HideReason::ExplicitReturn {
        return Ok(());
    }
    let app = window.app_handle().clone();
    let panel_controller = Arc::clone(window.state::<Arc<PluginPanelController>>().inner());
    let result = clear_and_hide_with(
        || {
            if !window.is_visible().map_err(|_| ())? {
                return Err(());
            }
            window
                .outer_position()
                .map(|position| WindowPosition {
                    x: position.x,
                    y: position.y,
                })
                .map_err(|_| ())
        },
        || {
            registry.hide_and_clear();
            if let Some(public) = app.try_state::<Arc<PublicPluginService>>() {
                public.cleanup_retired_result_actions(registry);
            }
        },
        || {
            window.hide().map_err(|error| {
                eprintln!(
                    "[launcher-hide] window.hide failed label={} error={error}",
                    window.label()
                );
            })
        },
        || {
            plugin_panel::trace_panel_focus(&app, panel_controller.as_ref(), "host-hide", || {
                format!("reason={reason:?}")
            });
            plugin_panel::teardown(&app, panel_controller.as_ref(), None);
            panel_controller.host_hidden();
            plugin_panel::trace_panel_focus(&app, panel_controller.as_ref(), "host-hidden", || {
                format!("reason={reason:?}")
            });
        },
        |position| settings.set_window_position(position).map_err(|_| ()),
    );
    if result.is_ok() {
        let _ = window.emit("launcher://hidden", ());
        if reason == HideReason::ExplicitReturn {
            lifecycle.restore_foreground_after_explicit_return();
        }
    }
    result
}

fn clear_and_hide_with<P, C, H, T, S>(
    read_position: P,
    clear: C,
    hide: H,
    teardown_panel: T,
    save_position: S,
) -> Result<(), CommandError>
where
    P: FnOnce() -> Result<WindowPosition, ()>,
    C: FnOnce(),
    H: FnOnce() -> Result<(), ()>,
    T: FnOnce(),
    S: FnOnce(WindowPosition) -> Result<(), ()>,
{
    let position = read_position();
    clear();
    hide().map_err(|_| CommandError::window_failed())?;
    teardown_panel();
    if let Ok(position) = position {
        let _ = save_position(position);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn selection_stable_route_preserves_exact_argument_whitespace() {
        assert_eq!(
            super::stable_command_argument("/translate   hello\n ", true),
            "  hello\n "
        );
        assert_eq!(
            super::stable_command_argument("/translate   hello\n ", false),
            "hello"
        );
    }

    #[test]
    fn screenshot_toggle_save_failure_restores_registration_and_enabled_state() {
        let dir = TestDir::new();
        let settings = settings_store(&dir);
        settings
            .set_screenshot_hotkey(Some("Ctrl+P".into()))
            .unwrap();
        let keys = crate::screenshot::hotkey::ScreenshotHotkeys::default();
        let chord = crate::hotkey::command_chord("Ctrl+P").unwrap();
        keys.edit(Some(chord), &mut |_| Ok(()), &mut |_| Ok(()), || Ok(()))
            .unwrap();
        fs::remove_file(dir.path().join("settings.json")).unwrap();
        fs::create_dir(dir.path().join("settings.json")).unwrap();
        let registered = Cell::new(true);
        assert!(super::set_builtin_enabled_with(
            &settings,
            &keys,
            ManagedBuiltinFeature::Screenshot,
            false,
            &mut |_| {
                registered.set(true);
                Ok(())
            },
            &mut |_| {
                registered.set(false);
                Ok(())
            }
        )
        .is_err());
        assert!(registered.get());
        assert!(keys.candidate(chord.id()).is_some());
        assert!(settings
            .snapshot()
            .builtin_feature_enabled(ManagedBuiltinFeature::Screenshot));
        assert_eq!(
            settings.snapshot().screenshot_hotkey.as_deref(),
            Some("Ctrl+P")
        );
    }

    #[test]
    fn editing_screenshot_hotkey_while_disabled_only_saves_configuration() {
        let dir = TestDir::new();
        let settings = settings_store(&dir);
        settings
            .set_builtin_feature_enabled(ManagedBuiltinFeature::Screenshot, false)
            .unwrap();
        let keys = crate::screenshot::hotkey::ScreenshotHotkeys::default();
        super::edit_screenshot_hotkey_with(
            &settings,
            &keys,
            Some(crate::hotkey::command_chord("Ctrl+P").unwrap()),
            &mut |_| panic!("disabled hotkey must not register"),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            settings.snapshot().screenshot_hotkey.as_deref(),
            Some(
                crate::hotkey::command_chord("Ctrl+P")
                    .unwrap()
                    .to_string()
                    .as_str()
            )
        );
        assert!(keys.registrations().is_empty());
        super::edit_screenshot_hotkey_with(
            &settings,
            &keys,
            None,
            &mut |_| panic!("disabled hotkey must not register"),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert!(settings.snapshot().screenshot_hotkey.is_none());
    }

    #[test]
    fn screenshot_toggle_restores_saved_chord_and_rolls_back_conflicts() {
        let dir = TestDir::new();
        let settings = settings_store(&dir);
        settings
            .set_screenshot_hotkey(Some("Ctrl+P".into()))
            .unwrap();
        let keys = crate::screenshot::hotkey::ScreenshotHotkeys::default();
        let chord = crate::hotkey::command_chord("Ctrl+P").unwrap();
        keys.edit(Some(chord), &mut |_| Ok(()), &mut |_| Ok(()), || Ok(()))
            .unwrap();
        super::set_builtin_enabled_with(
            &settings,
            &keys,
            ManagedBuiltinFeature::Screenshot,
            false,
            &mut |_| panic!("disable must not register"),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert!(keys.registrations().is_empty());
        assert_eq!(
            settings.snapshot().screenshot_hotkey.as_deref(),
            Some("Ctrl+P")
        );
        assert!(super::set_builtin_enabled_with(
            &settings,
            &keys,
            ManagedBuiltinFeature::Screenshot,
            true,
            &mut |_| Err(()),
            &mut |_| Ok(())
        )
        .is_err());
        assert!(!settings
            .snapshot()
            .builtin_feature_enabled(ManagedBuiltinFeature::Screenshot));
        assert_eq!(
            settings.snapshot().screenshot_hotkey.as_deref(),
            Some("Ctrl+P")
        );
        super::set_builtin_enabled_with(
            &settings,
            &keys,
            ManagedBuiltinFeature::Screenshot,
            true,
            &mut |_| Ok(()),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert!(keys.candidate(chord.id()).is_some());
    }

    #[test]
    fn disabled_builtins_are_absent_from_all_catalog_routes() {
        for query in [
            "",
            "/",
            "/screenshot",
            "screenshot",
            "/quicklinks",
            "/qu",
            "/web-search",
            "/web-search hello",
            "ordinary query",
            "/jd hello",
        ] {
            let registry = ready_registry("disabled");
            let quicklinks = vec![QuicklinkView {
                id: "1".into(),
                name: "JD".into(),
                command: "jd".into(),
                template: "https://example.com/?q={query}".into(),
                icon_data_url: None,
                created_at: "1".into(),
                updated_at: "1".into(),
            }];
            let response = search_apps_with_catalog(
                &registry,
                CatalogSearchRequest {
                    query,
                    invocation_id: "disabled",
                    query_sequence: 1,
                    submit: true,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &BTreeSet::new(),
                    disabled_builtin_features: &BTreeSet::from(ManagedBuiltinFeature::ALL),
                    quicklinks: &quicklinks,
                },
                |_| Ok(Vec::new()),
                Vec::new,
                |_| {},
            )
            .unwrap();
            assert!(
                response
                    .items
                    .iter()
                    .all(|item| item.icon_kind == Some(crate::model::ResultIconKind::Find)),
                "disabled entries for {query}: {:?}",
                response.items
            );
            assert!(response.auto_execute_result_id.is_none());
            assert!(response.command_hint.is_none());
        }
    }

    #[test]
    fn screenshot_directory_authorization_precedes_picker_and_mutation_and_cancel_does_not_save() {
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"directory": 42}),
            serde_json::json!({"directory": null, "extra": true}),
        ] {
            assert!(
                serde_json::from_value::<super::ScreenshotSaveDirectoryInput>(invalid).is_err()
            );
        }
        assert!(
            serde_json::from_value::<super::ScreenshotSaveDirectoryInput>(
                serde_json::json!({"directory": null})
            )
            .is_ok()
        );
        for label in ["screenshot", "find", "plugin-runtime", "forged"] {
            assert!(super::select_screenshot_directory_with(
                label,
                || panic!("unauthorized picker"),
                |_| -> Result<(), super::CommandError> { panic!("unauthorized save") }
            )
            .is_err());
            assert!(
                super::screenshot_directory_access(label, || panic!("unauthorized mutation"))
                    .is_err()
            );
        }
        assert_eq!(
            super::select_screenshot_directory_with(
                "main",
                || None,
                |_| -> Result<(), super::CommandError> { panic!("cancel must not persist") }
            )
            .unwrap(),
            None
        );
        let picked = PathBuf::from(r"C:\Screenshots");
        assert_eq!(
            super::select_screenshot_directory_with("main", || Some(picked.clone()), Ok).unwrap(),
            Some(picked)
        );
    }
    #[test]
    fn screenshot_hotkey_input_is_exact_and_main_guard_runs_before_protected_access() {
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"hotkey": 1}),
            serde_json::json!({"hotkey": null, "pluginId": "forged"}),
        ] {
            assert!(serde_json::from_value::<super::ScreenshotHotkeyInput>(invalid).is_err());
        }
        assert!(serde_json::from_value::<super::ScreenshotHotkeyInput>(
            serde_json::json!({"hotkey": null})
        )
        .is_ok());
        for label in ["screenshot", "find", "plugin-runtime", "forged"] {
            let accessed = std::cell::Cell::new(false);
            assert!(super::command_hotkey_access(label, || accessed.set(true)).is_err());
            assert!(!accessed.get());
        }
    }
    use std::{
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
    };

    use super::{
        builtin_feature_views, clear_and_hide_with, execute_file_action_with,
        execute_resolved_result_with, execute_result_with, focus_live_panel_host_content,
        host_input_focus_failure, load_settings_core, load_settings_ready_with,
        map_everything_search_error, map_file_preview_worker_result, map_save_worker_result,
        map_theme_preference_worker_result, panel_route_matches_identity,
        parse_panel_session_epoch, parse_timer_session_generation,
        parse_window_storage_session_generation, plugin_discovery_prefix, prepare_file_query,
        prepare_hotkey_save, prepare_settings_save, public_plugin_prompt,
        public_plugin_search_decision, publish_everything_search,
        publish_public_command_suggestions, publish_quick_evaluation, require_find_label,
        require_main_label, resolve_invocation_theme, save_hotkey_with, save_settings_core,
        save_settings_with, save_settings_worker_with, search_apps_with, search_apps_with_catalog,
        search_apps_with_catalog_and_currency, search_apps_with_clocks,
        search_apps_with_local_date, search_files_with, select_public_plugin_source_with,
        set_file_preview_preference_with, start_host_input_focus, submitted_main_result_command,
        with_managed_builtin, CatalogSearchRequest, CommandError, CompletionOriginInput,
        CompletionOriginPhase, ExecuteOutcome, FilePreviewPreferenceUpdate, FindReadyOutcome,
        HostInputFocusStart, HotkeySettingsUpdate, PreparedFileQuery, PublicPluginSearchDecision,
        ThemePreferenceUpdate, UserSettingsUpdate, EMPTY_DISABLED_BUILTINS,
    };
    use crate::{
        apps::{Application, ApplicationActionOutcome, ApplicationLaunchTarget},
        file_index::{
            FileExecutionOutcome, FileResultKind, IndexedKind, OpenIndexedPath, VolumeIdentity,
        },
        file_search::{
            everything::EverythingSearchError, windows::path_auth::AuthenticatedPathIdentity,
            EverythingPathAction, FileCategory, FileExecutionAction, FileIndexStatus, FilePathKind,
            PublishedFileBatch, PublishedFileDraft,
        },
        hotkey::{DoubleTapModifier, HotkeyKind},
        lifecycle::LifecycleCoordinator,
        plugin_panel::{PanelOwner, PanelSessionIdentity, PluginPanelController},
        public_plugins::{
            PluginInvocationTheme, PublicActivationMode, PublicCommandSuggestion, PublicOutputMode,
            PublicPluginRoute, TimerError, WindowStorageError,
        },
        quick_evaluation::currency::{service::CurrencyResolveError, CurrencyOutput},
        quicklinks::QuicklinkView,
        result_registry::{QueryDomain, RegistryError, ResultAction, ResultRegistry},
        settings::{
            BuiltinFeature, ManagedBuiltinFeature, Settings, SettingsStore, SettingsUpdate,
            ThemePreference, WebSearchEngine,
        },
    };
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn command_hotkey_ipcs_reject_non_main_before_protected_access_and_strict_input() {
        use std::cell::Cell;
        let accesses = Cell::new(0);
        for label in [
            "find",
            "plugin-runtime-com.example.notes-1",
            "plugin-panel",
            "plugin-window",
            "",
        ] {
            for _command in [
                "set_public_plugin_command_hotkey",
                "resolve_public_plugin_command_entry",
            ] {
                assert!(
                    super::command_hotkey_access(label, || accesses.set(accesses.get() + 1))
                        .is_err()
                );
            }
        }
        assert_eq!(accesses.get(), 0);
        super::command_hotkey_access("main", || accesses.set(accesses.get() + 1)).unwrap();
        assert_eq!(accesses.get(), 1);
        let input = serde_json::json!({ "pluginId": "com.example.notes", "commandId": "default", "hotkey": null });
        assert!(serde_json::from_value::<super::CommandHotkeyInput>(input.clone()).is_ok());
        for invalid in [
            serde_json::json!({ "pluginId": "com.example.notes", "commandId": "default" }),
            serde_json::json!({ "pluginId": "com.example.notes", "commandId": "default", "hotkey": 1 }),
            serde_json::json!({ "pluginId": "com.example.notes", "commandId": "default", "hotkey": null, "extra": true }),
        ] {
            assert!(serde_json::from_value::<super::CommandHotkeyInput>(invalid).is_err());
        }
        let source = include_str!("commands.rs");
        for name in [
            "set_public_plugin_command_hotkey",
            "resolve_public_plugin_command_entry",
        ] {
            let body = source
                .split(&format!("fn {name}("))
                .nth(1)
                .unwrap()
                .split("#[tauri::command]")
                .next()
                .unwrap();
            assert!(
                body.find("command_hotkey_access(window.label()").unwrap()
                    < body.find("app.state::<").unwrap()
            );
        }
    }

    #[test]
    fn save_plugin_settings_tears_down_only_after_real_effects_including_partial_failure() {
        use std::time::Instant;
        for case in [
            "empty",
            "same",
            "ordinary",
            "secret",
            "mixed",
            "second-failure",
            "invalid-dto",
        ] {
            crate::public_plugins::with_settings_service_for_test(|service, manager| {
                let now = Instant::now();
                let first = service
                    .schedule_command(
                        manager.route("/demo first").unwrap().unwrap(),
                        1,
                        "/demo first".into(),
                        now,
                    )
                    .unwrap();
                let running = first.dispatch.unwrap();
                let waiting = service
                    .schedule_command(
                        manager.route("/demo waiting").unwrap().unwrap(),
                        2,
                        "/demo waiting".into(),
                        now,
                    )
                    .unwrap();
                let settings = match case {
                    "same" => BTreeMap::from([("flag".into(), serde_json::json!(false))]),
                    "ordinary" | "mixed" => {
                        BTreeMap::from([("flag".into(), serde_json::json!(true))])
                    }
                    "invalid-dto" => {
                        BTreeMap::from([("flag".into(), serde_json::json!("invalid"))])
                    }
                    _ => BTreeMap::new(),
                };
                let secrets = match case {
                    "secret" | "mixed" | "invalid-dto" => {
                        BTreeMap::from([("first".into(), Some("synthetic-first".into()))])
                    }
                    "second-failure" => BTreeMap::from([
                        ("first".into(), Some("synthetic-first".into())),
                        ("second".into(), Some("x".repeat(65537))),
                    ]),
                    _ => BTreeMap::new(),
                };
                let teardown = Cell::new(0);
                let result = super::save_plugin_settings_with(
                    service,
                    super::SavePublicPluginSettingsInput {
                        plugin_id: "com.uipilot.demo".into(),
                        settings,
                        secrets,
                    },
                    &ready_registry("settings"),
                    &crate::plugins::PluginManager::new(),
                    || {
                        teardown.set(teardown.get() + 1);
                        // Settlement is complete before the command tears down its window.
                        assert_eq!(waiting.receiver.try_recv(), Ok(None));
                    },
                );
                let changed = matches!(case, "ordinary" | "secret" | "mixed" | "second-failure");
                assert_eq!(teardown.get(), usize::from(changed), "{case}");
                assert_eq!(
                    result.is_err(),
                    matches!(case, "second-failure" | "invalid-dto"),
                    "{case}"
                );
                let next = service
                    .schedule_command(
                        manager.route("/demo next").unwrap().unwrap(),
                        3,
                        "/demo next".into(),
                        now,
                    )
                    .unwrap();
                assert!(next.dispatch.is_none(), "{case}");
                let completion = manager.scheduler().complete(&running.context, now).unwrap();
                assert!(!completion.accepted);
                assert_eq!(
                    completion
                        .next
                        .unwrap()
                        .candidate
                        .owner
                        .command()
                        .unwrap()
                        .submission_token,
                    next.token
                );
            });
        }
    }

    #[test]
    fn find_thumbnail_command_is_find_only_and_opaque_id_based() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let marker = ["pub(crate) async fn load_find_", "thumbnail("].concat();
        let command = source
            .split(&marker)
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("find thumbnail command is missing");
        assert!(command.contains("require_find_window(&window)?"));
        assert!(command.contains("request_id: String"));
        assert!(command.contains("result_id: String"));
        assert!(!command.contains("full_path: String"));
        assert!(command.contains("resolve("));
        assert!(command.contains("spawn_blocking"));
    }

    #[test]
    fn plugin_network_command_rejects_non_runtime_and_mismatched_callers() {
        let input = crate::public_plugins::PluginNetworkCommandInput {
            context: crate::public_plugins::PluginRequestContext {
                plugin_id: "com.example.network".into(),
                plugin_generation: 7,
                request_id: "public-request-0000000000000001".into(),
            },
            request: crate::public_plugins::PluginNetworkRequest {
                url: "https://api.example.com/".into(),
                method: crate::public_plugins::PluginNetworkRequestMethod::Get,
                headers: None,
                body: None,
                credential: None,
            },
        };
        for label in [
            "main",
            "find",
            "plugin-shell-com.example.network",
            "plugin-content-com.example.network",
            "plugin-panel-com.example.network",
            "plugin-window-com.example.network",
            "plugin-runtime-not-a-valid-label",
        ] {
            assert_eq!(
                super::validate_plugin_network_caller(label, &input),
                Err(crate::public_plugins::PluginNetworkErrorCode::ExpiredRequest)
            );
        }
        let label = crate::public_plugins::runtime_label("com.example.network", 7).unwrap();
        assert!(super::validate_plugin_network_caller(&label, &input).is_ok());
        let wrong_generation =
            crate::public_plugins::runtime_label("com.example.network", 8).unwrap();
        assert_eq!(
            super::validate_plugin_network_caller(&wrong_generation, &input),
            Err(crate::public_plugins::PluginNetworkErrorCode::ExpiredRequest)
        );
    }

    #[test]
    fn plugin_network_command_checks_caller_before_manager_access() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let start = production
            .find("pub(crate) async fn plugin_network_request(")
            .expect("network command is missing");
        let command = &production[start..];
        let guard = command
            .find("validate_plugin_network_caller(window.label(), &input)")
            .unwrap();
        let manager = command.find("service.manager()").unwrap();
        assert!(guard < manager);
    }

    #[test]
    fn plugin_network_management_command_is_main_only_and_never_accepts_hosts() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let start = production
            .find("pub(crate) fn set_public_plugin_network_access(")
            .expect("network access management command is missing");
        let tail = &production[start..];
        let end = tail[1..]
            .find("#[tauri::command]")
            .map_or(tail.len(), |index| index + 1);
        let command = &tail[..end];
        let first = command[command.find('{').unwrap() + 1..].trim_start();
        assert!(first.starts_with("require_main_window(&window)?;"));
        assert!(command.contains("service.set_network_access_outcome("));
        assert!(command.contains("expected_inventory_revision.as_deref()"));
        assert!(!command.contains("https_hosts"));
        assert!(!command.contains("httpsHosts"));
    }

    #[test]
    fn permission_access_command_is_main_only_before_state_and_requires_all_fields() {
        for label in [
            "plugin-runtime-com.example.timer-1",
            "plugin-window-com.example.timer",
            "find",
        ] {
            assert!(super::require_main_label(label).is_err());
        }
        let source = include_str!("commands.rs");
        let start = source
            .find("pub(crate) fn set_public_plugin_permission_access(")
            .unwrap();
        let tail = &source[start..];
        let command = &tail[..tail.find("\n#[tauri::command]").unwrap()];
        assert!(
            command.find("require_main_window(&window)?;").unwrap()
                < command.find("app.state::<").unwrap()
        );
        for required in [
            "plugin_id: String",
            "permission: PublicPermission",
            "granted: bool",
            "expected_inventory_revision: String",
        ] {
            assert!(command.contains(required));
        }
        assert!(command.contains("Result<(), CommandError>"));
        assert!(command.contains("public-plugin-permissions-changed"));
        assert!(command.contains("failed_closed"));
        assert!(command.contains("reset_plugin_panel_checked("));
        assert!(command.contains("plugin_window::teardown_current_checked("));
    }

    #[test]
    fn find_ready_outcome_uses_camel_case_fields() {
        assert_eq!(
            serde_json::to_value(FindReadyOutcome::Ready {
                initialization_token: "find-initialization-1".into(),
            })
            .unwrap(),
            serde_json::json!({
                "status": "ready",
                "initializationToken": "find-initialization-1"
            })
        );
    }

    #[test]
    fn panel_host_key_enqueue_result_uses_camel_case_fields() {
        assert_eq!(
            serde_json::to_value(super::PluginPanelHostKeyEnqueueResult::Enqueued {
                route_sequence: "1".into(),
            })
            .unwrap(),
            serde_json::json!({
                "outcome": "enqueued",
                "routeSequence": "1"
            })
        );
    }

    #[test]
    fn panel_hide_admit_result_matches_bootstrap_contract() {
        assert_eq!(
            serde_json::to_value(super::PanelHideAdmitResult::Admitted {
                hide_ticket_id: "42".into(),
            })
            .unwrap(),
            serde_json::json!({
                "outcome": "admitted",
                "hideTicketId": "42"
            })
        );
        assert_eq!(
            serde_json::to_value(super::PanelHideAdmitResult::Noop).unwrap(),
            serde_json::json!({ "outcome": "noop" })
        );
    }

    #[test]
    fn public_plugin_theme_uses_the_effective_system_scheme() {
        for (preference, system_theme, expected) in [
            (
                ThemePreference::System,
                Some(tauri::Theme::Dark),
                PluginInvocationTheme::Dark,
            ),
            (
                ThemePreference::System,
                Some(tauri::Theme::Light),
                PluginInvocationTheme::Light,
            ),
            (
                ThemePreference::Dark,
                Some(tauri::Theme::Light),
                PluginInvocationTheme::Dark,
            ),
            (
                ThemePreference::Light,
                Some(tauri::Theme::Dark),
                PluginInvocationTheme::Light,
            ),
            (ThemePreference::System, None, PluginInvocationTheme::Light),
        ] {
            assert_eq!(resolve_invocation_theme(preference, system_theme), expected);
        }
    }

    #[test]
    fn public_plugin_command_discovery_publishes_safe_completions_and_hint() {
        for (query, expected) in [
            ("/", None),
            ("/a", Some("a")),
            ("/alpha-window", Some("alpha-window")),
            ("/alpha-window ", None),
            ("/alpha-window body", None),
            ("alpha", None),
        ] {
            assert_eq!(plugin_discovery_prefix(query), expected);
        }

        let registry = ResultRegistry::default();
        registry.on_show("plugin-discovery".into());
        let response = publish_public_command_suggestions(
            &registry,
            "plugin-discovery",
            1,
            vec![
                PublicCommandSuggestion {
                    command_id: "default".into(),
                    plugin_id: "com.example.alpha-return".into(),
                    effective_name: "alpha-return".into(),
                    display_name: "Public Plugin Alpha Return".into(),
                    summary: Some("返回示例文本到主界面".into()),
                    icon_url: Some(
                        "uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.alpha/1/icon.png"
                            .into(),
                    ),
                    favorite: false,
                    input_required: false,
                    output_mode: PublicOutputMode::MainResult,
                },
                PublicCommandSuggestion {
                    command_id: "default".into(),
                    plugin_id: "com.example.alpha-window".into(),
                    effective_name: "alpha-window".into(),
                    display_name: "Public Plugin Alpha Window".into(),
                    summary: None,
                    icon_url: None,
                    favorite: false,
                    input_required: false,
                    output_mode: PublicOutputMode::Window,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            response
                .items
                .iter()
                .map(|item| (
                    item.title.as_str(),
                    item.subtitle.as_deref(),
                    item.has_default_action,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "/alpha-return",
                    Some("Public Plugin Alpha Return · default · 返回示例文本到主界面"),
                    false,
                ),
                (
                    "/alpha-window",
                    Some("Public Plugin Alpha Window · default"),
                    false,
                ),
            ]
        );
        assert!(matches!(
            &response.items[0].activation,
            crate::model::LauncherResultActivation::MainResultActivation {
                command_id,
                plugin_id,
                command_label,
                initial_argument,
                favorite: false,
            } if command_id == "default" && plugin_id == "com.example.alpha-return" &&
                command_label == "alpha-return" && initial_argument.is_empty()
        ));
        assert!(matches!(
            &response.items[1].activation,
            crate::model::LauncherResultActivation::WindowActivation {
                command_id: _,
                plugin_id,
                command_label,
                initial_argument,
                favorite: false,
            } if plugin_id == "com.example.alpha-window" &&
                command_label == "alpha-window" && initial_argument.is_empty()
        ));
        assert_eq!(
            registry.resolve(&response.request_id, &response.items[0].result_id),
            Err(RegistryError::UnknownResult)
        );
        assert_eq!(response.command_hint, None);
        assert_eq!(
            response.items[0].plugin_icon_url.as_deref(),
            Some(
                "uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.alpha/1/icon.png"
            )
        );
        assert_eq!(response.items[1].plugin_icon_url, None);

        let hint = public_plugin_prompt(
            &registry,
            "plugin-discovery",
            2,
            Some("请输入信息回车".into()),
            None,
        )
        .unwrap();
        assert!(hint.items.is_empty());
        assert_eq!(hint.command_hint.as_deref(), Some("请输入信息回车"));
    }

    #[test]
    fn slash_prefix_query_publishes_matching_builtin_commands() {
        let registry = ResultRegistry::default();
        registry.on_show("builtin-prefix".into());
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/qu",
                invocation_id: "builtin-prefix",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| Ok(Vec::new()),
            || panic!("builtin prefix must not read applications"),
            |_| {},
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["/quicklinks"]
        );
        assert_eq!(
            response.items[0].activation,
            crate::model::LauncherResultActivation::OpenQuicklinks
        );
    }

    #[test]
    fn screenshot_catalog_routes_and_favorites_are_host_owned() {
        use crate::model::{LauncherResultActivation, ResultFavorite, ResultIconKind};
        for query in ["/screenshot", "/scr", "截图", "screenshot", ""] {
            let registry = ResultRegistry::default();
            registry.on_show("screenshot-query".into());
            let favorites = BTreeSet::from([BuiltinFeature::Screenshot]);
            let response = search_apps_with_catalog(
                &registry,
                CatalogSearchRequest {
                    query,
                    invocation_id: "screenshot-query",
                    query_sequence: 1,
                    submit: false,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &favorites,
                    disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                    quicklinks: &[],
                },
                |_| Ok(Vec::new()),
                Vec::new,
                |_| {},
            )
            .unwrap();
            let rows = response
                .items
                .iter()
                .filter(|item| item.icon_kind == Some(ResultIconKind::Screenshot))
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 1, "query {query}");
            assert_eq!(rows[0].activation, LauncherResultActivation::OpenScreenshot);
            assert_eq!(
                rows[0].favorite,
                Some(ResultFavorite::builtin(BuiltinFeature::Screenshot, true))
            );
            assert!(rows[0].has_default_action);
            assert!(response.auto_execute_result_id.is_none());
        }
    }

    #[test]
    fn public_plugin_prompt_and_dispatch_decisions_preserve_activation_modes() {
        let mut route = PublicPluginRoute {
            result_publication: None,
            command_id: "default".into(),
            plugin_id: "com.example.demo".into(),
            generation: 1,
            activation_id: 1,
            admission_epoch: 1,
            runtime_recovery_needed: false,
            runtime_label: "plugin-runtime-com.example.demo-g1".into(),
            activation_mode: PublicActivationMode::Submit,
            output_mode: PublicOutputMode::MainResult,
            command_label: "demo".into(),
            input: String::new(),
            input_required: true,
            input_placeholder: Some("请输入信息回车".into()),
            window_entry: None,
            panel_entry: None,
            host_keys: Vec::new(),
            host_key_focus: crate::public_plugins::PanelHostKeyFocus::Content,
            icon_url: None,
        };
        assert!(submitted_main_result_command(&route, false).is_none());
        let submitted = submitted_main_result_command(&route, true).unwrap();
        assert_eq!(submitted.plugin_id, "com.example.demo");
        assert_eq!(submitted.command_label, "demo");
        assert_eq!(submitted.argument, "");
        assert_eq!(
            public_plugin_search_decision(&route, false, None),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );
        assert_eq!(
            public_plugin_search_decision(&route, true, None),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );

        route.output_mode = PublicOutputMode::Window;
        assert_eq!(
            public_plugin_search_decision(
                &route,
                true,
                Some(&CompletionOriginInput {
                    phase: CompletionOriginPhase::Commit,
                    plugin_id: "com.example.demo".into(),
                    command_id: None,
                }),
            ),
            Ok(PublicPluginSearchDecision::Dispatch)
        );
        route.output_mode = PublicOutputMode::MainResult;

        route.input = "body".into();
        assert_eq!(
            public_plugin_search_decision(&route, false, None),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );
        assert_eq!(
            public_plugin_search_decision(&route, true, None),
            Ok(PublicPluginSearchDecision::Dispatch)
        );

        route.activation_mode = PublicActivationMode::Live;
        route.input.clear();
        assert_eq!(
            public_plugin_search_decision(&route, false, None),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );
        route.input = "body".into();
        assert_eq!(
            public_plugin_search_decision(&route, false, None),
            Ok(PublicPluginSearchDecision::Dispatch)
        );
        assert_eq!(
            public_plugin_search_decision(&route, true, None),
            Ok(PublicPluginSearchDecision::Ignore)
        );
    }

    #[test]
    fn favorite_completion_origin_preview_commit_and_rejection_matrix() {
        let mut route = PublicPluginRoute {
            result_publication: None,
            command_id: "default".into(),
            plugin_id: "com.example.demo".into(),
            generation: 1,
            activation_id: 1,
            admission_epoch: 1,
            runtime_recovery_needed: false,
            runtime_label: "plugin-runtime-com.example.demo-g1".into(),
            activation_mode: PublicActivationMode::Live,
            output_mode: PublicOutputMode::MainResult,
            command_label: "demo".into(),
            input: "body".into(),
            input_required: false,
            input_placeholder: Some("请输入信息回车".into()),
            window_entry: None,
            panel_entry: None,
            host_keys: Vec::new(),
            host_key_focus: crate::public_plugins::PanelHostKeyFocus::Content,
            icon_url: None,
        };
        let preview = CompletionOriginInput {
            phase: CompletionOriginPhase::Preview,
            plugin_id: route.plugin_id.clone(),
            command_id: None,
        };
        let commit = CompletionOriginInput {
            phase: CompletionOriginPhase::Commit,
            plugin_id: route.plugin_id.clone(),
            command_id: None,
        };

        assert_eq!(
            public_plugin_search_decision(&route, false, Some(&preview)),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );
        assert_eq!(
            public_plugin_search_decision(&route, true, Some(&commit)),
            Ok(PublicPluginSearchDecision::Dispatch)
        );
        route.activation_mode = PublicActivationMode::Submit;
        assert_eq!(
            public_plugin_search_decision(&route, true, Some(&commit)),
            Ok(PublicPluginSearchDecision::Dispatch)
        );

        for (submit, origin) in [(true, &preview), (false, &commit)] {
            assert_eq!(
                public_plugin_search_decision(&route, submit, Some(origin)),
                Err(())
            );
        }
        let mismatch = CompletionOriginInput {
            phase: CompletionOriginPhase::Commit,
            plugin_id: "com.example.other".into(),
            command_id: None,
        };
        assert_eq!(
            public_plugin_search_decision(&route, true, Some(&mismatch)),
            Err(())
        );
        assert!(
            serde_json::from_value::<CompletionOriginInput>(serde_json::json!({
                "phase": "preview",
                "pluginId": "com.example.demo",
                "extra": true
            }))
            .is_err()
        );
    }

    fn command_suggestion(name: &str) -> PublicCommandSuggestion {
        PublicCommandSuggestion {
            command_id: "default".into(),
            plugin_id: format!("com.example.{name}"),
            effective_name: name.into(),
            display_name: format!("Plugin {name}"),
            summary: Some(format!("Use {name}")),
            icon_url: None,
            favorite: false,
            input_required: false,
            output_mode: PublicOutputMode::Window,
        }
    }

    fn quicklink_view(command: &str) -> QuicklinkView {
        QuicklinkView {
            id: "1".into(),
            name: "京东搜索".into(),
            command: command.into(),
            template: "https://search.jd.com/Search?keyword={Query}".into(),
            icon_data_url: Some("data:image/png;base64,iVBORw==".into()),
            created_at: "1".into(),
            updated_at: "1".into(),
        }
    }

    #[test]
    fn quicklink_and_builtin_alias_conflicts_are_rejected_in_both_creation_orders() {
        let dir = TestDir::new();
        let store = super::QuicklinksStore::new(dir.path().to_path_buf());
        let input = || super::QuicklinkSaveInput {
            id: None,
            name: "Shared".into(),
            command: "shared".into(),
            template: "https://example.com/?q={Query}".into(),
            icon_token: None,
        };
        let mut settings = Settings::default();
        settings
            .builtin_names
            .insert(ManagedBuiltinFeature::Screenshot, "shared".into());
        let reserved = super::quicklink_external_names(&settings, vec!["public".into()]);
        assert_eq!(
            store
                .save_with_external_conflicts(input(), &reserved)
                .unwrap_err(),
            super::QuicklinkErrorCode::CommandConflict
        );
        settings.builtin_names.clear();
        store
            .save_with_external_conflicts(
                input(),
                &super::quicklink_external_names(&settings, vec![]),
            )
            .unwrap();
        let settings_store = SettingsStore::load(dir.path()).unwrap();
        assert!(settings_store
            .set_builtin_feature_name_checked(
                ManagedBuiltinFeature::Screenshot,
                Some("shared".into()),
                |name| !store.commands().iter().any(|command| command == name),
            )
            .is_err());
        assert_eq!(
            settings_store
                .snapshot()
                .builtin_command(ManagedBuiltinFeature::Screenshot),
            "/screenshot"
        );
    }

    #[test]
    fn sole_public_match_captures_stable_identity_and_argument() {
        let mut suggestion = command_suggestion("shared");
        suggestion.plugin_id = "com.example.eligible".into();
        suggestion.command_id = "second".into();
        let captured = super::sole_public_match(&[suggestion], "hello  world").unwrap();
        assert_eq!(
            captured,
            (
                "com.example.eligible".into(),
                "second".into(),
                "hello  world".into()
            )
        );
    }

    #[test]
    fn duplicate_public_rows_show_distinct_command_identifiers() {
        let mut first = command_suggestion("shared");
        first.command_id = "first".into();
        let mut second = first.clone();
        second.command_id = "second".into();
        let first = super::public_plugin_completion_result(first, Some("hello"))
            .unwrap()
            .0;
        let second = super::public_plugin_completion_result(second, Some("hello"))
            .unwrap()
            .0;
        assert!(first.subtitle.as_deref().unwrap().contains("first"));
        assert!(second.subtitle.as_deref().unwrap().contains("second"));
        assert_ne!(first.subtitle, second.subtitle);
    }

    #[test]
    fn renamed_builtin_old_alias_with_public_argument_falls_through_to_public_route() {
        let mut settings = Settings::default();
        settings
            .builtin_names
            .insert(ManagedBuiltinFeature::WebSearch, "search".into());
        settings
            .builtin_names
            .insert(ManagedBuiltinFeature::Quicklinks, "links".into());
        settings
            .builtin_names
            .insert(ManagedBuiltinFeature::Screenshot, "capture".into());
        for name in ["web-search", "quicklinks", "screenshot"] {
            let query = format!("/{name} hello  world");
            let (matched, builtins) =
                super::exact_launch_aliases(&settings, name, vec![command_suggestion(name)]);
            assert_eq!(matched.len(), 1);
            assert!(builtins.is_empty());
            assert!(
                !super::reserved_catalog_query(&query, &[]),
                "{query} must reach the public route"
            );
            assert_eq!(super::split_launcher_alias(&query[1..]).1, "hello  world");
        }
        assert!(super::reserved_catalog_query("/find document", &[]));
    }

    #[test]
    fn exact_alias_choices_disambiguate_public_and_builtins_and_preserve_argument() {
        let mut settings = Settings::default();
        settings
            .builtin_names
            .insert(ManagedBuiltinFeature::Screenshot, "shared".into());
        settings
            .builtin_names
            .insert(ManagedBuiltinFeature::Quicklinks, "shared".into());
        let (name, argument) = super::split_launcher_alias("shared hello  world");
        let mut public = command_suggestion("shared");
        public.command_id = "other".into();
        let (public_matches, builtin_matches) =
            super::exact_launch_aliases(&settings, name, vec![public]);
        assert_eq!(argument, "hello  world");
        assert_eq!(public_matches[0].command_id, "other");
        assert_eq!(
            builtin_matches,
            vec![
                ManagedBuiltinFeature::Screenshot,
                ManagedBuiltinFeature::Quicklinks
            ]
        );
        settings
            .disabled_builtin_features
            .insert(ManagedBuiltinFeature::Screenshot);
        assert_eq!(
            super::exact_launch_aliases(&settings, "shared", Vec::new()).1,
            vec![ManagedBuiltinFeature::Quicklinks]
        );
        assert!(
            super::exact_launch_aliases(&settings, "screenshot", Vec::new())
                .1
                .is_empty()
        );
    }

    #[test]
    fn main_result_rows_emit_direct_tag_activation_without_a_trailing_completion() {
        let (item, _) = super::public_plugin_completion_result(
            PublicCommandSuggestion {
                output_mode: PublicOutputMode::MainResult,
                ..command_suggestion("demo-return")
            },
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(item.activation).unwrap(),
            serde_json::json!({
                "kind": "mainResultActivation",
                "commandId": "default",
                "pluginId": "com.example.demo-return",
                "commandLabel": "demo-return",
                "initialArgument": "",
                "favorite": false,
            })
        );
    }

    fn completion_text(item: &crate::model::ResultItem) -> Option<&str> {
        match &item.activation {
            crate::model::LauncherResultActivation::Completion { completion_text } => {
                Some(completion_text)
            }
            _ => None,
        }
    }

    #[test]
    fn panel_activation_rows_emit_panel_activation_with_initial_argument_rules() {
        let panel = PublicCommandSuggestion {
            command_id: "default".into(),
            plugin_id: "com.example.panel".into(),
            effective_name: "panel".into(),
            display_name: "Panel".into(),
            summary: None,
            icon_url: None,
            favorite: true,
            input_required: false,
            output_mode: PublicOutputMode::Panel,
        };
        let (empty, _) = super::public_plugin_completion_result(panel.clone(), None).unwrap();
        match empty.activation {
            crate::model::LauncherResultActivation::PanelActivation {
                command_id: _,
                plugin_id,
                initial_argument,
                favorite,
            } => {
                assert_eq!(plugin_id, "com.example.panel");
                assert_eq!(initial_argument, "");
                assert!(favorite);
            }
            _ => panic!("expected panelActivation"),
        }

        let (matched, _) = super::public_plugin_completion_result(panel, Some("hello")).unwrap();
        match matched.activation {
            crate::model::LauncherResultActivation::PanelActivation {
                initial_argument, ..
            } => assert_eq!(initial_argument, "hello"),
            _ => panic!("expected panelActivation"),
        }

        let (window, _) = super::public_plugin_completion_result(
            PublicCommandSuggestion {
                output_mode: PublicOutputMode::Window,
                effective_name: "demo-win".into(),
                ..command_suggestion("demo-win")
            },
            None,
        )
        .unwrap();
        assert!(matches!(
            window.activation,
            crate::model::LauncherResultActivation::WindowActivation {
                command_id: _,
                plugin_id,
                command_label,
                initial_argument,
                favorite: false,
            } if plugin_id == "com.example.demo-win" &&
                command_label == "demo-win" && initial_argument.is_empty()
        ));
    }

    #[test]
    fn optional_window_rows_emit_direct_window_activation_with_initial_argument() {
        let (item, _) = super::public_plugin_completion_result(
            PublicCommandSuggestion {
                input_required: false,
                output_mode: PublicOutputMode::Window,
                ..command_suggestion("pomodoro")
            },
            Some("focus"),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(item.activation).unwrap(),
            serde_json::json!({
                "kind": "windowActivation",
                "commandId": "default",
                "pluginId": "com.example.pomodoro",
                "commandLabel": "pomodoro",
                "initialArgument": "focus",
                "favorite": false,
            })
        );

        let (required, _) = super::public_plugin_completion_result(
            PublicCommandSuggestion {
                input_required: true,
                output_mode: PublicOutputMode::Window,
                ..command_suggestion("demo-win")
            },
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(required.activation).unwrap(),
            serde_json::json!({
                "kind": "windowActivation",
                "commandId": "default",
                "pluginId": "com.example.demo-win",
                "commandLabel": "demo-win",
                "initialArgument": "",
                "favorite": false,
            })
        );
    }

    #[test]
    fn launcher_empty_query_publishes_capabilities_without_reading_applications() {
        let registry = ready_registry("launcher-empty");
        let demo_title = format!("/{}", "demo-win");
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "   ",
                invocation_id: "launcher-empty",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| {
                Ok(vec![
                    command_suggestion("demo-win"),
                    command_suggestion("alpha"),
                ])
            },
            || panic!("empty launcher query must not read the application snapshot"),
            |_| panic!("empty launcher query must not decorate applications"),
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| (item.title.clone(), completion_text(item).map(str::to_owned)))
                .collect::<Vec<_>>(),
            vec![
                ("/find".into(), None),
                ("/quicklinks".into(), None),
                ("/screenshot".into(), None),
                ("/web-search".into(), Some("/web-search ".into())),
                ("/alpha".into(), None),
                (demo_title, None),
            ]
        );
        assert_eq!(
            response.items[0].activation,
            crate::model::LauncherResultActivation::OpenFind {
                query: String::new()
            }
        );
        for item in &response.items {
            assert_eq!(
                registry.resolve(&response.request_id, &item.result_id),
                Err(RegistryError::UnknownResult)
            );
        }
    }

    #[test]
    fn launcher_builtin_commands_are_favoritable_and_exact_queries_keep_their_item() {
        let favorites = BTreeSet::from([
            BuiltinFeature::Find,
            BuiltinFeature::Quicklinks,
            BuiltinFeature::Screenshot,
            BuiltinFeature::WebSearch,
        ]);
        for (query, expected_title, expected_feature, expected_hint) in [
            ("/find", "/find", "find", None),
            ("/quicklinks", "/quicklinks", "quicklinks", None),
            ("/screenshot", "/screenshot", "screenshot", None),
            (
                "/web-search",
                "/web-search",
                "webSearch",
                Some("请输入搜索内容"),
            ),
        ] {
            let registry = ready_registry("builtin-exact");
            let response = search_apps_with_catalog(
                &registry,
                CatalogSearchRequest {
                    query,
                    invocation_id: "builtin-exact",
                    query_sequence: 1,
                    submit: false,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &favorites,
                    disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                    quicklinks: &[],
                },
                |_| panic!("reserved builtin command must not read plugin inventory"),
                || panic!("reserved builtin command must not read applications"),
                |_| {},
            )
            .unwrap();

            assert_eq!(response.items.len(), 1);
            assert_eq!(response.items[0].title, expected_title);
            assert_eq!(response.command_hint.as_deref(), expected_hint);
            assert_eq!(
                serde_json::to_value(&response.items[0]).unwrap()["favorite"],
                serde_json::json!({
                    "target": { "kind": "builtin", "feature": expected_feature },
                    "favorite": true,
                })
            );
        }
    }

    #[test]
    fn launcher_quicklinks_open_manage_panel_and_execute_user_commands() {
        let quicklinks = vec![quicklink_view("jd")];
        let registry = ready_registry("quicklinks-open");
        let manage = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/quicklinks",
                invocation_id: "quicklinks-open",
                query_sequence: 1,
                submit: true,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &quicklinks,
            },
            |_| panic!("reserved quicklinks command must not read plugin inventory"),
            || panic!("reserved quicklinks command must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(manage.items.len(), 1);
        assert_eq!(manage.items[0].title, "/quicklinks");
        assert_eq!(
            serde_json::to_value(&manage.items[0]).unwrap()["iconKind"],
            serde_json::json!("quicklinks")
        );
        assert_eq!(
            manage.items[0].activation,
            crate::model::LauncherResultActivation::OpenQuicklinks
        );
        assert_eq!(
            serde_json::to_value(&manage.items[0]).unwrap()["favorite"],
            serde_json::json!({
                "target": { "kind": "builtin", "feature": "quicklinks" },
                "favorite": false,
            })
        );

        let registry = ready_registry("quicklinks-prompt");
        let prompt = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/jd",
                invocation_id: "quicklinks-prompt",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &quicklinks,
            },
            |_| panic!("quicklink exact command must win before plugin inventory"),
            || panic!("quicklink command must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(prompt.command_hint.as_deref(), Some("请输入参数"));
        assert_eq!(prompt.items[0].title, "/jd");
        assert_eq!(prompt.items[0].subtitle.as_deref(), Some("京东搜索"));
        assert_eq!(
            prompt.items[0].icon.as_deref(),
            Some("data:image/png;base64,iVBORw==")
        );
        assert!(!prompt.items[0].has_default_action);
        assert_eq!(
            registry.resolve(&prompt.request_id, &prompt.items[0].result_id),
            Err(RegistryError::UnknownResult)
        );

        let registry = ready_registry("quicklinks-execute");
        let execute = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/jd 手机 A&B?",
                invocation_id: "quicklinks-execute",
                query_sequence: 1,
                submit: true,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &quicklinks,
            },
            |_| panic!("quicklink exact command must win before plugin inventory"),
            || panic!("quicklink command must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(execute.items.len(), 1);
        assert_eq!(
            execute.auto_execute_result_id.as_deref(),
            Some(execute.items[0].result_id.as_str())
        );
        assert_eq!(
            registry.resolve(&execute.request_id, &execute.items[0].result_id),
            Ok(ResultAction::OpenQuicklink {
                id: "1".into(),
                url: "https://search.jd.com/Search?keyword=%E6%89%8B%E6%9C%BA%20A%26B%3F".into(),
            })
        );
    }

    #[test]
    fn launcher_web_search_submit_uses_auto_execute_result_id() {
        let registry = ready_registry("web-search-auto");
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/web-search UiPilot docs",
                invocation_id: "web-search-auto",
                query_sequence: 1,
                submit: true,
                web_search_engine: WebSearchEngine::Baidu,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("web-search command must not read plugin inventory"),
            || panic!("web-search command must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            response.auto_execute_result_id.as_deref(),
            Some(response.items[0].result_id.as_str())
        );
        assert!(matches!(
            registry.resolve(&response.request_id, &response.items[0].result_id),
            Ok(ResultAction::OpenWebSearch { engine: WebSearchEngine::Baidu, query }) if query == "UiPilot docs"
        ));

        let registry = ready_registry("web-search-preview");
        let preview = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/web-search UiPilot docs",
                invocation_id: "web-search-preview",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Baidu,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("web-search command must not read plugin inventory"),
            || panic!("web-search command must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(preview.auto_execute_result_id, None);
    }

    #[test]
    fn launcher_empty_query_orders_all_favorites_before_all_other_features() {
        let registry = ready_registry("launcher-favorite-groups");
        let favorites = BTreeSet::from([BuiltinFeature::Find]);
        let demo_title = format!("/{}", "demo-win");
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "",
                invocation_id: "launcher-favorite-groups",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &favorites,
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| {
                let mut favorite = command_suggestion("alpha");
                favorite.favorite = true;
                Ok(vec![command_suggestion("demo-win"), favorite])
            },
            || panic!("empty launcher query must not read applications"),
            |_| {},
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/find",
                "/alpha",
                "/quicklinks",
                "/screenshot",
                "/web-search",
                demo_title.as_str(),
            ]
        );
        assert_eq!(
            serde_json::to_value(&response.items[1]).unwrap()["favorite"],
            serde_json::json!({
                "target": { "kind": "publicPlugin", "pluginId": "com.example.alpha", "commandId": "default" },
                "favorite": true,
            })
        );
    }

    #[test]
    fn launcher_slash_query_publishes_the_command_catalog_without_reading_applications() {
        let registry = ready_registry("launcher-slash");
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/",
                invocation_id: "launcher-slash",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |query| {
                assert_eq!(query, "");
                Ok(vec![command_suggestion("alpha")])
            },
            || panic!("slash command catalog must not read the application snapshot"),
            |_| panic!("slash command catalog must not decorate applications"),
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/find",
                "/quicklinks",
                "/screenshot",
                "/web-search",
                "/alpha"
            ]
        );
    }

    #[test]
    fn launcher_plain_query_orders_applications_favorites_find_web_then_other_plugins() {
        let registry = ready_registry("launcher-plain");
        let demo_title = format!("/{}", "demo-win");
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "  win  ",
                invocation_id: "launcher-plain",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Google,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |query| {
                assert_eq!(query, "win");
                let mut favorite = command_suggestion("alpha");
                favorite.favorite = true;
                Ok(vec![command_suggestion("demo-win"), favorite])
            },
            || {
                vec![Application {
                    app_id: "app-windows-terminal".into(),
                    display_name: "Windows Terminal".into(),
                    target: ApplicationLaunchTarget::Shortcut {
                        shortcut: PathBuf::from(r"C:\Private\Windows Terminal.lnk"),
                        executable: None,
                    },
                    icon: None,
                    use_count: 0,
                }]
            },
            |_| {},
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.clone())
                .collect::<Vec<_>>(),
            vec![
                "Windows Terminal".into(),
                "/alpha".into(),
                "/find".into(),
                "Google 搜索".into(),
                demo_title,
            ]
        );
        assert!(matches!(
            &response.items[1].activation,
            crate::model::LauncherResultActivation::WindowActivation {
                command_id: _,
                plugin_id,
                command_label,
                initial_argument,
                favorite: true,
            } if plugin_id == "com.example.alpha" &&
                command_label == "alpha" && initial_argument == "win"
        ));
        assert_eq!(
            response.items[2].activation,
            crate::model::LauncherResultActivation::OpenFind {
                query: "win".into()
            }
        );
        assert!(matches!(
            &response.items[4].activation,
            crate::model::LauncherResultActivation::WindowActivation {
                command_id: _,
                plugin_id,
                command_label,
                initial_argument,
                favorite: false,
            } if plugin_id == "com.example.demo-win" &&
                command_label == "demo-win" && initial_argument.is_empty()
        ));
        assert_eq!(
            registry.resolve(&response.request_id, &response.items[2].result_id),
            Err(RegistryError::UnknownResult)
        );
        assert!(matches!(
            registry.resolve(&response.request_id, &response.items[3].result_id),
            Ok(ResultAction::OpenWebSearch { engine: WebSearchEngine::Google, query }) if query == "win"
        ));
    }

    #[test]
    fn launcher_catalog_failure_keeps_built_ins_and_direct_web_command_uses_engine() {
        let registry = ready_registry("launcher-fallback");
        let fallback = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "",
                invocation_id: "launcher-fallback",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Baidu,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| Err(crate::public_plugins::PublicPluginManagementError::Unavailable),
            || panic!("empty launcher query must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            fallback
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["/find", "/quicklinks", "/screenshot", "/web-search"]
        );

        let direct = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: " /web-search  UiPilot docs ",
                invocation_id: "launcher-fallback",
                query_sequence: 2,
                submit: false,
                web_search_engine: WebSearchEngine::Baidu,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("reserved web command must not read plugin inventory"),
            || panic!("reserved web command must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(direct.items.len(), 1);
        assert!(matches!(
            registry.resolve(&direct.request_id, &direct.items[0].result_id),
            Ok(ResultAction::OpenWebSearch { engine: WebSearchEngine::Baidu, query }) if query == "UiPilot docs"
        ));

        let hint = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "/web-search   ",
                invocation_id: "launcher-fallback",
                query_sequence: 3,
                submit: false,
                web_search_engine: WebSearchEngine::Baidu,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("reserved web hint must not read plugin inventory"),
            || panic!("reserved web hint must not read applications"),
            |_| {},
        )
        .unwrap();
        assert_eq!(hint.items.len(), 1);
        assert_eq!(hint.items[0].title, "/web-search");
        assert_eq!(completion_text(&hint.items[0]), Some("/web-search "));
        assert_eq!(hint.command_hint.as_deref(), Some("请输入搜索内容"));
    }

    const APP_CURRENT: &str =
        "app-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const APP_DUPLICATE_A: &str =
        "app-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const APP_ABSENT: &str = "app-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-task5-commands-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn application(index: usize) -> Application {
        Application {
            app_id: format!("app-{index:064x}"),
            display_name: format!("App {index:02}"),
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(format!(r"C:\Private\App{index:02}.lnk")),
                executable: Some(PathBuf::from(format!(r"C:\Private\App{index:02}.exe"))),
            },
            icon: None,
            use_count: 0,
        }
    }

    fn trusted_action() -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: APP_CURRENT.into(),
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(r"C:\Private\Current.lnk"),
                executable: Some(PathBuf::from(r"C:\Private\Current.exe")),
            },
        }
    }

    fn file_action(row_id: i64, relative_path: &str, kind: IndexedKind) -> OpenIndexedPath {
        OpenIndexedPath::for_test(
            0,
            row_id,
            VolumeIdentity::for_test(r"\\?\Volume{COMMANDS}\", 1, "ntfs"),
            relative_path,
            kind,
        )
    }

    fn everything_action_for_test() -> EverythingPathAction {
        EverythingPathAction::for_test(AuthenticatedPathIdentity {
            display_path: r"C:\Visible\report.pdf".into(),
            volume_guid_path: r"\\?\Volume{COMMANDS}\".into(),
            relative_path: r"docs\report.pdf".into(),
            volume_serial: 42,
            file_id: [7; 16],
            kind: FilePathKind::File,
        })
    }

    fn ready_registry(invocation_id: &str) -> ResultRegistry {
        let registry = ResultRegistry::default();
        registry.on_show(invocation_id.into());
        registry
    }

    fn prepared_query(query: &str, invocation_id: &str, query_sequence: u64) -> PreparedFileQuery {
        prepare_file_query(
            query.into(),
            "all".into(),
            "modifiedDesc".into(),
            invocation_id.into(),
            query_sequence,
        )
        .unwrap()
    }

    fn everything_batch_for_test(index_revision: u64, item_count: usize) -> PublishedFileBatch {
        PublishedFileBatch {
            index_revision,
            items: (0..item_count)
                .map(|index| PublishedFileDraft {
                    action: FileExecutionAction::Everything(everything_action_for_test()),
                    name: format!("Result {index}"),
                    kind: FileResultKind::File,
                    size_bytes: Some(index as u64),
                    modified_utc: "2026-07-30T00:00:00.000Z".into(),
                    full_path: format!("Result {index}"),
                })
                .collect(),
        }
    }

    fn settings_store(dir: &TestDir) -> SettingsStore {
        let settings = Settings {
            hotkey: "Alt+Space".into(),
            autostart: false,
            theme: ThemePreference::System,
            web_search_engine: WebSearchEngine::Bing,
            file_preview_enabled: true,
            favorite_builtin_features: Default::default(),
            disabled_builtin_features: Default::default(),
            use_counts: BTreeMap::from([(APP_DUPLICATE_A.into(), 9), (APP_ABSENT.into(), 13)]),
            window_position: None,
            find_window_position: None,
            plugin_window_positions: BTreeMap::new(),
            public_plugin_command_hotkeys: BTreeMap::new(),
            screenshot_hotkey: None,
            translation_selection_hotkey: Some("Alt+A".into()),
            builtin_hotkeys: BTreeMap::new(),
            builtin_names: BTreeMap::new(),
            screenshot_save_directory: None,
        };
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        SettingsStore::load(dir.path()).unwrap()
    }

    #[test]
    fn builtin_management_authorizes_and_validates_before_settings_access() {
        assert!(with_managed_builtin(
            "plugin-window",
            "screenshot",
            |_| -> Result<(), CommandError> { panic!("protected settings accessed") }
        )
        .is_err());
        assert!(
            with_managed_builtin("main", "find", |_| -> Result<(), CommandError> {
                panic!("protected settings accessed")
            })
            .is_err()
        );
        assert!(with_managed_builtin(
            "main",
            "com.example.plugin",
            |_| -> Result<(), CommandError> { panic!("protected settings accessed") }
        )
        .is_err());
        let value = builtin_feature_views(&Settings::default());
        assert_eq!(
            value.iter().map(|item| item.feature).collect::<Vec<_>>(),
            ManagedBuiltinFeature::ALL
        );
        assert_eq!(
            value
                .iter()
                .map(|item| item.command.as_str())
                .collect::<Vec<_>>(),
            ["/screenshot", "/quicklinks", "/web-search"]
        );
        assert_eq!(value[1].examples, vec!["/quicklinks"]);
        assert!(value.iter().all(|item| item.enabled
            && !item.examples.is_empty()
            && item.version == env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn caller_guard_rejects_non_main_commands_without_side_effects() {
        assert_eq!(require_main_label("main"), Ok(()));
        assert_eq!(require_find_label("find"), Ok(()));
        assert_eq!(
            require_find_label("main"),
            Err(CommandError::invalid_caller())
        );
        for command in [
            "search_apps",
            "search_files",
            "execute_result",
            "list_plugins",
            "reload_plugin",
            "delete_plugin",
            "load_settings",
            "save_settings",
            "save_hotkey",
            "set_file_preview_preference",
            "hide_launcher",
            "list_public_plugins",
            "get_public_plugin_diagnostics",
            "copy_public_plugin_diagnostics",
            "prepare_public_plugin_install",
            "commit_public_plugin_install",
            "cancel_public_plugin_install",
            "set_plugin_enabled",
            "set_plugin_favorite",
            "set_plugin_effective_name",
            "save_plugin_settings",
            "uninstall_plugin",
            "get_message_summary",
            "open_message_center",
            "read_message_center",
            "clear_messages",
            "open_plugin_panel",
            "submit_plugin_panel",
            "set_plugin_panel_bounds",
            "close_plugin_panel",
        ] {
            let trace = RefCell::new(Vec::new());
            let result = require_main_label("secondary").map(|()| {
                trace.borrow_mut().push(command);
            });

            assert_eq!(result, Err(CommandError::invalid_caller()), "{command}");
            assert!(trace.borrow().is_empty(), "{command} touched state");
        }
    }

    #[test]
    fn panel_open_submit_bounds_and_close_are_main_only_and_use_decimal_session_epochs() {
        assert_eq!(parse_panel_session_epoch("1"), Ok(1));
        assert_eq!(
            parse_panel_session_epoch("18446744073709551615"),
            Ok(u64::MAX)
        );
        for invalid in ["", "0", "01", "-1", "18446744073709551616"] {
            assert_eq!(
                parse_panel_session_epoch(invalid),
                Err(CommandError::plugin_query_failed()),
                "{invalid}"
            );
        }

        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let open = source
            .split("pub(crate) async fn open_plugin_panel(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("missing open_plugin_panel");
        assert!(open.contains("window: WebviewWindow"));
        let open_statements = open.split_once("{\n").unwrap().1;
        let open_guard = open_statements
            .find("require_main_window(&window)?;")
            .unwrap();
        assert!(open_guard < open_statements.find("public").unwrap());

        let submit = source
            .split("pub(crate) async fn submit_plugin_panel(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("missing submit_plugin_panel");
        assert!(submit.contains("webview: tauri::Webview"));
        assert!(!submit.contains("window: WebviewWindow"));
        let submit_statements = submit.split_once("{\n").unwrap().1;
        let submit_guard = submit_statements
            .find("require_main_label(webview.label())?;")
            .unwrap();
        assert!(submit_guard < submit_statements.find("public").unwrap());

        let close = source
            .split("pub(crate) fn close_plugin_panel(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("missing close_plugin_panel");
        assert!(close.contains("webview: tauri::Webview"));
        assert!(!close.contains("window: WebviewWindow"));
        let statements = close.split_once("{\n").unwrap().1;
        let guard = statements
            .find("require_main_label(webview.label())?;")
            .unwrap();
        let state_access = statements.find("controller.inner()").unwrap();
        assert!(guard < state_access);
        assert!(close.contains("Some(session_epoch)"));

        let bounds = source
            .split("pub(crate) fn set_plugin_panel_bounds(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("missing set_plugin_panel_bounds");
        assert!(bounds.contains("webview: tauri::Webview"));
        assert!(!bounds.contains("window: WebviewWindow"));
        let statements = bounds.split_once("{\n").unwrap().1;
        let guard = statements
            .find("require_main_label(webview.label())?;")
            .unwrap();
        let state_access = statements.find("controller.inner()").unwrap();
        assert!(guard < state_access);
        assert!(bounds.contains("parse_panel_session_epoch"));
        assert!(bounds.contains("plugin_panel::set_bounds("));
    }

    #[test]
    fn panel_submit_route_must_match_the_live_session_identity() {
        let identity = PanelSessionIdentity {
            session_epoch: 7,
            plugin_id: "com.example.panel".into(),
            generation: 11,
            activation_id: 13,
            admission_epoch: 17,
            command_label: "panel".into(),
            content_label:
                "plugin-panel-content-636f6d2e6578616d706c652e70616e656c-s0000000000000007".into(),
            host_keys: vec![
                crate::public_plugins::PanelHostKeyDeclaration::ArrowDown,
                crate::public_plugins::PanelHostKeyDeclaration::PrimaryN,
            ],
            host_key_focus: crate::public_plugins::PanelHostKeyFocus::Content,
        };
        let mut route = PublicPluginRoute {
            result_publication: None,
            command_id: "default".into(),
            plugin_id: identity.plugin_id.clone(),
            generation: identity.generation,
            activation_id: identity.activation_id,
            admission_epoch: identity.admission_epoch,
            runtime_recovery_needed: false,
            runtime_label: "plugin-runtime".into(),
            activation_mode: PublicActivationMode::Submit,
            output_mode: PublicOutputMode::Panel,
            command_label: identity.command_label.clone(),
            input: String::new(),
            input_required: false,
            input_placeholder: None,
            window_entry: None,
            panel_entry: Some("dist/panel.html".into()),
            host_keys: identity.host_keys.clone(),
            host_key_focus: identity.host_key_focus,
            icon_url: None,
        };
        assert!(panel_route_matches_identity(&route, &identity));
        route.input_placeholder = Some("搜索目录".into());
        let result = serde_json::to_value(super::panel_command_result(
            &identity,
            route.input_placeholder.as_deref(),
        ))
        .unwrap();
        assert_eq!(
            result["hostKeys"],
            serde_json::json!(["ArrowDown", "Primary+N"])
        );
        assert_eq!(result["inputPlaceholder"], "搜索目录");
        route.host_key_focus = crate::public_plugins::PanelHostKeyFocus::Host;
        assert!(!panel_route_matches_identity(&route, &identity));
        route.host_key_focus = identity.host_key_focus;
        route
            .host_keys
            .push(crate::public_plugins::PanelHostKeyDeclaration::ArrowUp);
        assert!(!panel_route_matches_identity(&route, &identity));
        route.host_keys.clear();
        route.generation += 1;
        assert!(!panel_route_matches_identity(&route, &identity));
        route.generation = identity.generation;
        route.command_label = "renamed".into();
        assert!(!panel_route_matches_identity(&route, &identity));
    }

    #[test]
    fn panel_open_returns_ownership_before_submit_can_emit_epoch_bound_errors() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let open = source
            .split("async fn open_plugin_panel_impl(")
            .nth(1)
            .and_then(|tail| tail.split("async fn submit_plugin_panel_impl").next())
            .unwrap();
        let mount = open.find("plugin_panel::mount(").unwrap();
        let focus = open.find("focus_main_window_content(").unwrap();
        let returned = open.find("Ok(Some(panel_command_result(").unwrap();
        assert!(mount < focus && focus < returned);
        assert!(open.contains("plugin_panel::teardown("));
        assert!(!open.contains("prepare_command("));
        assert!(!open.contains("spawn_panel_submission("));

        let submit = source
            .split("async fn submit_plugin_panel_impl(")
            .nth(1)
            .and_then(|tail| {
                tail.split("#[tauri::command]\npub(crate) async fn search_apps")
                    .next()
            })
            .unwrap();
        let background_start = submit.find("spawn_panel_submission(").unwrap();
        let returned = submit.find("Ok(Some(result))").unwrap();
        assert!(background_start < returned);
        let background = source
            .split("fn spawn_panel_submission(")
            .nth(1)
            .and_then(|tail| tail.split("async fn open_plugin_panel_impl").next())
            .unwrap();
        assert!(background.contains("run_panel_submission("));
        assert!(background.contains("uipilot-plugin-panel-error"));
        assert!(background.contains("PluginPanelErrorEvent { session_epoch }"));
    }

    #[test]
    fn message_center_commands_are_main_only_and_have_exact_capabilities() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let build = include_str!("../build.rs");
        let main = include_str!("../capabilities/main.json");
        let runtime = include_str!("../capabilities/plugin-runtime.json");
        for command in [
            "get_message_summary",
            "open_message_center",
            "read_message_center",
            "clear_messages",
        ] {
            let body = source
                .split(&format!("pub(crate) fn {command}("))
                .nth(1)
                .and_then(|tail| tail.split("\n#[tauri::command]").next())
                .unwrap_or_else(|| panic!("missing {command}"));
            assert!(body.contains("require_main_window(&window)?;"));
            let permission = format!("{}-{}", ["al", "low"].concat(), command.replace('_', "-"));
            assert!(build.contains(&format!("\"{command}\",")));
            assert!(main.contains(&permission));
            assert!(!runtime.contains(&permission));
        }
    }

    #[test]
    fn quicklink_commands_are_main_only_and_have_exact_capabilities() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("commands test module marker is missing");
        let build = include_str!("../build.rs");
        let main = include_str!("../capabilities/main.json");
        let runtime = include_str!("../capabilities/plugin-runtime.json");
        let find = include_str!("../capabilities/find.json");
        let panel = include_str!("../capabilities/plugin-panel-content.json");
        for command in [
            "list_quicklinks",
            "save_quicklink",
            "delete_quicklink",
            "choose_quicklink_icon",
        ] {
            let marker = if command == "choose_quicklink_icon" {
                format!("pub(crate) async fn {command}(")
            } else {
                format!("pub(crate) fn {command}(")
            };
            let body = production
                .split(&marker)
                .nth(1)
                .and_then(|tail| tail.split("\n#[tauri::command]").next())
                .unwrap_or_else(|| panic!("missing {command}"));
            let first_statement = body[body.find('{').unwrap() + 1..].trim_start();
            assert!(
                first_statement.starts_with("require_main_window(&window)?;"),
                "{command} must guard caller before state access"
            );
            let permission = format!("{}-{}", ["al", "low"].concat(), command.replace('_', "-"));
            assert!(build.contains(&format!("\"{command}\",")), "{command}");
            assert!(main.contains(&permission), "{command}");
            assert!(!runtime.contains(&permission), "{command}");
            assert!(!find.contains(&permission), "{command}");
            assert!(!panel.contains(&permission), "{command}");
        }
    }

    #[test]
    fn quicklink_icon_picker_is_async_and_moves_blocking_dialog_off_command_thread() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("commands test module marker is missing");
        let marker = ["pub(crate) async fn choose_quicklink_", "icon("].concat();
        let body = production
            .split(&marker)
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("choose_quicklink_icon command must be async");
        let first_statement = body[body.find('{').unwrap() + 1..].trim_start();
        assert!(first_statement.starts_with("require_main_window(&window)?;"));

        let spawn = body
            .find("tauri::async_runtime::spawn_blocking")
            .expect("quicklink icon selection must leave the command thread");
        let picker = body
            .find(".blocking_pick_file()")
            .expect("quicklink icon selection must use the native file picker");
        assert!(
            spawn < picker,
            "blocking native file picker must run inside spawn_blocking"
        );
    }

    #[test]
    fn public_plugin_command_callers_are_guarded_by_exact_labels() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        for command in [
            "list_public_plugins",
            "get_public_plugin_diagnostics",
            "prepare_public_plugin_install",
            "commit_public_plugin_install",
            "cancel_public_plugin_install",
            "set_plugin_enabled",
            "set_plugin_favorite",
            "set_plugin_effective_name",
            "save_plugin_settings",
            "uninstall_plugin",
        ] {
            let marker = format!(
                "pub(crate) {}fn {command}(",
                if matches!(
                    command,
                    "commit_public_plugin_install" | "set_plugin_enabled"
                ) {
                    "async "
                } else {
                    ""
                }
            );
            let body = source
                .split(&marker)
                .nth(1)
                .and_then(|tail| tail.split("\n#[tauri::command]").next())
                .expect("public management command markers are missing");
            let statements = body
                .split_once("{\n")
                .map(|(_, statements)| statements)
                .expect("public management command body is missing");
            let guard = statements
                .find("require_main_window(&window)?;")
                .expect("public management command must guard main caller");
            let state_access = statements
                .find("service")
                .unwrap_or_else(|| panic!("{command} must reach public plugin service"));
            assert!(guard < state_access, "{command} reaches state before guard");
        }
        let api = source
            .split("pub(crate) fn plugin_api_call(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .unwrap();
        assert!(api.contains("execute_api(window.label(), request)"));
        assert!(!api.contains("starts_with(\"plugin-\")"));
        let complete = source
            .split("pub(crate) fn complete_plugin_command(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .unwrap();
        assert!(complete.contains("let now = Instant::now();"));
        assert!(complete.contains(".complete(window.label(), &completion, now)"));
        assert!(complete.contains("complete_submission(&app, &completion, outcome, now)"));
        assert!(!complete.contains("starts_with(\"plugin-\")"));
    }

    #[test]
    fn public_plugin_picker_suppresses_focus_loss_through_focus_restore_then_releases() {
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let hides = Cell::new(0);
        let expected = PathBuf::from(r"C:\Plugins\demo");

        let selected = select_public_plugin_source_with(
            "main",
            &coordinator,
            || {
                coordinator
                    .handle_focus_event_with(false, || {
                        hides.set(hides.get() + 1);
                        Ok(())
                    })
                    .unwrap();
                Some(expected.clone())
            },
            || {
                coordinator
                    .handle_focus_event_with(false, || {
                        hides.set(hides.get() + 1);
                        Ok(())
                    })
                    .unwrap();
                Ok(())
            },
        );

        assert_eq!(selected, Ok(Some(expected)));
        assert_eq!(hides.get(), 0);

        coordinator
            .handle_focus_event_with(false, || {
                hides.set(hides.get() + 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(hides.get(), 1);
    }

    #[test]
    fn public_plugin_install_commit_suppresses_focus_before_runtime_creation() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let body = source
            .split("pub(crate) async fn commit_public_plugin_install(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("commit command is missing");
        let suppression = body
            .find("suppress_transient_focus_loss()")
            .expect("commit must suppress transient focus loss");
        let runtime = body
            .find("commit_with_readiness_with_effects(")
            .expect("commit readiness call is missing");
        assert!(suppression < runtime);
        let restore = body
            .find("focus_main_window_content_now(&app)")
            .expect("commit must restore main window and webview focus");
        assert!(runtime < restore);
    }

    #[test]
    fn public_plugin_enable_suppresses_focus_before_runtime_creation() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let body = source
            .split("pub(crate) async fn set_plugin_enabled(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("enable command is missing");
        let suppression = body
            .find("suppress_transient_focus_loss()")
            .expect("enable must suppress transient focus loss");
        let runtime = body
            .find("set_enabled_with_readiness_with_effects(")
            .expect("enable readiness call is missing");
        assert!(suppression < runtime);
    }

    #[test]
    fn public_plugin_upgrade_disable_and_uninstall_reset_matching_panel_sessions() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        for (command, next_marker) in [
            (
                "commit_public_plugin_install",
                "#[tauri::command]\npub(crate) async fn set_plugin_enabled",
            ),
            (
                "set_plugin_enabled",
                "#[tauri::command]\npub(crate) fn set_plugin_favorite",
            ),
            (
                "uninstall_plugin",
                "#[tauri::command]\npub(crate) fn plugin_api_call",
            ),
        ] {
            let signature = if command == "uninstall_plugin" {
                format!("pub(crate) fn {command}(")
            } else {
                format!("pub(crate) async fn {command}(")
            };
            let body = source
                .split(&signature)
                .nth(1)
                .and_then(|tail| tail.split(next_marker).next())
                .unwrap_or_else(|| panic!("missing management command: {command}"));
            assert!(body.contains("panel_controller: State<'_, Arc<PluginPanelController>>"));
            assert!(body.contains("reset_plugin_panel("));
        }
    }

    #[test]
    fn public_plugin_window_prepare_suppresses_transient_main_blur() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let search = source
            .split("pub(crate) async fn search_apps(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn invocation_theme(").next())
            .expect("search_apps command is missing");
        let window_branch = search
            .split("PublicPluginResponse::Window(response) =>")
            .nth(1)
            .expect("public plugin window branch is missing");
        let suppression = window_branch
            .find("let _focus = coordinator")
            .expect("plugin window prepare must hold a focus suppression guard");
        let prepare = window_branch
            .find("plugin_window::prepare(")
            .expect("plugin window prepare call is missing");
        let wait = window_branch[prepare..]
            .find(".await")
            .map(|index| prepare + index)
            .expect("plugin window prepare must remain asynchronous");

        assert!(suppression < prepare);
        assert!(prepare < wait);
    }

    #[test]
    fn public_plugin_search_recovers_runtime_before_dispatch() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let search = source
            .split("pub(crate) async fn search_apps(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn invocation_theme(").next())
            .expect("search_apps command is missing");
        let recovery = search
            .find("recover_runtime(")
            .expect("recovery-needed submissions must rebuild Runtime");
        let dispatch = search
            .find("public.dispatch(")
            .expect("public plugin dispatch is missing");

        assert!(recovery < dispatch);
    }

    #[test]
    fn public_plugin_uninstall_destroys_old_instances_before_abort_recovery() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let uninstall = source
            .split("pub(crate) fn uninstall_plugin(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn plugin_api_call(").next())
            .expect("uninstall command is missing");
        let close_failure = uninstall
            .split("if !window_controller.close_for_uninstall(&plugin_id) {")
            .nth(1)
            .and_then(|tail| tail.split("\n    }").next())
            .expect("window-close failure branch is missing");
        let destroy_runtime = close_failure
            .find("PublicPluginService::destroy_runtime(")
            .expect("close failure must destroy the old Runtime");
        let destroy_window = close_failure
            .find("plugin_window::destroy_current(")
            .expect("close failure must destroy the old window");
        let abort = close_failure
            .find("abort_uninstall_before_commit(")
            .expect("close failure must publish recovery after teardown");
        assert!(destroy_runtime < abort);
        assert!(destroy_window < abort);

        let commit = uninstall
            .split("let committed = match manager.commit_uninstall(transaction) {")
            .nth(1)
            .expect("commit failures must be handled explicitly");
        let failure = commit
            .split("Err(mut error) => {")
            .nth(1)
            .and_then(|tail| tail.split("\n        }").next())
            .expect("commit failure branch is missing");
        assert!(failure.contains("PublicPluginService::destroy_runtime("));
        assert!(failure.contains("plugin_window::destroy_current("));
        let abort = failure
            .find("abort_uninstall_before_commit(")
            .expect("recoverable commit failure must publish recovery after teardown");
        assert!(
            failure
                .find("PublicPluginService::destroy_runtime(")
                .unwrap()
                < abort
        );
        assert!(failure.find("plugin_window::destroy_current(").unwrap() < abort);
    }

    #[test]
    fn public_plugin_every_runtime_creation_from_management_commands_is_focus_suppressed() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let runtime_call = "service.create_runtime(&app, candidate)";
        let mut remaining = production;
        let mut checked = 0;
        while let Some(index) = remaining.find(runtime_call) {
            let command_prefix = &remaining[..index];
            let command = command_prefix
                .rsplit("#[tauri::command]")
                .next()
                .expect("runtime creation must belong to a Tauri command");
            assert!(
                command.contains("suppress_transient_focus_loss()"),
                "public Runtime creation is missing transient focus suppression"
            );
            checked += 1;
            remaining = &remaining[index + runtime_call.len()..];
        }
        assert!(
            checked >= 2,
            "expected install and enable Runtime creation paths"
        );
    }

    #[test]
    fn public_plugin_install_commit_is_async_so_runtime_events_can_settle() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        assert!(
            source.contains("#[tauri::command]\npub(crate) async fn commit_public_plugin_install(")
        );
    }

    #[test]
    fn public_plugin_enable_is_async_so_runtime_events_can_settle() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        assert!(source.contains("#[tauri::command]\npub(crate) async fn set_plugin_enabled("));
    }

    #[test]
    fn plugin_window_commands_derive_identity_from_exact_caller_labels() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let command_body = |name: &str| {
            let synchronous = format!("pub(crate) fn {name}(");
            let asynchronous = format!("pub(crate) async fn {name}(");
            let marker = if source.contains(&asynchronous) {
                asynchronous.as_str()
            } else {
                synchronous.as_str()
            };
            source
                .split(marker)
                .nth(1)
                .and_then(|tail| tail.split("\n#[tauri::command]").next())
                .unwrap_or_else(|| panic!("missing command {name}"))
        };
        let ready = command_body("plugin_window_content_ready");
        assert!(ready.contains("webview.label()"));
        assert!(!ready.contains("plugin_id:"));
        let ack = command_body("plugin_window_content_ack");
        assert!(ack.contains("webview.label(), &request_id"));
        assert!(!ack.contains("plugin_id:"));
        let content_close = command_body("plugin_window_content_close");
        assert!(content_close.contains("webview.label()"));
        assert!(!content_close.contains("plugin_id:"));
        let content_pin = command_body("plugin_window_content_toggle_pin");
        assert!(content_pin.contains("begin_window_call(webview.label(), session, true)"));
        assert!(content_pin.contains("plugin_shell_label(&lease.owner().plugin_id)"));
        assert!(!content_pin.contains("plugin_id:"));
        assert!(!content_pin.contains("label: String"));
        let commit = command_body("commit_plugin_window_transfer");
        let guard = commit.find("require_main_window(&window)?;").unwrap();
        let transfer = commit.find("plugin_window::commit(").unwrap();
        assert!(guard < transfer);
        let pin = command_body("set_plugin_window_pinned");
        assert!(source
            .contains("pub(crate) fn set_plugin_window_pinned(\n    webview: tauri::Webview,"));
        assert!(pin.contains("webview.label()"));
        assert!(!pin.contains("plugin_id:"));
        let close = command_body("close_plugin_window");
        assert!(source.contains("pub(crate) fn close_plugin_window(\n    webview: tauri::Webview,"));
        assert!(close.contains("webview.label()"));
        assert!(!close.contains("plugin_id:"));
        let identity = command_body("get_public_plugin_window_identity");
        let label_guard = identity
            .find("plugin_id_from_shell_label(webview.label())")
            .unwrap();
        let manager_read = identity.find("service.manager()?").unwrap();
        assert!(label_guard < manager_read);
        assert!(!identity.contains("plugin_id:"));
    }

    #[test]
    fn plugin_panel_commands_derive_identity_from_exact_caller_labels() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let command_body = |name: &str| {
            source
                .split(&format!("pub(crate) fn {name}("))
                .nth(1)
                .and_then(|tail| tail.split("\n#[tauri::command]").next())
                .unwrap_or_else(|| panic!("missing command {name}"))
        };
        for command in [
            "plugin_panel_content_ready",
            "plugin_panel_content_ack",
            "plugin_panel_host_key_ack",
            "plugin_panel_request_hide_admit",
            "plugin_panel_request_hide_admit_observed",
            "plugin_panel_request_hide_commit",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
        ] {
            let body = command_body(command);
            assert!(body.contains("webview: tauri::Webview"));
            assert!(body.contains("webview.label()"));
            assert!(!body.contains("plugin_id: String"));
        }
        for command in [
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
        ] {
            let body = command_body(command);
            assert!(body.contains("session_epoch: String"));
            assert!(body.contains("begin_content_call(webview.label(), session_epoch"));
        }
        for command in [
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
        ] {
            let body = command_body(command);
            assert!(body.contains("session_epoch: String"));
            assert!(body.contains("begin_clipboard_history_panel_call("));
        }
        let clipboard_history_gate = source
            .split("fn begin_clipboard_history_panel_call(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("clipboard history panel gate is missing");
        assert!(clipboard_history_gate.contains("begin_content_call(caller_label, session_epoch"));
        let ready = command_body("plugin_panel_content_ready");
        assert!(ready.contains("session_epoch: String"));
        assert!(ready.contains("content_ready_with_host_keys("));
        assert!(ready.contains("webview.label(),"));
        assert!(ready.contains("host_key_receiver_registered,"));
        assert!(ready.contains("host_key_registration_violation,"));
        let ack = command_body("plugin_panel_content_ack");
        assert!(ack.contains("session_epoch: String"));
        assert!(ack.contains("content_ack("));
        assert!(ack.contains("session_epoch,"));
        let capability = include_str!("../capabilities/plugin-panel-content.json");
        assert!(capability.contains("\"webviews\": [\"plugin-panel-content-*\"]"));
        for command in [
            "plugin_panel_content_ready",
            "plugin_panel_content_ack",
            "plugin_panel_host_key_ack",
            "plugin_panel_request_hide_admit",
            "plugin_panel_request_hide_admit_observed",
            "plugin_panel_request_hide_commit",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
        ] {
            let permission = format!(
                "\"{}-{}\"",
                ["al", "low"].concat(),
                command.replace('_', "-")
            );
            assert!(capability.contains(&permission));
        }
        assert!(!capability.contains("timer"));
        assert!(!capability.contains("plugin-window-"));
        assert!(!capability.contains("close"));
        let enqueue = command_body("plugin_panel_host_key_enqueue");
        assert!(
            enqueue
                .find("require_main_label(webview.label())?")
                .unwrap()
                < enqueue.find("enqueue_host_key(").unwrap()
        );
        let main_capability = include_str!("../capabilities/main.json");
        assert!(main_capability.contains("allow-plugin-panel-host-key-enqueue"));
        assert!(!capability.contains("allow-plugin-panel-host-key-enqueue"));
        for command in [
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
        ] {
            assert!(!main_capability.contains(&format!("allow-{}", command.replace('_', "-"))));
        }
    }

    fn panel_focus_owner() -> PanelOwner {
        PanelOwner {
            plugin_id: "com.uipilot.demo-panel".into(),
            plugin_generation: 1,
            activation_id: 2,
            admission_epoch: 3,
            command_label: "demo-panel".into(),
            request_id: "request-a".into(),
            submission_token: "token-a".into(),
            argument: "query".into(),
            host_keys: Vec::new(),
            host_key_focus: crate::public_plugins::PanelHostKeyFocus::Content,
        }
    }

    #[test]
    fn initial_panel_host_focus_is_epoch_bound_before_and_after_native_effect() {
        let controller = PluginPanelController::default();
        let stale = controller.open_session(panel_focus_owner()).unwrap();
        assert!(controller
            .teardown_session(Some(stale.session_epoch))
            .is_some());
        let native_calls = Cell::new(0);
        assert!(!focus_live_panel_host_content(
            &controller,
            stale.session_epoch,
            || {
                native_calls.set(native_calls.get() + 1);
                true
            },
        ));
        assert_eq!(native_calls.get(), 0);

        let replaced = controller.open_session(panel_focus_owner()).unwrap();
        let current = controller.open_session(panel_focus_owner()).unwrap();
        assert!(!focus_live_panel_host_content(
            &controller,
            replaced.session_epoch,
            || {
                native_calls.set(native_calls.get() + 1);
                true
            },
        ));
        assert_eq!(native_calls.get(), 0);

        assert!(!focus_live_panel_host_content(
            &controller,
            current.session_epoch,
            || {
                native_calls.set(native_calls.get() + 1);
                assert!(controller
                    .teardown_session(Some(current.session_epoch))
                    .is_some());
                true
            },
        ));
        assert_eq!(native_calls.get(), 1);
        assert!(controller.live_identity().is_none());
    }

    #[test]
    fn plugin_panel_focus_main_thread_effects_run_only_after_a_current_claim() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(panel_focus_owner()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let stale = controller
            .prepare_host_input_focus(&session.content_label, session.session_epoch, deadline)
            .unwrap()
            .unwrap();
        assert!(controller
            .teardown_session(Some(session.session_epoch))
            .is_some());
        let native_calls = Cell::new(0);
        let event_calls = Cell::new(0);
        assert_eq!(
            start_host_input_focus(
                &controller,
                stale,
                || {
                    native_calls.set(native_calls.get() + 1);
                    true
                },
                std::time::Instant::now,
                || {
                    event_calls.set(event_calls.get() + 1);
                    true
                },
            ),
            HostInputFocusStart::Noop
        );
        assert_eq!(native_calls.get(), 0);
        assert_eq!(event_calls.get(), 0);

        let session = controller.open_session(panel_focus_owner()).unwrap();
        assert!(controller
            .mark_content_visible(session.session_epoch)
            .is_ok());
        assert!(controller.main_content_got_focus());
        let blur = controller.main_content_lost_focus(false).unwrap();
        let current = controller
            .prepare_host_input_focus(&session.content_label, session.session_epoch, deadline)
            .unwrap()
            .unwrap();
        assert_eq!(
            start_host_input_focus(
                &controller,
                current,
                || {
                    native_calls.set(native_calls.get() + 1);
                    false
                },
                std::time::Instant::now,
                || {
                    event_calls.set(event_calls.get() + 1);
                    true
                },
            ),
            HostInputFocusStart::Failed
        );
        assert_eq!(native_calls.get(), 1);
        assert_eq!(event_calls.get(), 0);
        assert!(controller.confirm_app_blur(&blur));

        let expired = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                std::time::Instant::now(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            start_host_input_focus(
                &controller,
                expired,
                || {
                    native_calls.set(native_calls.get() + 1);
                    true
                },
                std::time::Instant::now,
                || {
                    event_calls.set(event_calls.get() + 1);
                    true
                },
            ),
            HostInputFocusStart::Failed
        );
        assert_eq!(native_calls.get(), 1);
        assert_eq!(event_calls.get(), 0);

        assert!(controller.main_content_got_focus());
        let late_blur = controller.main_content_lost_focus(false).unwrap();
        let late_deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let late = controller
            .prepare_host_input_focus(&session.content_label, session.session_epoch, late_deadline)
            .unwrap()
            .unwrap();
        assert_eq!(
            start_host_input_focus(
                &controller,
                late,
                || {
                    native_calls.set(native_calls.get() + 1);
                    true
                },
                || late_deadline,
                || {
                    event_calls.set(event_calls.get() + 1);
                    true
                },
            ),
            HostInputFocusStart::Failed
        );
        assert_eq!(native_calls.get(), 2);
        assert_eq!(event_calls.get(), 0);
        assert!(host_input_focus_failure(&controller, late).is_err());
        assert!(!controller.confirm_app_blur(&late_blur));

        assert!(controller.main_content_got_focus());
        let in_flight_blur = controller.main_content_lost_focus(false).unwrap();
        let in_flight = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                std::time::Instant::now() + std::time::Duration::from_secs(1),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            start_host_input_focus(
                &controller,
                in_flight,
                || {
                    native_calls.set(native_calls.get() + 1);
                    assert!(host_input_focus_failure(&controller, in_flight).is_err());
                    true
                },
                std::time::Instant::now,
                || {
                    event_calls.set(event_calls.get() + 1);
                    true
                },
            ),
            HostInputFocusStart::Noop
        );
        assert_eq!(native_calls.get(), 3);
        assert_eq!(event_calls.get(), 0);
        assert!(!controller.confirm_app_blur(&in_flight_blur));

        let session = controller.open_session(panel_focus_owner()).unwrap();
        let teardown_during_native = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                std::time::Instant::now() + std::time::Duration::from_secs(1),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            start_host_input_focus(
                &controller,
                teardown_during_native,
                || {
                    native_calls.set(native_calls.get() + 1);
                    assert!(controller
                        .teardown_session(Some(session.session_epoch))
                        .is_some());
                    true
                },
                std::time::Instant::now,
                || {
                    event_calls.set(event_calls.get() + 1);
                    true
                },
            ),
            HostInputFocusStart::Noop
        );
        assert_eq!(native_calls.get(), 4);
        assert_eq!(event_calls.get(), 0);
        assert!(controller.live_identity().is_none());
    }

    #[test]
    fn plugin_panel_focus_commands_split_content_request_and_main_ack_capabilities() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("commands test module marker is missing");
        let request = production
            .split("pub(crate) async fn plugin_panel_focus_host_input(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("panel focus request command is missing");
        assert!(request.contains("webview: tauri::Webview"));
        assert!(request.contains("webview.label()"));
        assert!(request.contains("session_epoch: String"));
        assert!(!request.contains("plugin_id: String"));
        assert!(request.contains("focus_main_window_content_now(&focus_app)"));

        let native_focus = production
            .split("fn focus_main_window_content_now(")
            .nth(1)
            .and_then(|tail| tail.split("\nasync fn focus_main_window_content").next())
            .expect("main content focus helper is missing");
        let window_lookup = native_focus
            .find("get_window(\"main\")")
            .expect("main window lookup is missing");
        let webview_lookup = native_focus
            .find("get_webview(\"main\")")
            .expect("main webview lookup is missing");
        let window_focus = native_focus
            .find("main_window.set_focus()")
            .expect("main window focus request is missing");
        let webview_focus = native_focus
            .find("main_webview.set_focus()")
            .expect("main webview focus request is missing");
        assert!(window_lookup < webview_lookup && webview_lookup < window_focus);
        assert!(window_focus < webview_focus);

        let ack = production
            .split("pub(crate) fn plugin_panel_focus_host_input_ack(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("panel focus acknowledgement command is missing");
        let guard = ack
            .find("require_main_label(webview.label())?")
            .expect("main caller guard is missing");
        let settlement = ack
            .find("ack_host_input_focus(")
            .expect("focus acknowledgement settlement is missing");
        assert!(guard < settlement);

        let request_permission = ["al", "low", "-plugin-panel-focus-host-input"].concat();
        let ack_permission = [request_permission.as_str(), "-ack"].concat();
        let content = include_str!("../capabilities/plugin-panel-content.json");
        assert!(content.contains(&request_permission));
        assert!(!content.contains(&ack_permission));
        let main = include_str!("../capabilities/main.json");
        assert!(main.contains(&ack_permission));
        assert!(!main.contains(&format!("{request_permission}\"")));
        assert!(main.contains("\"webviews\": [\"main\"]"));
        assert!(!main.contains("\"windows\":"));
    }

    #[test]
    fn plugin_window_timer_and_storage_commands_are_content_only_and_session_bound() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        for command in [
            "plugin_window_storage_get",
            "plugin_window_storage_set",
            "plugin_window_storage_remove",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
        ] {
            let body = source
                .split(&format!("pub(crate) fn {command}("))
                .nth(1)
                .and_then(|tail| tail.split("\n#[tauri::command]").next())
                .unwrap_or_else(|| panic!("missing plugin window command {command}"));
            assert!(body.contains("webview: tauri::Webview"));
            assert!(body.contains("session_generation: String"));
            assert!(!body.contains("plugin_id: String"));
        }
        let capability = include_str!("../capabilities/plugin-window-content.json");
        for command in [
            "plugin_window_storage_get",
            "plugin_window_storage_set",
            "plugin_window_storage_remove",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
        ] {
            let permission = format!("{}-{}", ["al", "low"].concat(), command.replace('_', "-"));
            assert!(capability.contains(&permission));
        }
    }

    #[test]
    fn window_session_generation_is_canonical_nonzero_u64() {
        for value in ["1", "9", "10", "18446744073709551615"] {
            assert!(parse_timer_session_generation(value).is_ok());
            assert!(parse_window_storage_session_generation(value).is_ok());
        }
        for value in ["", "0", "00", "01", "-1", "+1", "18446744073709551616"] {
            assert_eq!(
                parse_timer_session_generation(value),
                Err(TimerError::ExpiredWindowSessionError)
            );
            assert_eq!(
                parse_window_storage_session_generation(value),
                Err(WindowStorageError::ExpiredWindowSessionError)
            );
        }
    }
    #[test]
    fn search_rejects_old_or_hidden_queries_before_state_reads() {
        let registry = ResultRegistry::default();
        assert!(search_apps_with(
            &registry,
            "app",
            "old",
            1,
            || panic!("rejected query must not read cache"),
            |_| panic!("rejected query must not read settings"),
            WebSearchEngine::Bing,
        )
        .is_none());

        registry.on_show("current".into());
        assert!(search_apps_with(
            &registry,
            "app",
            "old",
            2,
            || panic!("old invocation must not read cache"),
            |_| panic!("old invocation must not read settings"),
            WebSearchEngine::Bing,
        )
        .is_none());
    }

    #[test]
    fn search_caps_results_and_keeps_actions_private() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with(
            &registry,
            "app",
            "invocation",
            1,
            || (0..25).map(application).collect(),
            |_| {},
            WebSearchEngine::Bing,
        )
        .unwrap();

        assert_eq!(response.items.len(), 22);
        assert_eq!(response.items[0].title, "App 00");
        assert_eq!(response.items[20].title, "/find");
        assert_eq!(response.items[21].title, "Bing 搜索");
        assert_eq!(response.items[21].subtitle.as_deref(), Some("搜索：app"));
        assert_eq!(
            registry
                .resolve(&response.request_id, &response.items[21].result_id)
                .unwrap(),
            ResultAction::OpenWebSearch {
                engine: WebSearchEngine::Bing,
                query: "app".into(),
            }
        );
        let json = serde_json::to_string(&response).unwrap();
        for private in ["appId", "Private", "shortcut", "executable"] {
            assert!(!json.contains(private));
        }
        assert!(registry
            .resolve(&response.request_id, &response.items[21].result_id)
            .is_ok());
    }

    #[test]
    fn browser_search_result_snapshots_selected_engine() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        for (sequence, engine, expected_title) in [
            (1, WebSearchEngine::Bing, "Bing 搜索"),
            (2, WebSearchEngine::Baidu, "百度搜索"),
            (3, WebSearchEngine::Google, "Google 搜索"),
        ] {
            let response = search_apps_with(
                &registry,
                "windows",
                "invocation",
                sequence,
                Vec::new,
                |_| {},
                engine,
            )
            .unwrap();

            assert_eq!(response.items[1].title, expected_title);
            assert_eq!(response.refresh_at_utc, None);
            assert_eq!(response.items[1].subtitle.as_deref(), Some("搜索：windows"));
            assert_eq!(
                response.items[1].icon_kind,
                Some(crate::model::ResultIconKind::WebSearch)
            );
            assert_eq!(
                registry
                    .resolve(&response.request_id, &response.items[1].result_id)
                    .unwrap(),
                ResultAction::OpenWebSearch {
                    engine,
                    query: "windows".into(),
                }
            );
        }
    }

    #[tokio::test]
    async fn currency_search_is_exclusive_and_keeps_exact_copy_private_without_other_reads() {
        let registry = ready_registry("currency");
        let favorites = BTreeSet::new();
        let response = search_apps_with_catalog_and_currency(
            &registry,
            CatalogSearchRequest {
                query: "100 USD to CNY",
                invocation_id: "currency",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &favorites,
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("recognized currency must not read plugins"),
            || panic!("recognized currency must not read the app cache"),
            |_| panic!("recognized currency must not decorate applications"),
            || panic!("recognized currency must not read the local date"),
            || panic!("recognized currency must not read the UTC clock"),
            |intent| async move {
                assert_eq!(intent.base.as_str(), "USD");
                assert_eq!(intent.quote.as_str(), "CNY");
                Ok(CurrencyOutput {
                    title: "713.245 CNY".into(),
                    subtitle: "100 USD · 1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03"
                        .into(),
                    copy_text: "713.245 CNY".into(),
                })
            },
        )
        .await
        .unwrap();

        assert!(response.replace_local_results);
        assert_eq!(response.refresh_at_utc, None);
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.title, "713.245 CNY");
        assert_eq!(
            item.subtitle.as_deref(),
            Some("100 USD · 1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03")
        );
        assert_eq!(
            serde_json::to_value(item).unwrap()["iconKind"],
            serde_json::json!("currency")
        );
        assert_eq!(
            item.activation,
            crate::model::LauncherResultActivation::ExecuteResult
        );
        assert!(item.has_default_action);
        assert_eq!(item.favorite, None);
        assert_eq!(item.detail, None);
        assert!(!item.result_id.is_empty());
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Ok(ResultAction::CopyBuiltInText {
                text: "713.245 CNY".into()
            })
        );
        let wire = serde_json::to_value(&response).unwrap();
        assert!(wire.get("refreshAtUtc").is_none());
        assert!(!wire.to_string().contains("CopyBuiltInText"));
    }

    #[test]
    fn currency_command_routes_before_catalog_settings_quicklinks_apps_and_plugins() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let command = source
            .split("pub(crate) async fn search_apps(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("search_apps command body is missing");
        let currency = command
            .find("resolve_currency_search(")
            .expect("currency early route is missing");
        for deferred in [
            "app.state::<Arc<AppCache>>()",
            "app.state::<SettingsStore>()",
            "let public = app.state::<Arc<PublicPluginService>>()",
            "app.state::<Arc<QuicklinksStore>>()",
            "settings.snapshot()",
            "quicklinks.list()",
        ] {
            assert!(
                currency < command.find(deferred).expect("catalog read is missing"),
                "currency route must precede {deferred}"
            );
        }
    }

    #[tokio::test]
    async fn currency_stale_fallback_dto_keeps_cache_date_and_fetch_time() {
        let registry = ready_registry("stale-currency-cache");
        let favorites = BTreeSet::new();
        let response = search_apps_with_catalog_and_currency(
            &registry,
            CatalogSearchRequest {
                query: "100 USD to CNY",
                invocation_id: "stale-currency-cache",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &favorites,
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("recognized currency must not read plugins"),
            || panic!("recognized currency must not read the app cache"),
            |_| panic!("recognized currency must not decorate applications"),
            || panic!("recognized currency must not read the local date"),
            || panic!("recognized currency must not read the UTC clock"),
            |_| async {
                Ok(CurrencyOutput {
                    title: "713.245 CNY".into(),
                    subtitle: "100 USD · 1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03 · 缓存 · 获取于 2026-09-04T01:02:03Z".into(),
                    copy_text: "713.245 CNY".into(),
                })
            },
        )
        .await
        .unwrap();

        assert_eq!(
            response.items[0].subtitle.as_deref(),
            Some("100 USD · 1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03 · 缓存 · 获取于 2026-09-04T01:02:03Z")
        );
        assert_eq!(response.refresh_at_utc, None);
    }

    #[tokio::test]
    async fn currency_partial_text_continues_the_existing_application_route() {
        let registry = ready_registry("partial-currency");
        let favorites = BTreeSet::new();
        let mut matching_application = application(1);
        matching_application.display_name = "100 USD".into();
        let response = search_apps_with_catalog_and_currency(
            &registry,
            CatalogSearchRequest {
                query: "100 USD",
                invocation_id: "partial-currency",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &favorites,
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| Ok(Vec::new()),
            || vec![matching_application],
            |_| {},
            || panic!("partial currency must not become date arithmetic"),
            || panic!("partial currency must not become time-zone evaluation"),
            |_| async { panic!("partial currency must not call the resolver") },
        )
        .await
        .unwrap();

        assert!(!response.replace_local_results);
        assert!(response.items.iter().any(|item| item.title == "100 USD"));
        assert!(response.items.iter().any(|item| item.title == "Bing 搜索"));
    }

    #[tokio::test]
    async fn currency_malformed_complete_codes_publish_exclusive_information_without_resolving() {
        for (sequence, query) in [
            (1, "100 USD to CNYY"),
            (2, "100 CNYY to USD"),
            (3, "100 USD to CN1"),
            (4, "100 CN1 to USD"),
        ] {
            let invocation_id = format!("malformed-currency-{sequence}");
            let registry = ready_registry(&invocation_id);
            let favorites = BTreeSet::new();
            let response = search_apps_with_catalog_and_currency(
                &registry,
                CatalogSearchRequest {
                    query,
                    invocation_id: &invocation_id,
                    query_sequence: sequence,
                    submit: false,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &favorites,
                    disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                    quicklinks: &[],
                },
                |_| panic!("owned malformed currency must not read plugins"),
                || panic!("owned malformed currency must not read the app cache"),
                |_| panic!("owned malformed currency must not decorate applications"),
                || panic!("owned malformed currency must not read the local date"),
                || panic!("owned malformed currency must not read the UTC clock"),
                |_| async { panic!("owned malformed currency must not call the resolver") },
            )
            .await
            .unwrap();

            assert!(response.replace_local_results, "query={query}");
            assert_eq!(response.items.len(), 1, "query={query}");
            let item = &response.items[0];
            assert_eq!(item.title, "不支持的货币代码", "query={query}");
            assert_eq!(
                item.subtitle.as_deref(),
                Some("请检查三字母 ISO 4217 代码"),
                "query={query}"
            );
            assert_eq!(
                item.activation,
                crate::model::LauncherResultActivation::Informational,
                "query={query}"
            );
        }
    }

    #[tokio::test]
    async fn currency_words_and_symbols_continue_the_application_route() {
        for (sequence, query) in [
            (1, "100 USD to yuan"),
            (2, "100 dollars to CNY"),
            (3, "$100 USD to CNY"),
            (4, "¥100 JPY to USD"),
        ] {
            let invocation_id = format!("currency-fallthrough-{sequence}");
            let registry = ready_registry(&invocation_id);
            let favorites = BTreeSet::new();
            let mut matching_application = application(sequence as usize);
            matching_application.display_name = query.into();
            let response = search_apps_with_catalog_and_currency(
                &registry,
                CatalogSearchRequest {
                    query,
                    invocation_id: &invocation_id,
                    query_sequence: sequence,
                    submit: false,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &favorites,
                    disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                    quicklinks: &[],
                },
                |_| Ok(Vec::new()),
                || vec![matching_application],
                |_| {},
                || panic!("currency fallthrough must not become date arithmetic"),
                || panic!("currency fallthrough must not become time-zone evaluation"),
                |_| async { panic!("currency fallthrough must not call the resolver") },
            )
            .await
            .unwrap();

            assert!(!response.replace_local_results, "query={query}");
            assert!(
                response.items.iter().any(|item| item.title == query),
                "query={query}"
            );
            assert!(
                response.items.iter().any(|item| item.title == "Bing 搜索"),
                "query={query}"
            );
        }
    }

    #[tokio::test]
    async fn currency_query_begins_owner_before_await_and_discards_a_late_response() {
        let registry = ready_registry("late-currency");
        let favorites = BTreeSet::new();
        let (release, released) = tokio::sync::oneshot::channel();
        let pending = search_apps_with_catalog_and_currency(
            &registry,
            CatalogSearchRequest {
                query: "100 USD to CNY",
                invocation_id: "late-currency",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &favorites,
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |_| panic!("recognized currency must not read plugins"),
            || panic!("recognized currency must not read the app cache"),
            |_| panic!("recognized currency must not decorate applications"),
            || panic!("recognized currency must not read the local date"),
            || panic!("recognized currency must not read the UTC clock"),
            |_| async move {
                released.await.unwrap();
                Ok(CurrencyOutput {
                    title: "713.245 CNY".into(),
                    subtitle: "fresh".into(),
                    copy_text: "713.245 CNY".into(),
                })
            },
        );
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        assert!(registry
            .begin_query(QueryDomain::Application, "late-currency", 1)
            .is_none());
        assert!(registry
            .begin_query(QueryDomain::Application, "late-currency", 2)
            .is_some());

        release.send(()).unwrap();
        assert!(pending.await.is_none());
    }

    #[tokio::test]
    async fn currency_unsupported_and_unavailable_rows_are_informational_without_metadata() {
        for (sequence, error, title, subtitle) in [
            (
                1,
                CurrencyResolveError::Invalid(
                    crate::quick_evaluation::currency::CurrencyFailure::InvalidCode,
                ),
                "不支持的货币代码",
                "请检查三字母 ISO 4217 代码",
            ),
            (
                2,
                CurrencyResolveError::Unavailable,
                "汇率暂不可用",
                "请稍后重试",
            ),
        ] {
            let invocation_id = format!("currency-error-{sequence}");
            let registry = ready_registry(&invocation_id);
            let favorites = BTreeSet::new();
            let response = search_apps_with_catalog_and_currency(
                &registry,
                CatalogSearchRequest {
                    query: "100 USD to XYZ",
                    invocation_id: &invocation_id,
                    query_sequence: sequence,
                    submit: false,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &favorites,
                    disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                    quicklinks: &[],
                },
                |_| panic!("recognized currency must not read plugins"),
                || panic!("recognized currency must not read the app cache"),
                |_| panic!("recognized currency must not decorate applications"),
                || panic!("recognized currency must not read the local date"),
                || panic!("recognized currency must not read the UTC clock"),
                |_| async move { Err(error) },
            )
            .await
            .unwrap();

            assert_eq!(response.items.len(), 1);
            let item = &response.items[0];
            assert_eq!(item.title, title);
            assert_eq!(item.subtitle.as_deref(), Some(subtitle));
            assert_eq!(
                item.activation,
                crate::model::LauncherResultActivation::Informational
            );
            assert_eq!(item.icon_kind, Some(crate::model::ResultIconKind::Currency));
            assert!(!item.has_default_action);
            assert_eq!(item.favorite, None);
            assert_eq!(item.detail, None);
            assert_eq!(
                registry.resolve(&response.request_id, &item.result_id),
                Err(RegistryError::UnknownResult)
            );
            let wire = serde_json::to_value(&response).unwrap();
            assert!(wire.get("refreshAtUtc").is_none());
            assert!(wire["items"][0].get("favorite").is_none());
            assert!(wire["items"][0].get("detail").is_none());
        }
    }

    #[test]
    fn math_search_replaces_app_results_and_keeps_copy_private() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with_local_date(
            &registry,
            "2*(3+4)",
            "invocation",
            1,
            || panic!("math search must not read the app cache"),
            |_| panic!("math search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("arithmetic must not read the local date"),
        )
        .unwrap();

        assert!(response.replace_local_results);
        assert_eq!(response.refresh_at_utc, None);
        assert!(serde_json::to_value(&response)
            .unwrap()
            .get("refreshAtUtc")
            .is_none());
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "14");
        assert_eq!(response.items[0].subtitle.as_deref(), Some("复制结果"));
        assert_eq!(
            response.items[0].icon_kind,
            Some(crate::model::ResultIconKind::Calculator)
        );
        assert!(response.items[0].has_default_action);
        assert_eq!(
            registry
                .resolve(&response.request_id, &response.items[0].result_id)
                .unwrap(),
            ResultAction::CopyBuiltInText { text: "14".into() }
        );
    }

    #[test]
    fn unit_search_is_exclusive_and_keeps_exact_copy_private_without_reading_apps() {
        let registry = ResultRegistry::default();
        registry.on_show("unit".into());
        let response = search_apps_with_local_date(
            &registry,
            "1000 mm to in",
            "unit",
            1,
            || panic!("unit search must not read the app cache"),
            |_| panic!("unit search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("unit search must not read the local date"),
        )
        .unwrap();

        assert!(response.replace_local_results);
        assert_eq!(response.refresh_at_utc, None);
        assert!(serde_json::to_value(&response)
            .unwrap()
            .get("refreshAtUtc")
            .is_none());
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.title, "39.3701 in");
        assert_eq!(item.subtitle.as_deref(), Some("1000 mm · 长度换算"));
        assert_eq!(
            serde_json::to_value(item).unwrap()["iconKind"],
            serde_json::json!("unit")
        );
        assert_eq!(
            item.activation,
            crate::model::LauncherResultActivation::ExecuteResult
        );
        assert!(item.has_default_action);
        assert_eq!(item.favorite, None);
        assert_eq!(item.detail, None);
        assert!(!item.result_id.is_empty());
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Ok(ResultAction::CopyBuiltInText {
                text: "39.3701 in".into()
            })
        );
    }

    #[test]
    fn unit_us_volume_explanation_is_visible_on_the_command_wire_without_changing_copy() {
        const EXPLANATION: &str =
            "默认采用美制（US）。若需英制容量（加仑/夸脱/品脱/盎司），请在单位前加 imp，例如 100 imp gal to L。";
        let registry = ResultRegistry::default();
        registry.on_show("us-volume-unit".into());
        let response = search_apps_with_local_date(
            &registry,
            "1 gal to L",
            "us-volume-unit",
            1,
            || panic!("unit search must not read the app cache"),
            |_| panic!("unit search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("unit search must not read the local date"),
        )
        .unwrap();

        assert!(response.replace_local_results);
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.title, "3.78541 L");
        assert_eq!(
            item.subtitle.as_deref(),
            Some(format!("1 gal · 体积换算 · {EXPLANATION}").as_str())
        );
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Ok(ResultAction::CopyBuiltInText {
                text: "3.78541 L".into()
            })
        );
        assert_eq!(
            serde_json::to_value(&response).unwrap()["items"][0]["subtitle"],
            format!("1 gal · 体积换算 · {EXPLANATION}")
        );
    }

    #[test]
    fn unit_owned_failures_are_informational_and_register_no_action() {
        for (sequence, query, title, explanation) in [
            (1, "1 kg to L", "无法转换单位", "源单位与目标单位的维度不同"),
            (
                2,
                "1 kg to parsec",
                "无法识别单位",
                "请检查源单位和目标单位",
            ),
            (
                3,
                "-1 K to C",
                "温度低于绝对零度",
                "输入温度不能低于绝对零度",
            ),
        ] {
            let registry = ResultRegistry::default();
            registry.on_show(format!("unit-failure-{sequence}"));
            let response = search_apps_with_local_date(
                &registry,
                query,
                &format!("unit-failure-{sequence}"),
                sequence,
                || panic!("owned unit failure must not read the app cache"),
                |_| panic!("owned unit failure must not decorate applications"),
                WebSearchEngine::Bing,
                || panic!("owned unit failure must not read the local date"),
            )
            .unwrap();

            assert!(response.replace_local_results, "query={query}");
            assert_eq!(response.items.len(), 1, "query={query}");
            let item = &response.items[0];
            assert_eq!(item.title, title, "query={query}");
            assert_eq!(item.subtitle.as_deref(), Some(explanation), "query={query}");
            assert_eq!(item.icon_kind, Some(crate::model::ResultIconKind::Unit));
            assert_eq!(
                item.activation,
                crate::model::LauncherResultActivation::Informational
            );
            assert!(!item.has_default_action);
            assert_eq!(item.favorite, None);
            assert_eq!(item.detail, None);
            assert_eq!(
                registry.resolve(&response.request_id, &item.result_id),
                Err(RegistryError::UnknownResult),
                "query={query}"
            );
        }
    }

    #[test]
    fn unit_partial_and_unowned_text_keep_ordinary_results() {
        for (sequence, query) in [(1, "1 m to"), (2, "weather to forecast")] {
            let registry = ResultRegistry::default();
            let invocation_id = format!("ordinary-unit-{sequence}");
            registry.on_show(invocation_id.clone());
            let mut matching_application = application(sequence);
            matching_application.display_name = query.into();
            let response = search_apps_with_catalog(
                &registry,
                CatalogSearchRequest {
                    query,
                    invocation_id: &invocation_id,
                    query_sequence: sequence as u64,
                    submit: false,
                    web_search_engine: WebSearchEngine::Bing,
                    favorite_builtin_features: &BTreeSet::new(),
                    disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                    quicklinks: &[],
                },
                |_| Ok(vec![command_suggestion("demo")]),
                || vec![matching_application],
                |_| {},
            )
            .unwrap();

            assert!(!response.replace_local_results, "query={query}");
            assert!(
                response.items.iter().any(|item| item.title == query),
                "query={query}"
            );
            assert!(
                response.items.iter().any(|item| item.title == "Bing 搜索"),
                "query={query}"
            );
        }
    }

    #[test]
    fn unit_stale_publication_cannot_replace_a_newer_query_owner() {
        let registry = ResultRegistry::default();
        registry.on_show("stale-unit".into());
        let stale = registry
            .begin_query(QueryDomain::Application, "stale-unit", 1)
            .unwrap();
        let classification = crate::quick_evaluation::classify("1000 mm to in");
        assert!(registry
            .begin_query(QueryDomain::Application, "stale-unit", 2)
            .is_some());

        assert!(publish_quick_evaluation(
            &registry,
            stale,
            classification,
            || panic!("unit publication must not read the local date"),
            || panic!("unit publication must not read the UTC clock"),
        )
        .is_none());
    }

    #[test]
    fn time_search_uses_one_utc_instant_for_row_copy_and_refresh_metadata() {
        let registry = ResultRegistry::default();
        registry.on_show("time".into());
        let utc_clock_calls = Cell::new(0);
        let utc_now = time::Date::from_calendar_date(2026, time::Month::January, 15)
            .unwrap()
            .with_hms(5, 5, 20)
            .unwrap()
            .assume_utc();
        let response = search_apps_with_clocks(
            &registry,
            "Mumbai time",
            "time",
            1,
            || panic!("time search must not read the app cache"),
            |_| panic!("time search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("time search must not read the local date"),
            || {
                utc_clock_calls.set(utc_clock_calls.get() + 1);
                Ok(utc_now)
            },
        )
        .unwrap();

        assert_eq!(utc_clock_calls.get(), 1);
        assert!(response.replace_local_results);
        assert_eq!(
            response.refresh_at_utc.as_deref(),
            Some("2026-01-15T05:06:00Z")
        );
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.title, "10:35");
        assert_eq!(
            item.subtitle.as_deref(),
            Some("2026-01-15 · Mumbai · UTC+05:30 (IST)")
        );
        assert_eq!(item.icon_kind, Some(crate::model::ResultIconKind::TimeZone));
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Ok(ResultAction::CopyBuiltInText {
                text: item.title.clone()
            })
        );
        let wire = serde_json::to_value(&response).unwrap();
        assert_eq!(wire["refreshAtUtc"], "2026-01-15T05:06:00Z");
        assert_eq!(wire["items"][0]["iconKind"], "timeZone");
        assert!(!wire.to_string().contains("CopyBuiltInText"));
        assert!(!wire.to_string().contains("Asia/Kolkata"));
    }

    #[test]
    fn time_invalid_search_has_no_action_or_refresh_metadata() {
        let registry = ResultRegistry::default();
        registry.on_show("invalid-time".into());
        let response = search_apps_with_clocks(
            &registry,
            "Atlantis time",
            "invalid-time",
            1,
            || panic!("invalid time search must not read the app cache"),
            |_| panic!("invalid time search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("invalid time search must not read the local date"),
            || panic!("invalid time search must not read the UTC clock"),
        )
        .unwrap();

        assert_eq!(response.refresh_at_utc, None);
        assert!(serde_json::to_value(&response)
            .unwrap()
            .get("refreshAtUtc")
            .is_none());
        let item = &response.items[0];
        assert_eq!(item.title, "无法识别时区");
        assert_eq!(item.icon_kind, Some(crate::model::ResultIconKind::TimeZone));
        assert_eq!(
            item.activation,
            crate::model::LauncherResultActivation::Informational
        );
        assert!(!item.has_default_action);
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Err(RegistryError::UnknownResult)
        );
    }

    #[test]
    fn time_stale_publication_returns_no_response_or_refresh_metadata() {
        let registry = ResultRegistry::default();
        registry.on_show("stale-time".into());
        let utc_clock_calls = Cell::new(0);
        let utc_now = time::Date::from_calendar_date(2026, time::Month::September, 3)
            .unwrap()
            .with_hms(14, 35, 20)
            .unwrap()
            .assume_utc();

        let response = search_apps_with_clocks(
            &registry,
            "UTC time",
            "stale-time",
            1,
            || panic!("time search must not read the app cache"),
            |_| panic!("time search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("time search must not read the local date"),
            || {
                utc_clock_calls.set(utc_clock_calls.get() + 1);
                assert!(registry
                    .begin_query(QueryDomain::Application, "stale-time", 2)
                    .is_some());
                Ok(utc_now)
            },
        );

        assert_eq!(utc_clock_calls.get(), 1);
        assert!(response.is_none());
    }

    #[test]
    fn date_search_publishes_one_executable_row_with_private_copy_text() {
        let registry = ResultRegistry::default();
        registry.on_show("date".into());
        let local_date_calls = Cell::new(0);
        let response = search_apps_with_local_date(
            &registry,
            "today + 7 days",
            "date",
            1,
            || panic!("date search must not read the app cache"),
            |_| panic!("date search must not decorate applications"),
            WebSearchEngine::Bing,
            || {
                local_date_calls.set(local_date_calls.get() + 1);
                time::Date::from_calendar_date(2026, time::Month::September, 3).map_err(|_| ())
            },
        )
        .unwrap();

        assert_eq!(local_date_calls.get(), 1);
        assert!(response.replace_local_results);
        assert_eq!(response.refresh_at_utc, None);
        assert!(serde_json::to_value(&response)
            .unwrap()
            .get("refreshAtUtc")
            .is_none());
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.title, "2026-09-10");
        assert_eq!(item.subtitle.as_deref(), Some("Thursday · today + 7 days"));
        assert_eq!(item.icon_kind, Some(crate::model::ResultIconKind::Date));
        assert_eq!(
            item.activation,
            crate::model::LauncherResultActivation::ExecuteResult
        );
        assert!(item.has_default_action);
        assert_eq!(item.favorite, None);
        assert_eq!(item.detail, None);
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Ok(ResultAction::CopyBuiltInText {
                text: "2026-09-10".into()
            })
        );
    }

    #[test]
    fn invalid_date_search_is_informational_and_registers_no_action() {
        let registry = ResultRegistry::default();
        registry.on_show("invalid-date".into());
        let response = search_apps_with_local_date(
            &registry,
            "today + 1 fortnight",
            "invalid-date",
            1,
            || panic!("invalid date search must not read the app cache"),
            |_| panic!("invalid date search must not decorate applications"),
            WebSearchEngine::Bing,
            || panic!("classified invalid dates must not read the local date"),
        )
        .unwrap();

        assert!(response.replace_local_results);
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.title, "日期表达式无效");
        assert_eq!(
            item.subtitle.as_deref(),
            Some("请使用 today 或 YYYY-MM-DD 加减非负整数日期单位")
        );
        assert_eq!(
            item.activation,
            crate::model::LauncherResultActivation::Informational
        );
        assert!(!item.has_default_action);
        assert_eq!(item.favorite, None);
        assert_eq!(item.detail, None);
        assert_eq!(
            registry.resolve(&response.request_id, &item.result_id),
            Err(RegistryError::UnknownResult)
        );
    }

    #[test]
    fn local_date_failure_publishes_explicit_information_without_utc_fallback() {
        let registry = ResultRegistry::default();
        registry.on_show("clock-failure".into());
        let calls = Cell::new(0);
        let response = search_apps_with_local_date(
            &registry,
            "today + 1 day",
            "clock-failure",
            1,
            || panic!("clock failure must not read the app cache"),
            |_| panic!("clock failure must not decorate applications"),
            WebSearchEngine::Bing,
            || {
                calls.set(calls.get() + 1);
                Err(())
            },
        )
        .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "无法读取本地日期");
        assert_eq!(
            response.items[0].subtitle.as_deref(),
            Some("请检查 Windows 日期和时区设置")
        );
        assert_eq!(
            response.items[0].activation,
            crate::model::LauncherResultActivation::Informational
        );
        assert_eq!(
            registry.resolve(&response.request_id, &response.items[0].result_id),
            Err(RegistryError::UnknownResult)
        );
    }

    #[test]
    fn incomplete_date_expression_keeps_the_ordinary_catalog() {
        let registry = ResultRegistry::default();
        registry.on_show("incomplete-date".into());
        let mut matching_application = application(1);
        matching_application.display_name = "today + 1".into();
        let response = search_apps_with_catalog(
            &registry,
            CatalogSearchRequest {
                query: "today + 1",
                invocation_id: "incomplete-date",
                query_sequence: 1,
                submit: false,
                web_search_engine: WebSearchEngine::Bing,
                favorite_builtin_features: &BTreeSet::new(),
                disabled_builtin_features: &EMPTY_DISABLED_BUILTINS,
                quicklinks: &[],
            },
            |query| {
                assert_eq!(query, "today + 1");
                Ok(vec![command_suggestion("demo")])
            },
            || vec![matching_application],
            |_| {},
        )
        .unwrap();

        let titles = response
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>();
        assert!(titles.contains(&"today + 1"));
        assert!(titles.contains(&"/find"));
        assert!(titles.contains(&"Bing 搜索"));
        assert!(titles.contains(&"/demo"));
        assert!(!response.replace_local_results);
    }

    #[test]
    fn stale_date_publication_cannot_replace_a_newer_application_owner() {
        let registry = ResultRegistry::default();
        registry.on_show("stale-date".into());
        assert!(search_apps_with_local_date(
            &registry,
            "today + 1 day",
            "stale-date",
            1,
            || panic!("stale date must not read the app cache"),
            |_| panic!("stale date must not decorate applications"),
            WebSearchEngine::Bing,
            || {
                assert!(registry
                    .begin_query(QueryDomain::Application, "stale-date", 2)
                    .is_some());
                time::Date::from_calendar_date(2026, time::Month::September, 3).map_err(|_| ())
            },
        )
        .is_none());
    }
    #[test]
    fn slash_input_does_not_offer_browser_search() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with(
            &registry,
            &["/", "demo value"].concat(),
            "invocation",
            1,
            Vec::new,
            |_| {},
            WebSearchEngine::Bing,
        )
        .unwrap();

        assert!(response.items.is_empty());
    }

    #[test]
    fn search_publish_loses_newer_query_and_hide_races() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        assert!(search_apps_with(
            &registry,
            "app",
            "invocation",
            1,
            || vec![application(1)],
            |_| {
                assert!(registry
                    .begin_query(QueryDomain::Application, "invocation", 2)
                    .is_some());
            },
            WebSearchEngine::Bing,
        )
        .is_none());

        registry.on_show("next".into());
        assert!(search_apps_with(
            &registry,
            "app",
            "next",
            1,
            || vec![application(1)],
            |_| registry.hide_and_clear(),
            WebSearchEngine::Bing,
        )
        .is_none());
    }

    #[test]
    fn search_empty_query_publishes_only_builtin_completions() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with(
            &registry,
            "",
            "invocation",
            1,
            || panic!("empty query must not read applications"),
            |_| panic!("empty query must not decorate applications"),
            WebSearchEngine::Bing,
        )
        .unwrap();
        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["/find", "/quicklinks", "/screenshot", "/web-search"]
        );
    }

    #[test]
    fn file_query_accepts_only_nonempty_all_modified_desc() {
        for category in [
            "all", "folder", "excel", "word", "ppt", "pdf", "image", "video", "audio", "archive",
        ] {
            assert!(
                prepare_file_query(
                    "report".into(),
                    category.into(),
                    "modifiedDesc".into(),
                    "inv".into(),
                    1,
                )
                .is_ok(),
                "category {category} should be accepted",
            );
        }
        for invalid in [
            ("", "all", "modifiedDesc"),
            ("report", "all", "modifiedAsc"),
        ] {
            assert!(matches!(
                prepare_file_query(
                    invalid.0.into(),
                    invalid.1.into(),
                    invalid.2.into(),
                    "inv".into(),
                    1,
                ),
                Err(error) if error == CommandError::invalid_file_query()
            ));
        }

        let prepared = prepared_query("RePort", "inv", 1);
        assert_eq!(prepared.query, "RePort");
    }

    #[test]
    fn file_query_preserves_existing_wire_limits() {
        assert!(prepare_file_query(
            "x".repeat(255),
            "all".into(),
            "modifiedDesc".into(),
            "inv".into(),
            1,
        )
        .is_ok());
        for (query, invocation_id, query_sequence) in [
            ("x".repeat(1_025), "inv".into(), 1),
            ("x".repeat(256), "inv".into(), 1),
            ("bad\0query".into(), "inv".into(), 1),
            ("ok".into(), String::new(), 1),
            ("ok".into(), "inv".into(), 0),
        ] {
            assert!(matches!(
                prepare_file_query(
                    query,
                    "all".into(),
                    "modifiedDesc".into(),
                    invocation_id,
                    query_sequence,
                ),
                Err(error) if error == CommandError::invalid_file_query()
            ));
        }
    }

    #[test]
    fn production_file_search_uses_everything_once_and_never_legacy_index() {
        let calls = Arc::new(AtomicUsize::new(0));
        let worker_calls = Arc::clone(&calls);
        let controller = crate::find_window::FindWindowController::default();
        controller.set_visible_for_test("inv");
        let response = tauri::async_runtime::block_on(search_files_with(
            &ready_registry("inv"),
            &controller,
            prepared_query("report", "inv", 1),
            move |query, category| {
                assert_eq!(query, "report");
                assert_eq!(category, FileCategory::All);
                worker_calls.fetch_add(1, Ordering::AcqRel);
                Ok(everything_batch_for_test(7, 2))
            },
        ))
        .unwrap()
        .unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(response.index_revision, "7");
        assert_eq!(response.total, "2");
        assert_eq!(response.status, FileIndexStatus::Ready);
    }

    #[test]
    fn stale_everything_query_consumes_no_publication_slot() {
        let registry = ready_registry("inv");
        let old = registry.begin_query(QueryDomain::File, "inv", 1).unwrap();
        let _new = registry.begin_query(QueryDomain::File, "inv", 2).unwrap();
        assert!(
            publish_everything_search(&registry, old, everything_batch_for_test(8, 1)).is_none()
        );
    }

    #[test]
    fn search_files_everything_failures_map_to_stable_path_free_errors() {
        for error in [
            EverythingSearchError::Unavailable,
            EverythingSearchError::RevisionExhausted,
        ] {
            let command = map_everything_search_error(error);
            assert_eq!(command, CommandError::search_unavailable());
            assert!(!command.message.contains('\\'));
            assert!(!command.message.contains(':'));
        }
        let starting = map_everything_search_error(EverythingSearchError::Starting);
        assert_eq!(starting, CommandError::file_search_starting());
        assert_ne!(starting, CommandError::search_unavailable());
        assert!(!starting.message.contains("runtime"));
    }

    #[test]
    fn search_files_caller_guard_is_first_statement() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let start = source
            .find("pub(crate) async fn search_files(")
            .expect("search_files command missing");
        let body = &source[start..];
        let body = &body[..body.find("\n}").expect("search_files body missing")];
        let guard = body
            .find("require_find_window(&window)?;")
            .expect("find guard missing");
        let opening_brace = body.find('{').expect("search_files opening brace missing");
        assert!(body[opening_brace + 1..]
            .trim_start()
            .starts_with("require_find_window(&window)?;"));
        for forbidden in [
            "registry.inner()",
            "prepare_file_query(",
            "begin_query(",
            "spawn_blocking",
        ] {
            assert!(
                body[..guard].find(forbidden).is_none(),
                "{forbidden} occurs before caller guard"
            );
        }
        assert!(body.contains("everything_search.inner()"));
        for forbidden in [
            "file_index.inner()",
            "state::<Arc<FileIndex>>()",
            "app.path()",
            "app_data_dir",
            "FileIndex::search",
        ] {
            assert!(
                !body.contains(forbidden),
                "search_files contains {forbidden}"
            );
        }
    }

    #[test]
    fn settings_load_uses_alias_free_wire_contract() {
        let dir = TestDir::new();
        let store = settings_store(&dir);

        assert_eq!(
            serde_json::to_value(load_settings_core(&store)).unwrap(),
            serde_json::json!({
                "screenshotHotkey": null,
                "screenshotHotkeyStatus": "unset",
                "screenshotSaveDirectory": null,
                "screenshotSaveDirectoryPath": null,
                "hotkey": "Alt+Space",
                "autostart": false,
                "filePreviewEnabled": true,
                "theme": "system",
                "webSearchEngine": "bing"
            })
        );
    }

    #[test]
    fn theme_preference_input_is_exact() {
        assert_eq!(
            serde_json::from_value::<ThemePreferenceUpdate>(serde_json::json!({
                "theme": "dark"
            }))
            .unwrap()
            .theme,
            ThemePreference::Dark
        );
        assert!(
            serde_json::from_value::<ThemePreferenceUpdate>(serde_json::json!({
                "theme": "sepia"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ThemePreferenceUpdate>(serde_json::json!({
                "theme": "dark",
                "extra": true
            }))
            .is_err()
        );
    }

    #[test]
    fn theme_preference_maps_worker_and_storage_failures() {
        assert_eq!(map_theme_preference_worker_result(Ok(Ok(()))), Ok(()));
        assert_eq!(
            map_theme_preference_worker_result(Ok(Err(()))),
            Err(CommandError::settings_failed())
        );
        assert_eq!(
            map_theme_preference_worker_result(Err(())),
            Err(CommandError::settings_failed())
        );

        let dir = TestDir::new();
        let store = settings_store(&dir);
        let before = store.snapshot();
        let current = dir.path().join("settings.json");
        let before_disk = fs::read(&current).unwrap();
        fs::remove_file(&current).unwrap();
        fs::create_dir(&current).unwrap();
        assert_eq!(
            store.set_theme_preference_with_revision(ThemePreference::Dark),
            Err(crate::settings::SettingsError::Storage)
        );
        assert_eq!(store.snapshot(), before);
        fs::remove_dir(&current).unwrap();
        fs::write(&current, &before_disk).unwrap();
        assert_eq!(fs::read(current).unwrap(), before_disk);
    }

    #[test]
    fn theme_preference_command_is_narrow_and_guarded_first() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let production = source.split(&marker).next().unwrap();
        let start = production
            .find("pub(crate) async fn set_theme_preference(")
            .expect("set_theme_preference command missing");
        let end = production[start..]
            .find("#[tauri::command]\npub(crate) async fn set_web_search_engine")
            .map(|offset| start + offset)
            .expect("set_theme_preference command end missing");
        let command = &production[start..end];
        let first = command[command.find('{').unwrap() + 1..].trim_start();
        assert!(first.starts_with("require_main_window(&window)?;"));
        assert_eq!(command.matches(".state::<SettingsStore>()").count(), 1);
        assert!(!command.contains("save_settings_transaction"));
        assert!(!command.contains("reconcile_runtime_settings"));
        let guard = command.find("require_main_window(&window)?;").unwrap();
        for forbidden in ["reserve_critical", "state::<SettingsStore>"] {
            assert!(guard < command.find(forbidden).unwrap());
        }
    }

    #[test]
    fn web_search_engine_command_is_narrow_and_guarded_first() {
        let source = include_str!("commands.rs").replace(
            "
", "
",
        );
        let marker = [
            "#[cfg(",
            "test",
            ")]
mod tests",
        ]
        .concat();
        let production = source.split(&marker).next().unwrap();
        let start = production
            .find("pub(crate) async fn set_web_search_engine(")
            .expect("set_web_search_engine command missing");
        let end = production[start..]
            .find("pub(crate) async fn set_builtin_feature_favorite(")
            .map(|offset| start + offset)
            .expect("set_web_search_engine command end missing");
        let command = &production[start..end];
        let first = command[command.find('{').unwrap() + 1..].trim_start();

        assert!(first.starts_with("require_main_window(&window)?;"));
        assert_eq!(command.matches(".state::<SettingsStore>()").count(), 1);
        assert!(!command.contains("reconcile_runtime_settings"));
        let guard = command.find("require_main_window(&window)?;").unwrap();
        for forbidden in ["reserve_critical", "state::<SettingsStore>"] {
            assert!(guard < command.find(forbidden).unwrap());
        }
    }

    #[test]
    fn builtin_feature_favorite_command_is_narrow_and_guarded_first() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let start = production
            .find("pub(crate) async fn set_builtin_feature_favorite(")
            .expect("set_builtin_feature_favorite command missing");
        let end = production[start..]
            .find("async fn set_file_preview_preference_with")
            .map(|offset| start + offset)
            .expect("set_builtin_feature_favorite command end missing");
        let command = &production[start..end];
        let first = command[command.find('{').unwrap() + 1..].trim_start();

        assert!(first.starts_with("require_main_window(&window)?;"));
        assert_eq!(command.matches(".state::<SettingsStore>()").count(), 1);
        let guard = command.find("require_main_window(&window)?;").unwrap();
        for forbidden in ["reserve_critical", "state::<SettingsStore>"] {
            assert!(guard < command.find(forbidden).unwrap());
        }
    }
    #[test]
    fn settings_research_id_is_rejected_from_the_update_contract() {
        let dir = TestDir::new();
        let store = settings_store(&dir);
        let view_json = serde_json::to_value(load_settings_core(&store)).unwrap();

        assert!(!view_json.as_object().unwrap().contains_key("researchId"));
        assert!(
            serde_json::from_value::<UserSettingsUpdate>(serde_json::json!({
                "hotkey": "Alt+Space",
                "autostart": false,
                "researchId": null
            }))
            .is_err()
        );
    }

    #[test]
    fn settings_update_preserves_use_counts() {
        let dir = TestDir::new();
        let store = settings_store(&dir);
        let before = store.snapshot().use_counts;

        save_settings_core(
            UserSettingsUpdate {
                hotkey: "Ctrl+Space".into(),
                autostart: true,
                theme: ThemePreference::Dark,
                web_search_engine: WebSearchEngine::Google,
            },
            &store,
        )
        .unwrap();

        assert_eq!(store.snapshot().use_counts, before);
        assert_eq!(store.snapshot().theme, ThemePreference::Dark);
    }

    #[test]
    fn settings_wire_contract_carries_and_persists_web_search_engine() {
        for engine in ["bing", "baidu", "google"] {
            let dir = TestDir::new();
            let store = settings_store(&dir);
            let input: UserSettingsUpdate = serde_json::from_value(serde_json::json!({
                "hotkey": "Ctrl+Space",
                "autostart": false,
                "theme": "system",
                "webSearchEngine": engine
            }))
            .unwrap();

            save_settings_core(input, &store).unwrap();

            assert_eq!(
                serde_json::to_value(load_settings_core(&store)).unwrap()["webSearchEngine"],
                engine
            );
            assert_eq!(
                serde_json::to_value(store.snapshot()).unwrap()["webSearchEngine"],
                engine
            );
        }
    }
    #[test]
    fn readiness_load_settings_guards_then_marks_ready_before_store_reads() {
        for (label, expected) in [
            ("secondary", Err(CommandError::invalid_caller())),
            ("main", Ok(17)),
        ] {
            let trace = RefCell::new(Vec::new());
            let result = require_main_label(label).and_then(|()| {
                trace.borrow_mut().push("caller-guard");
                load_settings_ready_with(
                    || {
                        trace.borrow_mut().push("frontend-ready");
                        Ok(())
                    },
                    || {
                        trace.borrow_mut().push("settings-snapshot");
                        17
                    },
                )
            });

            assert_eq!(result, expected);
            if label == "main" {
                assert_eq!(
                    *trace.borrow(),
                    ["caller-guard", "frontend-ready", "settings-snapshot"]
                );
            } else {
                assert!(trace.borrow().is_empty());
            }
        }
    }

    #[test]
    fn readiness_load_settings_keeps_hidden_startup_and_drains_early_target_once() {
        let trace = RefCell::new(Vec::new());
        let shows = Cell::new(0);
        let pending = Cell::new(false);
        let mark_frontend_ready = || {
            trace.borrow_mut().push("frontend-ready");
            if pending.replace(false) {
                shows.set(shows.get() + 1);
                trace.borrow_mut().push("show-pending");
            }
            Ok(())
        };

        assert_eq!(
            load_settings_ready_with(mark_frontend_ready, || {
                trace.borrow_mut().push("stores");
                1
            }),
            Ok(1)
        );
        assert_eq!(*trace.borrow(), ["frontend-ready", "stores"]);
        assert_eq!(shows.get(), 0);

        trace.borrow_mut().clear();
        pending.set(true);
        for expected in [2, 3] {
            assert_eq!(
                load_settings_ready_with(
                    || {
                        trace.borrow_mut().push("frontend-ready");
                        if pending.replace(false) {
                            shows.set(shows.get() + 1);
                            trace.borrow_mut().push("show-pending");
                        }
                        Ok(())
                    },
                    || {
                        trace.borrow_mut().push("stores");
                        expected
                    },
                ),
                Ok(expected)
            );
        }
        assert_eq!(
            *trace.borrow(),
            [
                "frontend-ready",
                "show-pending",
                "stores",
                "frontend-ready",
                "stores"
            ]
        );
        assert_eq!(shows.get(), 1);
    }

    #[test]
    fn execute_stale_or_unknown_result_stops_before_all_side_effects() {
        for registry_error in [RegistryError::StaleRequest, RegistryError::UnknownResult] {
            let side_effects = Cell::new(0);
            let result = execute_result_with(
                ("request", "result"),
                |request_id, result_id| {
                    assert_eq!(request_id, "request");
                    assert_eq!(result_id, "result");
                    Err(registry_error)
                },
                |_| {
                    side_effects.set(side_effects.get() + 1);
                    unreachable!()
                },
                || {
                    side_effects.set(side_effects.get() + 1);
                    Ok(())
                },
                |_| {
                    side_effects.set(side_effects.get() + 1);
                    Ok(())
                },
            );

            let expected = match registry_error {
                RegistryError::StaleRequest => CommandError::stale_request(),
                RegistryError::UnknownResult => CommandError::unknown_result(),
            };
            assert_eq!(result, Err(expected));
            assert_eq!(side_effects.get(), 0);
        }
    }

    #[test]
    fn execute_file_marker_fails_closed_before_application_side_effects() {
        let execute_calls = Cell::new(0);
        let later_calls = Cell::new(0);
        let result = execute_result_with(
            ("request", "result"),
            |_, _| {
                Ok(ResultAction::OpenFile(FileExecutionAction::Indexed(
                    file_action(4, "blocked.txt", IndexedKind::File),
                )))
            },
            |_| {
                execute_calls.set(execute_calls.get() + 1);
                Ok(ApplicationActionOutcome::LaunchRequested)
            },
            || {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
            |_| {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(result, Err(CommandError::application_entry_unavailable()));
        assert_eq!(execute_calls.get(), 0);
        assert_eq!(later_calls.get(), 0);
    }

    #[test]
    fn execute_success_clears_and_hides_before_persistence_in_order() {
        let cases = [
            (
                ApplicationActionOutcome::LaunchRequested,
                ExecuteOutcome::LaunchRequested,
            ),
            (
                ApplicationActionOutcome::ActivationRequested,
                ExecuteOutcome::ActivationRequested,
            ),
            (
                ApplicationActionOutcome::ActivationRefusedLaunchRequested,
                ExecuteOutcome::ActivationRefusedLaunchRequested {
                    message: "Windows 拒绝了前台切换，已发送启动请求",
                },
            ),
        ];

        for (action_outcome, expected_outcome) in cases {
            let trace = RefCell::new(Vec::new());
            let result = execute_result_with(
                ("request", "result"),
                |_, _| {
                    trace.borrow_mut().push("resolve");
                    Ok(trusted_action())
                },
                |_| {
                    trace.borrow_mut().push("system-action");
                    Ok(action_outcome)
                },
                || {
                    trace.borrow_mut().push("registry-hide-and-clear");
                    trace.borrow_mut().push("window-hide");
                    Ok(())
                },
                |app_id| {
                    trace.borrow_mut().push("settings-increment");
                    assert_eq!(app_id, APP_CURRENT);
                    Ok(())
                },
            );

            assert_eq!(result, Ok(expected_outcome));
            assert_eq!(
                *trace.borrow(),
                [
                    "resolve",
                    "system-action",
                    "registry-hide-and-clear",
                    "window-hide",
                    "settings-increment",
                ]
            );
        }
    }

    #[test]
    fn execute_uses_fixed_post_action_error_priority_and_runs_every_step_once() {
        let cases = [
            (true, false, CommandError::settings_failed()),
            (false, true, CommandError::window_failed()),
            (true, true, CommandError::settings_failed()),
        ];

        for (settings_fails, hide_fails, expected) in cases {
            let actions = Cell::new(0);
            let helpers = Cell::new(0);
            let increments = Cell::new(0);
            let hides = Cell::new(0);
            let result = execute_result_with(
                ("request", "result"),
                |_, _| Ok(trusted_action()),
                |_| {
                    actions.set(actions.get() + 1);
                    Ok(ApplicationActionOutcome::LaunchRequested)
                },
                || {
                    helpers.set(helpers.get() + 1);
                    hides.set(hides.get() + 1);
                    if hide_fails {
                        Err(CommandError::window_failed())
                    } else {
                        Ok(())
                    }
                },
                |_| {
                    increments.set(increments.get() + 1);
                    if settings_fails {
                        Err(())
                    } else {
                        Ok(())
                    }
                },
            );

            assert_eq!(result, Err(expected));
            assert_eq!(actions.get(), 1);
            assert_eq!(helpers.get(), 1);
            assert_eq!(increments.get(), 1);
            assert_eq!(hides.get(), 1);
        }
    }

    #[test]
    fn execute_system_action_failure_preserves_registry_window_and_counts() {
        let later_calls = Cell::new(0);
        let result = execute_result_with(
            ("request", "result"),
            |_, _| Ok(trusted_action()),
            |_| Err(()),
            || {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
            |_| {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(result, Err(CommandError::application_entry_unavailable()));
        assert_eq!(later_calls.get(), 0);
    }

    #[test]
    fn maintenance_shared_clear_and_hide_saves_after_successful_hide() {
        let trace = RefCell::new(Vec::new());
        let position = crate::settings::WindowPosition { x: 40, y: -20 };
        let result = clear_and_hide_with(
            || {
                trace.borrow_mut().push("position");
                Ok(position)
            },
            || {
                trace.borrow_mut().push("clear");
            },
            || {
                trace.borrow_mut().push("hide");
                Ok(())
            },
            || {
                trace.borrow_mut().push("panel");
            },
            |saved| {
                assert_eq!(saved, position);
                trace.borrow_mut().push("save");
                Err(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(
            *trace.borrow(),
            ["position", "clear", "hide", "panel", "save"]
        );
    }

    #[test]
    fn maintenance_shared_clear_and_hide_ignores_position_failure_but_not_hide_failure() {
        assert_eq!(
            clear_and_hide_with(
                || Err(()),
                || {},
                || Ok(()),
                || {},
                |_| panic!("missing position must not be saved"),
            ),
            Ok(())
        );
        assert_eq!(
            clear_and_hide_with(
                || Ok(crate::settings::WindowPosition { x: 1, y: 2 }),
                || {},
                || Err(()),
                || panic!("failed hide must not tear down panel"),
                |_| panic!("failed hide must not be persisted"),
            ),
            Err(CommandError::window_failed())
        );
    }

    #[test]
    fn shared_clear_and_hide_simulated_show_failure_invalidates_active_mapping() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with(
            &registry,
            "app",
            "invocation",
            1,
            || vec![application(1)],
            |_| {},
            WebSearchEngine::Bing,
        )
        .unwrap();
        let result_id = &response.items[0].result_id;
        assert!(registry.resolve(&response.request_id, result_id).is_ok());

        assert_eq!(
            clear_and_hide_with(
                || Err(()),
                || registry.hide_and_clear(),
                || Err(()),
                || panic!("failed hide must not tear down panel"),
                |_| Ok(()),
            ),
            Err(CommandError::window_failed())
        );
        assert_eq!(
            registry.resolve(&response.request_id, result_id),
            Err(RegistryError::StaleRequest)
        );
        assert!(registry
            .begin_query(QueryDomain::Application, "invocation", 2)
            .is_none());
    }

    #[test]
    fn maintenance_hide_launcher_uses_only_shared_clear_and_hide_after_guard() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let start = source.find("fn hide_launcher(").unwrap();
        let body = &source[start..source[start..].find("\n}\n").unwrap() + start + 3];
        assert!(body.contains("webview: tauri::Webview"));
        assert!(!body.contains("window: WebviewWindow"));
        assert!(body.contains("require_main_label(webview.label())?;"));
        let first_statement = body[body.find('{').unwrap() + 1..].trim_start();
        assert!(first_statement.starts_with("require_main_label(webview.label())?;"));
        assert!(body.contains("clear_and_hide_window("));
        assert!(body.contains("HideReason::ExplicitReturn"));
        assert!(!body.contains("registry.hide_and_clear"));
        assert!(!body.contains("window.hide()"));
    }

    #[test]
    fn shared_clear_and_hide_does_not_save_a_never_visible_startup_position() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let body = source
            .split("pub(crate) fn clear_and_hide(")
            .nth(1)
            .and_then(|tail| tail.split("fn clear_and_hide_with(").next())
            .expect("clear_and_hide source markers are missing");
        assert!(body.find("window.is_visible()").unwrap() < body.find("outer_position()").unwrap());
    }

    #[test]
    fn shared_clear_and_hide_window_notifies_frontend_after_successful_hide() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let body = source
            .split("pub(crate) fn clear_and_hide_window(")
            .nth(1)
            .and_then(|tail| tail.split("fn clear_and_hide_with(").next())
            .expect("clear_and_hide_window source markers are missing");
        let result = body
            .find("if result.is_ok()")
            .expect("successful hide branch is missing");
        let hidden = body
            .find("window.emit(\"launcher://hidden\", ())")
            .expect("frontend hidden notification is missing");

        assert!(result < hidden);
    }

    #[test]
    fn maintenance_wrappers_guard_before_their_first_body_statement() {
        let source = include_str!("commands.rs");
        for command in ["search_apps", "load_settings", "save_settings"] {
            let start = source
                .find(&format!("fn {command}("))
                .unwrap_or_else(|| panic!("missing command wrapper: {command}"));
            let body = &source[start..];
            let first_statement = body[body.find('{').unwrap() + 1..].trim_start();
            assert!(
                first_statement.starts_with("require_main_window(&window)?;"),
                "{command} must guard before state access or side effects"
            );
        }
        let start = source.find("fn execute_result(").unwrap();
        let body = &source[start..];
        let first_statement = body[body.find('{').unwrap() + 1..].trim_start();
        assert!(
            first_statement
                .starts_with("if window.label() != \"main\" && window.label() != \"find\""),
            "execute_result must validate its exact caller before state access"
        );
    }

    fn user_settings(hotkey: &str) -> UserSettingsUpdate {
        UserSettingsUpdate {
            hotkey: hotkey.into(),
            autostart: false,
            theme: ThemePreference::System,
            web_search_engine: WebSearchEngine::Bing,
        }
    }

    #[derive(Default)]
    struct SaveSideEffectCounts {
        reservation: AtomicUsize,
        dispatch: AtomicUsize,
        register: AtomicUsize,
        unregister: AtomicUsize,
        autostart: AtomicUsize,
        persist: AtomicUsize,
        store: AtomicUsize,
    }

    impl SaveSideEffectCounts {
        fn assert_zero(&self) {
            assert_eq!(self.reservation.load(Ordering::Relaxed), 0);
            assert_eq!(self.dispatch.load(Ordering::Relaxed), 0);
            assert_eq!(self.register.load(Ordering::Relaxed), 0);
            assert_eq!(self.unregister.load(Ordering::Relaxed), 0);
            assert_eq!(self.autostart.load(Ordering::Relaxed), 0);
            assert_eq!(self.persist.load(Ordering::Relaxed), 0);
            assert_eq!(self.store.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn save_settings_preflight_rejects_invalid_input_before_worker_dispatch() {
        let dir = TestDir::new();
        let _settings_store = settings_store(&dir);
        for settings in [user_settings("not a shortcut"), user_settings("doublectrl")] {
            let coordinator = Arc::new(LifecycleCoordinator::default());
            let counts = Arc::new(SaveSideEffectCounts::default());
            let reserve_counts = Arc::clone(&counts);
            let reserve_coordinator = Arc::clone(&coordinator);
            let worker_counts = Arc::clone(&counts);
            assert_eq!(
                tauri::async_runtime::block_on(save_settings_with(
                    settings,
                    move || {
                        reserve_counts.reservation.fetch_add(1, Ordering::Relaxed);
                        reserve_coordinator.reserve_critical().map_err(|_| ())
                    },
                    move |_, _, _| {
                        worker_counts.dispatch.fetch_add(1, Ordering::Relaxed);
                        worker_counts.register.fetch_add(1, Ordering::Relaxed);
                        worker_counts.unregister.fetch_add(1, Ordering::Relaxed);
                        worker_counts.autostart.fetch_add(1, Ordering::Relaxed);
                        worker_counts.persist.fetch_add(1, Ordering::Relaxed);
                        worker_counts.store.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    },
                )),
                Err(CommandError::settings_failed())
            );
            counts.assert_zero();
        }

        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let production = source.split(&marker).next().unwrap();
        let start = production
            .find("pub(crate) async fn save_settings(")
            .unwrap();
        let body = &production[start..];
        let after_open = body[body.find('{').unwrap() + 1..].trim_start();
        assert!(after_open.starts_with("require_main_window(&window)?;\n    save_settings_with("));
    }

    #[test]
    fn save_settings_preflight_accepts_valid_input_without_persisting() {
        let dir = TestDir::new();
        let store = settings_store(&dir);
        let before_memory = store.snapshot();
        let before_disk = fs::read(dir.path().join("settings.json")).unwrap();

        let (kind, update) = prepare_settings_save(user_settings("Control+Space")).unwrap();

        match &kind {
            HotkeyKind::Chord(shortcut) => {
                assert_eq!(shortcut.to_string(), "control+Space");
            }
            _ => panic!("expected chord"),
        }
        assert_eq!(update.hotkey, "control+Space");
        assert_eq!(store.snapshot(), before_memory);
        assert_eq!(
            fs::read(dir.path().join("settings.json")).unwrap(),
            before_disk
        );
    }

    #[test]
    fn save_settings_preflight_accepts_double_tap_without_shortcut_parse() {
        let (kind, update) = prepare_settings_save(user_settings("DoubleCtrl")).unwrap();
        assert_eq!(kind, HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl));
        assert_eq!(update.hotkey, "DoubleCtrl");
    }

    #[test]
    fn save_hotkey_preflight_accepts_only_hotkey_and_canonicalizes() {
        let input: HotkeySettingsUpdate =
            serde_json::from_value(serde_json::json!({ "hotkey": "Control+Space" })).unwrap();
        let (kind, view) = prepare_hotkey_save(input).unwrap();
        match &kind {
            HotkeyKind::Chord(shortcut) => assert_eq!(shortcut.to_string(), "control+Space"),
            _ => panic!("expected chord"),
        }
        assert_eq!(view.hotkey, "control+Space");

        let input: HotkeySettingsUpdate =
            serde_json::from_value(serde_json::json!({ "hotkey": "DoubleCtrl" })).unwrap();
        let (kind, view) = prepare_hotkey_save(input).unwrap();
        assert_eq!(kind, HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl));
        assert_eq!(view.hotkey, "DoubleCtrl");

        assert!(
            serde_json::from_value::<HotkeySettingsUpdate>(serde_json::json!({
                "hotkey": "DoubleCtrl",
                "autostart": true
            }))
            .is_err()
        );
        assert_eq!(
            prepare_hotkey_save(HotkeySettingsUpdate {
                hotkey: "doublectrl".into()
            }),
            Err(CommandError::settings_failed())
        );
    }

    #[test]
    fn save_settings_known_launcher_conflict_is_recoverable() {
        for hotkey_only in [false, true] {
            let coordinator = Arc::new(LifecycleCoordinator::default());
            let reserve_coordinator = Arc::clone(&coordinator);
            let worker_coordinator = Arc::clone(&coordinator);
            let reserve = move || reserve_coordinator.reserve_critical().map_err(|_| ());
            let reject = move |kind| {
                worker_coordinator.with_launcher_hotkey_save(
                    kind,
                    || {
                        let mut settings = crate::settings::Settings::default();
                        settings.public_plugin_command_hotkeys.insert(
                            "com.example.notes".into(),
                            [("default".into(), "Control+N".into())].into(),
                        );
                        settings
                    },
                    |_, _| panic!("conflict must not run native or persistence effects"),
                )
            };
            let result = if hotkey_only {
                tauri::async_runtime::block_on(save_hotkey_with(
                    HotkeySettingsUpdate {
                        hotkey: "Control+N".into(),
                    },
                    reserve,
                    move |_reservation, kind, _| reject(kind),
                ))
                .map(|_| ())
            } else {
                tauri::async_runtime::block_on(save_settings_with(
                    user_settings("Control+N"),
                    reserve,
                    move |_reservation, kind, _| reject(kind),
                ))
            };
            let error = result.unwrap_err();
            assert_eq!(error.code, "hotkeyConflict");
            assert_eq!(
                serde_json::to_value(&error).unwrap()["code"],
                "hotkeyConflict"
            );
            assert!(coordinator.reserve_critical().is_ok());
        }
    }

    #[test]
    fn save_settings_maps_worker_and_join_failures_to_fixed_error() {
        assert_eq!(map_save_worker_result(Ok(Ok(()))), Ok(()));
        assert_eq!(
            map_save_worker_result(Ok(Err(crate::lifecycle::SettingsSaveError::Failed))),
            Err(CommandError::settings_failed())
        );
        assert_eq!(
            map_save_worker_result(Err(())),
            Err(CommandError::settings_failed())
        );
    }

    #[test]
    fn save_settings_worker_state_uses_managed_store() {
        struct ManagedState {
            store: SettingsStore,
        }

        let dir = TestDir::new();
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let managed = Arc::new(ManagedState {
            store: settings_store(&dir),
        });
        let expected_store = &managed.store as *const SettingsStore as usize;
        let shortcut: Shortcut = "Alt+Space".parse().unwrap();
        let update = SettingsUpdate {
            hotkey: "Alt+Space".into(),
            autostart: false,
            theme: ThemePreference::System,
            web_search_engine: WebSearchEngine::Bing,
        };
        let caller = thread::current().id();
        let expected_coordinator = Arc::clone(&coordinator);
        let reserve_coordinator = Arc::clone(&coordinator);
        let worker_managed = Arc::clone(&managed);
        let worker_coordinator = Arc::clone(&coordinator);

        let result = tauri::async_runtime::block_on(save_settings_worker_with(
            move || reserve_coordinator.reserve_critical().map_err(|_| ()),
            move |reservation| {
                let _reservation = reservation;
                assert_ne!(thread::current().id(), caller);
                assert_eq!(
                    &worker_managed.store as *const SettingsStore as usize,
                    expected_store
                );
                assert!(Arc::ptr_eq(&worker_coordinator, &expected_coordinator));
                assert_eq!(shortcut.to_string(), "alt+Space");
                assert_eq!(update.hotkey, "Alt+Space");
                Ok(())
            },
        ));

        assert_eq!(result, Ok(()));

        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let production = source.split(&marker).next().unwrap();
        let command_start = production
            .find("pub(crate) async fn save_settings(")
            .unwrap();
        let command_end = production[command_start..]
            .find("#[tauri::command]\npub(crate) async fn save_hotkey(")
            .map(|offset| command_start + offset)
            .unwrap();
        let command = &production[command_start..command_end];
        assert_eq!(
            command
                .matches("app_for_worker.state::<SettingsStore>()")
                .count(),
            1
        );
        assert!(!command.contains("AppCache"));
        assert_eq!(
            command.matches("Arc::clone(coordinator.inner())").count(),
            1
        );
    }

    #[test]
    fn command_contract_preserves_settings_argument() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let test_marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        assert_eq!(source.matches(&test_marker).count(), 1);
        let production = source.split(&test_marker).next().unwrap();
        let expected = [
            "#[tauri::command]\n",
            "pub(crate) async fn save_",
            "settings(\n",
            "    window: tauri::WebviewWindow,\n",
            "    settings: UserSettingsUpdate,\n",
            "    app: tauri::AppHandle,\n",
            "    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,\n",
            ") -> Result<(), CommandError> {\n",
        ]
        .concat();
        let forbidden = ["input", ": UserSettingsUpdate"].concat();

        assert_eq!(production.matches(&expected).count(), 1);
        assert!(!production.contains(&forbidden));
        let command = &production[production.find(&expected).unwrap()..];
        let first_statement = command[command.find('{').unwrap() + 1..].trim_start();
        assert!(first_statement.starts_with("require_main_window(&window)?;"));
    }

    mod execute_plugin {
        use super::super::{execute_result_with_clipboard, ClipboardExecution};
        use super::*;
        use crate::plugins::PluginCopyError;

        #[test]
        fn stale_builtin_results_are_denied_and_reenable_preserves_their_data() {
            let dir = TestDir::new();
            let settings = settings_store(&dir);
            for (feature, action) in [
                (
                    ManagedBuiltinFeature::WebSearch,
                    ResultAction::OpenWebSearch {
                        engine: WebSearchEngine::Bing,
                        query: "saved query".into(),
                    },
                ),
                (
                    ManagedBuiltinFeature::Quicklinks,
                    ResultAction::OpenQuicklink {
                        id: "saved-link".into(),
                        url: "https://example.com/saved".into(),
                    },
                ),
            ] {
                let registry = ready_registry("stale-builtin");
                let token = registry
                    .begin_query(QueryDomain::Application, "stale-builtin", 1)
                    .unwrap();
                let (request, result) = registry
                    .publish_if_latest(
                        token,
                        vec![((), action)],
                        || true,
                        |request, items| (request, items[0].0.clone()),
                    )
                    .unwrap();
                settings
                    .set_builtin_feature_enabled(feature, false)
                    .unwrap();
                let opened = Cell::new(0);
                for enabled in [false, true] {
                    if enabled {
                        settings.set_builtin_feature_enabled(feature, true).unwrap();
                    }
                    let result = tauri::async_runtime::block_on(execute_result_with_clipboard(
                        (&request, &result),
                        ClipboardExecution {
                            resolve: |request: &str, result: &str| {
                                registry.resolve(request, result)
                            },
                            execute: |_: &ResultAction| unreachable!(),
                            execute_file: no_file_execution,
                            open_web_search: |_: WebSearchEngine, _: &str| {
                                super::super::open_builtin_with(&settings, feature, || {
                                    // A settings read here also proves native IO is outside the settings lock.
                                    assert!(settings.snapshot().builtin_feature_enabled(feature));
                                    opened.set(opened.get() + 1);
                                    Ok(())
                                })
                            },
                            open_quicklink: |_: url::Url| {
                                super::super::open_builtin_with(&settings, feature, || {
                                    assert!(settings.snapshot().builtin_feature_enabled(feature));
                                    opened.set(opened.get() + 1);
                                    Ok(())
                                })
                            },
                            copy_builtin: |_: &str| unreachable!(),
                            copy_plugin: |_: &str, _: Option<&str>, _: u64, _: &str| unreachable!(),
                            clear_and_hide: || Ok(()),
                            increment: |_: &str| unreachable!(),
                        },
                    ));
                    assert_eq!(result.is_ok(), enabled);
                    assert_eq!(opened.get(), usize::from(enabled));
                }
            }
        }

        #[test]
        fn management_effects_invalidate_real_copy_handles_and_inflight_query_before_return() {
            for case in ["rename", "noop", "favorite", "partial-error"] {
                crate::public_plugins::with_settings_service_for_test(|service, manager| {
                    let registry = ready_registry("management");
                    let token = registry
                        .begin_query(QueryDomain::Plugin, "management", 1)
                        .unwrap();
                    let route = manager.route("/demo first").unwrap().unwrap();
                    let action = ResultAction::CopyText {
                        plugin_id: route.plugin_id.clone(),
                        command_id: Some("default".into()),
                        generation: route.generation,
                        text: "private".into(),
                    };
                    let (request_id, result_id) = registry
                        .publish_if_latest(
                            token,
                            vec![((), action.clone())],
                            || true,
                            |request, items| (request, items[0].0.clone()),
                        )
                        .unwrap();
                    let a = service
                        .schedule_command(route, 1, "/demo first".into(), std::time::Instant::now())
                        .unwrap();
                    let outcome = match case {
                        "rename" => {
                            service.rename_command("com.uipilot.demo", "default", Some("renamed"))
                        }
                        "noop" => service.rename_command("com.uipilot.demo", "default", None),
                        "favorite" => {
                            service.set_command_favorite("com.uipilot.demo", "default", true)
                        }
                        "partial-error" => service.save_settings(
                            "com.uipilot.demo",
                            &BTreeMap::new(),
                            &BTreeMap::from([
                                ("first".into(), Some("synthetic".into())),
                                ("second".into(), Some("x".repeat(65537))),
                            ]),
                        ),
                        _ => unreachable!(),
                    };
                    let changed = matches!(case, "rename" | "partial-error");
                    let cleanup = Cell::new(false);
                    let result = super::super::finish_public_plugin_management(
                        service,
                        outcome,
                        &registry,
                        &crate::plugins::PluginManager::new(),
                        || {
                            assert_eq!(a.receiver.try_recv(), Ok(None));
                            assert!(
                                registry.resolve(&request_id, &result_id).is_err(),
                                "cleanup must follow result invalidation"
                            );
                            cleanup.set(true);
                        },
                    );
                    assert_eq!(result.is_err(), case == "partial-error");
                    assert_eq!(cleanup.get(), changed);
                    let copies = Cell::new(0);
                    let copied = tauri::async_runtime::block_on(execute_result_with_clipboard(
                        (&request_id, &result_id),
                        ClipboardExecution {
                            resolve: |request: &str, result: &str| {
                                registry.resolve(request, result)
                            },
                            execute: |_: &ResultAction| unreachable!(),
                            execute_file: no_file_execution,
                            open_web_search: no_web_search,
                            open_quicklink: no_quicklink,
                            copy_builtin: |_: &str| unreachable!(),
                            copy_plugin: |_: &str, _: Option<&str>, _: u64, _: &str| {
                                copies.set(copies.get() + 1);
                                Ok(())
                            },
                            clear_and_hide: || Ok(()),
                            increment: |_: &str| unreachable!(),
                        },
                    ));
                    assert_eq!(copied.is_err(), changed);
                    assert_eq!(copies.get(), usize::from(!changed));
                    assert_eq!(
                        registry
                            .publish_if_latest(token, vec![((), action)], || true, |_, _| ())
                            .is_none(),
                        changed
                    );
                });
            }
        }

        #[test]
        fn denied_public_command_never_falls_back_to_legacy_copy_authority() {
            for id in ["unknown", "constructor"] {
                let clipboard = Cell::new(0);
                let legacy = Cell::new(0);
                let result = super::super::copy_plugin_text_with(
                    Some(id),
                    |saved_id| {
                        assert_eq!(saved_id, id);
                        Err(PluginCopyError::PermissionDenied)
                    },
                    || {
                        // The internal manager would allow the same owner and clipboard write.
                        legacy.set(legacy.get() + 1);
                        clipboard.set(clipboard.get() + 1);
                        Ok(())
                    },
                );
                assert_eq!(result, Err(PluginCopyError::PermissionDenied));
                assert_eq!(legacy.get(), 0);
                assert_eq!(clipboard.get(), 0);
            }
            let clipboard = Cell::new(0);
            super::super::copy_plugin_text_with(
                None,
                |_| panic!("internal copy has no public command"),
                || {
                    clipboard.set(1);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(clipboard.get(), 1);
        }

        fn copy_action() -> ResultAction {
            ResultAction::CopyText {
                command_id: None,
                plugin_id: "plugin".into(),
                generation: 1,
                text: "copy me".into(),
            }
        }

        async fn no_file_execution(
            _: FileExecutionAction,
        ) -> Result<FileExecutionOutcome, CommandError> {
            unreachable!()
        }

        fn no_web_search(_: WebSearchEngine, _: &str) -> Result<(), ()> {
            unreachable!()
        }

        fn no_quicklink(_: url::Url) -> Result<(), ()> {
            unreachable!()
        }

        #[test]
        fn built_in_copy_bypasses_plugin_permission_and_hides_after_success() {
            let trace = RefCell::new(Vec::new());
            assert_eq!(
                tauri::async_runtime::block_on(execute_result_with_clipboard(
                    ("request", "result"),
                    ClipboardExecution {
                        resolve: |_: &str, _: &str| Ok(ResultAction::CopyBuiltInText {
                            text: "14".into(),
                        }),
                        execute: |_: &ResultAction| unreachable!(),
                        execute_file: no_file_execution,
                        open_web_search: no_web_search,
                        open_quicklink: no_quicklink,
                        copy_builtin: |text: &str| {
                            trace.borrow_mut().push("clipboard");
                            assert_eq!(text, "14");
                            Ok(())
                        },
                        copy_plugin: |_: &str, _: Option<&str>, _: u64, _: &str| unreachable!(),
                        clear_and_hide: || {
                            trace.borrow_mut().push("clear-hide");
                            Ok(())
                        },
                        increment: |_: &str| unreachable!(),
                    },
                )),
                Ok(ExecuteOutcome::TextCopied)
            );
            assert_eq!(*trace.borrow(), ["clipboard", "clear-hide"]);
        }
        #[test]
        fn copy_rechecks_permission_before_clipboard() {
            let clipboard = Cell::new(0);
            let hide = Cell::new(0);
            assert_eq!(
                tauri::async_runtime::block_on(execute_result_with_clipboard(
                    ("request", "result"),
                    ClipboardExecution {
                        resolve: |_: &str, _: &str| Ok(copy_action()),
                        execute: |_: &ResultAction| unreachable!(),
                        execute_file: no_file_execution,
                        open_web_search: no_web_search,
                        open_quicklink: no_quicklink,
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |_: &str, _: Option<&str>, _: u64, _: &str| {
                            Err(PluginCopyError::PermissionDenied)
                        },
                        clear_and_hide: || {
                            hide.set(hide.get() + 1);
                            Ok(())
                        },
                        increment: |_: &str| unreachable!(),
                    },
                )),
                Err(CommandError::plugin_permission_denied())
            );
            assert_eq!(clipboard.get(), 0);
            assert_eq!(hide.get(), 0);
        }

        #[test]
        fn copy_success_hides_once_without_app_validation_or_use_count() {
            let trace = RefCell::new(Vec::new());
            assert_eq!(
                tauri::async_runtime::block_on(execute_result_with_clipboard(
                    ("request", "result"),
                    ClipboardExecution {
                        resolve: |_: &str, _: &str| {
                            trace.borrow_mut().push("resolve");
                            Ok(copy_action())
                        },
                        execute: |_: &ResultAction| {
                            trace.borrow_mut().push("launch");
                            unreachable!()
                        },
                        execute_file: no_file_execution,
                        open_web_search: no_web_search,
                        open_quicklink: no_quicklink,
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |plugin_id: &str,
                                      command_id: Option<&str>,
                                      generation: u64,
                                      text: &str| {
                            assert_eq!(command_id, None);
                            trace.borrow_mut().push("permission");
                            assert_eq!(plugin_id, "plugin");
                            assert_eq!(generation, 1);
                            trace.borrow_mut().push("clipboard");
                            assert_eq!(text, "copy me");
                            Ok(())
                        },
                        clear_and_hide: || {
                            trace.borrow_mut().push("clear-hide");
                            Ok(())
                        },
                        increment: |_: &str| {
                            trace.borrow_mut().push("use-count");
                            unreachable!()
                        },
                    },
                )),
                Ok(ExecuteOutcome::TextCopied)
            );
            assert_eq!(
                *trace.borrow(),
                ["resolve", "permission", "clipboard", "clear-hide"]
            );
        }

        #[test]
        fn clipboard_failure_keeps_registry_and_window_usable() {
            let hide = Cell::new(0);
            assert_eq!(
                tauri::async_runtime::block_on(execute_result_with_clipboard(
                    ("request", "result"),
                    ClipboardExecution {
                        resolve: |_: &str, _: &str| Ok(copy_action()),
                        execute: |_: &ResultAction| unreachable!(),
                        execute_file: no_file_execution,
                        open_web_search: no_web_search,
                        open_quicklink: no_quicklink,
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |_: &str, _: Option<&str>, _: u64, _: &str| {
                            Err(PluginCopyError::SideEffectFailed)
                        },
                        clear_and_hide: || {
                            hide.set(hide.get() + 1);
                            Ok(())
                        },
                        increment: |_: &str| unreachable!(),
                    },
                )),
                Err(CommandError::clipboard_write_failed())
            );
            assert_eq!(hide.get(), 0);
        }

        #[test]
        fn stale_or_unknown_copy_ids_stop_before_permission_and_clipboard() {
            for error in [RegistryError::StaleRequest, RegistryError::UnknownResult] {
                let side_effects = Cell::new(0);
                let result = tauri::async_runtime::block_on(execute_result_with_clipboard(
                    ("request", "result"),
                    ClipboardExecution {
                        resolve: |_: &str, _: &str| Err(error),
                        execute: |_: &ResultAction| unreachable!(),
                        execute_file: no_file_execution,
                        open_web_search: no_web_search,
                        open_quicklink: no_quicklink,
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |_: &str, _: Option<&str>, _: u64, _: &str| {
                            side_effects.set(side_effects.get() + 1);
                            Ok(())
                        },
                        clear_and_hide: || {
                            side_effects.set(side_effects.get() + 1);
                            Ok(())
                        },
                        increment: |_: &str| unreachable!(),
                    },
                ));
                assert_eq!(
                    result,
                    Err(match error {
                        RegistryError::StaleRequest => CommandError::stale_request(),
                        RegistryError::UnknownResult => CommandError::unknown_result(),
                    })
                );
                assert_eq!(side_effects.get(), 0);
            }
        }
    }

    mod execute_web_search {
        use std::{cell::RefCell, rc::Rc};

        use super::super::execute_web_search_with;
        use super::*;

        #[test]
        fn successful_browser_open_hides_only_after_the_launch_request() {
            let trace = Rc::new(RefCell::new(Vec::new()));
            let open_trace = Rc::clone(&trace);
            let hide_trace = Rc::clone(&trace);

            assert_eq!(
                execute_web_search_with(
                    WebSearchEngine::Bing,
                    "windows",
                    move |engine, query| {
                        assert_eq!(engine, WebSearchEngine::Bing);
                        assert_eq!(query, "windows");
                        open_trace.borrow_mut().push("open");
                        Ok(())
                    },
                    move || {
                        hide_trace.borrow_mut().push("clear-hide");
                        Ok(())
                    },
                ),
                Ok(ExecuteOutcome::LaunchRequested)
            );
            assert_eq!(*trace.borrow(), ["open", "clear-hide"]);
        }

        #[test]
        fn browser_open_failure_keeps_the_launcher_visible() {
            let hide_calls = Cell::new(0);
            assert_eq!(
                execute_web_search_with(
                    WebSearchEngine::Bing,
                    "windows",
                    |_, _| Err(()),
                    || {
                        hide_calls.set(hide_calls.get() + 1);
                        Ok(())
                    },
                ),
                Err(CommandError::web_search_failed())
            );
            assert_eq!(hide_calls.get(), 0);
        }
    }

    mod execute_quicklink {
        use std::{cell::RefCell, rc::Rc};

        use super::super::execute_quicklink_with;
        use super::*;
        use crate::quicklinks::QuicklinkErrorCode;

        #[test]
        fn successful_browser_open_hides_only_after_the_launch_request() {
            let trace = Rc::new(RefCell::new(Vec::new()));
            let open_trace = Rc::clone(&trace);
            let hide_trace = Rc::clone(&trace);

            assert_eq!(
                execute_quicklink_with(
                    "https://search.jd.com/Search?keyword=phone",
                    move |url| {
                        assert_eq!(url.as_str(), "https://search.jd.com/Search?keyword=phone");
                        open_trace.borrow_mut().push("open");
                        Ok(())
                    },
                    move || {
                        hide_trace.borrow_mut().push("clear-hide");
                        Ok(())
                    },
                ),
                Ok(ExecuteOutcome::LaunchRequested)
            );
            assert_eq!(*trace.borrow(), ["open", "clear-hide"]);
        }

        #[test]
        fn browser_open_failure_keeps_the_launcher_visible() {
            let hide_calls = Cell::new(0);
            assert_eq!(
                execute_quicklink_with(
                    "https://search.jd.com/Search?keyword=phone",
                    |_| Err(()),
                    || {
                        hide_calls.set(hide_calls.get() + 1);
                        Ok(())
                    },
                ),
                Err(CommandError::quicklink(QuicklinkErrorCode::OpenFailed))
            );
            assert_eq!(hide_calls.get(), 0);
        }
    }

    mod plugin {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        use super::super::{
            delete_plugin_with_label, install_plugin_with_label, list_plugins_with_label,
            publish_plugin_results_with_label, reload_plugin_with_label, CommandError,
        };
        use crate::lifecycle::LifecycleCoordinator;
        use crate::plugins::{
            PluginInventorySnapshot, PluginManagementError, PluginMutationOutcome, PluginQueryError,
        };

        #[test]
        fn list_guard_and_fixed_error_mapping_precede_manager_access() {
            let calls = AtomicUsize::new(0);
            assert_eq!(
                list_plugins_with_label("secondary", || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(PluginInventorySnapshot {
                        revision: "1".into(),
                        items: Vec::new(),
                    })
                }),
                Err(CommandError::invalid_caller())
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);

            let expected = PluginInventorySnapshot {
                revision: "1".into(),
                items: Vec::new(),
            };
            assert_eq!(
                list_plugins_with_label("main", || Ok(expected.clone())),
                Ok(expected)
            );
            assert_eq!(
                list_plugins_with_label("main", || Err(PluginManagementError::Unavailable)),
                Err(CommandError::plugin_list_failed())
            );
        }

        #[test]
        fn install_guard_and_fixed_error_mapping_precede_manager_access() {
            let calls = AtomicUsize::new(0);
            let coordinator = Arc::new(LifecycleCoordinator::default());
            assert_eq!(
                install_plugin_with_label("secondary", &coordinator, || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Err(PluginManagementError::Unavailable)
                }),
                Err(CommandError::invalid_caller())
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            assert_eq!(
                install_plugin_with_label("main", &coordinator, || {
                    Err(PluginManagementError::Unavailable)
                }),
                Err(CommandError::plugin_install_failed())
            );
            let expected = PluginMutationOutcome {
                revision: "2".into(),
            };
            assert_eq!(
                install_plugin_with_label("main", &coordinator, || Ok(expected.clone())),
                Ok(expected)
            );
        }

        #[test]
        fn reload_guard_and_fixed_error_mapping_precede_manager_access() {
            let calls = AtomicUsize::new(0);
            let coordinator = Arc::new(LifecycleCoordinator::default());
            assert_eq!(
                reload_plugin_with_label("secondary", &coordinator, || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Err(PluginManagementError::Unavailable)
                }),
                Err(CommandError::invalid_caller())
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);

            assert_eq!(
                reload_plugin_with_label("main", &coordinator, || {
                    Err(PluginManagementError::Unavailable)
                }),
                Err(CommandError::plugin_reload_failed())
            );
        }

        #[test]
        fn delete_guard_and_fixed_error_mapping_precede_manager_access() {
            let calls = AtomicUsize::new(0);
            let coordinator = Arc::new(LifecycleCoordinator::default());
            assert_eq!(
                delete_plugin_with_label("secondary", &coordinator, || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Err(PluginManagementError::Unavailable)
                }),
                Err(CommandError::invalid_caller())
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);

            assert_eq!(
                delete_plugin_with_label("main", &coordinator, || {
                    Err(PluginManagementError::Unavailable)
                }),
                Err(CommandError::plugin_delete_failed())
            );
        }

        #[test]
        fn publish_rejects_non_plugin_label_before_state_access() {
            let calls = AtomicUsize::new(0);
            assert_eq!(
                publish_plugin_results_with_label("main", || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }),
                Err(CommandError::invalid_caller())
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn publish_maps_plugin_validation_failure_to_fixed_error() {
            assert_eq!(
                publish_plugin_results_with_label("plugin-abc", || {
                    Err(PluginQueryError::InvalidResponse)
                }),
                Err(CommandError::plugin_query_failed())
            );
        }

        #[test]
        fn plugin_timeout_publishes_empty_results_silently() {
            let source = include_str!("commands.rs");
            let production = source.split("#[cfg(test)]").next().unwrap();
            assert!(production.contains("Err(PluginQueryError::Timeout) => Vec::new(),"));
        }
    }

    #[test]
    fn file_preview_preference_guards_reserves_and_reacquires_managed_store() {
        let invalid_dispatch = AtomicUsize::new(0);
        assert_eq!(
            require_main_label("secondary"),
            Err(CommandError::invalid_caller())
        );
        assert_eq!(invalid_dispatch.load(Ordering::Relaxed), 0);

        for rejected_phase in ["Cleaning", "SystemEnding"] {
            let dispatch = Arc::new(AtomicUsize::new(0));
            let worker_dispatch = Arc::clone(&dispatch);
            assert_eq!(
                tauri::async_runtime::block_on(set_file_preview_preference_with(
                    FilePreviewPreferenceUpdate { enabled: false },
                    || Err::<crate::lifecycle::CriticalReservation, _>(rejected_phase),
                    move |_, _| {
                        worker_dispatch.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    },
                )),
                Err(CommandError::settings_failed())
            );
            assert_eq!(dispatch.load(Ordering::Relaxed), 0);
        }

        let coordinator = Arc::new(LifecycleCoordinator::default());
        let reserve_coordinator = Arc::clone(&coordinator);
        let order = Arc::new(Mutex::new(Vec::new()));
        let reserve_order = Arc::clone(&order);
        let worker_order = Arc::clone(&order);
        assert_eq!(
            tauri::async_runtime::block_on(set_file_preview_preference_with(
                FilePreviewPreferenceUpdate { enabled: false },
                move || {
                    reserve_order.lock().unwrap().push("reserve");
                    reserve_coordinator.reserve_critical().map_err(|_| ())
                },
                move |_reservation, preference| {
                    worker_order.lock().unwrap().push("worker");
                    assert!(!preference.enabled);
                    Ok(())
                },
            )),
            Ok(())
        );
        assert_eq!(*order.lock().unwrap(), ["reserve", "worker"]);

        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let production = source.split(&marker).next().unwrap();
        let start = production
            .find("pub(crate) async fn set_file_preview_preference(")
            .unwrap();
        let end = production[start..]
            .find("#[tauri::command]\npub(crate) async fn set_find_preview_preference(")
            .map(|offset| start + offset)
            .unwrap();
        let command = &production[start..end];
        let first = command[command.find('{').unwrap() + 1..].trim_start();
        assert!(first.starts_with("require_main_window(&window)?;"));
        assert_eq!(command.matches(".state::<SettingsStore>()").count(), 1);
        assert!(!command.contains("AppCache"));
        assert!(!command.contains("reconcile_runtime_settings"));
    }

    #[test]
    fn file_preview_preference_maps_worker_and_storage_failures() {
        assert_eq!(map_file_preview_worker_result(Ok(Ok(()))), Ok(()));
        assert_eq!(
            map_file_preview_worker_result(Ok(Err(()))),
            Err(CommandError::settings_failed())
        );
        assert_eq!(
            map_file_preview_worker_result(Err(())),
            Err(CommandError::settings_failed())
        );

        let dir = TestDir::new();
        let store = settings_store(&dir);
        let before = store.snapshot();
        let current = dir.path().join("settings.json");
        let before_disk = fs::read(&current).unwrap();
        fs::remove_file(&current).unwrap();
        fs::create_dir(&current).unwrap();
        assert_eq!(
            store.set_file_preview_enabled(!before.file_preview_enabled),
            Err(crate::settings::SettingsError::Storage)
        );
        assert_eq!(store.snapshot(), before);
        fs::remove_dir(&current).unwrap();
        fs::write(&current, &before_disk).unwrap();
        assert_eq!(fs::read(current).unwrap(), before_disk);
    }

    #[test]
    fn file_preview_preference_caller_guard_is_first_statement() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let production = source.split(&marker).next().unwrap();
        let start = production
            .find("pub(crate) async fn set_file_preview_preference(")
            .unwrap();
        let command = &production[start..];
        let first = command[command.find('{').unwrap() + 1..].trim_start();
        assert!(first.starts_with("require_main_window(&window)?;"));
        let guard = command.find("require_main_window(&window)?;").unwrap();
        for forbidden in [
            "reserve_critical",
            "spawn_blocking",
            "state::<SettingsStore>",
        ] {
            assert!(guard < command.find(forbidden).unwrap());
        }
    }

    #[test]
    fn execute_result_preserves_application_branch_and_isolates_file_branch() {
        let volume = VolumeIdentity::for_test(r"\\?\Volume{EXECUTION}\", 41, "ntfs");
        let file = ResultAction::OpenFile(FileExecutionAction::Indexed(OpenIndexedPath::for_test(
            7,
            19,
            volume,
            r"docs\report.pdf",
            IndexedKind::File,
        )));
        let application_calls = Cell::new(0);
        let file_calls = Cell::new(0);
        let hide_calls = Cell::new(0);
        let later_application_calls = Cell::new(0);

        let outcome = tauri::async_runtime::block_on(execute_resolved_result_with(
            ("request", "result"),
            |_, _| Ok(file),
            |_| {
                application_calls.set(application_calls.get() + 1);
                Ok(ApplicationActionOutcome::LaunchRequested)
            },
            |_| {
                file_calls.set(file_calls.get() + 1);
                async { Ok(FileExecutionOutcome::FileRevealRequested) }
            },
            || {
                hide_calls.set(hide_calls.get() + 1);
                Ok(())
            },
            |_| {
                later_application_calls.set(later_application_calls.get() + 1);
                Ok(())
            },
        ));

        assert_eq!(outcome, Ok(ExecuteOutcome::FileRevealRequested));
        assert_eq!(application_calls.get(), 0);
        assert_eq!(file_calls.get(), 1);
        assert_eq!(hide_calls.get(), 1);
        assert_eq!(later_application_calls.get(), 0);
    }

    #[test]
    fn execute_file_action_dispatches_each_backend_once() {
        for (action, expected_backend) in [
            (
                FileExecutionAction::Indexed(file_action(1, "report.pdf", IndexedKind::File)),
                "indexed",
            ),
            (
                FileExecutionAction::Everything(everything_action_for_test()),
                "everything",
            ),
        ] {
            let calls = RefCell::new(Vec::new());
            let outcome = execute_file_action_with(
                action,
                |action| {
                    calls.borrow_mut().push(("indexed", action.kind_for_test()));
                    Ok(FileExecutionOutcome::FileRevealRequested)
                },
                |action| {
                    calls
                        .borrow_mut()
                        .push(("everything", action.kind_for_test()));
                    Ok(FileExecutionOutcome::FileRevealRequested)
                },
            )
            .unwrap();

            assert_eq!(outcome, FileExecutionOutcome::FileRevealRequested);
            assert_eq!(
                calls.borrow().as_slice(),
                [(expected_backend, FilePathKind::File)]
            );
        }
    }
    #[test]
    fn production_execute_result_dispatches_both_file_backends() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let start = production
            .find("pub(crate) async fn execute_result(")
            .expect("execute_result command is missing");
        let command = &production[start..production.find("struct ClipboardExecution").unwrap()];

        for required in [
            "let file_index = app.state::<Arc<FileIndex>>();",
            "let worker_index = Arc::clone(file_index.inner());",
            "execute_file_action_with(",
            "|action| worker_index.execute_indexed_path(action),",
            "|action| path_auth::execute_authenticated_path(action.identity()),",
        ] {
            assert!(
                command.contains(required),
                "missing production dispatch: {required}"
            );
        }
        assert!(!command.contains("action.into_everything()"));

        assert_eq!(
            production
                .matches("fn execute_file_action_with<I, E>(")
                .count(),
            1
        );
        assert!(!production.contains("#[cfg(test)]\nfn execute_file_action_with<I, E>("));
    }
}

#[cfg(test)]
mod command_management_ipc_tests {
    use super::CommandError;
    use serde_json::{json, Value};
    use std::sync::Mutex;
    use tauri::{Manager, State, WebviewWindow};

    #[derive(Default)]
    struct Probe {
        commands: Vec<String>,
        reads: usize,
        writes: Vec<(String, String, Value)>,
    }
    fn resolve_probe(
        label: &str,
        request: &tauri::ipc::Request<'_>,
        probe: &Mutex<Probe>,
    ) -> Result<String, CommandError> {
        super::resolve_management_command(label, request, |id| {
            let mut probe = probe.lock().unwrap();
            probe.reads += 1;
            Ok(probe.commands.iter().any(|command| command == id))
        })
    }
    #[tauri::command]
    fn favorite_probe<R: tauri::Runtime>(
        window: WebviewWindow<R>,
        probe: State<'_, Mutex<Probe>>,
        plugin_id: String,
        request: tauri::ipc::Request<'_>,
        favorite: bool,
    ) -> Result<(), CommandError> {
        let id = resolve_probe(window.label(), &request, probe.inner())?;
        let mut probe = probe.lock().unwrap();
        probe.writes.push((plugin_id, id, json!(favorite)));
        Ok(())
    }
    #[tauri::command]
    fn rename_probe<R: tauri::Runtime>(
        window: WebviewWindow<R>,
        probe: State<'_, Mutex<Probe>>,
        plugin_id: String,
        request: tauri::ipc::Request<'_>,
        name_override: Option<String>,
    ) -> Result<(), CommandError> {
        let id = resolve_probe(window.label(), &request, probe.inner())?;
        let mut probe = probe.lock().unwrap();
        probe.writes.push((plugin_id, id, json!(name_override)));
        Ok(())
    }
    #[test]
    fn generated_ipc_preserves_absent_only_legacy_command_id_and_guards_protected_resolution() {
        for command in ["favorite_probe", "rename_probe"] {
            for has_default in [true, false] {
                let app = tauri::test::mock_builder()
                    .manage(Mutex::new(Probe {
                        commands: if has_default {
                            vec!["default".into(), "upper".into(), "constructor".into()]
                        } else {
                            vec!["upper".into(), "constructor".into()]
                        },
                        ..Probe::default()
                    }))
                    .invoke_handler(tauri::generate_handler![favorite_probe, rename_probe])
                    .build(tauri::test::mock_context(tauri::test::noop_assets()))
                    .unwrap();
                let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                    .build()
                    .unwrap();
                let secondary =
                    tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default())
                        .build()
                        .unwrap();
                for (id, accepted) in [
                    (None, has_default),
                    (Some(json!(null)), false),
                    (Some(json!("")), false),
                    (Some(json!("bad id")), false),
                    (Some(json!("unknown")), false),
                    (Some(json!(1)), false),
                    (Some(json!(false)), false),
                    (Some(json!("constructor")), true),
                    (Some(json!("upper")), true),
                ] {
                    for (window, allowed) in [(&main, true), (&secondary, false)] {
                        let mut body = json!({ "pluginId": "com.example.multi", "favorite": true, "nameOverride": "new-name" });
                        if let Some(id) = &id {
                            body["commandId"] = id.clone();
                        }
                        let before = {
                            let probe = app.state::<Mutex<Probe>>();
                            let probe = probe.lock().unwrap();
                            (probe.reads, probe.writes.len())
                        };
                        let result = tauri::test::get_ipc_response(
                            window,
                            tauri::webview::InvokeRequest {
                                cmd: command.into(),
                                callback: tauri::ipc::CallbackFn(0),
                                error: tauri::ipc::CallbackFn(1),
                                url: "http://tauri.localhost".parse().unwrap(),
                                body: tauri::ipc::InvokeBody::Json(body.clone()),
                                headers: Default::default(),
                                invoke_key: tauri::test::INVOKE_KEY.into(),
                            },
                        );
                        assert_eq!(
                            result.is_ok(),
                            accepted && allowed,
                            "{command} default={has_default} caller={} body={body}",
                            window.label()
                        );
                        let probe = app.state::<Mutex<Probe>>();
                        let probe = probe.lock().unwrap();
                        if !allowed {
                            assert_eq!(result.unwrap_err()["code"], "invalidCaller");
                            assert_eq!(
                                probe.reads, before.0,
                                "unauthorized caller must not read membership"
                            );
                        }
                        if !(accepted && allowed) {
                            assert_eq!(probe.writes.len(), before.1);
                        } else {
                            assert_eq!(
                                probe.writes.last().unwrap().1,
                                id.as_ref().and_then(Value::as_str).unwrap_or("default")
                            );
                        }
                    }
                }
            }
        }
    }
}
