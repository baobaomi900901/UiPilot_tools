use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

use crate::{
    apps::{self, AppCache, Application},
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
    lifecycle::{self, CriticalReservation, LifecycleCoordinator, ReservationError},
    message_center::{
        MessageCenterError, MessageCenterService, MessageCenterSnapshot, MessageSummary,
    },
    model::{LauncherResultActivation, ResultIconKind, SearchResponse},
    plugin_window::{
        self, PluginWindowCallError, PluginWindowController, PluginWindowOwner,
        PluginWindowPinState, PluginWindowUpdate,
    },
    plugins::{
        PluginCopyError, PluginInventorySnapshot, PluginManagementError, PluginManager,
        PluginMutationOutcome, PluginQueryError, PluginQueryStart,
    },
    public_plugins::{
        PluginApiRequest, PluginCommandCompletion, PluginInvocationTheme, PluginRuntimeError,
        PluginTimerStartInput, PluginTimerState, PublicActivationMode, PublicMainResult,
        PublicOutputMode, PublicPermission, PublicPluginInstallSource, PublicPluginInventory,
        PublicPluginManagementError, PublicPluginManager, PublicPluginMutation,
        PublicPluginPrepareSummary, PublicPluginResponse, PublicPluginService,
        PublicPluginWindowIdentity, TimerError, WindowStorageError,
    },
    result_registry::{
        QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistries, ResultRegistry,
    },
    settings::{
        SettingsError, SettingsStore, SettingsUpdate, ThemePreference, WebSearchEngine,
        WindowPosition,
    },
    window_transfer::MainWindowTransferCoordinator,
};

const ACTIVATION_REFUSED_MESSAGE: &str = "Windows 拒绝了前台切换，已发送启动请求";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsView {
    hotkey: String,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct OpenFindInput {
    query: String,
    invocation_id: String,
    query_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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

fn parse_timer_session_generation(value: &str) -> Result<u64, TimerError> {
    if value == "0"
        || value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(TimerError::ExpiredWindowSessionError);
    }
    value
        .parse::<u64>()
        .map_err(|_| TimerError::ExpiredWindowSessionError)
}

fn parse_window_storage_session_generation(value: &str) -> Result<u64, WindowStorageError> {
    if value == "0"
        || value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(WindowStorageError::ExpiredWindowSessionError);
    }
    value
        .parse::<u64>()
        .map_err(|_| WindowStorageError::ExpiredWindowSessionError)
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginCommandCompletionResult {
    accepted: bool,
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

fn select_public_plugin_source_with<S>(
    label: &str,
    coordinator: &Arc<LifecycleCoordinator>,
    select: S,
) -> Result<Option<PathBuf>, CommandError>
where
    S: FnOnce() -> Option<PathBuf>,
{
    require_main_label(label)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_install_failed())?;
    Ok(select())
}

#[tauri::command]
pub(crate) async fn select_public_plugin_directory(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
) -> Result<Option<PathBuf>, CommandError> {
    select_public_plugin_source_with(window.label(), coordinator.inner(), || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title("选择插件开发目录")
            .blocking_pick_folder()
            .and_then(|path| path.into_path().ok())
    })
}

