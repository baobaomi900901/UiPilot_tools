use std::{io, sync::{Arc, Mutex}, time::{Duration, Instant}};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::watch;

use crate::{
    apps::{self, macos_backend::{self, BundleTarget, OpenAttempt, OpenOutcome}, AppCache, ApplicationLaunchTarget},
    host_capabilities::require_main,
    model::{search_response, SearchResponse},
    result_registry::{MainActionCompletionOwner, MainActionKind, QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistries, ResultRegistry},
    settings::SettingsStore,
    public_plugins::{launcher_runtime_macos::{CompletionOrigin, LauncherRuntime, Route, InvocationTheme}, management_macos::ManagementState},
};

struct MacApplications {
    cache: Arc<AppCache>,
    currency: std::sync::OnceLock<Arc<crate::quick_evaluation::currency::service::ExchangeRateService>>,
    registry: ResultRegistry,
    runtime: Arc<LauncherRuntime>,
    management: Arc<ManagementState>,
    // Status is None only before the first scan finishes. Refreshes keep serving the last good snapshot.
    ready: watch::Sender<Option<bool>>,
    scan: Mutex<Scan>,
    execute: Mutex<()>,
}

#[derive(Default)]
struct Scan {
    running: bool,
    last_started: Option<Instant>,
}

impl MacApplications {
    fn new(cache: AppCache, registry: ResultRegistry, management: Arc<ManagementState>) -> Self {
        let runtime = LauncherRuntime::closed(management.clone(), registry.clone());
        Self {
            cache: Arc::new(cache),
            currency: std::sync::OnceLock::new(),
            registry,
            runtime,
            management,
            ready: watch::channel(None).0,
            scan: Mutex::new(Scan::default()),
            execute: Mutex::new(()),
        }
    }

    fn refresh(self: &Arc<Self>) {
        let mut scan = self.scan.lock().expect("scan state");
        if scan.running || scan.last_started.is_some_and(|start| start.elapsed() < Duration::from_secs(30)) {
            return;
        }
        let retrying = *self.ready.borrow() == Some(false);
        if retrying { self.ready.send_replace(None); }
        scan.running = true;
        scan.last_started = Some(Instant::now());
        let service = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let result = objc2::rc::autoreleasepool(|_| service.cache.refresh_with(apps::discover));
            let usable = result.is_ok() || *service.ready.borrow() == Some(true);
            service.ready.send_replace(Some(usable));
            service.scan.lock().expect("scan state").running = false;
        });
    }
}

pub(crate) fn setup(app: &mut tauri::App) {
    let registries = Arc::new(ResultRegistries::default());
    let management = app.state::<Arc<ManagementState>>().inner().clone();
    let service = Arc::new(MacApplications::new(AppCache::new(), registries.main().clone(), management));
    app.manage(registries);
    app.manage(Arc::clone(&service));
    service.refresh();
}

pub(crate) fn begin_invocation(app: &AppHandle, id: u64) {
    let service = app.state::<Arc<MacApplications>>();
    service.registry.on_show(id.to_string());
    service.runtime.begin_invocation(id);
    service.refresh();
}

pub(crate) fn end_invocation(app: &AppHandle) {
    let service = app.state::<Arc<MacApplications>>();
    service.registry.hide_and_clear();
    service.runtime.invalidate_current();
}

