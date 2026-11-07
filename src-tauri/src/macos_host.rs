#[path = "macos_hotkey.rs"]
mod hotkey;

use std::{sync::Mutex, time::Duration};
use tauri_plugin_global_shortcut::Builder as ShortcutBuilder;
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
    visible: Option<u64>,
    focused: Option<u64>,
    recording: Option<u64>,
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
    fn begin_present(&mut self, id: u64) -> bool {
        if self.quitting || self.invocation != id { return false; }
        self.visible = Some(id);
        self.focused = None;
        self.recording = None;
        true
    }

    fn owns(&self, id: u64) -> bool {
        !self.quitting && self.visible == Some(id) && self.invocation == id
    }

    fn set_recording(&mut self, id: u64, armed: bool) -> bool {
        if !self.owns(id) { return false; }
        self.recording = armed.then_some(id);
        true
    }

    fn recording_owner(&self) -> Option<u64> {
        self.recording.filter(|id| self.owns(*id))
    }

    fn observe_focus(&mut self) {
        if let Some(id) = self.visible.filter(|id| self.owns(*id)) {
            self.focused = Some(id);
        }
    }

    fn retire(&mut self, id: u64) {
        if self.visible == Some(id) {
            self.visible = None;
            self.focused = None;
            self.recording = None;
        }
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

// Position and focus operations are serialized on the application main thread.
fn position_on_active_display(window: &WebviewWindow) -> tauri::Result<()> {
    let current = window.outer_position()?;
    let size = window.outer_size()?;
    let pointer_monitor = window.cursor_position().ok()
        .and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten());
    let monitor = match pointer_monitor {
        Some(monitor) => monitor,
        None => match window.primary_monitor()? {
            Some(primary) => primary,
            None => return Ok(()),
        }
    };
    let area = monitor.work_area();
    // Restore the normal logical panel size on a larger display and clamp it
    // to the available physical work area after display removal or scaling.
    let desired = window.app_handle().config().app.windows.iter().find(|w| w.label == "main")
        .map(|config| {
            let scale = monitor.scale_factor();
            tauri::PhysicalSize::new((config.width * scale).round() as u32, (config.height * scale).round() as u32)
        }).unwrap_or(size);
    let fitted = tauri::PhysicalSize::new(desired.width.min(area.size.width), desired.height.min(area.size.height));
    if fitted.width == 0 || fitted.height == 0 { return Ok(()); }
    let position = tauri::PhysicalPosition::new(
        fit_axis(current.x, fitted.width, area.position.x, area.size.width),
        fit_axis(current.y, fitted.height, area.position.y, area.size.height),
    );
    if current != position { window.set_position(position)?; }
    if fitted != size { window.set_size(fitted)?; }
    Ok(())
}

fn fit_axis(current: i32, size: u32, start: i32, extent: u32) -> i32 {
    let offset = i64::from(current) - i64::from(start);
    let spare = i64::from(extent.saturating_sub(size));
    if offset >= 0 && offset <= spare { current }
    else { (i64::from(start) + spare / 2).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32 }
}

#[cfg(feature = "macos-find-preview")]
pub(crate) fn main_invocation_current(app: &AppHandle, id: u64) -> bool {
    app.state::<Mutex<Presentation>>().lock().expect("presentation state").owns(id)
}

// A queued blur or application completion can only hide its own presentation.
pub(crate) fn hide_if_current(app: &AppHandle, id: u64) -> Result<(), ()> {
    let state = app.state::<Mutex<Presentation>>();
    if !state.lock().expect("presentation state").owns(id) { return Ok(()); }
    let window = app.get_webview_window("main").ok_or(())?;
    window.hide().map_err(|_| ())?;
    state.lock().expect("presentation state").retire(id);
    crate::macos_apps::end_invocation(app);
    window.emit("launcher://hidden", ()).map_err(|_| ())
}

fn recheck_focus(app: &AppHandle, id: u64, delay: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let state = handle.state::<Mutex<Presentation>>();
            if !state.lock().expect("presentation state").owns(id) { return; }
            let Some(window) = handle.get_webview_window("main") else { return; };
            if window.is_focused().unwrap_or(false) {
                state.lock().expect("presentation state").observe_focus();
            } else {
                #[cfg(feature = "macos-find-preview")]
                if crate::macos_find_host::owns_main_blur(&handle) { return; }
                if hide_if_current(&handle, id).is_err() {
                    eprintln!("UiPilot: could not hide unfocused invocation {id}");
                }
            }
        });
    });
}

