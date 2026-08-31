#[cfg(any(test, not(feature = "test-instrumentation")))]
use std::sync::Arc;

#[cfg(any(test, not(feature = "test-instrumentation")))]
use tauri::{Emitter, Manager};

#[cfg(any(test, not(feature = "test-instrumentation")))]
use find_window::{FocusEffect, WindowLabel};
#[cfg(any(test, not(feature = "test-instrumentation")))]
use lifecycle::ShowTarget;

#[cfg(any(test, not(feature = "test-instrumentation")))]
use plugins::{PluginManager, Version};

#[cfg(any(test, not(feature = "test-instrumentation")))]
const HOTKEY_RECORDING_CURRENT_EVENT: &str = "hotkey-recording://current";

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod atomic_file;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod message_center;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod native_attention;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod commands;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod calculator;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod web_search;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod apps;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod model;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod quicklinks;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod result_registry;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod clipboard_history;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod public_plugins;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod find_window;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod settings;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod hotkey;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod double_tap;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod hotkey_hook;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod lifecycle;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod file_index;
#[cfg(any(test, not(feature = "test-instrumentation")))]
mod file_search;

#[cfg(any(test, not(feature = "test-instrumentation")))]
mod plugin_panel;
#[cfg(any(test, not(feature = "test-instrumentation")))]
mod plugin_window;
#[cfg(any(test, not(feature = "test-instrumentation")))]
mod plugins;
#[cfg(any(test, not(feature = "test-instrumentation")))]
mod window_transfer;

#[cfg(any(test, not(feature = "test-instrumentation")))]
#[doc(hidden)]
pub fn public_plugin_manifest_schema() -> serde_json::Value {
    serde_json::to_value(public_plugins::public_manifest_v1_schema())
        .expect("public plugin schema must serialize")
}

#[cfg(all(not(test), feature = "test-instrumentation"))]
mod security_probe;

#[cfg(any(test, not(feature = "test-instrumentation")))]
fn load_settings_store(
    app_data_dir: &std::path::Path,
) -> Result<settings::SettingsStore, settings::SettingsError> {
    settings::SettingsStore::load(app_data_dir)
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
    plugin_manager: &Arc<PluginManager>,
    public_plugin_service: &Arc<public_plugins::PublicPluginService>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    plugin_manager.load(&app_data_dir, Version::new(0, 3, 0))?;
    plugin_manager.create_runtimes(app, &app_data_dir)?;
    let message_center = Arc::new(message_center::MessageCenterService::load(&app_data_dir));
    if !app.manage(Arc::clone(&message_center)) {
        return Err(lifecycle_setup_error().into());
    }
    let settings = load_settings_store(&app_data_dir)?;
    let persisted_settings = settings.snapshot();
    if !app.manage(settings) {
        return Err(lifecycle_setup_error().into());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(lifecycle_setup_error)?;
    let panel_controller = Arc::clone(
        app.state::<Arc<plugin_panel::PluginPanelController>>()
            .inner(),
    );
    plugin_panel::register_main_focus_events(
        app.handle(),
        &window,
        Arc::clone(&panel_controller),
        Arc::clone(coordinator),
    )
    .map_err(|_| lifecycle_setup_error())?;
    let event_app = app.handle().clone();
    let event_window = window.clone();
    let event_coordinator = Arc::clone(coordinator);
    let event_panel_controller = Arc::clone(&panel_controller);
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(focused) => {
            let transfers =
                event_app.state::<Arc<window_transfer::MainWindowTransferCoordinator>>();
            let expected_blur = !*focused && transfers.consume_expected_main_blur();
            let main_owns_native_foreground =
                !*focused && lifecycle::main_window_owns_native_foreground(&event_app);
            if lifecycle::should_ignore_main_focus_loss(
                *focused,
                expected_blur,
                main_owns_native_foreground,
            ) {
                return;
            }
            event_app
                .state::<Arc<message_center::MessageCenterService>>()
                .observe_main_focus(*focused);
            if expected_blur {
                return;
            }
            if !*focused
                && event_panel_controller.consume_internal_main_blur(std::time::Instant::now())
            {
                return;
            }
            let registries = event_app.state::<result_registry::ResultRegistries>();
            let controller = event_app.state::<Arc<find_window::FindWindowController>>();
            let effect = controller.observe_focus(WindowLabel::Main, *focused);
            if effect == FocusEffect::ClearAndHideMain {
                let _ = event_coordinator.handle_focus_event_with(*focused, || {
                    commands::clear_and_hide(registries.main(), &event_window).map_err(|_| ())
                });
            } else {
                let _ = lifecycle::handle_find_focus_effect(
                    &event_app,
                    controller.inner().as_ref(),
                    &registries,
                    effect,
                );
            }
        }
        tauri::WindowEvent::CloseRequested { api, .. }
            if event_coordinator.should_prevent_close() =>
        {
            api.prevent_close();
            let registries = event_app.state::<result_registry::ResultRegistries>();
            let _ = commands::clear_and_hide(registries.main(), &event_window);
        }
        _ => {}
    });

    let find = app
        .get_webview_window("find")
        .ok_or_else(lifecycle_setup_error)?;
    let find_app = app.handle().clone();
    let find_window = find.clone();
    let find_coordinator = Arc::clone(coordinator);
    find.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(focused) => {
            let registries = find_app.state::<result_registry::ResultRegistries>();
            let controller = find_app.state::<Arc<find_window::FindWindowController>>();
            let effect = controller.observe_focus(WindowLabel::Find, *focused);
            let _ = lifecycle::handle_find_focus_effect(
                &find_app,
                controller.inner().as_ref(),
                &registries,
                effect,
            );
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            if find_coordinator.should_prevent_close() {
                api.prevent_close();
                let registries = find_app.state::<result_registry::ResultRegistries>();
                let controller = find_app.state::<Arc<find_window::FindWindowController>>();
                if let Some(invocation_id) = controller.current_invocation() {
                    let hidden = find_window.hide().is_ok();
                    controller.finish_explicit_hide(&invocation_id, hidden, &registries);
                } else {
                    let _ = find_window.hide();
                }
            }
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
        .map(tauri::image::Image::to_owned)
        .ok_or_else(lifecycle_setup_error)?;
    let tray_coordinator = Arc::clone(coordinator);
    let tray = tauri::tray::TrayIconBuilder::new()
        .icon(icon.clone())
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

    let notification_app = app.handle().clone();
    let notification_coordinator = Arc::clone(coordinator);
    let route_messages: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = notification_coordinator.request_show(&notification_app, ShowTarget::Messages);
    });
    let toast = native_attention::windows_toast();
    let tray_attention = native_attention::tauri_tray(tray, icon);
    let message_sound = app
        .path()
        .resolve(
            "resources/sounds/message-notification.wav",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|_| lifecycle_setup_error())?;
    let attention_audio = native_attention::windows_audio(message_sound);
    message_center
        .install_native_effects(toast, tray_attention, attention_audio)
        .map_err(|_| lifecycle_setup_error())?;
    public_plugin_service.initialize(
        app.handle(),
        &app_data_dir,
        ["find".into(), "math".into(), "web-search".into()],
        Arc::clone(&message_center),
        native_attention::attention_route(route_messages),
    )?;

    lifecycle::install_session_end_hook(app.handle(), &window)
        .map_err(|_| lifecycle_setup_error())?;
    lifecycle::install_find_position_hook(app.handle(), &find)
        .map_err(|_| lifecycle_setup_error())?;
    let hwnd = window.hwnd().map_err(|_| lifecycle_setup_error())?;
    app.state::<Arc<file_index::FileIndex>>()
        .install_main_window_hwnd(hwnd.0 as isize)
        .map_err(|_| lifecycle_setup_error())?;
    let _ = coordinator.reconcile_runtime_settings(app.handle(), &persisted_settings);
    let _ = apps::start_initial_refresh(Arc::clone(app_cache))?;
    coordinator
        .mark_setup_ready(app.handle())
        .map_err(|_| lifecycle_setup_error())?;
    Ok(())
}