fn runtime_theme(preference: crate::settings::ThemePreference, system: Option<tauri::Theme>) -> InvocationTheme {
    use crate::settings::ThemePreference;
    match preference {
        ThemePreference::Dark => InvocationTheme::Dark,
        ThemePreference::Light => InvocationTheme::Light,
        ThemePreference::System if system == Some(tauri::Theme::Dark) => InvocationTheme::Dark,
        ThemePreference::System => InvocationTheme::Light,
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Error {
    code: &'static str,
    message: &'static str,
}

impl Error {
    fn caller() -> Self { Self { code: "invalidCaller", message: "调用窗口无权访问应用" } }
    fn stale() -> Self { Self { code: "staleRequest", message: "结果已过期" } }
    fn unavailable() -> Self { Self { code: "applicationEntryUnavailable", message: "应用已移除或无法打开" } }
}

impl From<RegistryError> for Error {
    fn from(error: RegistryError) -> Self {
        match error {
            RegistryError::StaleRequest => Self::stale(),
            RegistryError::UnknownResult => Self { code: "unknownResult", message: "找不到此结果" },
        }
    }
}

#[tauri::command]
pub(crate) async fn search_apps(
    window: WebviewWindow, query: String, invocation_id: String, query_sequence: u64, submit: Option<bool>,
    completion_origin: Option<CompletionOrigin>, stable_command_target: Option<serde_json::Value>,
    stable_builtin_target: Option<serde_json::Value>,
) -> Result<Option<SearchResponse>, Error> {
    require_main(&window).map_err(|_| Error::caller())?;
    if stable_command_target.is_some() || stable_builtin_target.is_some() {
        return Err(Error { code: "searchUnavailable", message: "此平台未启用命令搜索" });
    }
    let app = window.app_handle();
    let service = app.state::<Arc<MacApplications>>();
    if completion_origin.is_some() && !service.runtime.available() {
        return Err(Error { code: "searchUnavailable", message: "此平台未启用命令搜索" });
    }
    // The production factory is closed until an isolated transport is verified.
    // Only the installed metadata catalog is read in that configuration; no Runtime worker starts.
    if service.runtime.available() {
        let runtime = service.runtime.clone();
        let root = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
        let id = invocation_id.parse::<u64>().map_err(|_| Error::stale())?;
        if id.to_string() != invocation_id { return Err(Error::stale()); }
        let theme = runtime_theme(app.state::<SettingsStore>().snapshot().theme, window.theme().ok());
        let runtime_query = query.clone();
        let route = tauri::async_runtime::spawn_blocking(move || runtime.route(&root, id, query_sequence, &runtime_query, theme, submit.unwrap_or(false), completion_origin))
            .await.map_err(|_| Error::unavailable())?
            .map_err(plugin_query_error)?;
        match route {
            Route::Published(response) => return Ok(Some(response)),
            Route::Stale => return Ok(None),
            Route::NotHandled => {},
        }
    }
    let Some(token) = service.registry.begin_query(QueryDomain::Application, &invocation_id, query_sequence) else {
        return Ok(None);
    };
    if let crate::quick_evaluation::QuickClassification::Ready(crate::quick_evaluation::QuickIntent::Currency(intent)) = crate::quick_evaluation::classify(query.trim()) {
        let root = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
        let owner = service.inner().clone();
        // Cache IO and provider construction stay off the main thread. One service
        // coalesces requests across amounts, queries and launcher invocations.
        let currency = tauri::async_runtime::spawn_blocking(move || owner.currency.get_or_init(||
            Arc::new(crate::quick_evaluation::currency::service::ExchangeRateService::load_or_unavailable(&root))
        ).clone()).await.map_err(|_| Error::unavailable())?;
        return Ok(crate::macos_quick_evaluation::publish_currency(&service.registry, token, intent,
            |intent| async move { currency.resolve(intent).await }).await);
    }
    if let Some(response) = quick_query(&service, token, &query) { return Ok(response); }
    let plugins = if !service.runtime.available() {
        let root = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
        let management = service.management.clone();
        let catalog_query = query.clone();
        tauri::async_runtime::spawn_blocking(move || management.unavailable_launcher_entries(&root, &catalog_query))
            .await.map_err(|_| Error::unavailable())?
            .map_err(|_| Error { code: "searchUnavailable", message: "无法读取插件命令，请在设置中刷新插件列表" })?
    } else { Vec::new() };
    let root = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
    let links = service.management.quicklinks(&root).map_err(|_| Error::unavailable())?;
    let links = tauri::async_runtime::spawn_blocking(move || links.list()).await.map_err(|_| Error::unavailable())?;
    if links.load_error.is_some() { return Err(Error { code: "quicklinkLoadFailed", message: "无法读取快速链接" }); }
    if query.trim().starts_with('/') || (crate::host_capabilities::mac_find_enabled() && query.trim().is_empty()) {
        return Ok(publish_with_links(&service, app.state::<SettingsStore>().inner(), token, &query, plugins, &links.items));
    }
    let retrying = *service.ready.borrow() == Some(false);
    if retrying { service.refresh(); }
    let mut ready = service.ready.subscribe();
    let usable = *ready.wait_for(Option::is_some).await.map_err(|_| Error::unavailable())?;
    if usable != Some(true) {
        return Err(Error { code: "searchUnavailable", message: "无法读取应用目录" });
    }
    Ok(publish_with_links(&service, app.state::<SettingsStore>().inner(), token, &query, plugins, &links.items))
}

// Outer None means application search should continue; Some(None) means a
// recognized calculation became stale and must not fall through to app search.
fn quick_query(service: &MacApplications, token: QueryToken, query: &str) -> Option<Option<SearchResponse>> {
    let classification = crate::quick_evaluation::classify(query.trim());
    if matches!(classification, crate::quick_evaluation::QuickClassification::NoMatch) { None }
    else { Some(crate::macos_quick_evaluation::publish(&service.registry, token, classification)) }
}

const MAX_WEB_QUERY_BYTES: usize = 4096;
fn web_entry(settings: &SettingsStore, query: &str) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    let query = query.trim();
    if query.is_empty() || query.starts_with('/') || query.len() > MAX_WEB_QUERY_BYTES || query.contains('\0')
        || !matches!(crate::quick_evaluation::classify(query), crate::quick_evaluation::QuickClassification::NoMatch) { return None; }
    web_query_entry(settings, query)
}

fn web_query_entry(settings: &SettingsStore, query: &str) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    if query.is_empty() || query.len() > MAX_WEB_QUERY_BYTES || query.contains('\0') { return None; }
    let settings = settings.snapshot();
    if !settings.builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::WebSearch) { return None; }
    let engine = settings.web_search_engine;
    let url = crate::web_search::search_url(engine, query)?;
    if url.len() > crate::macos_browser_open::MAX_URL_BYTES { return None; }
    Some((crate::model::ResultItem {
        result_actions: None, result_id: String::new(), activation: crate::model::LauncherResultActivation::ExecuteResult,
        title: crate::web_search::search_result_title(engine).into(), subtitle: Some(format!("搜索：{query}")),
        icon: None, plugin_icon_url: None, icon_kind: Some(crate::model::ResultIconKind::WebSearch),
        detail: None, favorite: None, has_default_action: true,
    }, Some(ResultAction::OpenWebSearch { engine, query: query.into() })))
}

fn web_command_entry(settings: &SettingsStore, query: &str) -> Option<(crate::model::ResultItem, Option<ResultAction>)> {
    use crate::{model::{LauncherResultActivation, ResultFavorite, ResultIconKind, ResultItem}, settings::{BuiltinFeature, ManagedBuiltinFeature}};
    const COMMAND: &str = "/web-search";
    let snapshot = settings.snapshot();
    if !snapshot.builtin_feature_enabled(ManagedBuiltinFeature::WebSearch) { return None; }
    if query.is_empty() || (query.starts_with('/') && COMMAND.starts_with(query)) {
        return Some((ResultItem {
            result_id: String::new(), title: COMMAND.into(), subtitle: Some("输入关键词搜索网页".into()),
            activation: LauncherResultActivation::Completion { completion_text: format!("{COMMAND} ") },
            result_actions: None, icon: None, plugin_icon_url: None, icon_kind: Some(ResultIconKind::WebSearch), detail: None,
            favorite: Some(ResultFavorite::builtin(BuiltinFeature::WebSearch, snapshot.favorite_builtin_features.contains(&BuiltinFeature::WebSearch))),
            has_default_action: false,
        }, None));
    }
    let argument = query.strip_prefix(COMMAND).filter(|tail| tail.starts_with(char::is_whitespace))?.trim();
    web_query_entry(settings, argument)
}

#[cfg(test)]
fn publish(service: &MacApplications, settings: &SettingsStore, token: QueryToken, query: &str) -> Option<SearchResponse> {
    publish_with_plugins(service, settings, token, query, Vec::new())
}
fn publish_with_plugins(service: &MacApplications, settings: &SettingsStore, token: QueryToken, query: &str, plugins: Vec<crate::model::ResultItem>) -> Option<SearchResponse> {
    publish_with_links(service, settings, token, query, plugins, &[])
}
fn publish_with_links(service: &MacApplications, settings: &SettingsStore, token: QueryToken, query: &str, plugins: Vec<crate::model::ResultItem>, links: &[crate::quicklinks::QuicklinkView]) -> Option<SearchResponse> {
    let query = query.trim();
    let preview = crate::host_capabilities::mac_find_enabled();
    let command = query.starts_with('/') || (preview && query.is_empty());
    let mut entries = Vec::new();
    if !command {
        let mut applications = service.cache.snapshot();
        settings.decorate_applications(&mut applications);
        entries.extend(apps::rank(&applications, query).iter().map(|app| {
            let (item, action) = apps::registry_entry(app);
            (item, Some(action))
        }));
    }
    if preview {
        let arg = if !command { Some(query) }
            else if query.is_empty() || "/find".starts_with(query) { Some("") }
            else { query.strip_prefix("/find ").map(str::trim) };
        if let Some(argument) = arg {
            let favorite = settings.snapshot().favorite_builtin_features.contains(&crate::settings::BuiltinFeature::Find);
            let subtitle = if argument.is_empty() { "搜索文件".into() } else { format!("搜索文件：{argument}") };
            entries.push((crate::model::find_item(argument.into(), subtitle, favorite), None));
        }
    }
    entries.extend(crate::macos_quicklinks::entries(&settings.snapshot(), query, links));
    if let Some(entry) = web_command_entry(settings, query) { entries.push(entry); }
    entries.extend(plugins.into_iter().map(|item| (item, None)));
    if let Some(entry) = web_entry(settings, query) { entries.push(entry); }
    service.registry.publish_if_latest(token, entries, || true, |id, items| {
        let mut response = search_response(id, items);
        response.replace_local_results = command;
        response
    })
}

