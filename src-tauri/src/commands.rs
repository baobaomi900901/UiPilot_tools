use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri::{State, WebviewWindow};

use crate::{
    apps::{self, AppCache, Application},
    model::SearchResponse,
    result_registry::{RegistryError, ResultAction, ResultRegistry},
    settings::{SettingsStore, SettingsUpdate},
    validation_data::{ValidationEvent, ValidationStore},
};

const ACTIVATION_REFUSED_MESSAGE: &str = "Windows 拒绝了前台切换，已发送启动请求";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserSettings {
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
    settings: State<'_, SettingsStore>,
    cache: State<'_, Arc<AppCache>>,
) -> Result<UserSettings, CommandError> {
    require_main_window(&window)?;
    Ok(load_settings_core(&settings, &cache))
}

fn load_settings_core(settings: &SettingsStore, cache: &AppCache) -> UserSettings {
    let settings = settings.snapshot();
    let applications = cache.snapshot();
    let aliases = settings
        .aliases
        .into_iter()
        .filter(|(app_id, _)| {
            applications
                .iter()
                .any(|application| application.app_id == *app_id)
        })
        .collect();
    UserSettings {
        hotkey: settings.hotkey,
        autostart: settings.autostart,
        research_id: settings.research_id,
        aliases,
    }
}

#[tauri::command]
pub(crate) fn save_settings(
    window: WebviewWindow,
    settings: UserSettings,
    settings_store: State<'_, SettingsStore>,
    cache: State<'_, Arc<AppCache>>,
) -> Result<(), CommandError> {
    require_main_window(&window)?;
    save_settings_core(settings, &settings_store, &cache)
}

fn save_settings_core(
    settings: UserSettings,
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
        || registry.hide_and_clear(),
        |event| validation.record(event).map_err(|_| ()),
        |app_id| settings.increment_use_count(app_id, &cache).map_err(|_| ()),
        || window.hide().map_err(|_| ()),
    )
}

fn execute_result_with<R, A, I, V, S, H>(
    ids: (&str, &str),
    resolve: R,
    execute: A,
    invalidate: I,
    record: V,
    increment: S,
    hide: H,
) -> Result<ExecuteOutcome, CommandError>
where
    R: FnOnce(&str, &str) -> Result<ResultAction, RegistryError>,
    A: FnOnce(&ResultAction) -> Result<apps::ApplicationActionOutcome, ()>,
    I: FnOnce(),
    V: FnOnce(ValidationEvent) -> Result<(), ()>,
    S: FnOnce(&str) -> Result<(), ()>,
    H: FnOnce() -> Result<(), ()>,
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

    invalidate();
    let mut first_error = None;
    if record(event).is_err() {
        first_error = Some(CommandError::validation_failed());
    }
    if increment(app_id).is_err() && first_error.is_none() {
        first_error = Some(CommandError::settings_failed());
    }
    if hide().is_err() && first_error.is_none() {
        first_error = Some(CommandError::window_failed());
    }

    first_error.map_or(Ok(response), Err)
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

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        execute_result_with, load_settings_core, require_main_label, save_settings_core,
        search_apps_with, CommandError, ExecuteOutcome, UserSettings,
    };
    use crate::{
        apps::{AppCache, Application, ApplicationActionOutcome},
        result_registry::{RegistryError, ResultAction, ResultRegistry},
        settings::{Settings, SettingsStore},
        validation_data::ValidationEvent,
    };

    const APP_CURRENT: &str =
        "app-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const APP_ABSENT: &str = "app-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const APP_UNKNOWN: &str =
        "app-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
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

    fn current_application() -> Application {
        Application {
            app_id: APP_CURRENT.into(),
            display_name: "Current".into(),
            shortcut: PathBuf::from(r"C:\Private\Current.lnk"),
            executable: None,
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        }
    }

    fn trusted_action() -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: APP_CURRENT.into(),
            shortcut: PathBuf::from(r"C:\Private\Current.lnk"),
            executable: Some(PathBuf::from(r"C:\Private\Current.exe")),
        }
    }

    fn settings_store(dir: &TestDir) -> SettingsStore {
        let settings = Settings {
            hotkey: "Alt+Space".into(),
            autostart: false,
            research_id: Some("study_01".into()),
            aliases: BTreeMap::from([
                (APP_CURRENT.into(), vec!["current alias".into()]),
                (APP_ABSENT.into(), vec!["absent alias".into()]),
            ]),
            use_counts: BTreeMap::from([(APP_CURRENT.into(), 9)]),
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
    fn settings_load_save_roundtrip_projects_current_alias_and_preserves_absent_data() {
        let dir = TestDir::new();
        let store = settings_store(&dir);
        let cache = AppCache::from_apps(vec![current_application()]);

        let loaded = load_settings_core(&store, &cache);
        assert_eq!(
            loaded.aliases,
            BTreeMap::from([(APP_CURRENT.into(), vec!["current alias".into()])])
        );
        assert!(!serde_json::to_string(&loaded)
            .unwrap()
            .contains("useCounts"));

        save_settings_core(loaded, &store, &cache).unwrap();

        let final_settings = store.snapshot();
        assert_eq!(final_settings.aliases[APP_CURRENT], ["current alias"]);
        assert_eq!(final_settings.aliases[APP_ABSENT], ["absent alias"]);
        assert_eq!(final_settings.use_counts[APP_CURRENT], 9);
    }

    #[test]
    fn settings_save_rejects_forged_or_unknown_ids_without_mutation() {
        let dir = TestDir::new();
        let store = settings_store(&dir);
        let cache = AppCache::from_apps(vec![current_application()]);
        let before = store.snapshot();

        for key in ["forged", APP_UNKNOWN] {
            let update = UserSettings {
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
                || side_effects.set(side_effects.get() + 1),
                |_| {
                    side_effects.set(side_effects.get() + 1);
                    Ok(())
                },
                |_| {
                    side_effects.set(side_effects.get() + 1);
                    Ok(())
                },
                || {
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
    fn execute_success_invalidates_then_persists_and_hides_in_order() {
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
                || trace.borrow_mut().push("registry-hide-and-clear"),
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
                || {
                    trace.borrow_mut().push("window-hide");
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
                    "validation-record",
                    "settings-increment",
                    "window-hide",
                ]
            );
        }
    }

    #[test]
    fn execute_returns_earliest_post_action_error_but_always_attempts_hide_once() {
        let cases = [
            (true, false, false, CommandError::validation_failed()),
            (false, true, false, CommandError::settings_failed()),
            (false, false, true, CommandError::window_failed()),
            (true, true, true, CommandError::validation_failed()),
            (false, true, true, CommandError::settings_failed()),
        ];

        for (validation_fails, settings_fails, hide_fails, expected) in cases {
            let actions = Cell::new(0);
            let invalidations = Cell::new(0);
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
                || invalidations.set(invalidations.get() + 1),
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
                || {
                    hides.set(hides.get() + 1);
                    if hide_fails {
                        Err(())
                    } else {
                        Ok(())
                    }
                },
            );

            assert_eq!(result, Err(expected));
            assert_eq!(actions.get(), 1);
            assert_eq!(invalidations.get(), 1);
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
            || later_calls.set(later_calls.get() + 1),
            |_| {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
            |_| {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
            || {
                later_calls.set(later_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(result, Err(CommandError::application_entry_unavailable()));
        assert_eq!(later_calls.get(), 0);
    }
}
