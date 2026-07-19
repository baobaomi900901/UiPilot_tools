#[cfg(any(test, not(feature = "test-instrumentation")))]
use std::sync::Arc;

#[cfg(any(test, not(feature = "test-instrumentation")))]
use tauri::Manager;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod atomic_file;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod commands;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod apps;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod model;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod result_registry;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod session_marker;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod settings;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod validation_data;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod validation_export;

#[cfg(test)]
mod lifecycle;

#[cfg(all(not(test), feature = "test-instrumentation"))]
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
    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let app_cache = Arc::new(apps::AppCache::new());

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let builder = builder
        .manage(Arc::clone(&app_cache))
        .manage(result_registry::ResultRegistry::default())
        .invoke_handler(tauri::generate_handler![
            commands::search_apps,
            commands::execute_result,
            commands::load_settings,
            commands::save_settings,
            commands::rescan_apps,
            commands::export_validation_data,
            commands::clear_validation_data,
            commands::hide_launcher,
        ]);

    #[cfg(all(not(test), feature = "test-instrumentation"))]
    let builder = builder.invoke_handler(tauri::generate_handler![security_probe::load_settings]);

    builder
        .setup(move |_app| {
            #[cfg(all(not(test), feature = "test-instrumentation"))]
            security_probe::setup(_app)?;

            #[cfg(any(test, not(feature = "test-instrumentation")))]
            {
                let app_data_dir = _app.path().app_data_dir()?;
                let settings = load_settings_store(&app_data_dir)?;
                assert!(_app.manage(settings), "settings store already managed");

                let validation = load_and_open_validation_store(&app_data_dir)?;
                assert!(_app.manage(validation), "validation store already managed");

                let _ = apps::start_initial_refresh(Arc::clone(&app_cache))?;
            }
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

    fn has_forbidden_production_lint_suppression(source: &str) -> bool {
        let compact = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        let approved_task6 =
            "#[cfg_attr(all(not(test),not(feature=\"test-instrumentation\")),allow(dead_code))]";
        let test_only = "#[cfg_attr(test,allow(dead_code))]";
        let enum_variant_names = "#[allow(clippy::enum_variant_names)]";
        let unapproved = compact
            .replace(approved_task6, "")
            .replace(test_only, "")
            .replace(enum_variant_names, "");
        let has_directive = |keyword: &str| {
            unapproved.match_indices(keyword).any(|(index, _)| {
                let previous = unapproved[..index].chars().next_back();
                let has_boundary = !matches!(
                    previous,
                    Some(character)
                        if character.is_ascii_alphanumeric()
                            || character == '_'
                            || character == '.'
                );
                let next = unapproved[index + keyword.len()..].chars().next();
                let has_next_boundary = !matches!(
                    next,
                    Some(character) if character.is_ascii_alphanumeric() || character == '_'
                );
                has_boundary && has_next_boundary
            })
        };

        has_directive("allow") || has_directive("expect")
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

    #[test]
    fn production_commands_are_exact_and_feature_handler_stays_probe_only() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production_marker = [
            "#[cfg(any(test, not(feature = ",
            "\"test-instrumentation\"",
            ")))]\n    let builder = builder",
        ]
        .concat();
        let production_start = source
            .find(&production_marker)
            .expect("production handler cfg is missing");
        let production = &source[production_start..];
        let feature_marker = [
            "\n\n    #[cfg(all(not(test), feature = ",
            "\"test-instrumentation\"",
            "))]",
        ]
        .concat();
        let production_end = production
            .find(&feature_marker)
            .expect("production handler block is not narrow");
        let production = &production[..production_end];

        assert_eq!(production.matches("commands::").count(), 8);
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
            assert!(production.contains(&format!("commands::{command}")));
        }
        assert_eq!(
            production
                .matches("manage(result_registry::ResultRegistry::default())")
                .count(),
            1
        );

        let probe_handler = [
            "#[cfg(all(not(test), feature = ",
            "\"test-instrumentation\"",
            "))]\n    let builder = builder.invoke_handler(tauri::generate_handler![",
            "security_probe::load_settings",
            "]);",
        ]
        .concat();
        assert!(source.contains(&probe_handler));
        assert!(source.contains(
            "#[cfg(all(not(test), feature = \"test-instrumentation\"))]\nmod security_probe;"
        ));
    }

    #[test]
    fn lint_oracle_rejects_unapproved_production_suppressions() {
        for fixture in [
            ["#![", "allow(", "dead_code", ")]"].concat(),
            ["#![", "allow /*gap*/ (", "dead_code", ")]"].concat(),
            ["#![", "allow(", "unused", ")]"].concat(),
            ["#![", "allow(", "warnings", ")]"].concat(),
            ["#[", "allow(", "clippy::all", ")] enum Broad {}"].concat(),
            ["#[", "allow(", "nonstandard_style", ")] struct Broad;"].concat(),
            ["#[", "expect(", "dead_code", ")] fn expected() {}"].concat(),
            "macro_rules! linted { ($level:ident, $lint:ident, $item:item) => { #[$level($lint)] $item }; } linted!(allow, dead_code, fn unused() {});".into(),
            ["#![cfg_attr(not(test), ", "allow(", "unused_imports", "))]"].concat(),
            ["#[", "allow(", "dead_code", ")] mod nested;"].concat(),
            ["#[", "allow(", "dead_code", ")] fn unapproved() {}"].concat(),
            [
                "#[",
                "allow(",
                "dead_code",
                ")] #[doc = \"x\"] mod nested {}",
            ]
            .concat(),
            [
                "#[cfg_attr(not(test), ",
                "allow(",
                "unused_imports",
                "))] pub(crate) mod nested;",
            ]
            .concat(),
        ] {
            assert!(has_forbidden_production_lint_suppression(&fixture));
        }

        let approved_item = [
            "#[cfg_attr(all(not(test), not(feature = \"test-instrumentation\")), ",
            "allow(",
            "dead_code",
            "))] fn reserved_for_task6() {}",
        ]
        .concat();
        assert!(!has_forbidden_production_lint_suppression(&approved_item));
    }

    #[test]
    fn production_modules_use_only_exact_task6_item_lint_exceptions() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let product_cfg = "#[cfg(any(test, not(feature = \"test-instrumentation\")))]";
        for module in [
            "atomic_file",
            "apps",
            "commands",
            "model",
            "result_registry",
            "session_marker",
            "settings",
            "validation_data",
            "validation_export",
        ] {
            assert!(
                source.contains(&format!("{product_cfg}\nmod {module};")),
                "product module has the wrong cfg: {module}"
            );
        }

        let production_root = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let allow_prefix = ["allow", "("].concat();
        assert!(!production_root.contains(&allow_prefix));

        let top_level_item_allow = [
            "#[cfg_attr(\n    all(not(test), not(feature = \"test-instrumentation\")),\n    ",
            "allow",
            "(",
            "dead_code",
            ")\n)]",
        ]
        .concat();
        let nested_item_allow = [
            "#[cfg_attr(\n        all(not(test), not(feature = \"test-instrumentation\")),\n        ",
            "allow",
            "(",
            "dead_code",
            ")\n    )]",
        ]
        .concat();
        let result_registry = include_str!("result_registry.rs").replace("\r\n", "\n");
        let session_marker = include_str!("session_marker.rs").replace("\r\n", "\n");
        let validation_data = include_str!("validation_data.rs").replace("\r\n", "\n");
        let commands = include_str!("commands.rs").replace("\r\n", "\n");
        let action = include_str!("apps/action.rs").replace("\r\n", "\n");
        let cache = include_str!("apps/cache.rs").replace("\r\n", "\n");
        let product_sources = [
            ("lib.rs", production_root),
            ("atomic_file.rs", include_str!("atomic_file.rs")),
            ("commands.rs", commands.as_str()),
            ("apps/mod.rs", include_str!("apps/mod.rs")),
            ("apps/action.rs", action.as_str()),
            ("apps/cache.rs", cache.as_str()),
            ("apps/discovery.rs", include_str!("apps/discovery.rs")),
            ("apps/rank.rs", include_str!("apps/rank.rs")),
            ("apps/shortcut.rs", include_str!("apps/shortcut.rs")),
            (
                "apps/windows_backend.rs",
                include_str!("apps/windows_backend.rs"),
            ),
            ("model.rs", include_str!("model.rs")),
            ("result_registry.rs", result_registry.as_str()),
            ("session_marker.rs", session_marker.as_str()),
            ("settings.rs", include_str!("settings.rs")),
            ("validation_data.rs", validation_data.as_str()),
            ("validation_export.rs", include_str!("validation_export.rs")),
        ];

        for (name, product_source) in product_sources {
            assert!(
                !has_forbidden_production_lint_suppression(product_source),
                "unapproved production lint suppression is forbidden: {name}"
            );
        }
        let task6_exception_count = product_sources
            .iter()
            .map(|(_, product_source)| {
                product_source.matches(&top_level_item_allow).count()
                    + product_source.matches(&nested_item_allow).count()
            })
            .sum::<usize>();
        assert_eq!(task6_exception_count, 6);

        let enum_variant_allow = "#[allow(clippy::enum_variant_names)]";
        assert_eq!(
            product_sources
                .iter()
                .map(|(_, product_source)| product_source.matches(enum_variant_allow).count())
                .sum::<usize>(),
            2
        );
        assert!(commands.contains(&format!(
            "{enum_variant_allow}\npub(crate) enum ExecuteOutcome"
        )));
        assert!(action.contains(&format!(
            "{enum_variant_allow}\npub(crate) enum ApplicationActionOutcome"
        )));

        let test_only_allow = "#[cfg_attr(test, allow(dead_code))]";
        assert_eq!(
            product_sources
                .iter()
                .map(|(_, product_source)| product_source.matches(test_only_allow).count())
                .sum::<usize>(),
            1
        );
        assert!(cache.contains(&format!("{test_only_allow}\n    pub(crate) fn refresh")));

        assert!(
            result_registry.contains(&format!("{nested_item_allow}\n    pub(crate) fn on_show"))
        );
        assert!(session_marker.contains(&format!(
            "{top_level_item_allow}\npub(crate) fn read_marker_for_clean"
        )));
        for item in [
            "    LauncherInvoked,",
            "    SessionOwnershipLost,",
            "    pub(crate) fn mark_clean_exit",
            "    fn mark_clean_exit_with",
        ] {
            assert!(
                validation_data.contains(&format!("{nested_item_allow}\n{item}")),
                "missing exact Task 6 item lint exception: {item}"
            );
        }
        assert_eq!(result_registry.matches(&nested_item_allow).count(), 1);
        assert_eq!(session_marker.matches(&top_level_item_allow).count(), 1);
        assert_eq!(validation_data.matches(&nested_item_allow).count(), 4);
    }
}
