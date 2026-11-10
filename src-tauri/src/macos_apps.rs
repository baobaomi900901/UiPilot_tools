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
    public_plugins::{launcher_runtime_macos::{LauncherRuntime, Route, InvocationTheme}, management_macos::ManagementState},
};

struct MacApplications {
    cache: Arc<AppCache>,
    registry: ResultRegistry,
    runtime: Arc<LauncherRuntime>,
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
        let runtime = LauncherRuntime::closed(management, registry.clone());
        Self {
            cache: Arc::new(cache),
            registry,
            runtime,
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
    completion_origin: Option<serde_json::Value>, stable_command_target: Option<serde_json::Value>,
    stable_builtin_target: Option<serde_json::Value>,
) -> Result<Option<SearchResponse>, Error> {
    require_main(&window).map_err(|_| Error::caller())?;
    if completion_origin.is_some() || stable_command_target.is_some() || stable_builtin_target.is_some() {
        return Err(Error { code: "searchUnavailable", message: "此平台未启用命令搜索" });
    }
    let app = window.app_handle();
    let service = app.state::<Arc<MacApplications>>();
    // The production factory is closed until an isolated transport is verified.
    // Do not even resolve the data path or start a worker in that configuration.
    if service.runtime.available() {
        let runtime = service.runtime.clone();
        let root = app.path().app_data_dir().map_err(|_| Error::unavailable())?;
        let id = invocation_id.parse::<u64>().map_err(|_| Error::stale())?;
        if id.to_string() != invocation_id { return Err(Error::stale()); }
        let theme = runtime_theme(app.state::<SettingsStore>().snapshot().theme, window.theme().ok());
        let runtime_query = query.clone();
        let route = tauri::async_runtime::spawn_blocking(move || runtime.route(&root, id, query_sequence, &runtime_query, theme, submit.unwrap_or(false)))
            .await.map_err(|_| Error::unavailable())?
            .map_err(|_| Error { code: "pluginQueryFailed", message: "插件运行未完成，请重新查询" })?;
        match route {
            Route::Published(response) => return Ok(Some(response)),
            Route::Stale => return Ok(None),
            Route::NotHandled => {},
        }
    }
    let Some(token) = service.registry.begin_query(QueryDomain::Application, &invocation_id, query_sequence) else {
        return Ok(None);
    };
    #[cfg(feature = "macos-find-preview")]
    if crate::host_capabilities::mac_find_enabled() && (query.trim().is_empty() || query.trim().starts_with('/')) {
        return Ok(publish(&service, app.state::<SettingsStore>().inner(), token, &query));
    }
    if let Some(response) = quick_query(&service, token, &query) { return Ok(response); }
    let retrying = *service.ready.borrow() == Some(false);
    if retrying { service.refresh(); }
    let mut ready = service.ready.subscribe();
    let usable = *ready.wait_for(Option::is_some).await.map_err(|_| Error::unavailable())?;
    if usable != Some(true) {
        return Err(Error { code: "searchUnavailable", message: "无法读取应用目录" });
    }
    Ok(publish(&service, app.state::<SettingsStore>().inner(), token, &query))
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

fn publish(service: &MacApplications, settings: &SettingsStore, token: QueryToken, query: &str) -> Option<SearchResponse> {
    let query = query.trim();
    let preview = crate::host_capabilities::mac_find_enabled();
    let command = preview && (query.starts_with('/') || query.is_empty());
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
    if let Some(entry) = web_entry(settings, query) { entries.push(entry); }
    service.registry.publish_if_latest(token, entries, || true, |id, items| {
        let mut response = search_response(id, items);
        response.replace_local_results = command;
        response
    })
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
        || query.is_empty() || query.starts_with('/') || query.len() > MAX_WEB_QUERY_BYTES || query.contains('\0') {
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
        assert!(matches!(service.runtime.route(&unused, 1, 1, "/settings-demo", InvocationTheme::Light, true).unwrap(), Route::NotHandled));
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
