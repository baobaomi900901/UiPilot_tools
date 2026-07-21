use std::{collections::BTreeMap, future::Future, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::{
    apps::{self, AppCache, Application},
    hotkey::HotkeyKind,
    lifecycle::{CriticalReservation, FocusDecision, LifecycleCoordinator, ReservationError},
    model::SearchResponse,
    result_registry::{RegistryError, ResultAction, ResultRegistry},
    settings::{SettingsError, SettingsStore, SettingsUpdate},
    validation_data::{ValidationEvent, ValidationStore},
    validation_export::{choose_export_destination, write_validation_export, ExportDestination},
};

const ACTIVATION_REFUSED_MESSAGE: &str = "Windows 拒绝了前台切换，已发送启动请求";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppAliasTarget {
    app_id: String,
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    aliases: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsView {
    hotkey: String,
    autostart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    research_id: Option<String>,
    applications: Vec<AppAliasTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserSettingsUpdate {
    hotkey: String,
    autostart: bool,
    research_id: Option<String>,
    aliases: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
#[allow(clippy::enum_variant_names)]
pub(crate) enum ExecuteOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested { message: &'static str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum ExportOutcome {
    Cancelled,
    Exported,
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

    fn validation_failed() -> Self {
        Self {
            code: "validationFailed",
            message: "validation data operation failed",
        }
    }

    fn window_failed() -> Self {
        Self {
            code: "windowFailed",
            message: "launcher window operation failed",
        }
    }

    fn scan_failed() -> Self {
        Self {
            code: "scanFailed",
            message: "application scan failed",
        }
    }

    fn scan_worker_failed() -> Self {
        Self {
            code: "scanWorkerFailed",
            message: "application scan worker failed",
        }
    }

    fn main_thread_dispatch_failed() -> Self {
        Self {
            code: "mainThreadDispatchFailed",
            message: "main thread dispatch failed",
        }
    }

    fn export_failed() -> Self {
        Self {
            code: "exportFailed",
            message: "validation export failed",
        }
    }

    fn export_worker_failed() -> Self {
        Self {
            code: "exportWorkerFailed",
            message: "validation export worker failed",
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

fn require_main_label(label: &str) -> Result<(), CommandError> {
    (label == "main")
        .then_some(())
        .ok_or_else(CommandError::invalid_caller)
}

fn require_main_window(window: &WebviewWindow) -> Result<(), CommandError> {
    require_main_label(window.label())
}

#[tauri::command]
pub(crate) fn search_apps(
    window: WebviewWindow,
    registry: State<'_, ResultRegistry>,
    cache: State<'_, Arc<AppCache>>,
    settings: State<'_, SettingsStore>,
    query: String,
    invocation_id: String,
    query_sequence: u64,
) -> Result<Option<SearchResponse>, CommandError> {
    require_main_window(&window)?;
    Ok(search_apps_with(
        &registry,
        &query,
        &invocation_id,
        query_sequence,
        || cache.snapshot(),
        |applications| settings.decorate_applications(applications),
    ))
}

fn search_apps_with<S, D>(
    registry: &ResultRegistry,
    query: &str,
    invocation_id: &str,
    query_sequence: u64,
    snapshot: S,
    decorate: D,
) -> Option<SearchResponse>
where
    S: FnOnce() -> Vec<Application>,
    D: FnOnce(&mut [Application]),
{
    let token = registry.begin_query(invocation_id, query_sequence)?;
    let mut applications = snapshot();
    decorate(&mut applications);
    let entries = apps::rank(&applications, query)
        .iter()
        .map(apps::registry_entry)
        .collect();
    registry.publish_if_latest(token, entries)
}

#[tauri::command]
pub(crate) fn load_settings(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    settings: State<'_, SettingsStore>,
    cache: State<'_, Arc<AppCache>>,
) -> Result<SettingsView, CommandError> {
    require_main_window(&window)?;
    load_settings_ready_with(
        || {
            coordinator
                .mark_frontend_ready(&app)
                .map_err(|_| CommandError::window_failed())
        },
        || load_settings_core(&settings, &cache),
    )
}

fn load_settings_ready_with<R, L, T>(mark_ready: R, load: L) -> Result<T, CommandError>
where
    R: FnOnce() -> Result<(), CommandError>,
    L: FnOnce() -> T,
{
    mark_ready()?;
    Ok(load())
}

fn load_settings_core(settings: &SettingsStore, cache: &AppCache) -> SettingsView {
    let settings = settings.snapshot();
    let applications = cache
        .snapshot()
        .into_iter()
        .map(|application| {
            let aliases = settings
                .aliases
                .get(&application.app_id)
                .cloned()
                .unwrap_or_default();
            AppAliasTarget {
                app_id: application.app_id,
                display_name: application.display_name,
                icon: application.icon,
                aliases,
            }
        })
        .collect();
    SettingsView {
        hotkey: settings.hotkey,
        autostart: settings.autostart,
        research_id: settings.research_id,
        applications,
    }
}

#[derive(Clone, Copy)]
struct SaveSettingsCache<'a>(&'a AppCache);

impl<'a> SaveSettingsCache<'a> {
    fn inner(self) -> &'a AppCache {
        self.0
    }
}

fn prepare_settings_save(
    settings: UserSettingsUpdate,
    cache: SaveSettingsCache<'_>,
) -> Result<(HotkeyKind, SettingsUpdate), CommandError> {
    let kind = HotkeyKind::parse(&settings.hotkey).map_err(|_| CommandError::settings_failed())?;
    let update = SettingsUpdate {
        hotkey: kind.canonical(),
        autostart: settings.autostart,
        research_id: settings.research_id,
        aliases: settings.aliases,
    };
    SettingsStore::validate_user_settings(&update, cache.inner())?;
    Ok((kind, update))
}

async fn save_settings_with<R, E, W>(
    settings: UserSettingsUpdate,
    _settings_store: &SettingsStore,
    cache: SaveSettingsCache<'_>,
    reserve: R,
    worker: W,
) -> Result<(), CommandError>
where
    R: FnOnce() -> Result<CriticalReservation, E>,
    W: FnOnce(CriticalReservation, HotkeyKind, SettingsUpdate) -> Result<(), ()> + Send + 'static,
{
    let (kind, update) = prepare_settings_save(settings, cache)?;
    save_settings_worker_with(reserve, move |reservation| {
        worker(reservation, kind, update)
    })
    .await
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
    settings_store: tauri::State<'_, SettingsStore>,
    cache: tauri::State<'_, std::sync::Arc<AppCache>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    save_settings_with(
        settings,
        settings_store.inner(),
        SaveSettingsCache(cache.inner()),
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
                let cache = app_for_worker.state::<Arc<AppCache>>();
                coordinator_for_worker.save_settings_transaction(
                    &app_for_worker,
                    &settings,
                    cache.inner(),
                    kind,
                    update,
                )
            }
        },
    )
    .await
}

#[cfg(test)]
fn save_settings_core(
    settings: UserSettingsUpdate,
    store: &SettingsStore,
    cache: &AppCache,
) -> Result<(), CommandError> {
    store
        .update_user_settings(
            SettingsUpdate {
                hotkey: settings.hotkey,
                autostart: settings.autostart,
                research_id: settings.research_id,
                aliases: settings.aliases,
            },
            cache,
        )
        .map_err(|_| CommandError::settings_failed())
}

#[tauri::command]
pub(crate) fn execute_result(
    window: WebviewWindow,
    registry: State<'_, ResultRegistry>,
    validation: State<'_, ValidationStore>,
    settings: State<'_, SettingsStore>,
    cache: State<'_, Arc<AppCache>>,
    request_id: String,
    result_id: String,
) -> Result<ExecuteOutcome, CommandError> {
    require_main_window(&window)?;
    execute_result_with(
        (&request_id, &result_id),
        |request_id, result_id| registry.resolve(request_id, result_id),
        |action| apps::execute_application(action).map_err(|_| ()),
        || clear_and_hide(&registry, &window),
        |event| validation.record(event).map_err(|_| ()),
        |app_id| settings.increment_use_count(app_id, &cache).map_err(|_| ()),
    )
}

fn execute_result_with<R, A, H, V, S>(
    ids: (&str, &str),
    resolve: R,
    execute: A,
    clear_and_hide: H,
    record: V,
    increment: S,
) -> Result<ExecuteOutcome, CommandError>
where
    R: FnOnce(&str, &str) -> Result<ResultAction, RegistryError>,
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    H: FnOnce() -> Result<(), CommandError>,
    V: FnOnce(ValidationEvent) -> Result<(), ()>,
    S: FnOnce(&str) -> Result<(), ()>,
{
    let (request_id, result_id) = ids;
    let action = resolve(request_id, result_id).map_err(|error| match error {
        RegistryError::StaleRequest => CommandError::stale_request(),
        RegistryError::UnknownResult => CommandError::unknown_result(),
    })?;
    let outcome = execute(&action).map_err(|_| CommandError::application_entry_unavailable())?;
    let app_id = match &action {
        ResultAction::LaunchApplication { app_id, .. } => app_id.as_str(),
    };
    let (response, event) = outcome_parts(outcome);

    let window_error = clear_and_hide().err();
    let validation_error = record(event)
        .err()
        .map(|_| CommandError::validation_failed());
    let settings_error = increment(app_id)
        .err()
        .map(|_| CommandError::settings_failed());

    validation_error
        .or(settings_error)
        .or(window_error)
        .map_or(Ok(response), Err)
}

fn outcome_parts(outcome: apps::ApplicationActionOutcome) -> (ExecuteOutcome, ValidationEvent) {
    match outcome {
        apps::ApplicationActionOutcome::LaunchRequested => (
            ExecuteOutcome::LaunchRequested,
            ValidationEvent::LaunchRequested,
        ),
        apps::ApplicationActionOutcome::ActivationRequested => (
            ExecuteOutcome::ActivationRequested,
            ValidationEvent::ActivationRequested,
        ),
        apps::ApplicationActionOutcome::ActivationRefusedLaunchRequested => (
            ExecuteOutcome::ActivationRefusedLaunchRequested {
                message: ACTIVATION_REFUSED_MESSAGE,
            },
            ValidationEvent::ActivationRefusedLaunchRequested,
        ),
    }
}

#[tauri::command]
pub(crate) async fn rescan_apps(
    window: WebviewWindow,
    cache: State<'_, Arc<AppCache>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    let cache = Arc::clone(cache.inner());
    rescan_apps_with(move || cache.refresh().map(|_| ()).map_err(|_| ())).await
}

async fn rescan_apps_with<W>(worker: W) -> Result<(), CommandError>
where
    W: FnOnce() -> Result<(), ()> + Send + 'static,
{
    let result = tauri::async_runtime::spawn_blocking(worker)
        .await
        .map_err(|_| ());
    map_rescan_result(result)
}

fn map_rescan_result(result: Result<Result<(), ()>, ()>) -> Result<(), CommandError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(CommandError::scan_failed()),
        Err(()) => Err(CommandError::scan_worker_failed()),
    }
}