pub fn prepare_windows_identity() {
    #[cfg(any(test, not(feature = "test-instrumentation")))]
    native_attention::prepare_process_identity();
}

pub fn run() {
    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let app_cache = Arc::new(apps::AppCache::new());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let coordinator = Arc::new(lifecycle::LifecycleCoordinator::default());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let result_registries = result_registry::ResultRegistries::default();

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let find_controller = Arc::new(find_window::FindWindowController::default());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let plugin_window_controller = Arc::new(plugin_window::PluginWindowController::default());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let plugin_panel_controller = Arc::new(plugin_panel::PluginPanelController::default());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let main_window_transfers = Arc::new(window_transfer::MainWindowTransferCoordinator::default());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let file_index = Arc::new(file_index::FileIndex::new(
        Arc::clone(&coordinator),
        result_registries.find().clone(),
    ));

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let everything_search = Arc::new(file_search::everything::EverythingSearchState::new());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let plugin_manager = Arc::new(PluginManager::new());

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let public_plugin_service = Arc::new(public_plugins::PublicPluginService::default());

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
                        let main_foreground = lifecycle::main_window_owns_native_foreground(app);
                        let dispatch = lifecycle::should_dispatch_hotkey_show(main_foreground);
                        if dispatch {
                            let _ = shortcut_coordinator.request_show(app, ShowTarget::Launcher);
                        } else {
                            let _ = app.emit_to("main", HOTKEY_RECORDING_CURRENT_EVENT, ());
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::clone(&app_cache))
        .manage(Arc::clone(&coordinator))
        .manage(Arc::clone(&file_index))
        .manage(everything_search)
        .manage(Arc::clone(&plugin_manager))
        .register_uri_scheme_protocol("uipilot-public-plugin", {
            let public_plugin_service = Arc::clone(&public_plugin_service);
            move |ctx, request| {
                public_plugin_service.asset_response(
                    ctx.webview_label(),
                    request.uri().path(),
                    request.uri().query(),
                )
            }
        })
        .manage(Arc::clone(&public_plugin_service))
        .register_uri_scheme_protocol("uipilot-plugin", {
            let plugin_manager = Arc::clone(&plugin_manager);
            move |ctx, request| {
                plugin_manager.asset_response(ctx.webview_label(), request.uri().path())
            }
        })
        .manage(result_registries)
        .manage(Arc::clone(&find_controller))
        .manage(Arc::clone(&plugin_window_controller))
        .manage(Arc::clone(&plugin_panel_controller))
        .manage(Arc::clone(&main_window_transfers))
        .invoke_handler(tauri::generate_handler![
            commands::open_find_window,
            commands::prepare_find_initialization,
            commands::commit_find_ready,
            commands::get_find_ready_status,
            commands::set_find_pinned,
            commands::set_find_preview_preference,
            commands::load_find_thumbnail,
            commands::hide_find_window,
            commands::select_public_plugin_directory,
            commands::list_public_plugins,
            commands::prepare_public_plugin_install,
            commands::commit_public_plugin_install,
            commands::cancel_public_plugin_install,
            commands::set_plugin_enabled,
            commands::set_public_plugin_network_access,
            commands::set_plugin_favorite,
            commands::set_plugin_effective_name,
            commands::save_plugin_settings,
            commands::uninstall_plugin,
            commands::plugin_network_request,
            commands::plugin_api_call,
            commands::complete_plugin_command,
            commands::plugin_window_content_ready,
            commands::plugin_window_content_ack,
            commands::plugin_window_content_close,
            commands::plugin_window_storage_get,
            commands::plugin_window_storage_set,
            commands::plugin_window_storage_remove,
            commands::plugin_window_timer_get_state,
            commands::plugin_window_timer_start,
            commands::plugin_window_timer_stop,
            commands::plugin_window_timer_reset,
            commands::plugin_panel_content_ready,
            commands::plugin_panel_content_ack,
            commands::plugin_panel_host_key_enqueue,
            commands::plugin_panel_host_key_ack,
            commands::plugin_panel_request_hide_admit,
            commands::plugin_panel_request_hide_admit_observed,
            commands::plugin_panel_request_hide_commit,
            commands::plugin_panel_focus_host_input,
            commands::plugin_panel_focus_host_input_ack,
            commands::plugin_panel_storage_get,
            commands::plugin_panel_storage_set,
            commands::plugin_panel_storage_remove,
            commands::plugin_panel_clipboard_history_list,
            commands::plugin_panel_clipboard_history_paste,
            commands::plugin_panel_clipboard_history_remove,
            commands::plugin_panel_clipboard_history_clear,
            commands::open_plugin_panel,
            commands::submit_plugin_panel,
            commands::set_plugin_panel_bounds,
            commands::close_plugin_panel,
            commands::commit_plugin_window_transfer,
            commands::get_public_plugin_window_identity,
            commands::set_plugin_window_pinned,
            commands::close_plugin_window,
            commands::get_message_summary,
            commands::open_message_center,
            commands::read_message_center,
            commands::clear_messages,
            commands::search_apps,
            commands::publish_plugin_results,
            commands::search_files,
            commands::execute_result,
            commands::list_plugins,
            commands::install_plugin,
            commands::reload_plugin,
            commands::delete_plugin,
            commands::load_settings,
            commands::save_settings,
            commands::save_hotkey,
            commands::set_file_preview_preference,
            commands::set_theme_preference,
            commands::set_web_search_engine,
            commands::set_builtin_feature_favorite,
            commands::hide_launcher,
        ]);

    #[cfg(all(not(test), feature = "test-instrumentation"))]
    let builder = builder.invoke_handler(tauri::generate_handler![security_probe::load_settings]);

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let run_coordinator = Arc::clone(&coordinator);

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let run_file_index = Arc::clone(&file_index);

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let run_find_controller = Arc::clone(&find_controller);

    #[cfg(any(test, not(feature = "test-instrumentation")))]
    let run_public_plugin_service = Arc::clone(&public_plugin_service);

    let app = builder
        .setup(move |_app| {
            #[cfg(all(not(test), feature = "test-instrumentation"))]
            security_probe::setup(_app)?;

            #[cfg(any(test, not(feature = "test-instrumentation")))]
            setup_production_lifecycle(
                _app,
                &app_cache,
                &coordinator,
                &plugin_manager,
                &public_plugin_service,
            )?;
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
            tauri::RunEvent::Exit => {
                run_public_plugin_service.shutdown();
                _app.state::<Arc<message_center::MessageCenterService>>()
                    .shutdown();
                run_find_controller.shutdown();
                run_file_index.enter_terminal();
                run_coordinator.uninstall_hook_for_exit();
                run_coordinator.observe_run_exit();
            }
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
        apps::{AppCache, Application, ApplicationLaunchTarget},
        load_settings_store,
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
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(r"C:\Menu\App.lnk"),
                executable: None,
            },
            icon: None,
            use_count: 0,
        }]);
        store.increment_use_count(APP_A, &cache).unwrap();
        drop(store);

        let reloaded = load_settings_store(dir.path()).unwrap();

        assert_eq!(reloaded.snapshot().use_counts[APP_A], 1);
    }

    #[test]
    fn production_has_no_retired_validation_subsystem() {
        let lib = include_str!("lib.rs").replace("\r\n", "\n");
        let commands_source = include_str!("commands.rs").replace("\r\n", "\n");
        let lifecycle_source = include_str!("lifecycle.rs").replace("\r\n", "\n");
        let production = lib
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let commands = commands_source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("commands test module marker is missing");
        let lifecycle = lifecycle_source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("lifecycle test module marker is missing");
        let build = include_str!("../build.rs");
        let capability = include_str!("../capabilities/main.json");

        for forbidden in [
            "validation_data",
            "validation_export",
            "session_marker",
            "load_and_open_validation_store",
            "rescan_apps",
            "export_validation_data",
            "clear_validation_data",
            "ValidationFailed",
            "validationFailed",
        ] {
            assert!(
                ![production, commands, lifecycle, build, capability]
                    .iter()
                    .any(|source| source.contains(forbidden)),
                "retired validation surface remains: {forbidden}"
            );
        }
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

        assert_eq!(production.matches("commands::").count(), 76);
        for command in [
            "open_find_window",
            "prepare_find_initialization",
            "commit_find_ready",
            "get_find_ready_status",
            "set_find_pinned",
            "set_find_preview_preference",
            "load_find_thumbnail",
            "hide_find_window",
            "select_public_plugin_directory",
            "list_public_plugins",
            "prepare_public_plugin_install",
            "commit_public_plugin_install",
            "cancel_public_plugin_install",
            "set_plugin_enabled",
            "set_public_plugin_network_access",
            "set_plugin_favorite",
            "set_plugin_effective_name",
            "save_plugin_settings",
            "uninstall_plugin",
            "plugin_network_request",
            "plugin_api_call",
            "complete_plugin_command",
            "plugin_window_content_ready",
            "plugin_window_content_ack",
            "plugin_window_content_close",
            "plugin_window_storage_get",
            "plugin_window_storage_set",
            "plugin_window_storage_remove",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
            "plugin_panel_content_ready",
            "plugin_panel_content_ack",
            "plugin_panel_host_key_enqueue",
            "plugin_panel_host_key_ack",
            "plugin_panel_request_hide_admit",
            "plugin_panel_request_hide_admit_observed",
            "plugin_panel_request_hide_commit",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
            "plugin_panel_focus_host_input",
            "plugin_panel_focus_host_input_ack",
            "open_plugin_panel",
            "submit_plugin_panel",
            "set_plugin_panel_bounds",
            "close_plugin_panel",
            "commit_plugin_window_transfer",
            "get_public_plugin_window_identity",
            "set_plugin_window_pinned",
            "close_plugin_window",
            "get_message_summary",
            "open_message_center",
            "read_message_center",
            "clear_messages",
            "search_apps",
            "publish_plugin_results",
            "search_files",
            "execute_result",
            "list_plugins",
            "install_plugin",
            "reload_plugin",
            "delete_plugin",
            "load_settings",
            "save_settings",
            "save_hotkey",
            "set_file_preview_preference",
            "set_theme_preference",
            "set_web_search_engine",
            "set_builtin_feature_favorite",
            "hide_launcher",
        ] {
            assert!(production.contains(&format!("commands::{command}")));
        }
        let production_root = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        assert_eq!(
            production_root
                .matches("result_registry::ResultRegistries::default()")
                .count(),
            1
        );
        assert!(production_root
            .contains("let result_registries = result_registry::ResultRegistries::default();"));
        assert!(production_root.contains(
            "let file_index = Arc::new(file_index::FileIndex::new(\n        Arc::clone(&coordinator),\n        result_registries.find().clone(),\n    ));"
        ));
        assert_eq!(
            production_root
                .matches(".manage(result_registries)")
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
    fn save_hotkey_command_is_declared_and_allowed() {
        assert!(include_str!("../build.rs").contains("\"save_hotkey\","));
        assert!(include_str!("../capabilities/main.json").contains("\"allow-save-hotkey\""));
    }

    #[test]
    fn theme_preference_command_is_declared_and_main_only() {
        assert!(include_str!("../build.rs").contains("\"set_theme_preference\","));
        assert!(
            include_str!("../capabilities/main.json").contains("\"allow-set-theme-preference\"")
        );
        assert!(
            !include_str!("../capabilities/plugin-runtime.json").contains("set-theme-preference")
        );
    }

    #[test]
    fn builtin_feature_favorite_command_is_declared_and_main_only() {
        let build = include_str!("../build.rs");
        let main = include_str!("../capabilities/main.json");
        let runtime = include_str!("../capabilities/plugin-runtime.json");
        let shell = include_str!("../capabilities/plugin-window-shell.json");
        let content = include_str!("../capabilities/plugin-window-content.json");
        let panel = include_str!("../capabilities/plugin-panel-content.json");
        let find = include_str!("../capabilities/find.json");

        assert!(build.contains("\"set_builtin_feature_favorite\","));
        assert!(main.contains("\"allow-set-builtin-feature-favorite\""));
        for capability in [runtime, shell, content, panel, find] {
            assert!(!capability.contains("set-builtin-feature-favorite"));
        }
    }

    #[test]
    fn public_plugin_commands_have_non_overlapping_exact_capabilities() {
        let build = include_str!("../build.rs");
        let main = include_str!("../capabilities/main.json");
        let runtime = include_str!("../capabilities/plugin-runtime.json");
        let shell = include_str!("../capabilities/plugin-window-shell.json");
        let content = include_str!("../capabilities/plugin-window-content.json");
        let panel = include_str!("../capabilities/plugin-panel-content.json");
        let find = include_str!("../capabilities/find.json");
        for command in [
            "list_public_plugins",
            "prepare_public_plugin_install",
            "commit_public_plugin_install",
            "cancel_public_plugin_install",
            "set_plugin_enabled",
            "set_public_plugin_network_access",
            "set_plugin_favorite",
            "set_plugin_effective_name",
            "save_plugin_settings",
            "uninstall_plugin",
            "open_plugin_panel",
            "submit_plugin_panel",
            "set_plugin_panel_bounds",
            "close_plugin_panel",
        ] {
            assert!(build.contains(&format!("\"{command}\",")));
            let permission = format!("\"allow-{}\"", command.replace('_', "-"));
            assert!(main.contains(&permission));
            assert!(!runtime.contains(&permission));
        }
        for command in [
            "plugin_api_call",
            "plugin_network_request",
            "complete_plugin_command",
        ] {
            assert!(build.contains(&format!("\"{command}\",")));
            let permission = format!("\"allow-{}\"", command.replace('_', "-"));
            assert!(runtime.contains(&permission));
            assert!(!main.contains(&permission));
            assert!(!find.contains(&permission));
            assert!(!shell.contains(&permission));
            assert!(!content.contains(&permission));
            assert!(!panel.contains(&permission));
        }
        assert!(main.contains("allow-commit-plugin-window-transfer"));
        assert!(!shell.contains("commit-plugin-window-transfer"));
        assert!(!content.contains("commit-plugin-window-transfer"));
        assert!(!panel.contains("commit-plugin-window-transfer"));
        assert!(shell.contains("\"webviews\": [\"plugin-shell-*\"]"));
        for command in [
            "get_public_plugin_window_identity",
            "set_plugin_window_pinned",
            "close_plugin_window",
        ] {
            let permission = format!("allow-{}", command.replace('_', "-"));
            assert!(build.contains(&format!("\"{command}\",")));
            assert!(shell.contains(&permission));
            assert!(!main.contains(&permission));
            assert!(!runtime.contains(&permission));
            assert!(!content.contains(&permission));
            assert!(!panel.contains(&permission));
        }
        assert!(content.contains("\"webviews\": [\"plugin-content-*\"]"));
        for command in [
            "plugin_window_content_ready",
            "plugin_window_content_ack",
            "plugin_window_content_close",
            "plugin_window_storage_get",
            "plugin_window_storage_set",
            "plugin_window_storage_remove",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
        ] {
            let permission = format!("allow-{}", command.replace('_', "-"));
            assert!(build.contains(&format!("\"{command}\",")));
            assert!(content.contains(&permission));
            assert!(!main.contains(&permission));
            assert!(!runtime.contains(&permission));
            assert!(!shell.contains(&permission));
            assert!(!panel.contains(&permission));
        }
        assert!(panel.contains("\"webviews\": [\"plugin-panel-content-*\"]"));
        for command in [
            "plugin_panel_content_ready",
            "plugin_panel_content_ack",
            "plugin_panel_host_key_ack",
            "plugin_panel_request_hide_admit",
            "plugin_panel_request_hide_admit_observed",
            "plugin_panel_request_hide_commit",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
            "plugin_panel_focus_host_input",
        ] {
            let permission = format!("\"allow-{}\"", command.replace('_', "-"));
            assert!(build.contains(&format!("\"{command}\",")));
            assert!(panel.contains(&permission));
            assert!(!main.contains(&permission));
            assert!(!runtime.contains(&permission));
            assert!(!shell.contains(&permission));
            assert!(!content.contains(&permission));
            assert!(!find.contains(&permission));
        }
        let host_key_enqueue = "allow-plugin-panel-host-key-enqueue";
        assert!(main.contains(host_key_enqueue));
        assert!(!panel.contains(host_key_enqueue));
        let host_key_ack = "allow-plugin-panel-host-key-ack";
        assert!(panel.contains(host_key_ack));
        assert!(!main.contains(host_key_ack));
        let focus_ack = ["allow", "-plugin-panel-focus-host-input-ack"].concat();
        assert!(main.contains(&focus_ack));
        assert!(!runtime.contains(&focus_ack));
        assert!(!shell.contains(&focus_ack));
        assert!(!content.contains(&focus_ack));
        assert!(!panel.contains(&focus_ack));
        assert!(!find.contains(&focus_ack));
        assert!(main.contains("\"webviews\": [\"main\"]"));
        assert!(!main.contains("\"windows\":"));
        for capability in [main, runtime, shell, content, panel, find] {
            assert!(!capability.contains("\"shell:"));
        }
        assert!(runtime.contains("\"windows\": [\"plugin-runtime-*\"]"));
        assert!(!runtime.contains("\"plugin-*\""));
        assert!(!runtime.contains("plugin-shell-"));
        assert!(!runtime.contains("plugin-content-"));
        assert!(!runtime.contains("plugin-panel-content-"));
        assert!(!content.contains("plugin-panel-"));
        assert!(!panel.contains("plugin-window-"));
        assert!(!panel.contains("timer"));
    }

    #[test]
    fn plugin_panel_bounds_command_is_registered_for_main_only() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let build = include_str!("../build.rs");
        let main = include_str!("../capabilities/main.json");
        let runtime = include_str!("../capabilities/plugin-runtime.json");
        let panel = include_str!("../capabilities/plugin-panel-content.json");

        assert_eq!(
            production
                .matches("commands::set_plugin_panel_bounds,")
                .count(),
            1
        );
        assert!(build.contains("\"set_plugin_panel_bounds\","));
        assert!(main.contains("\"allow-set-plugin-panel-bounds\""));
        assert!(!runtime.contains("allow-set-plugin-panel-bounds"));
        assert!(!panel.contains("allow-set-plugin-panel-bounds"));
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
        assert_eq!(
            production
                .matches(".manage(Arc::clone(&plugin_manager))")
                .count(),
            1
        );
        for fragment in [
            "let coordinator = Arc::new(lifecycle::LifecycleCoordinator::default());",
            "let plugin_manager = Arc::new(PluginManager::new());",
            "tauri_plugin_single_instance::init(",
            "move |app, _args, _cwd|",
            "tauri_plugin_global_shortcut::Builder::new()",
            "tauri_plugin_global_shortcut::ShortcutState::Pressed",
            "lifecycle::main_window_owns_native_foreground(app)",
            "HOTKEY_RECORDING_CURRENT_EVENT",
            "setup_production_lifecycle(",
            "&public_plugin_service,",
            "let public_plugin_service = Arc::new(public_plugins::PublicPluginService::default());",
            "let plugin_window_controller = Arc::new(plugin_window::PluginWindowController::default());",
            "let plugin_panel_controller = Arc::new(plugin_panel::PluginPanelController::default());",
            "window_transfer::MainWindowTransferCoordinator::default()",
            ".manage(Arc::clone(&plugin_window_controller))",
            ".manage(Arc::clone(&plugin_panel_controller))",
            ".manage(Arc::clone(&main_window_transfers))",
            "transfers.consume_expected_main_blur()",
            "public_plugin_service.initialize(",
            ".register_uri_scheme_protocol(\"uipilot-public-plugin\"",
            ".manage(Arc::clone(&public_plugin_service))",
            "plugin_manager.load(&app_data_dir, Version::new(0, 3, 0))?;",
            "plugin_manager.create_runtimes(app, &app_data_dir)?;",
            "lifecycle::install_session_end_hook",
            "lifecycle::install_find_position_hook",
            "tauri::tray::TrayIconBuilder::new()",
            "tauri::WindowEvent::Focused(focused)",
            "handle_focus_event_with(",
            "*focused,",
            "tauri::WindowEvent::CloseRequested",
            "tauri::RunEvent::ExitRequested",
            "tauri::RunEvent::Exit",
            "uninstall_hook_for_exit",
        ] {
            assert!(
                production.contains(fragment),
                "missing production wiring: {fragment}"
            );
        }
        assert_eq!(production.matches(".mark_setup_ready(").count(), 1);
        let hook = production
            .find("lifecycle::install_session_end_hook")
            .unwrap();
        let hwnd = production.find(".install_main_window_hwnd(").unwrap();
        let ready = production.find(".mark_setup_ready(").unwrap();
        assert!(hook < hwnd && hwnd < ready);
        assert_eq!(production.matches(".install_main_window_hwnd(").count(), 1);
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
    fn public_plugin_cleanup_recovery_precedes_activation() {
        let lifecycle = include_str!("lib.rs").replace("\r\n", "\n");
        let production = lifecycle
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let settings = production
            .find("let settings = load_settings_store(&app_data_dir)?;")
            .expect("settings must load before public plugins");
        let initialize = production
            .find("public_plugin_service.initialize(")
            .expect("public plugin initialization is missing");
        assert!(settings < initialize);

        let service = include_str!("public_plugins.rs").replace("\r\n", "\n");
        let initialize_body = service
            .split("pub(crate) fn initialize(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn manager(").next())
            .expect("public plugin initialize body is missing");
        let recovery = initialize_body
            .find("retry_pending_owner_cleanup(")
            .expect("owner cleanup recovery is missing");
        let manager_load = initialize_body
            .find("PublicPluginManager::load(")
            .expect("manager load is missing");
        assert!(recovery < manager_load);
    }

    #[test]
    fn process_identity_is_prepared_before_tauri_builder() {
        let main = include_str!("main.rs");
        let prepare = main
            .find("uipilot_lib::prepare_windows_identity()")
            .expect("process identity preparation is missing");
        let run = main
            .find("uipilot_lib::run()")
            .expect("application run call is missing");
        assert!(prepare < run);

        let library = include_str!("lib.rs");
        let identity = library
            .find("native_attention::prepare_process_identity()")
            .expect("native identity call is missing");
        let builder = library
            .find("let builder = tauri::Builder::default();")
            .expect("Tauri builder is missing");
        assert!(identity < builder);
    }

    #[test]
    fn delayed_plugin_messages_start_with_app_and_stop_before_native_effects() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        assert!(production.contains(
            "public_plugin_service.initialize(\n        app.handle(),\n        &app_data_dir,"
        ));
        assert!(production
            .contains("let run_public_plugin_service = Arc::clone(&public_plugin_service);"));
        let run_exit = production
            .split("tauri::RunEvent::Exit => {")
            .nth(1)
            .and_then(|tail| tail.split("_ => {}").next())
            .expect("run exit branch is missing");
        let delayed_shutdown = run_exit
            .find("run_public_plugin_service.shutdown();")
            .expect("delayed scheduler shutdown is missing");
        let native_shutdown = run_exit
            .find("_app.state::<Arc<message_center::MessageCenterService>>()")
            .expect("message center shutdown is missing");
        assert!(delayed_shutdown < native_shutdown);
    }

    #[test]
    fn main_focus_filters_spurious_blur_before_attention_and_expected_return() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let focused_branch = production
            .split("tauri::WindowEvent::Focused(focused) => {")
            .nth(1)
            .and_then(|tail| tail.split("tauri::WindowEvent::CloseRequested").next())
            .expect("main focused branch is missing");
        let observe = focused_branch
            .find("observe_main_focus(*focused)")
            .expect("tray attention focus observation is missing");
        let consume = focused_branch
            .find("consume_expected_main_blur()")
            .expect("expected main blur handling is missing");
        let filter = focused_branch
            .find("should_ignore_main_focus_loss(")
            .expect("spurious main blur filter is missing");
        let early_return = focused_branch
            .find("if expected_blur")
            .expect("expected blur early return is missing");

        assert!(consume < filter && filter < observe && observe < early_return);
    }

    #[test]
    fn main_focus_checks_webview_content_ownership_before_hide_dispatch() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let focused_branch = source
            .split("tauri::WindowEvent::Focused(focused) => {")
            .nth(1)
            .and_then(|tail| tail.split("tauri::WindowEvent::CloseRequested").next())
            .expect("main focused branch is missing");
        let expected_blur = focused_branch
            .find("consume_expected_main_blur()")
            .expect("expected main blur handling is missing");
        let panel_focus = focused_branch
            .find("consume_internal_main_blur(std::time::Instant::now())")
            .expect("webview content focus normalization is missing");
        let hide = focused_branch
            .find("commands::clear_and_hide(")
            .expect("main hide dispatch is missing");

        assert!(expected_blur < panel_focus);
        assert!(panel_focus < hide);
        assert!(!focused_branch.contains("main_content_got_focus()"));
    }

    #[test]
    fn startup_public_plugin_runtime_waits_for_main_frontend_ready() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let commands_source = include_str!("commands.rs").replace("\r\n", "\n");
        let commands = commands_source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("commands test module marker is missing");
        let public_source = include_str!("public_plugins.rs").replace("\r\n", "\n");
        let starter = public_source
            .split("pub(crate) fn start_enabled_runtimes(")
            .nth(1)
            .and_then(|tail| tail.split("\n    pub(crate) fn ").next())
            .expect("public plugin frontend-ready starter is missing");
        let setup = production
            .split("fn setup_production_lifecycle(")
            .nth(1)
            .and_then(|tail| tail.split("pub fn run() {").next())
            .expect("production setup markers are missing");
        let load_settings = commands
            .split("pub(crate) fn load_settings(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("load_settings command is missing");
        let ready = load_settings
            .find("mark_frontend_ready")
            .expect("main frontend ready signal is missing");
        let start = load_settings
            .find("start_enabled_runtimes")
            .expect("public Runtime startup must follow main frontend readiness");
        let claim = starter
            .find("compare_exchange")
            .expect("public Runtime startup must be one-shot");
        let spawn = starter
            .find("tauri::async_runtime::spawn_blocking")
            .expect("public Runtime readiness must leave the command thread");
        let create = starter
            .find(".create_runtime(")
            .expect("public Runtime startup creation is missing");

        assert!(ready < start);
        assert!(claim < spawn && spawn < create);
        assert!(!setup.contains("start_enabled_runtimes"));
        assert!(!setup.contains(".create_runtime("));
    }
    #[test]
    fn tray_show_does_not_wait_for_application_discovery() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        let tray_callback = production
            .split(".on_menu_event(")
            .nth(1)
            .and_then(|tail| tail.split(".build(app)").next())
            .expect("tray callback markers are missing");
        assert!(tray_callback.contains("tray_coordinator.request_show(app, target)"));
        for forbidden in [
            "start_initial_refresh",
            "discover",
            "icon::",
            "GetImage",
            "WIC",
            ".join(",
            ".recv(",
        ] {
            assert!(
                !tray_callback.contains(forbidden),
                "tray callback waits for application discovery: {forbidden}"
            );
        }

        let setup = production
            .split("fn setup_production_lifecycle(")
            .nth(1)
            .and_then(|tail| tail.split("pub fn run() {").next())
            .expect("production setup markers are missing");
        let background_start = "let _ = apps::start_initial_refresh(Arc::clone(app_cache))?;";
        assert_eq!(setup.matches(background_start).count(), 1);
        for forbidden in [".join(", ".recv(", "apps::discover", "GetImage", "WIC"] {
            assert!(
                !setup.contains(forbidden),
                "production setup waits for application discovery: {forbidden}"
            );
        }
    }

    #[test]
    fn application_icon_module_is_covered_by_the_lint_oracle() {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let expected = ["(\"apps/icon.rs\", include_str!(\"apps/", "icon.rs\"))"].concat();
        assert!(source.contains(&expected));
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
            "find_window",
            "settings",
            "hotkey",
            "double_tap",
            "hotkey_hook",
            "lifecycle",
            "file_index",
            "plugins",
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

        let commands_source = include_str!("commands.rs").replace("\r\n", "\n");
        let commands = commands_source
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("commands test module marker is missing");
        let action = include_str!("apps/action.rs").replace("\r\n", "\n");
        let cache = include_str!("apps/cache.rs").replace("\r\n", "\n");
        let file_index = include_str!("file_index/mod.rs").replace("\r\n", "\n");
        let file_store = include_str!("file_index/store.rs").replace("\r\n", "\n");
        let file_windows = include_str!("file_index/windows_backend.rs").replace("\r\n", "\n");
        let file_search = include_str!("file_search/mod.rs").replace("\r\n", "\n");
        let path_auth = include_str!("file_search/windows/path_auth.rs").replace("\r\n", "\n");
        let file_windows_production = file_windows
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("windows backend test module marker is missing");
        let search_files_allow = "#[allow(clippy::too_many_arguments)]";
        let search_files_command =
            format!("{search_files_allow}\n#[tauri::command]\npub(crate) async fn search_files(");
        assert_eq!(commands.matches(search_files_allow).count(), 1);
        assert!(commands.contains(&search_files_command));
        let commands_without_search_files_allow = commands.replacen(search_files_allow, "", 1);
        assert!(has_forbidden_production_lint_suppression(&format!(
            "{search_files_allow}\n#[tauri::command]\nfn near_miss() {{}}"
        )));
        assert!(has_forbidden_production_lint_suppression(&format!(
            "{commands_without_search_files_allow}\n{search_files_allow}"
        )));
        let product_sources = [
            ("lib.rs", production_root),
            ("atomic_file.rs", include_str!("atomic_file.rs")),
            ("commands.rs", commands_without_search_files_allow.as_str()),
            ("apps/mod.rs", include_str!("apps/mod.rs")),
            ("apps/action.rs", action.as_str()),
            ("apps/cache.rs", cache.as_str()),
            ("apps/discovery.rs", include_str!("apps/discovery.rs")),
            ("apps/icon.rs", include_str!("apps/icon.rs")),
            ("apps/rank.rs", include_str!("apps/rank.rs")),
            ("apps/shortcut.rs", include_str!("apps/shortcut.rs")),
            (
                "apps/windows_backend.rs",
                include_str!("apps/windows_backend.rs"),
            ),
            ("hotkey.rs", include_str!("hotkey.rs")),
            ("double_tap.rs", include_str!("double_tap.rs")),
            ("hotkey_hook.rs", include_str!("hotkey_hook.rs")),
            ("lifecycle.rs", include_str!("lifecycle.rs")),
            ("file_index/mod.rs", file_index.as_str()),
            ("file_index/store.rs", file_store.as_str()),
            ("file_index/windows_backend.rs", file_windows_production),
            ("file_search/mod.rs", file_search.as_str()),
            ("file_search/windows/path_auth.rs", path_auth.as_str()),
            ("model.rs", include_str!("model.rs")),
            ("result_registry.rs", include_str!("result_registry.rs")),
            ("find_window.rs", include_str!("find_window.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("plugins.rs", include_str!("plugins.rs")),
        ];

        for (name, product_source) in product_sources {
            assert!(
                !has_forbidden_production_lint_suppression(product_source),
                "unapproved production lint suppression is forbidden: {name}"
            );
        }

        for (name, product_source) in [
            ("file_index/mod.rs", file_index.as_str()),
            ("file_index/store.rs", file_store.as_str()),
            ("file_index/windows_backend.rs", file_windows_production),
        ] {
            for directive in ["#[allow(dead_code)]", "#[expect(dead_code)]"] {
                assert!(
                    has_forbidden_production_lint_suppression(&format!(
                        "{product_source}\n{directive}\nfn injected() {{}}"
                    )),
                    "file index lint fixture was accepted: {name} {directive}"
                );
            }
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
    }

    #[test]
    fn host_uses_builtin_calculator_without_legacy_math_command() {
        let lib_source = include_str!("lib.rs").replace("\r\n", "\n");
        let product_lib = lib_source.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(product_lib.contains("mod calculator;"));
        let plugin_source = include_str!("plugins.rs");
        assert!(plugin_source.contains("fn retired_plugin_id("));
        let forbidden_command = ["/", "math"].concat();
        for source in [product_lib, plugin_source] {
            assert!(!source.contains(&forbidden_command));
        }

        let legacy = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples")
            .join("plugins")
            .join("internal.math");
        assert!(!legacy.exists());
    }

    #[test]
    fn plugin_runtime_capability_is_narrow() {
        let capability = include_str!("../capabilities/plugin-runtime.json");
        assert!(capability.contains("\"windows\": [\"plugin-runtime-*\"]"));
        assert!(capability.contains("\"allow-plugin-api-call\""));
        assert!(capability.contains("\"allow-complete-plugin-command\""));
        assert!(capability.contains("\"core:event:allow-listen\""));
        assert!(capability.contains("\"core:event:allow-unlisten\""));
        for forbidden in [
            "\"*\"",
            "clipboard",
            "allow-search-apps",
            "allow-publish-plugin-results",
            "plugin-shell-",
            "plugin-content-",
            "main",
        ] {
            assert!(!capability.contains(forbidden));
        }
    }

    #[test]
    fn plugin_runtime_wiring_is_narrow() {
        let lib = include_str!("lib.rs").replace("\r\n", "\n");
        let plugins = include_str!("plugins.rs").replace("\r\n", "\n");
        for fragment in [
            ".register_uri_scheme_protocol(\"uipilot-plugin\",",
            "ctx.webview_label()",
            "plugin_manager.asset_response(",
            "plugin_manager.create_runtimes(app, &app_data_dir)?;",
        ] {
            assert!(lib.contains(fragment), "missing runtime wiring: {fragment}");
        }
        for fragment in [
            "WebviewWindowBuilder::new(",
            "WebviewUrl::CustomProtocol(url)",
            ".visible(false)",
            ".focusable(false)",
            ".skip_taskbar(true)",
            ".incognito(true)",
            ".data_directory(data_directory)",
            ".on_navigation(",
            ".on_new_window(|_, _| NewWindowResponse::Deny)",
            ".on_download(|_, _| false)",
            ".on_document_title_changed(",
            ".initialization_script(PLUGIN_BRIDGE)",
            "WebviewWindow::with_webview",
            "ProcessFailedEventHandler",
            "disable_runtime",
        ] {
            assert!(
                plugins.contains(fragment),
                "missing runtime builder: {fragment}"
            );
        }
        for forbidden in [
            [".visible", "(true)"].concat(),
            "NewWindowResponse::Allow".into(),
            "ShellOpen".into(),
            "open_path".into(),
            "asset://".into(),
            "file://".into(),
            "appDataDir".into(),
        ] {
            assert!(
                !plugins.contains(&forbidden),
                "forbidden plugin runtime wiring: {forbidden}"
            );
        }
    }
}

#[cfg(test)]
mod lib {
    mod tests {
        #[test]
        fn production_file_search_state_commands_and_permissions_are_exact() {
            let source = include_str!("lib.rs").replace("\r\n", "\n");
            let production = source
                .split("#[cfg(test)]\nmod tests")
                .next()
                .expect("test module marker is missing");

            assert!(!production.contains("retain_legacy_file_search_linkage"));
            assert!(!production.contains("FileIndex::search"));
            for forbidden in ["#[allow(dead_code)]", "#[expect(dead_code)]"] {
                assert!(!production.contains(forbidden));
                assert!(!include_str!("file_search/windows/path_auth.rs").contains(forbidden));
            }
            assert_eq!(
                production
                    .matches("let file_index = Arc::new(file_index::FileIndex::new(")
                    .count(),
                1
            );
            assert!(production.contains(
                "let file_index = Arc::new(file_index::FileIndex::new(\n        Arc::clone(&coordinator),\n        result_registries.find().clone(),\n    ));"
            ));
            assert_eq!(
                production
                    .matches(".manage(Arc::clone(&file_index))")
                    .count(),
                1
            );
            assert_eq!(
                production
                    .matches("file_search::everything::EverythingSearchState::new()")
                    .count(),
                1
            );
            assert!(production.contains(
                "let everything_search = Arc::new(file_search::everything::EverythingSearchState::new());"
            ));
            assert_eq!(production.matches(".manage(everything_search)").count(), 1);
            assert_eq!(
                production
                    .matches("let run_file_index = Arc::clone(&file_index);")
                    .count(),
                1
            );
            assert!(production.contains(
                "app.state::<Arc<file_index::FileIndex>>()\n        .install_main_window_hwnd(hwnd.0 as isize)"
            ));

            let run_exit = production
                .split("tauri::RunEvent::Exit => {")
                .nth(1)
                .and_then(|tail| tail.split("_ => {}").next())
                .expect("run exit branch is missing");
            assert!(run_exit.contains("run_file_index.enter_terminal();"));
            assert!(run_exit.contains("run_coordinator.uninstall_hook_for_exit();"));
            assert!(run_exit.contains("run_coordinator.observe_run_exit();"));
            assert!(
                run_exit.find("run_file_index.enter_terminal();").unwrap()
                    < run_exit
                        .find("run_coordinator.observe_run_exit();")
                        .unwrap()
            );

            let main_capability = include_str!("../capabilities/main.json");
            let find_capability = include_str!("../capabilities/find.json");
            let build = include_str!("../build.rs");
            assert!(!main_capability.contains("\"allow-search-files\""));
            assert!(find_capability.contains("\"allow-search-files\""));
            assert!(!main_capability.contains("\"allow-load-find-thumbnail\""));
            assert!(find_capability.contains("\"allow-load-find-thumbnail\""));
            assert!(main_capability.contains("\"allow-execute-result\""));
            assert!(find_capability.contains("\"allow-execute-result\""));
            assert_eq!(production.matches("commands::search_files,").count(), 1);
            assert_eq!(
                production.matches("commands::load_find_thumbnail,").count(),
                1
            );
            assert_eq!(production.matches("commands::execute_result,").count(), 1);
            for forbidden in ["refresh_files", "refresh-files"] {
                assert!(!production.contains(forbidden));
                assert!(!build.contains(forbidden));
                assert!(!main_capability.contains(forbidden));
            }
            let autogenerated = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("permissions")
                .join("autogenerated")
                .join("refresh_files.toml");
            assert!(!autogenerated.exists());
            assert!(!main_capability.contains("\"core:window:allow-start-dragging\""));
            assert!(!find_capability.contains("\"core:window:default\""));
            let probe_load_settings = ["security_probe", "::", "load_settings"].concat();
            assert_eq!(source.matches(&probe_load_settings).count(), 3);
            let probe_search_files = ["security_probe", "::", "search_files"].concat();
            assert!(!source.contains(&probe_search_files));
        }
    }
}