#[tauri::command]
pub(crate) fn list_public_plugins(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
) -> Result<PublicPluginInventory, CommandError> {
    require_main_window(&window)?;
    Ok(service.manager()?.inventory()?)
}
#[tauri::command]
pub(crate) fn prepare_public_plugin_install(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    source: PublicPluginInstallSource,
) -> Result<PublicPluginPrepareSummary, CommandError> {
    require_main_window(&window)?;
    Ok(service
        .manager()?
        .prepare(window.label(), source, Instant::now())?)
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
    window_controller: State<'_, Arc<PluginWindowController>>,
    input: CommitPublicPluginInstallInput,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| CommandError::plugin_install_failed())?;
    let mut created_runtime = None;
    let result = service.manager()?.commit_with_readiness(
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
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    service: State<'_, Arc<PublicPluginService>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    plugin_id: String,
    enabled: bool,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let _focus = coordinator
        .suppress_transient_focus_loss()
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let mut created_runtime = None;
    let result = service
        .manager()?
        .set_enabled_with_readiness(&plugin_id, enabled, |candidate| {
            match service.create_runtime(&app, candidate) {
                Ok(_) => {
                    created_runtime = Some(candidate.label.clone());
                    true
                }
                Err(_) => false,
            }
        });
    match result {
        Ok(commit) => {
            PublicPluginService::destroy_runtime(&app, commit.closed_runtime_label.as_deref());
            plugin_window::teardown_current(
                &app,
                window_controller.inner().as_ref(),
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
pub(crate) fn set_plugin_favorite(
    window: WebviewWindow,
    service: State<'_, Arc<PublicPluginService>>,
    plugin_id: String,
    favorite: bool,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    service.manager()?.set_favorite(&plugin_id, favorite)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_plugin_effective_name(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    plugin_id: String,
    name_override: Option<String>,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let mutation = service
        .manager()?
        .rename(&plugin_id, name_override.as_deref())?;
    plugin_window::teardown_current(&app, window_controller.inner().as_ref(), &plugin_id);
    Ok(mutation)
}

#[tauri::command]
pub(crate) fn save_plugin_settings(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    input: SavePublicPluginSettingsInput,
) -> Result<PublicPluginMutation, CommandError> {
    require_main_window(&window)?;
    let manager = service.manager()?;
    for (key, value) in &input.secrets {
        manager.save_secret(&input.plugin_id, key, value.as_deref())?;
    }
    let mutation = manager.save_settings(&input.plugin_id, &input.settings)?;
    plugin_window::teardown_current(&app, window_controller.inner().as_ref(), &input.plugin_id);
    Ok(mutation)
}

#[tauri::command]
pub(crate) fn uninstall_plugin(
    window: WebviewWindow,
    app: AppHandle,
    service: State<'_, Arc<PublicPluginService>>,
    window_controller: State<'_, Arc<PluginWindowController>>,
    plugin_id: String,
    retain_data: bool,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let manager = service.manager()?;
    let Some(mut transaction) = manager.begin_uninstall(&plugin_id, retain_data)? else {
        return Ok(());
    };
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
) -> Result<PluginCommandCompletionResult, CommandError> {
    let now = Instant::now();
    let outcome = service
        .manager()?
        .complete(window.label(), &completion, now)?;
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
pub(crate) async fn search_apps(
    window: WebviewWindow,
    query: String,
    invocation_id: String,
    query_sequence: u64,
    submit: Option<bool>,
    completion_origin: Option<CompletionOriginInput>,
) -> Result<Option<SearchResponse>, CommandError> {
    require_main_window(&window)?;
    let submit = submit.unwrap_or(false);
    let app = window.app_handle();
    let registries = app.state::<ResultRegistries>();
    let registry = registries.main();
    let cache = app.state::<Arc<AppCache>>();
    let settings = app.state::<SettingsStore>();
    let public = app.state::<Arc<PublicPluginService>>();
    let normalized_query = query.trim().to_owned();
    let web_search_engine = settings.snapshot().web_search_engine;
    if completion_origin.is_none()
        && (!normalized_query.starts_with('/') || normalized_query == "/")
    {
        return Ok(search_apps_with_catalog(
            registry,
            &normalized_query,
            &invocation_id,
            query_sequence,
            |plain_query| public.manager()?.launcher_command_suggestions(plain_query),
            || cache.snapshot(),
            |applications| settings.decorate_applications(applications),
            web_search_engine,
        ));
    }
    if completion_origin.is_none()
        && (normalized_query == "/find"
            || normalized_query.starts_with("/find ")
            || normalized_query == "/web-search"
            || normalized_query.starts_with("/web-search "))
    {
        return Ok(search_apps_with_catalog(
            registry,
            &normalized_query,
            &invocation_id,
            query_sequence,
            |_| Ok(Vec::new()),
            Vec::new,
            |_| {},
            web_search_engine,
        ));
    }
    if completion_origin.is_none() {
        if let Some(prefix) = plugin_discovery_prefix(&normalized_query) {
            return Ok(publish_public_command_suggestions(
                registry,
                &invocation_id,
                query_sequence,
                public.manager()?.command_suggestions(prefix)?,
            ));
        }
    }
    let route_query = completion_origin
        .as_ref()
        .map_or(normalized_query.as_str(), |_| query.as_str());
    if let Some(route) = public.manager()?.route(route_query)? {
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
                ))
            }
            PublicPluginSearchDecision::Dispatch => {}
        }
        let Some(registry_token) =
            registry.begin_query(QueryDomain::Plugin, &invocation_id, query_sequence)
        else {
            return Ok(None);
        };
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
            PublicPluginResponse::MainResults(results) => Ok(publish_public_main_results(
                registry,
                registry_token,
                &route,
                results,
            )),
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
                    window_transfer_token: Some(prepared.transfer_token),
                    replace_local_results: false,
                }))
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
    Ok(search_apps_with(
        registry,
        &normalized_query,
        &invocation_id,
        query_sequence,
        || cache.snapshot(),
        |applications| settings.decorate_applications(applications),
        web_search_engine,
    ))
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

fn public_plugin_search_decision(
    route: &crate::public_plugins::PublicPluginRoute,
    submit: bool,
    completion_origin: Option<&CompletionOriginInput>,
) -> Result<PublicPluginSearchDecision, ()> {
    if let Some(origin) = completion_origin {
        if !crate::public_plugins::valid_plugin_id(&origin.plugin_id)
            || origin.plugin_id != route.plugin_id
            || matches!(origin.phase, CompletionOriginPhase::Preview) == submit
        {
            return Err(());
        }
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
            window_transfer_token: None,
            replace_local_results: false,
        },
    )
}

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
) -> Option<SearchResponse> {
    let entries = results
        .into_iter()
        .map(|result| {
            let action = result.copy_text.clone().map(|text| ResultAction::CopyText {
                plugin_id: route.plugin_id.clone(),
                generation: route.generation,
                text,
            });
            (
                crate::model::ResultItem {
                    result_id: String::new(),
                    activation: LauncherResultActivation::ExecuteResult,
                    title: result.title,
                    subtitle: result.subtitle,
                    icon: None,
                    plugin_icon_url: route.icon_url.clone(),
                    icon_kind: None,
                    detail: result.detail,
                    has_default_action: action.is_some(),
                },
                action,
            )
        })
        .collect();
    registry.publish_if_latest(token, entries, || true, search_response)
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
        window_transfer_token: None,
        replace_local_results: false,
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
    search_apps_with_catalog(
        registry,
        query,
        invocation_id,
        query_sequence,
        |_| Ok(Vec::new()),
        snapshot,
        decorate,
        web_search_engine,
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
            result_id: String::new(),
            activation: LauncherResultActivation::completion(completion_text)?,
            title,
            subtitle: Some(subtitle),
            icon: None,
            plugin_icon_url,
            icon_kind,
            detail: None,
            has_default_action: false,
        },
        None,
    ))
}

