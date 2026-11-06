use std::{io, sync::{Arc, Mutex}, time::{Duration, Instant}};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::watch;

use crate::{
    apps::{self, macos_backend::{self, BundleTarget, OpenAttempt, OpenOutcome}, AppCache, ApplicationLaunchTarget},
    host_capabilities::require_main,
    model::{search_response, SearchResponse},
    result_registry::{QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistries, ResultRegistry},
    settings::SettingsStore,
};

struct MacApplications {
    cache: Arc<AppCache>,
    registry: ResultRegistry,
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
    fn new(cache: AppCache, registry: ResultRegistry) -> Self {
        Self {
            cache: Arc::new(cache),
            registry,
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
    let service = Arc::new(MacApplications::new(AppCache::new(), registries.main().clone()));
    app.manage(registries);
    app.manage(Arc::clone(&service));
    service.refresh();
}

pub(crate) fn begin_invocation(app: &AppHandle, id: u64) {
    let service = app.state::<Arc<MacApplications>>();
    service.registry.on_show(id.to_string());
    service.refresh();
}

pub(crate) fn end_invocation(app: &AppHandle) {
    app.state::<Arc<MacApplications>>().registry.hide_and_clear();
}

#[derive(Debug, Serialize)]
pub(crate) struct Error {
    code: &'static str,
    message: &'static str,
}

impl Error {
    fn caller() -> Self { Self { code: "invalidCaller", message: "调用窗口无权访问应用" } }
    fn stale() -> Self { Self { code: "staleRequest", message: "应用结果已过期" } }
    fn unavailable() -> Self { Self { code: "applicationEntryUnavailable", message: "应用已移除或无法打开" } }
}

impl From<RegistryError> for Error {
    fn from(error: RegistryError) -> Self {
        match error {
            RegistryError::StaleRequest => Self::stale(),
            RegistryError::UnknownResult => Self { code: "unknownResult", message: "找不到此应用结果" },
        }
    }
}

#[tauri::command]
pub(crate) async fn search_apps(
    window: WebviewWindow, query: String, invocation_id: String, query_sequence: u64,
    completion_origin: Option<serde_json::Value>, stable_command_target: Option<serde_json::Value>,
    stable_builtin_target: Option<serde_json::Value>,
) -> Result<Option<SearchResponse>, Error> {
    require_main(&window).map_err(|_| Error::caller())?;
    if completion_origin.is_some() || stable_command_target.is_some() || stable_builtin_target.is_some() {
        return Err(Error { code: "searchUnavailable", message: "此平台未启用命令搜索" });
    }
    let app = window.app_handle();
    let service = app.state::<Arc<MacApplications>>();
    let Some(token) = service.registry.begin_query(QueryDomain::Application, &invocation_id, query_sequence) else {
        return Ok(None);
    };
    #[cfg(feature = "macos-find-preview")]
    if crate::host_capabilities::mac_find_enabled() && (query.trim().is_empty() || query.trim().starts_with('/')) {
        return Ok(publish(&service, app.state::<SettingsStore>().inner(), token, &query));
    }
    let retrying = *service.ready.borrow() == Some(false);
    if retrying { service.refresh(); }
    let mut ready = service.ready.subscribe();
    let usable = *ready.wait_for(Option::is_some).await.map_err(|_| Error::unavailable())?;
    if usable != Some(true) {
        return Err(Error { code: "searchUnavailable", message: "无法读取应用目录" });
    }
    Ok(publish(&service, app.state::<SettingsStore>().inner(), token, &query))
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

#[tauri::command]
pub(crate) async fn execute_result(window: WebviewWindow, request_id: String, result_id: String) -> Result<serde_json::Value, serde_json::Value> {
    #[cfg(feature = "macos-find-preview")]
    if window.label() == "find" {
        return crate::macos_find_host::execute_file(window, request_id, result_id, false).await;
    }
    execute_app(window, request_id, result_id).await.map(|outcome| serde_json::json!(outcome))
        .map_err(|error| serde_json::json!(error))
}

async fn execute_app(window: WebviewWindow, request_id: String, result_id: String) -> Result<OpenOutcome, Error> {
    require_main(&window).map_err(|_| Error::caller())?;
    let app = window.app_handle().clone();
    let service = Arc::clone(app.state::<Arc<MacApplications>>().inner());
    tauri::async_runtime::spawn_blocking(move || {
        let (outcome, app_id, hide_owner) = execute_registered(&service, &request_id, &result_id, |bundle, check|
            objc2::rc::autoreleasepool(|_| macos_backend::open_application(bundle, check)))?;
        if app.state::<SettingsStore>().increment_use_count(&app_id, &service.cache).is_err() {
            eprintln!("UiPilot: could not save application use count");
        }
        if let Some(id) = hide_owner {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || { let _ = crate::macos_host::hide_if_current(&handle, id); });
        }
        Ok(outcome)
    }).await.map_err(|_| Error::unavailable())?
}

#[cfg(test)]
mod platform_app_commands {
    use super::*;
    use crate::apps::macos_backend::tests::Fixture;

    fn fixture() -> (Fixture, MacApplications, SettingsStore) {
        let f = Fixture::new();
        let path = f.bundle("中文 App.app", "com.fixture.registry", "中文 App");
        let app = macos_backend::application(&f.0, &path).unwrap();
        let service = MacApplications::new(AppCache::from_apps(vec![app]), ResultRegistries::default().main().clone());
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
}