fn plugin_query_error(error: crate::public_plugins::launcher_runtime_macos::Error) -> Error {
    if matches!(error, crate::public_plugins::launcher_runtime_macos::Error::PermissionDenied) {
        Error { code: "pluginPermissionDenied", message: "插件所需权限不可用，请在设置中检查" }
    } else {
        Error { code: "pluginQueryFailed", message: "插件运行未完成，请重新查询" }
    }
}

fn execute_registered(
    service: &MacApplications,
    request_id: &str,
    result_id: &str,
    open: impl FnOnce(&BundleTarget, &dyn Fn() -> io::Result<()>) -> io::Result<OpenAttempt>,
) -> Result<(OpenOutcome, String, Option<u64>), Error> {
    let _guard = service.execute.lock().expect("application execution");
    let (action, ticket) = service.registry.resolve_with_ticket(request_id, result_id)?;
    let ResultAction::LaunchApplication { app_id, target: ApplicationLaunchTarget::Bundle(bundle) } = action else {
        return Err(Error::unavailable());
    };
    let check_current = || {
        if service.registry.is_execution_ticket_current(&ticket) { Ok(()) }
        else { Err(io::Error::from(io::ErrorKind::Interrupted)) }
    };
    let attempt = open(&bundle, &check_current).map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted { Error::stale() } else { Error::unavailable() }
    })?;
    let hide_owner = if service.registry.retire_result_set_if_current(&ticket) {
        ticket.invocation_id().parse().ok()
    } else { None };
    match attempt {
        OpenAttempt::Confirmed(outcome) => Ok((outcome, app_id, hide_owner)),
        OpenAttempt::SubmittedUnconfirmed => Err(Error {
            code: "applicationLaunchUnconfirmed",
            message: "打开请求已提交，macOS 尚未确认启动结果",
        }),
    }
}

fn execute_copy_registered(
    service: &MacApplications, request_id: &str, result_id: &str,
    copy: impl FnOnce(&str, &mut dyn FnMut() -> bool) -> Result<crate::clipboard_history::HostCopyOutcome, crate::clipboard_history::HostCopyError>,
) -> Result<MainActionCompletionOwner, Error> {
    let _guard = service.execute.lock().expect("application execution");
    let (action, ticket) = service.registry.resolve_with_ticket(request_id, result_id)?;
    let ResultAction::CopyBuiltInText { text } = action else { return Err(Error::unavailable()); };
    let mut completion = None;
    let mut admit = || {
        if completion.is_some() { return false; }
        completion = service.registry.consume_main_action(&ticket, MainActionKind::CopyText).ok();
        completion.is_some()
    };
    copy(&text, &mut admit).map_err(copy_error)?;
    completion.ok_or_else(Error::stale)
}

fn copy_error(error: crate::clipboard_history::HostCopyError) -> Error {
    use crate::clipboard_history::HostCopyError;
    match error {
        HostCopyError::PermissionDenied => Error::stale(),
        HostCopyError::OwnershipLost | HostCopyError::WriteUnconfirmed => Error {
            code: "clipboardWriteUnconfirmed", message: "复制已尝试，尚未确认结果；请重新搜索后再试",
        },
        _ => Error { code: "clipboardWriteFailed", message: "无法复制到剪贴板" },
    }
}

fn execute_web_registered(
    service: &MacApplications, settings: &SettingsStore, request_id: &str, result_id: &str,
    open: impl FnOnce(&str, &mut dyn FnMut() -> bool) -> Result<(), crate::macos_browser_open::OpenError>,
) -> Result<MainActionCompletionOwner, Error> {
    use crate::{macos_browser_open::OpenError, settings::ManagedBuiltinFeature};
    let _guard = service.execute.lock().expect("application execution");
    let (action, ticket) = service.registry.resolve_with_ticket(request_id, result_id)?;
    let ResultAction::OpenWebSearch { engine, query } = action else { return Err(Error::unavailable()); };
    if !settings.snapshot().builtin_feature_enabled(ManagedBuiltinFeature::WebSearch)
        || query.is_empty() || query.len() > MAX_WEB_QUERY_BYTES || query.contains('\0') {
        return Err(Error { code: "webSearchFailed", message: "无法打开搜索页面" });
    }
    let url = crate::web_search::search_url(engine, &query).ok_or(Error { code: "webSearchFailed", message: "无法打开搜索页面" })?;
    let mut completion = None;
    let mut admit = || {
        if completion.is_some() { return false; }
        // Settings -> registry is held only across admission, never native open.
        completion = settings.with_enabled_builtin(ManagedBuiltinFeature::WebSearch, ||
            service.registry.consume_main_action(&ticket, MainActionKind::WebSearch).ok()).ok().flatten();
        completion.is_some()
    };
    open(&url, &mut admit).map_err(|error| match error {
        OpenError::Stale => Error::stale(),
        OpenError::Unavailable => Error { code: "webSearchFailed", message: "无法打开搜索页面" },
        OpenError::Unconfirmed => Error { code: "webSearchOpenUnconfirmed", message: "未确认打开搜索页面；请检查浏览器并重新搜索" },
    })?;
    completion.ok_or_else(Error::stale)
}

