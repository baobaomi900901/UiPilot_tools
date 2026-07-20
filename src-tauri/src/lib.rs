#[cfg(any(test, not(feature = "test-instrumentation")))]
use std::sync::Arc;

#[cfg(any(test, not(feature = "test-instrumentation")))]
use tauri::Manager;

#[cfg(any(test, not(feature = "test-instrumentation")))]
use lifecycle::ShowTarget;

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

#[cfg(any(test, not(feature = "test-instrumentation")))]
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

#[cfg(any(test, not(feature = "test-instrumentation")))]
fn lifecycle_setup_error() -> std::io::Error {
    std::io::Error::other("lifecycle setup failed")
}

#[cfg(any(test, not(feature = "test-instrumentation")))]
fn setup_production_lifecycle(
    app: &mut tauri::App,
    app_cache: &Arc<apps::AppCache>,
    coordinator: &Arc<lifecycle::LifecycleCoordinator>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    let settings = load_settings_store(&app_data_dir)?;
    let persisted_settings = settings.snapshot();
    if !app.manage(settings) {
        return Err(lifecycle_setup_error().into());
    }

    let validation = load_and_open_validation_store(&app_data_dir)?;
    if !app.manage(validation) {
        return Err(lifecycle_setup_error().into());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(lifecycle_setup_error)?;
    let event_app = app.handle().clone();
    let event_window = window.clone();
    let event_coordinator = Arc::clone(coordinator);
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(focused) => {
            let registry = event_app.state::<result_registry::ResultRegistry>();
            let _ = event_coordinator.handle_focus_event_with(
                *focused,
                || event_window.is_focused().map_err(|_| ()),
                || commands::clear_and_hide(&registry, &event_window).map_err(|_| ()),
            );
        }
        tauri::WindowEvent::CloseRequested { api, .. }
            if event_coordinator.should_prevent_close() =>
        {
            api.prevent_close();
            let registry = event_app.state::<result_registry::ResultRegistry>();
            let _ = commands::clear_and_hide(&registry, &event_window);
        }
        _ => {}
    });

    let open_launcher = tauri::menu::MenuItem::with_id(
        app,
        lifecycle::TRAY_OPEN_LAUNCHER,
        "打开主界面",
        true,
        None::<&str>,
    )
    .map_err(|_| lifecycle_setup_error())?;
    let open_settings = tauri::menu::MenuItem::with_id(
        app,
        lifecycle::TRAY_OPEN_SETTINGS,
        "打开设置",
        true,
        None::<&str>,
    )
    .map_err(|_| lifecycle_setup_error())?;
    let quit =
        tauri::menu::MenuItem::with_id(app, lifecycle::TRAY_QUIT, "退出", true, None::<&str>)
            .map_err(|_| lifecycle_setup_error())?;
    let menu = tauri::menu::Menu::with_items(app, &[&open_launcher, &open_settings, &quit])
        .map_err(|_| lifecycle_setup_error())?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(lifecycle_setup_error)?;
    let tray_coordinator = Arc::clone(coordinator);
    tauri::tray::TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .on_menu_event(
            move |app, event| match lifecycle::tray_action(event.id().as_ref()) {
                Some(lifecycle::TrayAction::Show(target)) => {
                    let _ = tray_coordinator.request_show(app, target);
                }
                Some(lifecycle::TrayAction::Quit) => tray_coordinator.request_tray_quit(app),
                _ => {}
            },
        )
        .build(app)
        .map_err(|_| lifecycle_setup_error())?;

    lifecycle::install_session_end_hook(app.handle(), &window)
        .map_err(|_| lifecycle_setup_error())?;
    let _ = coordinator.reconcile_runtime_settings(app.handle(), &persisted_settings);
    let _ = apps::start_initial_refresh(Arc::clone(app_cache))?;
    coordinator
        .mark_setup_ready(app.handle())
        .map_err(|_| lifecycle_setup_error())?;
    Ok(())
}

