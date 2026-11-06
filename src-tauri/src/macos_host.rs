use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, WebviewWindow,
};

use crate::{
    host_capabilities::require_main,
    settings::{SettingsStore, ThemePreference},
};

#[derive(Default)]
struct Presentation {
    ready: bool,
    pending: bool,
    quitting: bool,
    invocation: u64,
}

impl Presentation {
    fn request_show(&mut self) -> Option<u64> {
        if self.quitting { return None; }
        if !self.ready {
            self.pending = true;
            return None;
        }
        self.pending = false;
        self.invocation = self.invocation.checked_add(1)?;
        Some(self.invocation)
    }

    fn mark_ready(&mut self) -> Option<u64> {
        self.ready = true;
        if self.pending { self.request_show() } else { None }
    }
}

fn show(app: &AppHandle) {
    let state = app.state::<Mutex<Presentation>>();
    let id = state.lock().expect("presentation state").request_show();
    if let Some(id) = id { schedule_show(app, id); }
}

fn schedule_show(app: &AppHandle, id: u64) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || present(&handle, id)) {
        eprintln!("UiPilot: could not schedule the launcher: {error}");
    }
}

fn present(app: &AppHandle, id: u64) {
    {
        let state = app.state::<Mutex<Presentation>>();
        let state = state.lock().expect("presentation state");
        if state.quitting || state.invocation != id { return; }
    }
    let Some(window) = app.get_webview_window("main") else { return; };
    let result = window.show().and_then(|_| window.set_focus()).and_then(|_| {
        window.emit("launcher://shown", serde_json::json!({
            "invocationId": id.to_string(), "target": "launcher", "notice": null
        }))
    });
    if let Err(error) = result { eprintln!("UiPilot: could not show the launcher: {error}"); }
}

#[tauri::command]
fn load_settings(window: WebviewWindow) -> Result<serde_json::Value, &'static str> {
    require_main(&window)?;
    let app = window.app_handle();
    let snapshot = app.state::<SettingsStore>().snapshot();
    let pending = {
        let state = app.state::<Mutex<Presentation>>();
        let mut state = state.lock().expect("presentation state");
        if !state.ready {
            eprintln!("[UiPilot] main frontend ready");
        }
        state.mark_ready()
    };
    if let Some(id) = pending { schedule_show(app, id); }
    Ok(serde_json::json!({
        "hotkey": snapshot.hotkey, "autostart": snapshot.autostart, "theme": snapshot.theme,
        "webSearchEngine": snapshot.web_search_engine, "filePreviewEnabled": snapshot.file_preview_enabled,
        "screenshotHotkey": null, "screenshotHotkeyStatus": "unset",
        "screenshotSaveDirectory": null, "screenshotSaveDirectoryPath": null
    }))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeUpdate { theme: ThemePreference }

#[tauri::command]
fn set_theme_preference(window: WebviewWindow, preference: ThemeUpdate) -> Result<(), &'static str> {
    require_main(&window)?;
    window.app_handle().state::<SettingsStore>().set_theme_preference_with_revision(preference.theme)
        .map(|_| ()).map_err(|_| "settingsFailed")
}

#[tauri::command]
fn hide_launcher(window: WebviewWindow) -> Result<(), &'static str> {
    require_main(&window)?;
    window.hide().and_then(|_| window.emit("launcher://hidden", ())).map_err(|_| "windowFailed")
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let settings = SettingsStore::load(&app.path().app_data_dir()?)?;
    app.manage(settings);
    let open = MenuItem::with_id(app, "show", "显示 UiPilot", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app.default_window_icon().ok_or_else(|| std::io::Error::other("missing launcher icon"))?;
    TrayIconBuilder::with_id("launcher")
        .icon(icon.clone()).tooltip("UiPilot").menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show(app),
            "quit" => {
                app.state::<Mutex<Presentation>>().lock().expect("presentation state").quitting = true;
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(crate::webview_reload_guard::init())
        .plugin(tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
            .on_navigation(|webview, url| crate::host_capabilities::trusted_page(
                webview.label(), url, webview.app_handle().config()))
            .build())
        .manage(Mutex::new(Presentation::default()))
        .invoke_handler(tauri::generate_handler![crate::host_capabilities::get_host_capabilities, load_settings, set_theme_preference, hide_launcher])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !window.state::<Mutex<Presentation>>().lock().expect("presentation state").quitting {
                    api.prevent_close();
                    if window.hide().is_ok() { let _ = window.emit("launcher://hidden", ()); }
                }
            }
        })
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("failed to start UiPilot")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Reopen { .. }) {
                show(app);
            }
        });
}

#[cfg(test)]
mod platform_bootstrap {
    use super::*;
    #[test]
    fn startup_coalesces_early_show_and_quit_refuses_new_invocations() {
        let mut state = Presentation::default();
        assert_eq!(state.request_show(), None);
        assert_eq!(state.request_show(), None);
        assert!(state.pending);
        assert_eq!(state.mark_ready(), Some(1));
        assert_eq!(state.mark_ready(), None);
        assert!(!state.pending);
        state.quitting = true;
        assert_eq!(state.request_show(), None);
        assert_eq!(state.invocation, 1);
    }
    #[test]
    fn saved_autostart_and_unknown_fields_survive_a_theme_only_write() {
        use std::fs;
        let dir = std::env::temp_dir().canonicalize().unwrap()
            .join(format!("uipilot-mac-settings-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, br#"{"hotkey":"Alt+Space","autostart":true,"futureSetting":[1,2]}"#).unwrap();
        let store = SettingsStore::load(&dir).unwrap();
        store.set_theme_preference_with_revision(ThemePreference::Dark).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(data["autostart"], true);
        assert_eq!(data["futureSetting"], serde_json::json!([1,2]));
        assert_eq!(SettingsStore::load(&dir).unwrap().snapshot().theme, ThemePreference::Dark);
        fs::remove_dir_all(dir).unwrap();
    }
}