fn execute_quicklink_registered(
    service: &MacApplications, settings: &SettingsStore, appdata: &std::path::Path, request_id: &str, result_id: &str,
    open: impl FnOnce(&str, &mut dyn FnMut() -> bool) -> Result<(), crate::macos_browser_open::OpenError>,
) -> Result<MainActionCompletionOwner, Error> {
    let _guard = service.execute.lock().expect("application execution");
    let (action, ticket) = service.registry.resolve_with_ticket(request_id, result_id)?;
    let ResultAction::MacOpenQuicklink { id, command, template, query } = action else { return Err(Error::unavailable()); };
    let url = crate::macos_quicklinks::query_url(&template, &query).map_err(|_| Error { code: "quicklinkOpenFailed", message: "无法打开快速链接" })?;
    let store = service.management.quicklinks(appdata).map_err(|_| Error::stale())?;
    let unchanged = || store.list().items.iter().any(|q| q.id == id && q.command == command && q.template == template);
    if !unchanged() { return Err(Error::stale()); }
    let epoch = service.management.epoch().map_err(|_| Error::stale())?;
    let mut completion = None;
    let mut admit = || {
        if completion.is_some() { return false; }
        completion = service.management.with_epoch(appdata, epoch.clone(), |_| {
            if !unchanged() { return Err(crate::public_plugins::management_macos::Error::Stale); }
            Ok(settings.with_enabled_builtin(crate::settings::ManagedBuiltinFeature::Quicklinks, ||
                service.registry.consume_main_action(&ticket, MainActionKind::Quicklink).ok()).ok().flatten())
        }).ok().flatten();
        completion.is_some()
    };
    open(&url, &mut admit).map_err(|error| match error {
        crate::macos_browser_open::OpenError::Stale => Error::stale(),
        crate::macos_browser_open::OpenError::Unavailable => Error { code: "quicklinkOpenFailed", message: "无法打开快速链接" },
        crate::macos_browser_open::OpenError::Unconfirmed => Error { code: "webSearchOpenUnconfirmed", message: "未确认打开链接；请重新搜索" },
    })?;
    completion.ok_or_else(Error::stale)
}

fn finish_host_action(app: &AppHandle, service: &Arc<MacApplications>, owner: MainActionCompletionOwner) {
    let handle = app.clone(); let current_service = Arc::clone(service);
    let _ = app.run_on_main_thread(move || {
        if let Ok(id) = owner.invocation_id().parse() {
            if current_service.registry.finish_main_action(&owner) {
                let _ = crate::macos_host::hide_if_current(&handle, id);
            }
        }
    });
}

#[tauri::command]
pub(crate) async fn execute_result(window: WebviewWindow, request_id: String, result_id: String) -> Result<serde_json::Value, serde_json::Value> {
    #[cfg(feature = "macos-find-preview")]
    if window.label() == "find" {
        return crate::macos_find_host::execute_file(window, request_id, result_id, false).await;
    }
    execute_main(window, request_id, result_id).await
        .map_err(|error| serde_json::json!(error))
}

async fn execute_main(window: WebviewWindow, request_id: String, result_id: String) -> Result<serde_json::Value, Error> {
    require_main(&window).map_err(|_| Error::caller())?;
    let app = window.app_handle().clone();
    let service = Arc::clone(app.state::<Arc<MacApplications>>().inner());
    tauri::async_runtime::spawn_blocking(move || {
        let (action, _) = service.registry.resolve_with_ticket(&request_id, &result_id)?;
        if matches!(action, ResultAction::CopyBuiltInText { .. }) {
            let owner = execute_copy_registered(&service, &request_id, &result_id,
                |text, admit| crate::clipboard_history::copy_host_text(text, admit))?;
            finish_host_action(&app, &service, owner);
            return Ok(serde_json::json!({ "status": "textCopied" }));
        }
        if matches!(action, ResultAction::CopyText { .. }) {
            let appdata = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
            let owner = {
                let _guard = service.execute.lock().expect("application execution");
                service.runtime.copy_result(&appdata, &request_id, &result_id,
                    |text, admit| crate::clipboard_history::copy_host_text(text, admit))
                    .map_err(copy_error)?
            };
            finish_host_action(&app, &service, owner);
            return Ok(serde_json::json!({ "status": "textCopied" }));
        }
        if matches!(action, ResultAction::MacOpenQuicklink { .. }) {
            let root = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
            let owner = execute_quicklink_registered(&service, app.state::<SettingsStore>().inner(), &root, &request_id, &result_id,
                |url, admit| crate::macos_browser_open::open_url(url, admit))?;
            finish_host_action(&app, &service, owner);
            return Ok(serde_json::json!({ "status": "launchRequested" }));
        }
        if matches!(action, ResultAction::OpenWebSearch { .. }) {
            let owner = execute_web_registered(&service, app.state::<SettingsStore>().inner(), &request_id, &result_id,
                |url, admit| crate::macos_browser_open::open_url(url, admit))?;
            finish_host_action(&app, &service, owner);
            return Ok(serde_json::json!({ "status": "launchRequested" }));
        }
        let (outcome, app_id, hide_owner) = execute_registered(&service, &request_id, &result_id, |bundle, check|
            objc2::rc::autoreleasepool(|_| macos_backend::open_application(bundle, check)))?;
        if app.state::<SettingsStore>().increment_use_count(&app_id, &service.cache).is_err() {
            eprintln!("UiPilot: could not save application use count");
        }
        if let Some(id) = hide_owner {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || { let _ = crate::macos_host::hide_if_current(&handle, id); });
        }
        Ok(serde_json::json!(outcome))
    }).await.map_err(|_| Error::unavailable())?
}

#[cfg(test)]
mod platform_app_commands {
    use super::*;
    use crate::apps::macos_backend::tests::Fixture;

    #[test]
    fn plugin_permission_error_reaches_the_shared_launcher_message() {
        use crate::public_plugins::launcher_runtime_macos::Error as RuntimeError;
        assert_eq!(plugin_query_error(RuntimeError::PermissionDenied).code, "pluginPermissionDenied");
        for error in [RuntimeError::InvalidResult, RuntimeError::UnsupportedAction, RuntimeError::Unavailable, RuntimeError::Storage] {
            assert_eq!(plugin_query_error(error).code, "pluginQueryFailed");
        }
    }

    #[test]
    fn runtime_environment_resolves_the_same_theme_as_the_host() {
        use crate::settings::ThemePreference::{Dark, Light, System};
        for (preference, system, expected) in [
            (Dark, Some(tauri::Theme::Light), InvocationTheme::Dark),
            (Light, Some(tauri::Theme::Dark), InvocationTheme::Light),
            (System, Some(tauri::Theme::Dark), InvocationTheme::Dark),
            (System, Some(tauri::Theme::Light), InvocationTheme::Light),
            (System, None, InvocationTheme::Light),
        ] {
            assert_eq!(runtime_theme(preference, system), expected);
        }
    }

    #[test]
    fn closed_runtime_does_not_consume_the_application_query_or_create_inventory() {
        let (f, service, settings) = fixture();
        let unused = f.0.join("must-not-be-created");
        assert!(!service.runtime.available());
        service.runtime.begin_invocation(1);
        assert!(matches!(service.runtime.route(&unused, 1, 1, "/settings-demo", InvocationTheme::Light, true, None).unwrap(), Route::NotHandled));
        assert!(!unused.exists());
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        assert!(publish(&service, &settings, token, "中文 App").is_some());
        service.runtime.invalidate_current();
        assert!(service.registry.begin_query(QueryDomain::Application, "1", 2).is_some());
    }