pub fn run() {
    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let app_cache = Arc::new(apps::AppCache::new());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let coordinator = Arc::new(lifecycle::LifecycleCoordinator::default());

    let builder = tauri::Builder::default();

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let single_instance_coordinator = Arc::clone(&coordinator);

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let shortcut_coordinator = Arc::clone(&coordinator);

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(
            move |app, _args, _cwd| {
                let _ = single_instance_coordinator.request_show(app, ShowTarget::Launcher);
            },
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let _ = shortcut_coordinator.request_show(app, ShowTarget::Launcher);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(Arc::clone(&app_cache))
        .manage(Arc::clone(&coordinator))
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

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let run_coordinator = Arc::clone(&coordinator);

    let app = builder
        .setup(move |_app| {
            #[cfg(all(not(test), feature = "test-instrumentation"))]
            security_probe::setup(_app)?;

            #[cfg(any(test, not(feature = "test-instrumentation")))]
            setup_production_lifecycle(_app, &app_cache, &coordinator)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running Tauri application");

    app.run(move |_app, _event| {
        #[cfg(any(test, not(feature = "test-instrumentation")))]
        match _event {
            tauri::RunEvent::ExitRequested { api, .. } if run_coordinator.should_prevent_exit() => {
                api.prevent_exit();
            }
            tauri::RunEvent::Exit => run_coordinator.observe_run_exit(),
            _ => {}
        }
    });
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
        let test_only = "#[cfg_attr(test,allow(dead_code))]";
        let enum_variant_names = "#[allow(clippy::enum_variant_names)]";
        let unapproved = compact
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
    fn production_lifecycle_wires_one_coordinator_and_exact_event_sources() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        assert_eq!(
            production
                .matches(".manage(Arc::clone(&coordinator))")
                .count(),
            1
        );
        for fragment in [
            "let coordinator = Arc::new(lifecycle::LifecycleCoordinator::default());",
            "tauri_plugin_single_instance::init(",
            "move |app, _args, _cwd|",
            "tauri_plugin_global_shortcut::Builder::new()",
            "tauri_plugin_global_shortcut::ShortcutState::Pressed",
            "setup_production_lifecycle(_app, &app_cache, &coordinator)?;",
            "lifecycle::install_session_end_hook",
            "tauri::tray::TrayIconBuilder::new()",
            "tauri::WindowEvent::Focused(focused)",
            "handle_focus_event_with(",
            "*focused,",
            "tauri::WindowEvent::CloseRequested",
            "tauri::RunEvent::ExitRequested",
            "tauri::RunEvent::Exit",
        ] {
            assert!(
                production.contains(fragment),
                "missing production wiring: {fragment}"
            );
        }
        assert_eq!(production.matches(".mark_setup_ready(").count(), 1);
        assert_eq!(
            production
                .matches("request_show(app, ShowTarget::Launcher)")
                .count(),
            2
        );
        assert!(production.contains("lifecycle::TRAY_OPEN_LAUNCHER"));
        assert!(production.contains("打开主界面"));
        assert!(production.contains("Some(lifecycle::TrayAction::Show(target))"));
        assert!(production.contains("tray_coordinator.request_show(app, target)"));
        assert!(production.contains("lifecycle::TRAY_OPEN_SETTINGS"));
    }

    #[test]
    fn feature_only_lifecycle_keeps_every_production_plugin_behind_the_product_cfg() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let run = source
            .split("pub fn run() {")
            .nth(1)
            .and_then(|tail| tail.split("#[cfg(test)]\nmod tests").next())
            .expect("run source markers are missing");
        let production_marker = concat!(
            "#[cfg(any(test, not(feature = \"test-instrumentation\")))]\n",
            "    let coordinator = Arc::new(lifecycle::LifecycleCoordinator::default());",
        );
        let production_start = run
            .find(production_marker)
            .expect("production lifecycle cfg is missing");
        let common = &run[..production_start];
        for forbidden in [
            "tauri_plugin_single_instance",
            "tauri_plugin_global_shortcut",
            "tauri_plugin_autostart",
            "setup_production_lifecycle",
            "launcher://shown",
        ] {
            assert!(
                !common.contains(forbidden),
                "feature-only common builder contains {forbidden}"
            );
        }
        assert!(run.contains(concat!(
            "#[cfg(all(not(test), feature = \"test-instrumentation\"))]\n",
            "    let builder = builder.invoke_handler(tauri::generate_handler![",
            "security_probe::load_settings",
            "]);",
        )));
        assert!(run.contains(concat!(
            "#[cfg(all(not(test), feature = \"test-instrumentation\"))]\n",
            "            security_probe::setup(_app)?;",
        )));
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
        assert!(has_forbidden_production_lint_suppression(&approved_item));
    }

    #[test]
    fn production_modules_have_no_task6_lint_exceptions() {
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
            "lifecycle",
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
            ("lifecycle.rs", include_str!("lifecycle.rs")),
            ("model.rs", include_str!("model.rs")),
            ("result_registry.rs", include_str!("result_registry.rs")),
            ("session_marker.rs", include_str!("session_marker.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("validation_data.rs", include_str!("validation_data.rs")),
            ("validation_export.rs", include_str!("validation_export.rs")),
        ];

        for (name, product_source) in product_sources {
            assert!(
                !has_forbidden_production_lint_suppression(product_source),
                "unapproved production lint suppression is forbidden: {name}"
            );
        }

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
    }
}