#[tauri::command]
pub(crate) async fn export_validation_data(
    window: WebviewWindow,
    app: AppHandle,
    coordinator: State<'_, Arc<LifecycleCoordinator>>,
    registry: State<'_, ResultRegistry>,
) -> Result<ExportOutcome, CommandError> {
    require_main_window(&window)?;
    let coordinator = Arc::clone(coordinator.inner());
    let modal_guard = begin_modal_export_with(
        &coordinator,
        || window.is_focused().map_err(|_| ()),
        || clear_and_hide(&registry, &window),
    )?;
    let chooser_window = window.clone();
    export_validation_data_guarded_with(
        modal_guard,
        || choose_export_on_main(chooser_window),
        move |destination| {
            let app = app.clone();
            spawn_export_worker(move || {
                let settings = app.state::<SettingsStore>();
                let validation = app.state::<ValidationStore>();
                write_validation_export(destination, &settings, &validation).map_err(|_| ())
            })
        },
    )
    .await
}

fn begin_modal_export_with<Q, H>(
    coordinator: &Arc<LifecycleCoordinator>,
    query_focus: Q,
    clear_and_hide: H,
) -> Result<crate::lifecycle::ModalGuard, CommandError>
where
    Q: FnOnce() -> Result<bool, ()>,
    H: FnOnce() -> Result<(), CommandError>,
{
    match coordinator.begin_modal_export(query_focus) {
        Ok(guard) => Ok(guard),
        Err(FocusDecision::Suppress) => Err(CommandError::export_failed()),
        Err(FocusDecision::ClearAndHide) => {
            let _ = clear_and_hide();
            Err(CommandError::export_failed())
        }
        Err(FocusDecision::ReportWindowFailureAndHide) => {
            let _window_error = CommandError::window_failed();
            let _ = clear_and_hide();
            Err(CommandError::export_failed())
        }
    }
}