    fn fixture() -> (Fixture, MacApplications, SettingsStore) {
        let f = Fixture::new();
        let path = f.bundle("中文 App.app", "com.fixture.registry", "中文 App");
        let app = macos_backend::application(&f.0, &path).unwrap();
        let service = MacApplications::new(AppCache::from_apps(vec![app]), ResultRegistries::default().main().clone(), Arc::new(ManagementState::default()));
        let settings = SettingsStore::load(&f.0.join("settings")).unwrap();
        service.registry.on_show("1".into());
        (f, service, settings)
    }

    #[test]
    fn find_results_follow_the_build_gate_and_use_the_persisted_favorite() {
        let (_f, service, settings) = fixture();
        settings.set_builtin_feature_favorite(crate::settings::BuiltinFeature::Find, true).unwrap();
        for (index, (query, argument)) in [
            ("/fi", Some("")), ("/find", Some("")), ("/find 会议", Some("会议")),
            ("会议", Some("会议")), ("/invalid", None),
        ].into_iter().enumerate() {
            let token = service.registry.begin_query(QueryDomain::Application, "1", index as u64 + 1).unwrap();
            let response = publish(&service, &settings, token, query).unwrap();
            let item = response.items.iter().find(|row| matches!(row.icon_kind, Some(crate::model::ResultIconKind::Find)));
            if crate::host_capabilities::mac_find_enabled() && argument.is_some() {
                let row = item.unwrap();
                assert!(matches!(&row.activation, crate::model::LauncherResultActivation::OpenFind { query } if Some(query.as_str()) == argument));
                assert_eq!(row.favorite, Some(crate::model::ResultFavorite::builtin(crate::settings::BuiltinFeature::Find, true)));
                assert_eq!(service.registry.resolve(&response.request_id, &row.result_id), Err(RegistryError::UnknownResult));
            } else {
                assert!(item.is_none());
            }
        }
    }

    #[test]
    fn unavailable_plugin_catalog_publishes_favorites_without_executable_registry_targets() {
        use crate::model::{LauncherResultActivation, ResultFavorite, ResultItem};
        let (_f, service, settings) = fixture();
        let rows = || vec![ResultItem {
            result_id: String::new(), title: "/demo-upper".into(),
            subtitle: Some("当前 Mac 版本暂不可运行".into()),
            activation: LauncherResultActivation::UnavailablePlugin {
                plugin_id: "com.uipilot.demo-multi".into(), command_id: "upper".into(), favorite: true,
            },
            favorite: ResultFavorite::public_plugin("com.uipilot.demo-multi".into(), "upper".into(), true),
            has_default_action: false, result_actions: None, detail: None,
            icon: None, plugin_icon_url: None, icon_kind: None,
        }];
        let stale = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let current = service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        let response = publish_with_plugins(&service, &settings, current, "/demo", rows()).unwrap();
        let row = response.items.iter().find(|r| r.title == "/demo-upper").unwrap();
        assert!(row.favorite.as_ref().unwrap().favorite);
        assert!(!row.has_default_action);
        assert_eq!(service.registry.resolve(&response.request_id, &row.result_id), Err(RegistryError::UnknownResult));
        assert_eq!(execute_registered(&service, &response.request_id, &row.result_id, |_, _| panic!("must not execute")).unwrap_err().code, "unknownResult");
        assert!(publish_with_plugins(&service, &settings, stale, "", rows()).is_none());
        let current = service.registry.begin_query(QueryDomain::Application, "1", 3).unwrap();
        service.registry.hide_and_clear();
        assert!(publish_with_plugins(&service, &settings, current, "", rows()).is_none());
    }

    #[test]
    fn latest_query_uses_opaque_host_owned_targets_and_rejects_forged_paths() {
        let (f, service, settings) = fixture();
        let first = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let latest = service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        let response = publish(&service, &settings, latest, "中文").unwrap();
        assert!(publish(&service, &settings, first, "App").is_none());
        assert_eq!(response.items[0].title, "中文 App");
        let wire = serde_json::to_string(&response).unwrap();
        assert!(!wire.contains(&f.0.to_string_lossy().to_string()));
        assert!(!wire.contains("com.fixture.registry"));
        let forged = execute_registered(&service, &response.request_id, "/tmp/other.app", |_, _| panic!("must not open"));
        assert_eq!(forged.unwrap_err().code, "unknownResult");
        service.registry.hide_and_clear();
        let stale = execute_registered(&service, &response.request_id, &response.items[0].result_id, |_, _| panic!("must not open"));
        assert_eq!(stale.unwrap_err().code, "staleRequest");
    }

    #[test]
    fn native_failure_can_retry_but_an_accepted_action_cannot_run_twice() {
        let (_f, service, settings) = fixture();
        let query = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let r = publish(&service, &settings, query, " 中文 ").unwrap();
        let id = &r.items[0].result_id;
        let failed = execute_registered(&service, &r.request_id, id, |_, _| Err(io::Error::other("missing")));
        assert_eq!(failed.unwrap_err().code, "applicationEntryUnavailable");
        let accepted = execute_registered(&service, &r.request_id, id, |_, _| Ok(OpenAttempt::Confirmed(OpenOutcome::LaunchRequested))).unwrap();
        assert_eq!(accepted.2, Some(1));
        assert!(execute_registered(&service, &r.request_id, id, |_, _| panic!("duplicate open")).is_err());
    }

    #[test]
    fn an_unconfirmed_dispatch_consumes_its_ticket_and_does_not_hide() {
        let (_f, service, settings) = fixture();
        let query = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let r = publish(&service, &settings, query, "App").unwrap();
        let id = &r.items[0].result_id;
        let pending = execute_registered(&service, &r.request_id, id, |_, check| {
            check()?;
            Ok(OpenAttempt::SubmittedUnconfirmed)
        });
        assert_eq!(pending.unwrap_err().code, "applicationLaunchUnconfirmed");
        let retry = execute_registered(&service, &r.request_id, id, |_, _| panic!("must not resubmit"));
        assert_eq!(retry.unwrap_err().code, "staleRequest");
    }

    #[test]
    fn query_replacement_during_validation_rejects_before_native_dispatch() {
        let (_f, service, settings) = fixture();
        let query = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let r = publish(&service, &settings, query, "App").unwrap();
        let action = execute_registered(&service, &r.request_id, &r.items[0].result_id, |_, admit| {
            service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
            admit()?;
            panic!("stale application must not reach Launch Services");
        });
        assert_eq!(action.unwrap_err().code, "staleRequest");
    }