fn public_plugin_completion_result(
    suggestion: crate::public_plugins::PublicCommandSuggestion,
    argument: Option<&str>,
) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    let title = format!("/{}", suggestion.effective_name);
    let completion_text =
        argument.map_or_else(|| format!("{title} "), |value| format!("{title} {value}"));
    Some((
        crate::model::ResultItem {
            result_id: String::new(),
            activation: LauncherResultActivation::plugin_completion(
                completion_text,
                suggestion.plugin_id,
                suggestion.favorite,
            )?,
            title,
            subtitle: Some(suggestion.summary.unwrap_or(suggestion.display_name)),
            icon: None,
            plugin_icon_url: suggestion.icon_url,
            icon_kind: None,
            detail: None,
            has_default_action: false,
        },
        None,
    ))
}

fn search_apps_with_catalog<S, D, P>(
    registry: &ResultRegistry,
    query: &str,
    invocation_id: &str,
    query_sequence: u64,
    plugin_catalog: P,
    snapshot: S,
    decorate: D,
    web_search_engine: WebSearchEngine,
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
    let query = query.trim();
    let token = registry.begin_query(QueryDomain::Application, invocation_id, query_sequence)?;
    if let Some(result) = crate::calculator::evaluate(query) {
        let item = crate::model::ResultItem {
            result_id: String::new(),
            activation: LauncherResultActivation::ExecuteResult,
            title: result.clone(),
            subtitle: Some("复制结果".into()),
            icon: None,
            plugin_icon_url: None,
            icon_kind: Some(ResultIconKind::Calculator),
            detail: None,
            has_default_action: true,
        };
        return registry.publish_if_latest(
            token,
            vec![(item, Some(ResultAction::CopyBuiltInText { text: result }))],
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
                window_transfer_token: None,
                replace_local_results: true,
            },
        );
    }

    let mut command_hint = None;
    let mut replace_local_results = false;
    let mut entries: Vec<(crate::model::ResultItem, Option<ResultAction>)> = Vec::new();
    let catalog_query = if query == "/" { "" } else { query };
    if query == "/web-search" {
        command_hint = Some("请输入搜索内容".into());
        replace_local_results = true;
    } else if let Some(argument) = query.strip_prefix("/web-search ") {
        let argument = argument.trim();
        if argument.is_empty() {
            command_hint = Some("请输入搜索内容".into());
        } else {
            entries.push((
                crate::model::ResultItem {
                    result_id: String::new(),
                    activation: LauncherResultActivation::ExecuteResult,
                    title: crate::web_search::search_result_title(web_search_engine).into(),
                    subtitle: Some(format!("搜索：{argument}")),
                    icon: None,
                    plugin_icon_url: None,
                    icon_kind: Some(ResultIconKind::WebSearch),
                    detail: None,
                    has_default_action: true,
                },
                Some(ResultAction::OpenWebSearch {
                    engine: web_search_engine,
                    query: argument.to_owned(),
                }),
            ));
        }
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
            entries.extend(completion_result(
                "find",
                "搜索文件".into(),
                Some(ResultIconKind::Find),
                None,
                None,
            ));
            entries.extend(completion_result(
                "web-search",
                "使用默认搜索引擎搜索".into(),
                Some(ResultIconKind::WebSearch),
                None,
                None,
            ));
        } else {
            entries.push((
                crate::model::ResultItem {
                    result_id: String::new(),
                    activation: LauncherResultActivation::OpenFind {
                        query: catalog_query.to_owned(),
                    },
                    title: "/find".into(),
                    subtitle: Some(format!("搜索文件：{catalog_query}")),
                    icon: None,
                    plugin_icon_url: None,
                    icon_kind: Some(ResultIconKind::Find),
                    detail: None,
                    has_default_action: false,
                },
                None,
            ));
            entries.push((
                crate::model::ResultItem {
                    result_id: String::new(),
                    activation: LauncherResultActivation::ExecuteResult,
                    title: crate::web_search::search_result_title(web_search_engine).into(),
                    subtitle: Some(format!("搜索：{query}")),
                    icon: None,
                    plugin_icon_url: None,
                    icon_kind: Some(ResultIconKind::WebSearch),
                    detail: None,
                    has_default_action: true,
                },
                Some(ResultAction::OpenWebSearch {
                    engine: web_search_engine,
                    query: catalog_query.to_owned(),
                }),
            ));
        }
        for suggestion in suggestions {
            let argument =
                (!catalog_query.is_empty() && suggestion.favorite).then_some(catalog_query);
            entries.extend(public_plugin_completion_result(suggestion, argument));
        }
        if !catalog_query.is_empty() {
            let mut applications = snapshot();
            decorate(&mut applications);
            entries.extend(
                apps::rank(&applications, catalog_query)
                    .iter()
                    .map(apps::registry_entry)
                    .map(|(item, action)| (item, Some(action))),
            );
        }
    }

    registry.publish_if_latest(
        token,
        entries,
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
            command_hint,
            window_transfer_token: None,
            replace_local_results,
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

fn map_everything_search_error(_: EverythingSearchError) -> CommandError {
    CommandError::search_unavailable()
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
    let view = load_settings_ready_with(
        || {
            coordinator
                .mark_frontend_ready(&app)
                .map_err(|_| CommandError::window_failed())
        },
        || load_settings_core(&settings),
    )?;
    public_plugins.inner().start_enabled_runtimes(&app)?;
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
    W: FnOnce(CriticalReservation, HotkeyKind, SettingsUpdate) -> Result<(), ()> + Send + 'static,
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
    W: FnOnce(CriticalReservation, HotkeyKind, String) -> Result<(), ()> + Send + 'static,
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
    W: FnOnce(CriticalReservation) -> Result<(), ()> + Send + 'static,
{
    let reservation = reserve().map_err(|_| CommandError::settings_failed())?;
    let result = tauri::async_runtime::spawn_blocking(move || worker(reservation))
        .await
        .map_err(|_| ());
    map_save_worker_result(result)
}

fn map_save_worker_result(result: Result<Result<(), ()>, ()>) -> Result<(), CommandError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) | Err(()) => Err(CommandError::settings_failed()),
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
            open_web_search: crate::web_search::open_search,
            copy_builtin: |text: &str| app.clipboard().write_text(text.to_owned()).map_err(|_| ()),
            copy_plugin: |plugin_id: &str, generation: u64, text: &str| {
                if public_plugins
                    .manager()
                    .is_ok_and(|manager| manager.can_copy_text(plugin_id, generation))
                {
                    return app
                        .clipboard()
                        .write_text(text.to_owned())
                        .map_err(|_| PluginCopyError::SideEffectFailed);
                }
                plugins.copy_text(plugin_id, generation, || {
                    app.clipboard().write_text(text.to_owned()).map_err(|_| ())
                })
            },
            clear_and_hide: || clear_and_hide(registry, &window),
            increment: |app_id: &str| settings.increment_use_count(app_id, &cache).map_err(|_| ()),
        },
    )
    .await
}

