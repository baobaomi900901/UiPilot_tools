use std::{io, sync::{Arc, Mutex}, time::{Duration, Instant}};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::watch;

use crate::{
    apps::{self, macos_backend::{self, BundleTarget, OpenOutcome}, AppCache, ApplicationLaunchTarget},
    host_capabilities::require_main,
    model::{search_response, SearchResponse},
    result_registry::{QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistry},
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
    fn new(cache: AppCache) -> Self {
        Self {
            cache: Arc::new(cache),
            registry: ResultRegistry::for_main(),
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
    let service = Arc::new(MacApplications::new(AppCache::new()));
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
    let mut applications = service.cache.snapshot();
    settings.decorate_applications(&mut applications);
    let entries = apps::rank(&applications, query.trim()).iter().map(apps::registry_entry).collect();
    service.registry.publish_if_latest(token, entries, || true, search_response)
}

fn execute_registered(
    service: &MacApplications,
    request_id: &str,
    result_id: &str,
    open: impl FnOnce(&BundleTarget, &dyn Fn() -> io::Result<()>) -> io::Result<OpenOutcome>,
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
    let outcome = open(&bundle, &check_current).map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted { Error::stale() } else { Error::unavailable() }
    })?;
    let hide_owner = if service.registry.retire_result_set_if_current(&ticket) {
        ticket.invocation_id().parse().ok()
    } else { None };
    Ok((outcome, app_id, hide_owner))
}

#[tauri::command]
pub(crate) async fn execute_result(window: WebviewWindow, request_id: String, result_id: String) -> Result<OpenOutcome, Error> {
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
        let service = MacApplications::new(AppCache::from_apps(vec![app]));
        let settings = SettingsStore::load(&f.0.join("settings")).unwrap();
        service.registry.on_show("1".into());
        (f, service, settings)
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
        let accepted = execute_registered(&service, &r.request_id, id, |_, _| Ok(OpenOutcome::LaunchRequested)).unwrap();
        assert_eq!(accepted.2, Some(1));
        assert!(execute_registered(&service, &r.request_id, id, |_, _| panic!("duplicate open")).is_err());
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
            Ok(OpenOutcome::ActivationRequested)
        }).unwrap();
        assert_eq!(accepted.2, None);
        assert!(service.registry.begin_query(QueryDomain::Application, "2", 1).is_some());
    }
}