    #[test]
    fn a_new_invocation_keeps_focus_when_the_old_launch_finishes() {
        let (_f, service, settings) = fixture();
        let query = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let r = publish(&service, &settings, query, "App").unwrap();
        let accepted = execute_registered(&service, &r.request_id, &r.items[0].result_id, |_, check| {
            check()?;
            service.registry.on_show("2".into());
            Ok(OpenAttempt::Confirmed(OpenOutcome::ActivationRequested))
        }).unwrap();
        assert_eq!(accepted.2, None);
        assert!(service.registry.begin_query(QueryDomain::Application, "2", 1).is_some());
    }
    fn quick_row(service: &MacApplications) -> SearchResponse {
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        crate::macos_quick_evaluation::publish(&service.registry, token, crate::quick_evaluation::classify("2+2")).unwrap()
    }
    #[test]
    fn quick_copy_uses_registry_text_and_cannot_repeat_after_submission() {
        let (_f, service, _settings) = fixture(); let r = quick_row(&service);
        let owner = execute_copy_registered(&service, &r.request_id, &r.items[0].result_id, |text, admit| {
            assert_eq!(text, "4"); assert!(admit()); assert!(!admit());
            Ok(crate::clipboard_history::HostCopyOutcome::Accepted)
        }).unwrap();
        assert!(execute_copy_registered(&service, &r.request_id, &r.items[0].result_id, |_, _| panic!("duplicate copy")).is_err());
        assert_eq!(owner.invocation_id(), "1"); assert!(service.registry.finish_main_action(&owner));
    }
    #[test]
    fn quick_copy_rejects_query_or_invocation_replacement_before_admission() {
        for show in [false, true] {
            let (_f, service, _settings) = fixture(); let r = quick_row(&service);
            let result = execute_copy_registered(&service, &r.request_id, &r.items[0].result_id, |_, admit| {
                if show { service.registry.on_show("2".into()); }
                else { service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap(); }
                assert!(!admit()); Err(crate::clipboard_history::HostCopyError::PermissionDenied)
            });
            assert_eq!(result.err().unwrap().code, "staleRequest");
        }
    }
    #[test]
    fn quick_copy_completion_does_not_hide_new_query_or_window() {
        for show in [false, true] {
            let (_f, service, _settings) = fixture(); let r = quick_row(&service);
            let owner = execute_copy_registered(&service, &r.request_id, &r.items[0].result_id, |_, admit| {
                assert!(admit());
                if show { service.registry.on_show("2".into()); }
                else { service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap(); }
                Ok(crate::clipboard_history::HostCopyOutcome::Accepted)
            }).unwrap();
            assert!(!service.registry.finish_main_action(&owner));
        }
    }
    #[test]
    fn quick_copy_nonmutating_failure_can_retry_but_unconfirmed_is_consumed() {
        let (_f, service, _settings) = fixture(); let r = quick_row(&service); let id = &r.items[0].result_id;
        let error = execute_copy_registered(&service, &r.request_id, id, |_, _| Err(crate::clipboard_history::HostCopyError::ClipboardChanged)).err().unwrap();
        assert_eq!(error.code, "clipboardWriteFailed");
        assert!(service.registry.resolve(&r.request_id, id).is_ok());
        let error = execute_copy_registered(&service, &r.request_id, id, |_, admit| {
            assert!(admit()); Err(crate::clipboard_history::HostCopyError::WriteUnconfirmed)
        }).err().unwrap();
        assert_eq!(error.code, "clipboardWriteUnconfirmed");
        assert!(execute_copy_registered(&service, &r.request_id, id, |_, _| panic!("unconfirmed is not retryable")).is_err());
    }
    #[test]
    fn quick_copy_refuses_forged_result_or_application_action_without_driver() {
        let (_f, service, settings) = fixture(); let r = quick_row(&service);
        let error = execute_copy_registered(&service, &r.request_id, "forged", |_, _| panic!()).err().unwrap();
        assert_eq!(error.code, "unknownResult");
        let token = service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        let apps = publish(&service, &settings, token, "App").unwrap();
        assert_eq!(apps.items[0].title, "中文 App");
        let error = execute_copy_registered(&service, &apps.request_id, &apps.items[0].result_id, |_, _| panic!()).err().unwrap();
        assert_eq!(error.code, "applicationEntryUnavailable");
    }

    #[test]
    fn quick_query_publishes_before_scan_ready_and_no_match_preserves_app_path() {
        let (_f, service, settings) = fixture();
        assert!(service.ready.borrow().is_none());
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        assert_eq!(quick_query(&service, token, "2+2").unwrap().unwrap().items[0].title, "4");
        assert!(service.ready.borrow().is_none());
        let token = service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        assert!(quick_query(&service, token, "中文").is_none());
        assert_eq!(publish(&service, &settings, token, "中文").unwrap().items[0].title, "中文 App");
    }