fn present(app: &AppHandle, id: u64) {
    let Some(window) = app.get_webview_window("main") else { return; };
    let state = app.state::<Mutex<Presentation>>();
    if !state.lock().expect("presentation state").begin_present(id) { return; }
    if let Err(error) = position_on_active_display(&window) {
        eprintln!("UiPilot: could not position launcher: {error}");
    }
    let result = window.unminimize().and_then(|_| window.show()).and_then(|_| window.set_focus()).and_then(|_| {
        crate::macos_apps::begin_invocation(app, id);
        window.emit("launcher://shown", serde_json::json!({
            "invocationId": id.to_string(), "target": "launcher", "notice": null
        }))
    });
    if let Err(error) = result {
        eprintln!("UiPilot: could not show launcher: {error}");
        let _ = hide_if_current(app, id);
        return;
    }
    if window.is_focused().unwrap_or(false) {
        state.lock().expect("presentation state").observe_focus();
    }
    // set_focus can accept a request before AppKit reports the key window.
    recheck_focus(app, id, Duration::from_millis(600));
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
    let revision = window.app_handle().state::<SettingsStore>().set_theme_preference_with_revision(preference.theme)
        .map_err(|_| "settingsFailed")?;
    if let Some(find) = window.app_handle().get_webview_window("find") {
        find.emit_to("find", "find://theme-changed", serde_json::json!({
            "themeRevision": revision.to_string(), "theme": preference.theme
        })).map_err(|_| "windowFailed")?;
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WebSearchUpdate { engine: crate::settings::WebSearchEngine }

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginItemUpdate { enabled: bool }

fn require_login_main(window: &WebviewWindow) -> Result<(), &'static str> {
    require_main(window)?;
    login_item_available(cfg!(feature = "macos-login-item-preview"))
}

fn login_item_available(enabled: bool) -> Result<(), &'static str> {
    enabled.then_some(()).ok_or("loginItemUnavailable")
}

async fn login_main<T: Send + 'static>(window: &WebviewWindow,
    operation: impl FnOnce(objc2::MainThreadMarker) -> T + Send + 'static,
) -> Result<T, &'static str> {
    let (send, receive) = tokio::sync::oneshot::channel();
    window.run_on_main_thread(move || {
        let result = objc2::MainThreadMarker::new().map(operation).ok_or("loginItemUnavailable");
        let _ = send.send(result);
    }).map_err(|_| "loginItemUnavailable")?;
    receive.await.map_err(|_| "loginItemUnavailable")?
}

#[tauri::command]
async fn get_macos_login_item(window: WebviewWindow) -> Result<crate::macos_login_item::Snapshot, &'static str> {
    require_login_main(&window)?;
    login_main(&window, crate::macos_login_item::read_native).await
}

#[tauri::command]
async fn set_macos_login_item(window: WebviewWindow, input: LoginItemUpdate) -> Result<crate::macos_login_item::Mutation, &'static str> {
    require_login_main(&window)?;
    login_main(&window, move |mtm| crate::macos_login_item::set_native(mtm, input.enabled)).await
}

#[tauri::command]
async fn open_macos_login_item_settings(window: WebviewWindow) -> Result<(), &'static str> {
    require_login_main(&window)?;
    login_main(&window, crate::macos_login_item::open_settings_native).await?
        .map_err(|_| "loginItemUnavailable")
}

#[tauri::command]
fn set_web_search_engine(window: WebviewWindow, preference: WebSearchUpdate) -> Result<(), &'static str> {
    require_main(&window)?;
    window.state::<SettingsStore>().set_web_search_engine(preference.engine)
        .map_err(|_| "settingsFailed")
}

#[tauri::command]
async fn hide_launcher(window: WebviewWindow, invocation_id: String) -> Result<(), &'static str> {
    require_main(&window)?;
    let id = invocation_id.parse::<u64>().map_err(|_| "windowFailed")?;
    let app = window.app_handle().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    window.run_on_main_thread(move || {
        let _ = send.send(hide_if_current(&app, id));
    }).map_err(|_| "windowFailed")?;
    receive.await.map_err(|_| "windowFailed")?.map_err(|_| "windowFailed")
}

