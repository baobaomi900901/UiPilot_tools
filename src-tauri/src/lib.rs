use std::sync::Arc;

#[cfg(not(feature = "test-instrumentation"))]
use tauri::Manager;

#[cfg_attr(not(test), allow(dead_code))]
mod atomic_file;

#[cfg_attr(not(test), allow(dead_code))]
mod apps;
// ponytail: Task 2 defines the protocol before Task 5 wires commands; remove these allows then.
#[cfg_attr(not(test), allow(dead_code))]
mod model;
#[cfg_attr(not(test), allow(dead_code))]
mod result_registry;

#[cfg_attr(not(test), allow(dead_code))]
mod session_marker;

#[cfg_attr(not(test), allow(dead_code))]
mod settings;

#[cfg_attr(not(test), allow(dead_code))]
mod validation_data;

#[cfg(feature = "test-instrumentation")]
mod security_probe;

#[cfg(any(test, not(feature = "test-instrumentation")))]
fn load_settings_store(
    app_data_dir: &std::path::Path,
) -> Result<settings::SettingsStore, settings::SettingsError> {
    settings::SettingsStore::load(app_data_dir)
}

#[cfg(any(test, not(feature = "test-instrumentation")))]
fn load_and_open_validation_store(
    app_data_dir: &std::path::Path,
) -> Result<validation_data::ValidationStore, validation_data::ValidationError> {
    let store = validation_data::ValidationStore::load(app_data_dir)?;
    store.reconcile_and_open_session()?;
    Ok(store)
}

pub fn run() {
    let app_cache = Arc::new(apps::AppCache::new());
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(Arc::clone(&app_cache));

    #[cfg(feature = "test-instrumentation")]
    let builder = builder.invoke_handler(tauri::generate_handler![security_probe::load_settings]);

    builder
        .setup(move |_app| {
            #[cfg(feature = "test-instrumentation")]
            security_probe::setup(_app)?;

            #[cfg(not(feature = "test-instrumentation"))]
            {
                let app_data_dir = _app.path().app_data_dir()?;
                let settings = load_settings_store(&app_data_dir)?;
                assert!(_app.manage(settings), "settings store already managed");

                let validation = load_and_open_validation_store(&app_data_dir)?;
                assert!(_app.manage(validation), "validation store already managed");
            }

            let _ = apps::start_initial_refresh(Arc::clone(&app_cache))?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        apps::{AppCache, Application},
        load_and_open_validation_store, load_settings_store,
        settings::Settings,
    };

    const APP_A: &str = "app-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "uipilot-settings-setup-{}-{id}",
                std::process::id()
            )))
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

    #[test]
    fn load_settings_store_uses_the_same_persisted_path_on_reload() {
        let dir = TestDir::new();
        let store = load_settings_store(dir.path()).unwrap();
        assert_eq!(store.snapshot(), Settings::default());
        let cache = AppCache::from_apps(vec![Application {
            app_id: APP_A.into(),
            display_name: "App".into(),
            shortcut: PathBuf::from(r"C:\Menu\App.lnk"),
            executable: None,
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        }]);
        store.increment_use_count(APP_A, &cache).unwrap();
        drop(store);

        let reloaded = load_settings_store(dir.path()).unwrap();

        assert_eq!(reloaded.snapshot().use_counts[APP_A], 1);
    }

    #[test]
    fn load_and_open_validation_store_creates_marker_before_returning() {
        let dir = TestDir::new();

        let _store = load_and_open_validation_store(dir.path()).unwrap();

        assert!(dir.path().join("open-session.json").exists());
    }
}
