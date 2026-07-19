use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri::{State, WebviewWindow};

use crate::{
    apps::{self, AppCache, Application},
    model::SearchResponse,
    result_registry::ResultRegistry,
    settings::{SettingsStore, SettingsUpdate},
};

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

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        load_settings_core, require_main_label, save_settings_core, search_apps_with, CommandError,
        UserSettings,
    };
    use crate::{
        apps::{AppCache, Application},
        result_registry::ResultRegistry,
        settings::{Settings, SettingsStore},
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
}