async fn choose_export_on_main(
    window: WebviewWindow,
) -> Result<Option<ExportDestination>, CommandError> {
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    let chooser_window = window.clone();
    window
        .run_on_main_thread(move || {
            let result = chooser_window
                .hwnd()
                .map_err(|_| CommandError::export_failed())
                .and_then(|hwnd| {
                    choose_export_destination(hwnd).map_err(|_| CommandError::export_failed())
                });
            let _ = sender.blocking_send(result);
        })
        .map_err(|_| CommandError::main_thread_dispatch_failed())?;
    receiver
        .recv()
        .await
        .ok_or_else(CommandError::main_thread_dispatch_failed)?
}

#[cfg(test)]
async fn export_validation_data_with<D, C, CF, W, WF>(
    choose: C,
    write: W,
) -> Result<ExportOutcome, CommandError>
where
    C: FnOnce() -> CF,
    CF: Future<Output = Result<Option<D>, CommandError>>,
    W: FnOnce(D) -> WF,
    WF: Future<Output = Result<(), CommandError>>,
{
    export_validation_data_guarded_with((), choose, write).await
}

async fn export_validation_data_guarded_with<G, D, C, CF, W, WF>(
    guard: G,
    choose: C,
    write: W,
) -> Result<ExportOutcome, CommandError>
where
    C: FnOnce() -> CF,
    CF: Future<Output = Result<Option<D>, CommandError>>,
    W: FnOnce(D) -> WF,
    WF: Future<Output = Result<(), CommandError>>,
{
    let destination = choose().await;
    drop(guard);
    let Some(destination) = destination? else {
        return Ok(ExportOutcome::Cancelled);
    };
    write(destination).await?;
    Ok(ExportOutcome::Exported)
}

async fn spawn_export_worker<W>(writer: W) -> Result<(), CommandError>
where
    W: FnOnce() -> Result<(), ()> + Send + 'static,
{
    let result = tauri::async_runtime::spawn_blocking(writer)
        .await
        .map_err(|_| ());
    map_export_worker_result(result)
}

fn map_export_worker_result(result: Result<Result<(), ()>, ()>) -> Result<(), CommandError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(CommandError::export_failed()),
        Err(()) => Err(CommandError::export_worker_failed()),
    }
}

#[tauri::command]
pub(crate) fn clear_validation_data(
    window: WebviewWindow,
    validation: State<'_, ValidationStore>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    clear_validation_data_with(|| validation.clear_daily_counts().map_err(|_| ()))
}

fn clear_validation_data_with<C>(clear: C) -> Result<(), CommandError>
where
    C: FnOnce() -> Result<(), ()>,
{
    clear().map_err(|_| CommandError::validation_failed())
}

#[tauri::command]
pub(crate) fn hide_launcher(
    window: WebviewWindow,
    registry: State<'_, ResultRegistry>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    clear_and_hide(&registry, &window)
}

pub(crate) fn clear_and_hide(
    registry: &ResultRegistry,
    window: &WebviewWindow,
) -> Result<(), CommandError> {
    clear_and_hide_with(
        || registry.hide_and_clear(),
        || window.hide().map_err(|_| ()),
    )
}