#[cfg(feature = "macos-find-preview")]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FavoriteInput { feature: crate::settings::BuiltinFeature, favorite: bool }

#[cfg(feature = "macos-find-preview")]
#[tauri::command]
fn set_builtin_feature_favorite(window: WebviewWindow, input: FavoriteInput) -> Result<(), &'static str> {
    require_main(&window)?;
    if !crate::host_capabilities::mac_find_enabled() || input.feature != crate::settings::BuiltinFeature::Find { return Err("invalidCaller"); }
    window.state::<SettingsStore>().set_builtin_feature_favorite(input.feature, input.favorite)
        .map_err(|_| "settingsFailed")
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let settings = SettingsStore::load(&app.path().app_data_dir()?)?;
    app.manage(settings);
    crate::macos_apps::setup(app);
    #[cfg(feature = "macos-find-preview")]
    if crate::host_capabilities::mac_find_enabled() { crate::macos_find_host::setup(app)?; }
    let open = MenuItem::with_id(app, "show", "显示 UiPilot", true, None::<&str>)?;
    app.manage(hotkey::ShowMenu(open.clone()));
    if let Err(error) = app.handle().plugin(ShortcutBuilder::new().with_handler(|app, shortcut, event| {
        hotkey::on_pressed(app, shortcut, event.state)
    }).build()) {
        eprintln!("UiPilot: global shortcut service unavailable: {error}");
    }
    hotkey::initialize(app.handle());
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app.default_window_icon().ok_or_else(|| std::io::Error::other("missing launcher icon"))?;
    TrayIconBuilder::with_id("launcher")
        .icon(icon.clone()).tooltip("UiPilot").menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show(app),
            "quit" => {
                app.state::<Mutex<Presentation>>().lock().expect("presentation state").quitting = true;
                hotkey::stop(app);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    show(app.handle());
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
        .manage(hotkey::LauncherHotkey::default())
        .manage(Mutex::new(Presentation::default()))
        .invoke_handler(tauri::generate_handler![
            crate::host_capabilities::get_host_capabilities, load_settings, set_theme_preference,
            hide_launcher, set_web_search_engine, hotkey::save_hotkey, hotkey::set_hotkey_recording,
            get_macos_login_item, set_macos_login_item, open_macos_login_item_settings,
            crate::macos_apps::search_apps, crate::macos_apps::execute_result,
            #[cfg(feature = "macos-find-preview")] set_builtin_feature_favorite,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::open_find_window,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::prepare_find_initialization,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::commit_find_ready,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::get_find_ready_status,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::set_find_pinned,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::set_find_preview_preference,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::hide_find_window,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::search_files,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::cancel_find_query,
            #[cfg(feature = "macos-find-preview")] crate::macos_find_host::open_find_document,
            #[cfg(all(feature = "macos-find-preview", not(feature = "macos-find-system-qa")))]
            crate::macos_find_host::get_find_documents_access,
            #[cfg(all(feature = "macos-find-preview", not(feature = "macos-find-system-qa")))]
            crate::macos_find_host::grant_find_documents_access,
            #[cfg(all(feature = "macos-find-preview", not(feature = "macos-find-system-qa")))]
            crate::macos_find_host::revoke_find_documents_access,
        ])
        .on_window_event(|window, event| {
            #[cfg(feature = "macos-find-preview")]
            if crate::macos_find_host::on_window_event(window, event) { return; }
            if window.label() != "main" { return; }
            let state = window.state::<Mutex<Presentation>>();
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let guard = state.lock().expect("presentation state");
                    if guard.quitting { return; }
                    let visible = guard.visible;
                    drop(guard);
                    api.prevent_close();
                    if let Some(id) = visible { let _ = hide_if_current(window.app_handle(), id); }
                }
                tauri::WindowEvent::Focused(true) => state.lock().expect("presentation state").observe_focus(),
                tauri::WindowEvent::Focused(false) => {
                    let id = state.lock().expect("presentation state").focused;
                    if let Some(id) = id {
                        recheck_focus(window.app_handle(), id, Duration::from_millis(70));
                    }
                }
                _ => {}
            }
        })
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("failed to start UiPilot")
        .run(|app, event| {
            match event {
                tauri::RunEvent::Reopen { .. } => show(app),
                tauri::RunEvent::ExitRequested { code, api, .. } => {
                    #[cfg(not(feature = "macos-find-preview"))]
                    let _ = (code, &api);
                    app.state::<Mutex<Presentation>>().lock().expect("presentation state").quitting = true;
                    #[cfg(feature = "macos-find-preview")]
                    crate::macos_find_host::shutdown(app, code, &api);
                    hotkey::stop(app);
                    app.remove_tray_by_id("launcher");
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod platform_bootstrap {
    use super::*;
    #[test]
    fn login_item_gate_and_update_are_explicit() {
        assert_eq!(login_item_available(false), Err("loginItemUnavailable"));
        assert_eq!(login_item_available(true), Ok(()));
        for invalid in [r#"{}"#, r#"{"enabled":"true"}"#, r#"{"enabled":true,"bundle":"other"}"#] {
            assert!(serde_json::from_str::<LoginItemUpdate>(invalid).is_err());
        }
        assert!(serde_json::from_str::<LoginItemUpdate>(r#"{"enabled":true}"#).unwrap().enabled);
    }
    #[test]
    fn web_engine_update_is_strict_and_preserves_other_settings() {
        for invalid in [r#"{"engine":"https://other.test"}"#, r#"{"engine":"bing","url":"https://other.test"}"#] {
            assert!(serde_json::from_str::<WebSearchUpdate>(invalid).is_err());
        }
        let dir = std::env::temp_dir().canonicalize().unwrap()
            .join(format!("uipilot-mac-web-settings-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, br#"{"hotkey":"Alt+Space","autostart":true,"futureSetting":[1,2]}"#).unwrap();
        let store = SettingsStore::load(&dir).unwrap();
        let update = serde_json::from_str::<WebSearchUpdate>(r#"{"engine":"google"}"#).unwrap();
        store.set_web_search_engine(update.engine).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(data["autostart"], true);
        assert_eq!(data["futureSetting"], serde_json::json!([1,2]));
        assert_eq!(SettingsStore::load(&dir).unwrap().snapshot().web_search_engine, crate::settings::WebSearchEngine::Google);
        std::fs::remove_dir_all(dir).unwrap();
    }
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn older_blur_and_completion_cannot_hide_new_or_pending_invocation() {
        let mut state = Presentation { ready: true, ..Presentation::default() };
        let first = state.request_show().unwrap();
        assert!(state.begin_present(first));
        state.observe_focus();
        let blur = state.focused.unwrap();
        let second = state.request_show().unwrap();
        assert!(!state.owns(blur));
        assert!(!state.begin_present(first));
        assert!(state.begin_present(second));
        state.retire(first);
        assert!(state.owns(second));
        state.retire(second);
        let retry = state.request_show().unwrap();
        assert!(state.begin_present(retry));
        state.quitting = true;
        assert!(!state.owns(retry));
        assert_eq!(state.request_show(), None);
    }
    #[test]
    fn old_recording_arm_and_release_cannot_modify_a_later_session() {
        let mut state = Presentation { ready: true, ..Presentation::default() };
        let a = state.request_show().unwrap();
        assert!(state.begin_present(a));
        assert!(state.set_recording(a, true));
        state.retire(a);
        assert!(!state.set_recording(a, true));
        let b = state.request_show().unwrap();
        assert!(state.begin_present(b));
        assert!(!state.set_recording(a, true));
        assert_eq!(state.recording_owner(), None);
        assert!(state.set_recording(b, true));
        assert!(!state.set_recording(a, false));
        assert_eq!(state.recording_owner(), Some(b));
        state.quitting = true;
        assert_eq!(state.recording_owner(), None);
    }
    #[test]
    fn active_monitor_placement_handles_removed_displays_and_negative_origins() {
        assert_eq!(fit_axis(2300, 720, -1400, 1400), -1060);
        assert_eq!(fit_axis(-1300, 720, -1400, 1400), -1300);
        assert_eq!(fit_axis(200, 720, 0, 640), 0);
    }
}