struct ClipboardExecution<R, A, F, W, B, P, H, S> {
    resolve: R,
    execute: A,
    execute_file: F,
    open_web_search: W,
    copy_builtin: B,
    copy_plugin: P,
    clear_and_hide: H,
    increment: S,
}

async fn execute_result_with_clipboard<R, A, F, Fut, W, B, P, H, S>(
    ids: (&str, &str),
    execution: ClipboardExecution<R, A, F, W, B, P, H, S>,
) -> Result<ExecuteOutcome, CommandError>
where
    R: FnOnce(&str, &str) -> Result<ResultAction, RegistryError>,
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    F: FnOnce(FileExecutionAction) -> Fut,
    Fut: Future<Output = Result<FileExecutionOutcome, CommandError>>,
    W: FnOnce(WebSearchEngine, &str) -> Result<(), ()>,
    B: FnOnce(&str) -> Result<(), ()>,
    P: FnOnce(&str, u64, &str) -> Result<(), PluginCopyError>,
    H: FnOnce() -> Result<(), CommandError>,
    S: FnOnce(&str) -> Result<(), ()>,
{
    let (request_id, result_id) = ids;
    let ClipboardExecution {
        resolve,
        execute,
        execute_file,
        open_web_search,
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
    if let ResultAction::CopyBuiltInText { text } = &action {
        copy_builtin(text).map_err(|_| CommandError::clipboard_write_failed())?;
        clear_and_hide()?;
        return Ok(ExecuteOutcome::TextCopied);
    }
    if let ResultAction::CopyText {
        plugin_id,
        generation,
        text,
    } = &action
    {
        copy_plugin(plugin_id, *generation, text).map_err(|error| match error {
            PluginCopyError::PermissionDenied => CommandError::plugin_permission_denied(),
            PluginCopyError::SideEffectFailed => CommandError::clipboard_write_failed(),
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

#[tauri::command]
pub(crate) fn hide_launcher(
    window: WebviewWindow,
    registries: State<'_, ResultRegistries>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    clear_and_hide(registries.main(), &window)
}

pub(crate) fn clear_and_hide(
    registry: &ResultRegistry,
    window: &WebviewWindow,
) -> Result<(), CommandError> {
    let settings = window.state::<SettingsStore>();
    clear_and_hide_with(
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
        || registry.hide_and_clear(),
        || window.hide().map_err(|_| ()),
        |position| settings.set_window_position(position).map_err(|_| ()),
    )
}

fn clear_and_hide_with<P, C, H, S>(
    read_position: P,
    clear: C,
    hide: H,
    save_position: S,
) -> Result<(), CommandError>
where
    P: FnOnce() -> Result<WindowPosition, ()>,
    C: FnOnce(),
    H: FnOnce() -> Result<(), ()>,
    S: FnOnce(WindowPosition) -> Result<(), ()>,
{
    let position = read_position();
    clear();
    hide().map_err(|_| CommandError::window_failed())?;
    if let Ok(position) = position {
        let _ = save_position(position);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
    };

    use super::{
        clear_and_hide_with, execute_file_action_with, execute_resolved_result_with,
        execute_result_with, load_settings_core, load_settings_ready_with,
        map_everything_search_error, map_file_preview_worker_result, map_save_worker_result,
        map_theme_preference_worker_result, parse_timer_session_generation,
        parse_window_storage_session_generation, plugin_discovery_prefix, prepare_file_query,
        prepare_hotkey_save, prepare_settings_save, public_plugin_prompt,
        public_plugin_search_decision, publish_everything_search,
        publish_public_command_suggestions, require_find_label, require_main_label,
        resolve_invocation_theme, save_settings_core, save_settings_with,
        save_settings_worker_with, search_apps_with, search_apps_with_catalog, search_files_with,
        select_public_plugin_source_with, set_file_preview_preference_with, CommandError,
        CompletionOriginInput, CompletionOriginPhase, ExecuteOutcome, FilePreviewPreferenceUpdate,
        FindReadyOutcome, HotkeySettingsUpdate, PreparedFileQuery, PublicPluginSearchDecision,
        ThemePreferenceUpdate, UserSettingsUpdate,
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
        public_plugins::{
            PluginInvocationTheme, PublicActivationMode, PublicCommandSuggestion, PublicOutputMode,
            PublicPluginRoute, TimerError, WindowStorageError,
        },
        result_registry::{QueryDomain, RegistryError, ResultAction, ResultRegistry},
        settings::{Settings, SettingsStore, SettingsUpdate, ThemePreference, WebSearchEngine},
    };
    use tauri_plugin_global_shortcut::Shortcut;

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
                    plugin_id: "com.example.alpha-return".into(),
                    effective_name: "alpha-return".into(),
                    display_name: "Public Plugin Alpha Return".into(),
                    summary: Some("返回示例文本到主界面".into()),
                    icon_url: Some(
                        "uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.alpha/1/icon.png"
                            .into(),
                    ),
                    favorite: false,
                },
                PublicCommandSuggestion {
                    plugin_id: "com.example.alpha-window".into(),
                    effective_name: "alpha-window".into(),
                    display_name: "Public Plugin Alpha Window".into(),
                    summary: None,
                    icon_url: None,
                    favorite: false,
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
                    match &item.activation {
                        crate::model::LauncherResultActivation::PluginCompletion {
                            completion_text,
                            ..
                        } => Some(completion_text.as_str()),
                        _ => None,
                    },
                    item.has_default_action,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "/alpha-return",
                    Some("返回示例文本到主界面"),
                    Some("/alpha-return "),
                    false,
                ),
                (
                    "/alpha-window",
                    Some("Public Plugin Alpha Window"),
                    Some("/alpha-window "),
                    false,
                ),
            ]
        );
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
        )
        .unwrap();
        assert!(hint.items.is_empty());
        assert_eq!(hint.command_hint.as_deref(), Some("请输入信息回车"));
    }

    #[test]
    fn public_plugin_prompt_and_dispatch_decisions_preserve_activation_modes() {
        let mut route = PublicPluginRoute {
            plugin_id: "com.example.demo".into(),
            generation: 1,
            activation_id: 1,
            admission_epoch: 1,
            runtime_recovery_needed: false,
            runtime_label: "plugin-runtime-com.example.demo-g1".into(),
            activation_mode: PublicActivationMode::Submit,
            output_mode: PublicOutputMode::MainResult,
            input: String::new(),
            input_required: true,
            input_placeholder: Some("请输入信息回车".into()),
            window_entry: None,
            icon_url: None,
        };
        assert_eq!(
            public_plugin_search_decision(&route, false, None),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );
        assert_eq!(
            public_plugin_search_decision(&route, true, None),
            Ok(PublicPluginSearchDecision::Hint("请输入信息回车".into()))
        );

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
            plugin_id: "com.example.demo".into(),
            generation: 1,
            activation_id: 1,
            admission_epoch: 1,
            runtime_recovery_needed: false,
            runtime_label: "plugin-runtime-com.example.demo-g1".into(),
            activation_mode: PublicActivationMode::Live,
            output_mode: PublicOutputMode::MainResult,
            input: "body".into(),
            input_required: false,
            input_placeholder: Some("请输入信息回车".into()),
            window_entry: None,
            icon_url: None,
        };
        let preview = CompletionOriginInput {
            phase: CompletionOriginPhase::Preview,
            plugin_id: route.plugin_id.clone(),
        };
        let commit = CompletionOriginInput {
            phase: CompletionOriginPhase::Commit,
            plugin_id: route.plugin_id.clone(),
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
            plugin_id: format!("com.example.{name}"),
            effective_name: name.into(),
            display_name: format!("Plugin {name}"),
            summary: Some(format!("Use {name}")),
            icon_url: None,
            favorite: false,
        }
    }

    fn completion_text(item: &crate::model::ResultItem) -> Option<&str> {
        match &item.activation {
            crate::model::LauncherResultActivation::Completion { completion_text }
            | crate::model::LauncherResultActivation::PluginCompletion {
                completion_text, ..
            } => Some(completion_text),
            _ => None,
        }
    }

    #[test]
    fn launcher_empty_query_publishes_capabilities_without_reading_applications() {
        let registry = ready_registry("launcher-empty");
        let demo_title = format!("/{}", "demo-win");
        let demo_completion = format!("{demo_title} ");
        let response = search_apps_with_catalog(
            &registry,
            "   ",
            "launcher-empty",
            1,
            |_| {
                Ok(vec![
                    command_suggestion("demo-win"),
                    command_suggestion("alpha"),
                ])
            },
            || panic!("empty launcher query must not read the application snapshot"),
            |_| panic!("empty launcher query must not decorate applications"),
            WebSearchEngine::Bing,
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| (item.title.clone(), completion_text(item).map(str::to_owned)))
                .collect::<Vec<_>>(),
            vec![
                ("/find".into(), Some("/find ".into())),
                ("/web-search".into(), Some("/web-search ".into())),
                ("/alpha".into(), Some("/alpha ".into())),
                (demo_title, Some(demo_completion)),
            ]
        );
        for item in &response.items {
            assert_eq!(
                registry.resolve(&response.request_id, &item.result_id),
                Err(RegistryError::UnknownResult)
            );
        }
    }

    #[test]
    fn launcher_slash_query_publishes_the_command_catalog_without_reading_applications() {
        let registry = ready_registry("launcher-slash");
        let response = search_apps_with_catalog(
            &registry,
            "/",
            "launcher-slash",
            1,
            |query| {
                assert_eq!(query, "");
                Ok(vec![command_suggestion("alpha")])
            },
            || panic!("slash command catalog must not read the application snapshot"),
            |_| panic!("slash command catalog must not decorate applications"),
            WebSearchEngine::Bing,
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["/find", "/web-search", "/alpha"]
        );
    }

    #[test]
    fn launcher_plain_query_orders_find_web_plugins_then_applications() {
        let registry = ready_registry("launcher-plain");
        let demo_title = format!("/{}", "demo-win");
        let demo_completion = format!("{demo_title} ");
        let response = search_apps_with_catalog(
            &registry,
            "  win  ",
            "launcher-plain",
            1,
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
            WebSearchEngine::Google,
        )
        .unwrap();

        assert_eq!(
            response
                .items
                .iter()
                .map(|item| item.title.clone())
                .collect::<Vec<_>>(),
            vec![
                "/find".into(),
                "Google 搜索".into(),
                "/alpha".into(),
                demo_title,
                "Windows Terminal".into(),
            ]
        );
        assert_eq!(
            response.items[0].activation,
            crate::model::LauncherResultActivation::OpenFind {
                query: "win".into()
            }
        );
        assert_eq!(completion_text(&response.items[2]), Some("/alpha win"));
        assert_eq!(
            completion_text(&response.items[3]),
            Some(demo_completion.as_str())
        );
        assert!(matches!(
            &response.items[2].activation,
            crate::model::LauncherResultActivation::PluginCompletion {
                plugin_id,
                favorite: true,
                ..
            } if plugin_id == "com.example.alpha"
        ));
        assert!(matches!(
            &response.items[3].activation,
            crate::model::LauncherResultActivation::PluginCompletion {
                plugin_id,
                favorite: false,
                ..
            } if plugin_id == "com.example.demo-win"
        ));
        assert_eq!(
            registry.resolve(&response.request_id, &response.items[0].result_id),
            Err(RegistryError::UnknownResult)
        );
        assert!(matches!(
            registry.resolve(&response.request_id, &response.items[1].result_id),
            Ok(ResultAction::OpenWebSearch { engine: WebSearchEngine::Google, query }) if query == "win"
        ));
    }

    #[test]
    fn launcher_catalog_failure_keeps_built_ins_and_direct_web_command_uses_engine() {
        let registry = ready_registry("launcher-fallback");
        let fallback = search_apps_with_catalog(
            &registry,
            "",
            "launcher-fallback",
            1,
            |_| Err(crate::public_plugins::PublicPluginManagementError::Unavailable),
            || panic!("empty launcher query must not read applications"),
            |_| {},
            WebSearchEngine::Baidu,
        )
        .unwrap();
        assert_eq!(
            fallback
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["/find", "/web-search"]
        );

        let direct = search_apps_with_catalog(
            &registry,
            " /web-search  UiPilot docs ",
            "launcher-fallback",
            2,
            |_| panic!("reserved web command must not read plugin inventory"),
            || panic!("reserved web command must not read applications"),
            |_| {},
            WebSearchEngine::Baidu,
        )
        .unwrap();
        assert_eq!(direct.items.len(), 1);
        assert!(matches!(
            registry.resolve(&direct.request_id, &direct.items[0].result_id),
            Ok(ResultAction::OpenWebSearch { engine: WebSearchEngine::Baidu, query }) if query == "UiPilot docs"
        ));

        let hint = search_apps_with_catalog(
            &registry,
            "/web-search   ",
            "launcher-fallback",
            3,
            |_| panic!("reserved web hint must not read plugin inventory"),
            || panic!("reserved web hint must not read applications"),
            |_| {},
            WebSearchEngine::Baidu,
        )
        .unwrap();
        assert!(hint.items.is_empty());
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
            use_counts: BTreeMap::from([(APP_DUPLICATE_A.into(), 9), (APP_ABSENT.into(), 13)]),
            window_position: None,
            find_window_position: None,
            plugin_window_positions: BTreeMap::new(),
        };
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        SettingsStore::load(dir.path()).unwrap()
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
    fn public_plugin_command_callers_are_guarded_by_exact_labels() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        for command in [
            "list_public_plugins",
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
    fn public_plugin_picker_suppresses_focus_loss_only_while_the_dialog_is_open() {
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let hides = Cell::new(0);
        let expected = PathBuf::from(r"C:\Plugins\demo");

        let selected = select_public_plugin_source_with("main", &coordinator, || {
            coordinator
                .handle_focus_event_with(false, || {
                    hides.set(hides.get() + 1);
                    Ok(())
                })
                .unwrap();
            Some(expected.clone())
        });

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
            .find("commit_with_readiness(")
            .expect("commit readiness call is missing");
        assert!(suppression < runtime);
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
            .find("set_enabled_with_readiness(")
            .expect("enable readiness call is missing");
        assert!(suppression < runtime);
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
        assert_eq!(response.items[0].title, "/find");
        assert_eq!(response.items[1].title, "Bing 搜索");
        assert_eq!(response.items[1].subtitle.as_deref(), Some("搜索：app"));
        assert_eq!(
            registry
                .resolve(&response.request_id, &response.items[1].result_id)
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
            .resolve(&response.request_id, &response.items[1].result_id)
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

    #[test]
    fn math_search_replaces_app_results_and_keeps_copy_private() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with(
            &registry,
            "2*(3+4)",
            "invocation",
            1,
            || panic!("math search must not read the app cache"),
            |_| panic!("math search must not decorate applications"),
            WebSearchEngine::Bing,
        )
        .unwrap();

        assert!(response.replace_local_results);
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
            vec!["/find", "/web-search"]
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
    fn everything_search_failures_map_to_path_free_unavailable_errors() {
        for error in [
            EverythingSearchError::Unavailable,
            EverythingSearchError::RevisionExhausted,
        ] {
            let command = map_everything_search_error(error);
            assert_eq!(command, CommandError::search_unavailable());
            assert!(!command.message.contains('\\'));
            assert!(!command.message.contains(':'));
        }
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
            .find("async fn set_file_preview_preference_with")
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
            |saved| {
                assert_eq!(saved, position);
                trace.borrow_mut().push("save");
                Err(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(*trace.borrow(), ["position", "clear", "hide", "save"]);
    }

    #[test]
    fn maintenance_shared_clear_and_hide_ignores_position_failure_but_not_hide_failure() {
        assert_eq!(
            clear_and_hide_with(
                || Err(()),
                || {},
                || Ok(()),
                |_| panic!("missing position must not be saved"),
            ),
            Ok(())
        );
        assert_eq!(
            clear_and_hide_with(
                || Ok(crate::settings::WindowPosition { x: 1, y: 2 }),
                || {},
                || Err(()),
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
        let result_id = &response.items[1].result_id;
        assert!(registry.resolve(&response.request_id, result_id).is_ok());

        assert_eq!(
            clear_and_hide_with(
                || Err(()),
                || registry.hide_and_clear(),
                || Err(()),
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
        assert!(body.contains("clear_and_hide(registries.main(), &window)"));
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
    fn maintenance_wrappers_guard_before_their_first_body_statement() {
        let source = include_str!("commands.rs");
        for command in [
            "search_apps",
            "load_settings",
            "save_settings",
            "hide_launcher",
        ] {
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
    fn save_settings_maps_worker_and_join_failures_to_fixed_error() {
        assert_eq!(map_save_worker_result(Ok(Ok(()))), Ok(()));
        assert_eq!(
            map_save_worker_result(Ok(Err(()))),
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

        fn copy_action() -> ResultAction {
            ResultAction::CopyText {
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
                        copy_builtin: |text: &str| {
                            trace.borrow_mut().push("clipboard");
                            assert_eq!(text, "14");
                            Ok(())
                        },
                        copy_plugin: |_: &str, _: u64, _: &str| unreachable!(),
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
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |_: &str, _: u64, _: &str| {
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
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |plugin_id: &str, generation: u64, text: &str| {
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
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |_: &str, _: u64, _: &str| {
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
                        copy_builtin: |_: &str| unreachable!(),
                        copy_plugin: |_: &str, _: u64, _: &str| {
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