fn clear_and_hide_with<C, H>(clear: C, hide: H) -> Result<(), CommandError>
where
    C: FnOnce(),
    H: FnOnce() -> Result<(), ()>,
{
    clear();
    hide().map_err(|_| CommandError::window_failed())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        rc::Rc,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc,
        },
        thread,
    };

    use super::{
        begin_modal_export_with, clear_and_hide_with, clear_validation_data_with,
        execute_result_with, export_validation_data_guarded_with, export_validation_data_with,
        load_settings_core, load_settings_ready_with, map_export_worker_result, map_rescan_result,
        map_save_worker_result, prepare_settings_save, require_main_label, rescan_apps_with,
        save_settings_core, save_settings_with, save_settings_worker_with, search_apps_with,
        spawn_export_worker, AppAliasTarget, CommandError, ExecuteOutcome, ExportOutcome,
        SaveSettingsCache, SettingsView, UserSettingsUpdate,
    };
    use crate::{
        apps::{AppCache, Application, ApplicationActionOutcome},
        hotkey::{DoubleTapModifier, HotkeyKind},
        lifecycle::LifecycleCoordinator,
        result_registry::{RegistryError, ResultAction, ResultRegistry},
        settings::{Settings, SettingsStore, SettingsUpdate},
        validation_data::ValidationEvent,
    };
    use tauri_plugin_global_shortcut::Shortcut;

    const APP_CURRENT: &str =
        "app-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const APP_EMPTY: &str = "app-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const APP_DUPLICATE_A: &str =
        "app-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const APP_DUPLICATE_B: &str =
        "app-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const APP_ABSENT: &str = "app-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const APP_UNKNOWN: &str =
        "app-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
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
            shortcut: PathBuf::from(format!(r"C:\Private\App{index:02}.lnk")),
            executable: Some(PathBuf::from(format!(r"C:\Private\App{index:02}.exe"))),
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        }
    }

    fn settings_applications() -> Vec<Application> {
        vec![
            Application {
                app_id: APP_EMPTY.into(),
                display_name: "Empty App".into(),
                shortcut: PathBuf::from(r"C:\Private\Empty.lnk"),
                executable: Some(PathBuf::from(r"C:\Private\Empty.exe")),
                icon: None,
                aliases: vec!["cache alias must not leak".into()],
                use_count: 17,
            },
            Application {
                app_id: APP_DUPLICATE_A.into(),
                display_name: "Duplicate App".into(),
                shortcut: PathBuf::from(r"C:\Private\DuplicateA.lnk"),
                executable: Some(PathBuf::from(r"C:\Private\DuplicateA.exe")),
                icon: Some("icon-a".into()),
                aliases: Vec::new(),
                use_count: 23,
            },
            Application {
                app_id: APP_DUPLICATE_B.into(),
                display_name: "Duplicate App".into(),
                shortcut: PathBuf::from(r"C:\Private\DuplicateB.lnk"),
                executable: None,
                icon: None,
                aliases: Vec::new(),
                use_count: 31,
            },
        ]
    }

    fn trusted_action() -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: APP_CURRENT.into(),
            shortcut: PathBuf::from(r"C:\Private\Current.lnk"),
            executable: Some(PathBuf::from(r"C:\Private\Current.exe")),
        }
    }

    fn settings_store(dir: &TestDir, research_id: Option<&str>) -> SettingsStore {
        let settings = Settings {
            hotkey: "Alt+Space".into(),
            autostart: false,
            research_id: research_id.map(str::to_owned),
            aliases: BTreeMap::from([
                (APP_DUPLICATE_A.into(), vec!["seed alias".into()]),
                (APP_ABSENT.into(), vec!["absent alias".into()]),
            ]),
            use_counts: BTreeMap::from([(APP_DUPLICATE_A.into(), 9), (APP_ABSENT.into(), 13)]),
        };
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        SettingsStore::load(dir.path()).unwrap()
    }

    #[test]
    fn caller_guard_rejects_all_eight_non_main_commands_without_side_effects() {
        assert_eq!(require_main_label("main"), Ok(()));
        for command in [
            "search_apps",
            "execute_result",
            "load_settings",
            "save_settings",
            "rescan_apps",
            "export_validation_data",
            "clear_validation_data",
            "hide_launcher",
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
    fn search_rejects_old_or_hidden_queries_before_state_reads() {
        let registry = ResultRegistry::default();
        assert!(search_apps_with(
            &registry,
            "app",
            "old",
            1,
            || panic!("rejected query must not read cache"),
            |_| panic!("rejected query must not read settings"),
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
        )
        .unwrap();

        assert_eq!(response.items.len(), 20);
        let json = serde_json::to_string(&response).unwrap();
        for private in ["appId", "Private", "shortcut", "executable"] {
            assert!(!json.contains(private));
        }
        assert!(registry
            .resolve(&response.request_id, &response.items[0].result_id)
            .is_ok());
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
                assert!(registry.begin_query("invocation", 2).is_some());
            },
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
        )
        .is_none());
    }

    #[test]
    fn search_empty_query_publishes_an_empty_result_set() {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let response = search_apps_with(
            &registry,
            "",
            "invocation",
            1,
            || vec![application(1)],
            |_| {},
        )
        .unwrap();
        assert!(response.items.is_empty());
    }

    #[test]
    fn settings_load_projects_all_current_applications_in_cache_order() {
        let dir = TestDir::new();
        let store = settings_store(&dir, Some("study_01"));
        let cache = AppCache::from_apps(settings_applications());

        let loaded = load_settings_core(&store, &cache);
        assert_eq!(
            loaded,
            SettingsView {
                hotkey: "Alt+Space".into(),
                autostart: false,
                research_id: Some("study_01".into()),
                applications: vec![
                    AppAliasTarget {
                        app_id: APP_EMPTY.into(),
                        display_name: "Empty App".into(),
                        icon: None,
                        aliases: Vec::new(),
                    },
                    AppAliasTarget {
                        app_id: APP_DUPLICATE_A.into(),
                        display_name: "Duplicate App".into(),
                        icon: Some("icon-a".into()),
                        aliases: vec!["seed alias".into()],
                    },
                    AppAliasTarget {
                        app_id: APP_DUPLICATE_B.into(),
                        display_name: "Duplicate App".into(),
                        icon: None,
                        aliases: Vec::new(),
                    },
                ],
            }
        );

        let json = serde_json::to_string(&loaded).unwrap();
        assert!(json.contains(r#""researchId":"study_01""#));
        assert!(!json.contains(APP_ABSENT));
        for private in ["shortcut", "executable", "path", "useCounts"] {
            assert!(!json.contains(private), "settings view exposed {private}");
        }
        assert!(!json.contains(r#""researchId":null"#));
    }

    #[test]
    fn settings_research_id_json_contract_distinguishes_view_and_update() {
        let dir = TestDir::new();
        let store = settings_store(&dir, None);
        let cache = AppCache::from_apps(settings_applications());
        let view_json = serde_json::to_value(load_settings_core(&store, &cache)).unwrap();

        assert!(!view_json.as_object().unwrap().contains_key("researchId"));

        for input in [
            serde_json::json!({
                "hotkey": "Alt+Space",
                "autostart": false,
                "aliases": {}
            }),
            serde_json::json!({
                "hotkey": "Alt+Space",
                "autostart": false,
                "researchId": null,
                "aliases": {}
            }),
        ] {
            let update: UserSettingsUpdate = serde_json::from_value(input).unwrap();
            assert_eq!(update.research_id, None);
        }
    }

    #[test]
    fn settings_task7_update_preserves_absent_alias_and_all_use_counts() {
        let dir = TestDir::new();
        let store = settings_store(&dir, Some("study_01"));
        let cache = AppCache::from_apps(settings_applications());
        let before = store.snapshot();
        let loaded = load_settings_core(&store, &cache);
        let aliases = loaded
            .applications
            .into_iter()
            .map(|application| (application.app_id, application.aliases))
            .collect();

        save_settings_core(
            UserSettingsUpdate {
                hotkey: loaded.hotkey,
                autostart: loaded.autostart,
                research_id: loaded.research_id,
                aliases,
            },
            &store,
            &cache,
        )
        .unwrap();

        let final_settings = store.snapshot();
        assert_eq!(final_settings.aliases[APP_DUPLICATE_A], ["seed alias"]);
        assert_eq!(final_settings.aliases[APP_ABSENT], ["absent alias"]);
        assert_eq!(final_settings.use_counts, before.use_counts);
    }

    #[test]
    fn settings_save_rejects_forged_or_unknown_ids_without_state_change() {
        let dir = TestDir::new();
        let store = settings_store(&dir, Some("study_01"));
        let cache = AppCache::from_apps(settings_applications());
        let before = store.snapshot();

        for key in ["forged", APP_UNKNOWN] {
            let update = UserSettingsUpdate {
                hotkey: "Alt+Space".into(),
                autostart: false,
                research_id: Some("study_01".into()),
                aliases: BTreeMap::from([(key.into(), vec!["bad".into()])]),
            };
            assert_eq!(
                save_settings_core(update, &store, &cache),
                Err(CommandError::settings_failed())
            );
            assert_eq!(store.snapshot(), before);
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
                        trace.borrow_mut().push("cache-snapshot");
                        17
                    },
                )
            });

            assert_eq!(result, expected);
            if label == "main" {
                assert_eq!(
                    *trace.borrow(),
                    [
                        "caller-guard",
                        "frontend-ready",
                        "settings-snapshot",
                        "cache-snapshot"
                    ]
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
    fn modal_export_rejects_concurrent_and_drops_guard_after_chooser() {
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let queries = Cell::new(0);
        let clears = Cell::new(0);
        let guard = begin_modal_export_with(
            &coordinator,
            || {
                queries.set(queries.get() + 1);
                Ok(true)
            },
            || {
                clears.set(clears.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            begin_modal_export_with(
                &coordinator,
                || {
                    queries.set(queries.get() + 1);
                    Ok(true)
                },
                || {
                    clears.set(clears.get() + 1);
                    Ok(())
                },
            )
            .unwrap_err(),
            CommandError::export_failed()
        );
        assert_eq!(queries.get(), 0);
        assert_eq!(clears.get(), 0);

        assert_eq!(
            tauri::async_runtime::block_on(export_validation_data_guarded_with(
                guard,
                || async { Ok(None::<usize>) },
                |_| async { panic!("cancel must skip writer") },
            )),
            Ok(ExportOutcome::Cancelled)
        );
        let recovered = begin_modal_export_with(
            &coordinator,
            || {
                queries.set(queries.get() + 1);
                Ok(true)
            },
            || {
                clears.set(clears.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(queries.get(), 1);
        drop(recovered);
    }

    #[test]
    fn modal_export_recovery_classifies_queued_and_real_focus_loss() {
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let first = begin_modal_export_with(&coordinator, || Ok(true), || Ok(())).unwrap();
        drop(first);

        let queries = Cell::new(0);
        let clears = Cell::new(0);
        let queued = begin_modal_export_with(
            &coordinator,
            || {
                queries.set(queries.get() + 1);
                Ok(true)
            },
            || {
                clears.set(clears.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(queries.get(), 1);
        assert_eq!(clears.get(), 0);
        drop(queued);

        assert_eq!(
            begin_modal_export_with(
                &coordinator,
                || {
                    queries.set(queries.get() + 1);
                    Ok(false)
                },
                || {
                    clears.set(clears.get() + 1);
                    Ok(())
                },
            )
            .unwrap_err(),
            CommandError::export_failed()
        );
        assert_eq!(queries.get(), 2);
        assert_eq!(clears.get(), 1);
        let after_real_loss = begin_modal_export_with(
            &coordinator,
            || panic!("Normal must not query focus"),
            || panic!("Normal must not clear"),
        )
        .unwrap();
        drop(after_real_loss);
    }

    #[test]
    fn modal_export_query_error_and_successful_show_restore_normal() {
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let first = begin_modal_export_with(&coordinator, || Ok(true), || Ok(())).unwrap();
        drop(first);
        let clears = Cell::new(0);
        assert_eq!(
            begin_modal_export_with(
                &coordinator,
                || Err(()),
                || {
                    clears.set(clears.get() + 1);
                    Err(CommandError::window_failed())
                },
            )
            .unwrap_err(),
            CommandError::export_failed()
        );
        assert_eq!(clears.get(), 1);
        let after_error = begin_modal_export_with(
            &coordinator,
            || panic!("Normal must not query focus"),
            || panic!("Normal must not clear"),
        )
        .unwrap();
        drop(after_error);

        coordinator.on_successful_show();
        let after_show = begin_modal_export_with(
            &coordinator,
            || panic!("successful show must restore Normal"),
            || panic!("successful show must not clear"),
        )
        .unwrap();
        drop(after_show);
    }

    #[test]
    fn modal_export_preserves_cancel_dispatch_writer_and_join_failures() {
        struct DropProbe(Rc<Cell<bool>>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        for error in [
            CommandError::main_thread_dispatch_failed(),
            CommandError::main_thread_dispatch_failed(),
        ] {
            let dropped = Rc::new(Cell::new(false));
            let expected = error.clone();
            assert_eq!(
                tauri::async_runtime::block_on(export_validation_data_guarded_with(
                    DropProbe(Rc::clone(&dropped)),
                    || async { Err::<Option<usize>, _>(error.clone()) },
                    |_| async { panic!("failed chooser must skip writer") },
                )),
                Err(expected)
            );
            assert!(dropped.get());
        }

        for error in [
            CommandError::export_failed(),
            CommandError::export_worker_failed(),
        ] {
            let dropped = Rc::new(Cell::new(false));
            let writer_dropped = Rc::clone(&dropped);
            let writer_error = error.clone();
            assert_eq!(
                tauri::async_runtime::block_on(export_validation_data_guarded_with(
                    DropProbe(Rc::clone(&dropped)),
                    || async { Ok(Some(7_usize)) },
                    move |destination| async move {
                        assert!(writer_dropped.get());
                        assert_eq!(destination, 7);
                        Err(writer_error)
                    },
                )),
                Err(error)
            );
        }

        let dropped = Rc::new(Cell::new(false));
        let writer_dropped = Rc::clone(&dropped);
        assert_eq!(
            tauri::async_runtime::block_on(export_validation_data_guarded_with(
                DropProbe(Rc::clone(&dropped)),
                || async { Ok(Some(9_usize)) },
                move |destination| async move {
                    assert!(writer_dropped.get());
                    assert_eq!(destination, 9);
                    Ok(())
                },
            )),
            Ok(ExportOutcome::Exported)
        );
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
    fn execute_success_clears_and_hides_before_persistence_in_order() {
        let cases = [
            (
                ApplicationActionOutcome::LaunchRequested,
                ExecuteOutcome::LaunchRequested,
                ValidationEvent::LaunchRequested,
            ),
            (
                ApplicationActionOutcome::ActivationRequested,
                ExecuteOutcome::ActivationRequested,
                ValidationEvent::ActivationRequested,
            ),
            (
                ApplicationActionOutcome::ActivationRefusedLaunchRequested,
                ExecuteOutcome::ActivationRefusedLaunchRequested {
                    message: "Windows 拒绝了前台切换，已发送启动请求",
                },
                ValidationEvent::ActivationRefusedLaunchRequested,
            ),
        ];

        for (action_outcome, expected_outcome, expected_event) in cases {
            let trace = RefCell::new(Vec::new());
            let actual_event = Cell::new(None);
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
                |event| {
                    trace.borrow_mut().push("validation-record");
                    actual_event.set(Some(event));
                    Ok(())
                },
                |app_id| {
                    trace.borrow_mut().push("settings-increment");
                    assert_eq!(app_id, APP_CURRENT);
                    Ok(())
                },
            );

            assert_eq!(result, Ok(expected_outcome));
            assert_eq!(actual_event.get(), Some(expected_event));
            assert_eq!(
                *trace.borrow(),
                [
                    "resolve",
                    "system-action",
                    "registry-hide-and-clear",
                    "window-hide",
                    "validation-record",
                    "settings-increment",
                ]
            );
        }
    }

    #[test]
    fn execute_uses_fixed_post_action_error_priority_and_runs_every_step_once() {
        let cases = [
            (true, false, false, CommandError::validation_failed()),
            (false, true, false, CommandError::settings_failed()),
            (false, false, true, CommandError::window_failed()),
            (true, true, false, CommandError::validation_failed()),
            (true, false, true, CommandError::validation_failed()),
            (false, true, true, CommandError::settings_failed()),
            (true, true, true, CommandError::validation_failed()),
        ];

        for (validation_fails, settings_fails, hide_fails, expected) in cases {
            let actions = Cell::new(0);
            let helpers = Cell::new(0);
            let validations = Cell::new(0);
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
                    validations.set(validations.get() + 1);
                    if validation_fails {
                        Err(())
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
            assert_eq!(validations.get(), 1);
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
            |_| {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(result, Err(CommandError::application_entry_unavailable()));
        assert_eq!(later_calls.get(), 0);
    }

    #[test]
    fn maintenance_rescan_uses_blocking_worker_and_maps_both_failure_layers() {
        let caller = thread::current().id();
        let worker = tauri::async_runtime::block_on(rescan_apps_with(move || {
            assert_ne!(thread::current().id(), caller);
            Ok(())
        }));
        assert_eq!(worker, Ok(()));
        assert_eq!(
            map_rescan_result(Ok(Err(()))),
            Err(CommandError::scan_failed())
        );
        assert_eq!(
            map_rescan_result(Err(())),
            Err(CommandError::scan_worker_failed())
        );
    }

    #[test]
    fn maintenance_export_cancel_skips_writer_and_confirm_writes_in_worker() {
        let writes = Arc::new(AtomicUsize::new(0));
        let cancel_writes = Arc::clone(&writes);
        let cancelled = tauri::async_runtime::block_on(export_validation_data_with(
            || async { Ok(None::<usize>) },
            move |_| {
                cancel_writes.fetch_add(1, Ordering::Relaxed);
                async { Ok(()) }
            },
        ));
        assert_eq!(cancelled, Ok(ExportOutcome::Cancelled));
        assert_eq!(writes.load(Ordering::Relaxed), 0);

        let caller = thread::current().id();
        let confirmed_writes = Arc::clone(&writes);
        let exported = tauri::async_runtime::block_on(export_validation_data_with(
            || async { Ok(Some(17_usize)) },
            move |destination| {
                spawn_export_worker(move || {
                    assert_eq!(destination, 17);
                    assert_ne!(thread::current().id(), caller);
                    confirmed_writes.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
            },
        ));
        assert_eq!(exported, Ok(ExportOutcome::Exported));
        assert_eq!(writes.load(Ordering::Relaxed), 1);

        assert_eq!(
            map_export_worker_result(Ok(Err(()))),
            Err(CommandError::export_failed())
        );
        assert_eq!(
            map_export_worker_result(Err(())),
            Err(CommandError::export_worker_failed())
        );
    }

    #[test]
    fn maintenance_shared_clear_and_hide_runs_once_in_registry_first_order() {
        let trace = RefCell::new(Vec::new());
        let clears = Cell::new(0);
        let hides = Cell::new(0);
        let result = clear_and_hide_with(
            || {
                clears.set(clears.get() + 1);
                trace.borrow_mut().push("clear");
            },
            || {
                hides.set(hides.get() + 1);
                trace.borrow_mut().push("hide");
                Err(())
            },
        );
        assert_eq!(result, Err(CommandError::window_failed()));
        assert_eq!(*trace.borrow(), ["clear", "hide"]);
        assert_eq!(clears.get(), 1);
        assert_eq!(hides.get(), 1);

        assert_eq!(clear_validation_data_with(|| Ok(())), Ok(()));
        assert_eq!(
            clear_validation_data_with(|| Err(())),
            Err(CommandError::validation_failed())
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
        )
        .unwrap();
        let result_id = &response.items[0].result_id;
        assert!(registry.resolve(&response.request_id, result_id).is_ok());

        assert_eq!(
            clear_and_hide_with(|| registry.hide_and_clear(), || Err(())),
            Err(CommandError::window_failed())
        );
        assert_eq!(
            registry.resolve(&response.request_id, result_id),
            Err(RegistryError::StaleRequest)
        );
        assert!(registry.begin_query("invocation", 2).is_none());
    }

    #[test]
    fn maintenance_hide_launcher_uses_only_shared_clear_and_hide_after_guard() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        let start = source.find("fn hide_launcher(").unwrap();
        let body = &source[start..source[start..].find("\n}\n").unwrap() + start + 3];
        assert!(body.contains("clear_and_hide(&registry, &window)"));
        assert!(!body.contains("registry.hide_and_clear"));
        assert!(!body.contains("window.hide()"));
    }

    #[test]
    fn maintenance_all_eight_wrappers_guard_before_their_first_body_statement() {
        let source = include_str!("commands.rs");
        for command in [
            "search_apps",
            "execute_result",
            "load_settings",
            "save_settings",
            "rescan_apps",
            "export_validation_data",
            "clear_validation_data",
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
    }

    fn user_settings(
        hotkey: &str,
        research_id: Option<&str>,
        aliases: &[(&str, &[&str])],
    ) -> UserSettingsUpdate {
        UserSettingsUpdate {
            hotkey: hotkey.into(),
            autostart: false,
            research_id: research_id.map(str::to_owned),
            aliases: aliases
                .iter()
                .map(|(app_id, aliases)| {
                    (
                        (*app_id).into(),
                        aliases.iter().map(|alias| (*alias).into()).collect(),
                    )
                })
                .collect(),
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
        let settings_store = settings_store(&dir, Some("study_01"));
        let cache = AppCache::from_apps(settings_applications());
        for settings in [
            user_settings("not a shortcut", None, &[]),
            user_settings("doublectrl", None, &[]),
            user_settings("Alt+Space", Some(" "), &[]),
            user_settings("Alt+Space", None, &[("app-BAD", &["bad"])]),
            user_settings("Alt+Space", None, &[(APP_UNKNOWN, &["unknown"])]),
        ] {
            let coordinator = Arc::new(LifecycleCoordinator::default());
            let counts = Arc::new(SaveSideEffectCounts::default());
            let reserve_counts = Arc::clone(&counts);
            let reserve_coordinator = Arc::clone(&coordinator);
            let worker_counts = Arc::clone(&counts);
            assert_eq!(
                tauri::async_runtime::block_on(save_settings_with(
                    settings,
                    &settings_store,
                    SaveSettingsCache(&cache),
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
        let store = settings_store(&dir, Some("study_01"));
        let cache = AppCache::from_apps(settings_applications());
        let before_memory = store.snapshot();
        let before_disk = fs::read(dir.path().join("settings.json")).unwrap();

        let (kind, update) = prepare_settings_save(
            user_settings(
                "Control+Space",
                Some("study_02"),
                &[(APP_EMPTY, &["alias"])],
            ),
            SaveSettingsCache(&cache),
        )
        .unwrap();

        match &kind {
            HotkeyKind::Chord(shortcut) => {
                assert_eq!(shortcut.to_string(), "control+Space");
            }
            _ => panic!("expected chord"),
        }
        assert_eq!(update.hotkey, "control+Space");
        assert_eq!(update.research_id.as_deref(), Some("study_02"));
        assert_eq!(update.aliases[APP_EMPTY], ["alias"]);
        assert_eq!(store.snapshot(), before_memory);
        assert_eq!(
            fs::read(dir.path().join("settings.json")).unwrap(),
            before_disk
        );
    }

    #[test]
    fn save_settings_preflight_accepts_double_tap_without_shortcut_parse() {
        let dir = TestDir::new();
        let settings_store = settings_store(&dir, Some("study_01"));
        let cache = AppCache::from_apps(settings_applications());
        let (kind, update) = prepare_settings_save(
            user_settings("DoubleCtrl", None, &[]),
            SaveSettingsCache(&cache),
        )
        .unwrap();
        assert_eq!(kind, HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl));
        assert_eq!(update.hotkey, "DoubleCtrl");
        let _ = settings_store;
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
    fn save_settings_worker_state_uses_managed_singletons() {
        struct ManagedState {
            store: SettingsStore,
            cache: Arc<AppCache>,
        }

        let dir = TestDir::new();
        let cache = Arc::new(AppCache::from_apps(settings_applications()));
        let coordinator = Arc::new(LifecycleCoordinator::default());
        let managed = Arc::new(ManagedState {
            store: settings_store(&dir, Some("study_01")),
            cache: Arc::clone(&cache),
        });
        let expected_store = &managed.store as *const SettingsStore as usize;
        let shortcut: Shortcut = "Alt+Space".parse().unwrap();
        let update = SettingsUpdate {
            hotkey: "Alt+Space".into(),
            autostart: false,
            research_id: Some("study_02".into()),
            aliases: BTreeMap::new(),
        };
        let caller = thread::current().id();
        let expected_cache = Arc::clone(&cache);
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
                assert!(Arc::ptr_eq(&worker_managed.cache, &expected_cache));
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
            .find("#[cfg(test)]\nfn save_settings_core")
            .map(|offset| command_start + offset)
            .unwrap();
        let command = &production[command_start..command_end];
        for state_lookup in [
            "app_for_worker.state::<SettingsStore>()",
            "app_for_worker.state::<Arc<AppCache>>()",
        ] {
            assert_eq!(command.matches(state_lookup).count(), 1);
        }
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
            "    settings_store: tauri::State<'_, SettingsStore>,\n",
            "    cache: tauri::State<'_, std::sync::Arc<AppCache>>,\n",
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

    #[test]
    fn settings_validator_wrapper_is_production_consumed() {
        let marker = ["#[cfg(", "test", ")]\nmod tests"].concat();
        let settings_source = include_str!("settings.rs").replace("\r\n", "\n");
        let commands_source = include_str!("commands.rs").replace("\r\n", "\n");
        assert_eq!(settings_source.matches(&marker).count(), 1);
        assert_eq!(commands_source.matches(&marker).count(), 1);
        let settings_production = settings_source.split(&marker).next().unwrap();
        let commands_production = commands_source.split(&marker).next().unwrap();
        let wrapper = [
            "pub(crate) fn validate_user_",
            "settings(\n",
            "        update: &SettingsUpdate,\n",
            "        cache: &AppCache,\n",
            "    ) -> Result<(), SettingsError> {\n",
            "        validate_user_settings_against(update, &cache.snapshot())\n",
            "    }",
        ]
        .concat();
        let preflight = [
            "SettingsStore::validate_user_",
            "settings(&update, cache.inner())?;",
        ]
        .concat();
        let reservation = ["coordinator.reserve_", "critical()?;"].concat();

        assert_eq!(settings_production.matches(&wrapper).count(), 1);
        assert_eq!(commands_production.matches(&preflight).count(), 1);
        assert_eq!(commands_production.matches(&reservation).count(), 1);
        assert!(
            commands_production.find(&preflight).unwrap()
                < commands_production.find(&reservation).unwrap()
        );
    }
}