    #[test]
    fn web_fallback_exists_for_ordinary_query_without_a_reusable_favorite() {
        let (_f, service, settings) = fixture();
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let response = publish(&service, &settings, token, "我是 Jack & Jill").unwrap();
        let row = response.items.last().expect("ordinary unmatched query must offer Web fallback");
        assert_eq!(row.icon_kind, Some(crate::model::ResultIconKind::WebSearch));
        assert_eq!(row.title, "Bing 搜索"); assert!(row.favorite.is_none());
        assert!(matches!(service.registry.resolve(&response.request_id, &row.result_id),
            Ok(ResultAction::OpenWebSearch { engine: crate::settings::WebSearchEngine::Bing, query }) if query == "我是 Jack & Jill"));
    }
    #[test]
    fn reusable_web_command_has_persisted_favorite_and_no_executable_ticket() {
        let (f, service, settings) = fixture();
        settings.set_builtin_feature_favorite(crate::settings::BuiltinFeature::WebSearch, true).unwrap();
        let settings = SettingsStore::load(&f.0.join("settings")).unwrap();
        for (index, query) in ["", "/", "/web", "/web-search", "/web-search "].into_iter().enumerate() {
            let token = service.registry.begin_query(QueryDomain::Application, "1", index as u64 + 1).unwrap();
            let response = publish(&service, &settings, token, query).unwrap();
            let entry = response.items.iter().find(|item| item.title == "/web-search").expect("reusable web command");
            assert_eq!(entry.favorite, Some(crate::model::ResultFavorite::builtin(crate::settings::BuiltinFeature::WebSearch, true)));
            assert!(matches!(&entry.activation, crate::model::LauncherResultActivation::Completion { completion_text } if completion_text == "/web-search "));
            assert!(!entry.has_default_action);
            assert_eq!(service.registry.resolve(&response.request_id, &entry.result_id), Err(RegistryError::UnknownResult));
        }
    }
    #[test]
    fn explicit_web_command_routes_only_its_argument_and_still_consumes_one_host_ticket() {
        let (_f, service, settings) = fixture();
        settings.set_web_search_engine(crate::settings::WebSearchEngine::Google).unwrap();
        for (index, argument) in ["中文 & 空格 # ?", "2+2", "/find", "https://other.test/?a=1&b=2"].into_iter().enumerate() {
            service.registry.on_show("1".into());
            let token = service.registry.begin_query(QueryDomain::Application, "1", index as u64 + 1).unwrap();
            let response = publish(&service, &settings, token, &format!("/web-search {argument}")).unwrap();
            assert!(response.replace_local_results);
            assert_eq!(response.items.len(), 1);
            let row = &response.items[0];
            assert_eq!(row.title, "Google 搜索"); assert!(row.favorite.is_none());
            let owner = execute_web_registered(&service, &settings, &response.request_id, &row.result_id, |raw, admit| {
                let url = url::Url::parse(raw).unwrap();
                assert_eq!(url.host_str(), Some("www.google.com"));
                assert_eq!(url.query_pairs().collect::<Vec<_>>(), vec![("q".into(), argument.into())]);
                assert!(admit()); Ok(())
            }).unwrap();
            assert!(execute_web_registered(&service, &settings, &response.request_id, &row.result_id, |_, _| panic!("duplicate browser dispatch")).is_err());
            assert!(service.registry.finish_main_action(&owner));
        }
    }
    #[test]
    fn explicit_web_command_rejects_other_names_invalid_arguments_and_disabled_feature() {
        let (_f, service, settings) = fixture();
        let overlong = format!("/web-search {}", "a".repeat(MAX_WEB_QUERY_BYTES + 1));
        for (index, query) in ["/web-searcher hello", "/web hello", "/unknown", "/web-search a\0b", &overlong].into_iter().enumerate() {
            let token = service.registry.begin_query(QueryDomain::Application, "1", index as u64 + 1).unwrap();
            assert!(publish(&service, &settings, token, query).unwrap().items.is_empty(), "{query:?}");
        }
        settings.set_builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::WebSearch, false).unwrap();
        for (index, query) in ["/web", "/web-search query"].into_iter().enumerate() {
            let token = service.registry.begin_query(QueryDomain::Application, "1", index as u64 + 20).unwrap();
            assert!(publish(&service, &settings, token, query).unwrap().items.is_empty());
        }
    }
    fn quicklink_fixture(service: &MacApplications, root: &std::path::Path) -> crate::quicklinks::QuicklinkView {
        service.management.with(root, |m| Ok(m.save_quicklink(crate::quicklinks::QuicklinkSaveInput {
            id: None, name: "测试搜索".into(), command: "fixture-link".into(), template: "https://example.test/?q={Query}".into(), icon_token: None,
        })?.unwrap())).unwrap()
    }
    fn publish_quicklink(service: &MacApplications, settings: &SettingsStore, root: &std::path::Path, sequence: u64) -> SearchResponse {
        let links = service.management.quicklinks(root).unwrap().list().items;
        let token = service.registry.begin_query(QueryDomain::Application, "1", sequence).unwrap();
        publish_with_links(service, settings, token, "/fixture-link 中文 & test", Vec::new(), &links).unwrap()
    }
    #[test]
    fn quicklink_edit_or_delete_before_final_admission_prevents_browser_dispatch() {
        for delete in [false, true] {
            let (f, service, settings) = fixture();
            let link = quicklink_fixture(&service, &f.0);
            let response = publish_quicklink(&service, &settings, &f.0, 1);
            assert_eq!(response.items.len(), 1);
            let error = execute_quicklink_registered(&service, &settings, &f.0, &response.request_id, &response.items[0].result_id, |url, admit| {
                assert_eq!(url::Url::parse(url).unwrap().query_pairs().collect::<Vec<_>>(), vec![("q".into(), "中文 & test".into())]);
                service.management.with(&f.0, |m| {
                    if delete { m.quicklinks.delete(&link.id).unwrap(); }
                    else { m.save_quicklink(crate::quicklinks::QuicklinkSaveInput { id: Some(link.id.clone()), name: link.name.clone(), command: link.command.clone(), template: "https://other.test/?q={Query}".into(), icon_token: None })?.unwrap(); }
                    Ok(())
                }).unwrap();
                assert!(!admit());
                Err(crate::macos_browser_open::OpenError::Stale)
            }).err().unwrap();
            assert_eq!(error.code, "staleRequest");
        }
    }
    #[test]
    fn quicklink_disabling_before_admission_rejects_old_result_and_preserves_links() {
        for during in [false, true] {
            let (f, service, settings) = fixture();
            let link = quicklink_fixture(&service, &f.0);
            let response = publish_quicklink(&service, &settings, &f.0, 1);
            let disable = || settings.set_builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::Quicklinks, false).unwrap();
            if !during { disable(); }
            let result = execute_quicklink_registered(&service, &settings, &f.0, &response.request_id, &response.items[0].result_id, |_, admit| {
                if during { disable(); }
                assert!(!admit()); Err(crate::macos_browser_open::OpenError::Stale)
            });
            assert_eq!(result.err().unwrap().code, "staleRequest");
            let links = service.management.quicklinks(&f.0).unwrap().list().items;
            assert_eq!(links[0].id, link.id);
            assert!(crate::macos_quicklinks::entries(&settings.snapshot(), "", &links).is_empty());
            assert!(crate::macos_quicklinks::entries(&settings.snapshot(), "/test value", &links).is_empty());
            settings.set_builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::Quicklinks, true).unwrap();
            assert!(!crate::macos_quicklinks::entries(&settings.snapshot(), "", &links).is_empty());
        }
    }
    #[test]
    fn quicklink_admitted_before_delete_finishes_once_but_never_hides_new_query() {
        let (f, service, settings) = fixture();
        let link = quicklink_fixture(&service, &f.0);
        let response = publish_quicklink(&service, &settings, &f.0, 1);
        let id = &response.items[0].result_id;
        let owner = execute_quicklink_registered(&service, &settings, &f.0, &response.request_id, id, |_, admit| {
            assert!(admit()); assert!(!admit());
            service.management.with(&f.0, |m| { m.quicklinks.delete(&link.id).unwrap(); Ok(()) }).unwrap();
            Ok(())
        }).unwrap();
        assert!(execute_quicklink_registered(&service, &settings, &f.0, &response.request_id, id, |_, _| panic!("duplicate open")).is_err());
        service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        assert!(!service.registry.finish_main_action(&owner));
    }
    #[test]
    fn quicklinks_management_entry_is_favoritable_but_empty_custom_command_only_completes() {
        let (f, service, settings) = fixture();
        quicklink_fixture(&service, &f.0);
        settings.set_builtin_feature_favorite(crate::settings::BuiltinFeature::Quicklinks, true).unwrap();
        let links = service.management.quicklinks(&f.0).unwrap().list().items;
        for (i, query) in ["/quick", "/fixture-link"].into_iter().enumerate() {
            let token = service.registry.begin_query(QueryDomain::Application, "1", i as u64 + 1).unwrap();
            let response = publish_with_links(&service, &settings, token, query, Vec::new(), &links).unwrap();
            assert_eq!(response.items.len(), 1);
            let row = &response.items[0];
            assert!(!row.has_default_action);
            assert!(service.registry.resolve(&response.request_id, &row.result_id).is_err());
            if i == 0 { assert_eq!(row.favorite, Some(crate::model::ResultFavorite::builtin(crate::settings::BuiltinFeature::Quicklinks, true))); }
            else { assert!(matches!(&row.activation, crate::model::LauncherResultActivation::Completion { completion_text } if completion_text == "/fixture-link ")); }
        }
    }
    fn web_row(service: &MacApplications, settings: &SettingsStore) -> SearchResponse {
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        publish(service, settings, token, "浏览器 fixture").unwrap()
    }
    #[test]
    fn web_rows_use_saved_engine_and_keep_app_then_find_order_without_favorite() {
        let (_f, service, settings) = fixture();
        settings.set_web_search_engine(crate::settings::WebSearchEngine::Google).unwrap();
        settings.set_builtin_feature_favorite(crate::settings::BuiltinFeature::WebSearch, true).unwrap();
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let response = publish(&service, &settings, token, "中文").unwrap();
        assert_eq!(response.items[0].title, "中文 App");
        let row = response.items.last().unwrap(); assert_eq!(row.title, "Google 搜索"); assert!(row.favorite.is_none());
        if crate::host_capabilities::mac_find_enabled() { assert_eq!(response.items[response.items.len()-2].icon_kind, Some(crate::model::ResultIconKind::Find)); }
        settings.set_web_search_engine(crate::settings::WebSearchEngine::Baidu).unwrap();
        let owner = execute_web_registered(&service, &settings, &response.request_id, &row.result_id, |url, admit| {
            assert_eq!(url::Url::parse(url).unwrap().host_str(), Some("www.google.com"));
            assert!(admit()); Ok(())
        }).unwrap();
        assert!(service.registry.finish_main_action(&owner));
    }
    #[test]
    fn web_fallback_boundaries_keep_empty_commands_and_quick_values_out() {
        let (_f, service, settings) = fixture();
        for query in ["", "  ", "/", "/web-search", "/web-search test", "2+2", "100 USD to CNY", "a\0b"] {
            assert!(web_entry(&settings, query).is_none(), "{query:?}");
        }
        let exact = "🦀".repeat(MAX_WEB_QUERY_BYTES / 4);
        assert!(web_entry(&settings, &exact).is_some()); assert!(web_entry(&settings, &(exact + "a")).is_none());
        settings.set_builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::WebSearch, false).unwrap();
        assert!(web_entry(&settings, "browser").is_none());
        let token = service.registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        assert_eq!(publish(&service, &settings, token, "App").unwrap().items[0].title, "中文 App");
    }
    #[test]
    fn web_disable_after_publish_or_during_prepare_blocks_submission() {
        for during in [false, true] {
            let (_f, service, settings) = fixture(); let r = web_row(&service, &settings); let row = r.items.last().unwrap();
            if !during { settings.set_builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::WebSearch, false).unwrap(); }
            let result = execute_web_registered(&service, &settings, &r.request_id, &row.result_id, |_, admit| {
                assert!(during, "disabled result must not construct a browser driver");
                settings.set_builtin_feature_enabled(crate::settings::ManagedBuiltinFeature::WebSearch, false).unwrap();
                assert!(!admit()); Err(crate::macos_browser_open::OpenError::Stale)
            });
            assert_eq!(result.err().unwrap().code, if during { "staleRequest" } else { "webSearchFailed" });
        }
    }
    #[test]
    fn web_stale_forged_and_wrong_action_never_enter_opener() {
        let (_f, service, settings) = fixture(); let r = web_row(&service, &settings); let row = r.items.last().unwrap();
        assert_eq!(execute_web_registered(&service, &settings, &r.request_id, "forged", |_, _| panic!()).err().unwrap().code, "unknownResult");
        service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        assert_eq!(execute_web_registered(&service, &settings, &r.request_id, &row.result_id, |_, _| panic!()).err().unwrap().code, "staleRequest");
        service.registry.on_show("1".into()); let quick = quick_row(&service);
        assert!(execute_web_registered(&service, &settings, &quick.request_id, &quick.items[0].result_id, |_, _| panic!()).is_err());
    }
    #[test]
    fn web_final_admission_rejects_new_query_or_invocation() {
        for show in [false, true] {
            let (_f, service, settings) = fixture(); let r = web_row(&service, &settings);
            let result = execute_web_registered(&service, &settings, &r.request_id, &r.items.last().unwrap().result_id, |_, admit| {
                if show { service.registry.on_show("2".into()); } else { service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap(); }
                assert!(!admit()); Err(crate::macos_browser_open::OpenError::Stale)
            });
            assert_eq!(result.err().unwrap().code, "staleRequest");
        }
    }
    #[test]
    fn web_completion_is_single_use_and_never_hides_replacement_query() {
        for replace in [0, 1, 2] {
            let (_f, service, settings) = fixture(); let r = web_row(&service, &settings); let row = r.items.last().unwrap();
            let owner = execute_web_registered(&service, &settings, &r.request_id, &row.result_id, |_, admit| {
                assert!(admit()); assert!(!admit());
                if replace == 1 { service.registry.begin_query(QueryDomain::Application, "1", 2).unwrap(); }
                if replace == 2 { service.registry.on_show("2".into()); }
                Ok(())
            }).unwrap();
            assert!(execute_web_registered(&service, &settings, &r.request_id, &row.result_id, |_, _| panic!("duplicate open")).is_err());
            assert_eq!(service.registry.finish_main_action(&owner), replace == 0);
        }
    }
    #[test]
    fn web_prepare_failure_keeps_ticket_but_submission_error_consumes_it() {
        let (_f, service, settings) = fixture(); let r = web_row(&service, &settings); let id = &r.items.last().unwrap().result_id;
        let error = execute_web_registered(&service, &settings, &r.request_id, id, |_, _| Err(crate::macos_browser_open::OpenError::Unavailable)).err().unwrap();
        assert_eq!(error.code, "webSearchFailed"); assert!(service.registry.resolve(&r.request_id, id).is_ok());
        let error = execute_web_registered(&service, &settings, &r.request_id, id, |_, admit| {
            assert!(admit()); Err(crate::macos_browser_open::OpenError::Unconfirmed)
        }).err().unwrap();
        assert_eq!(error.code, "webSearchOpenUnconfirmed");
        assert!(execute_web_registered(&service, &settings, &r.request_id, id, |_, _| panic!("no retry")).is_err());
    }

}
