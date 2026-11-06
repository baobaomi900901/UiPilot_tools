use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Mutex,
};
use tauri::{menu::MenuItem, AppHandle, Emitter, Manager, WebviewWindow, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcut, Shortcut, ShortcutState};

use crate::{host_capabilities::require_main, settings::SettingsStore};

#[derive(Debug, Eq, PartialEq, serde::Serialize)]
pub(super) struct HotkeyError {
    code: &'static str,
}
const INVALID: HotkeyError = HotkeyError { code: "invalidHotkey" };
const CONFLICT: HotkeyError = HotkeyError { code: "hotkeyConflict" };
const FAILED: HotkeyError = HotkeyError { code: "settingsFailed" };

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HotkeyValue { hotkey: String }

fn chord(text: &str) -> Result<Shortcut, HotkeyError> {
    if text.len() > 96 { return Err(INVALID); }
    // Recorder uses DOM's Meta; the native parser calls it Super/Command.
    let native = text.split('+').map(|part| if part.eq_ignore_ascii_case("Meta") {
        "Super"
    } else { part }).collect::<Vec<_>>().join("+");
    let shortcut = native.parse::<Shortcut>().map_err(|_| INVALID)?;
    // global-hotkey uses a system-wide event tap for media keys on macOS.
    // The launcher may register Carbon chords only.
    if shortcut.mods.is_empty() || matches!(shortcut.key,
        Code::MediaPlay | Code::MediaPause | Code::MediaPlayPause | Code::MediaStop |
        Code::MediaTrackNext | Code::MediaTrackPrevious | Code::MediaFastForward |
        Code::MediaRewind | Code::AudioVolumeUp | Code::AudioVolumeDown | Code::AudioVolumeMute
    ) { return Err(INVALID); }
    Ok(shortcut)
}

trait Port {
    fn register(&mut self, key: Shortcut) -> Result<(), HotkeyError>;
    fn unregister(&mut self, key: Shortcut) -> Result<(), HotkeyError>;
    fn save(&mut self, text: &str) -> Result<(), HotkeyError>;
}

#[derive(Default)]
struct Bindings {
    active: Option<Shortcut>,
    // If native cleanup fails the stale registration is inert. Retry it before
    // admitting another change rather than accumulating system reservations.
    stale: Option<Shortcut>,
}
impl Bindings {
    fn initialize(&mut self, text: &str, port: &mut impl Port) -> Result<Shortcut, HotkeyError> {
        let key = chord(text)?;
        port.register(key)?;
        self.active = Some(key);
        Ok(key)
    }

    fn clear_stale(&mut self, port: &mut impl Port) -> Result<(), HotkeyError> {
        if let Some(key) = self.stale {
            port.unregister(key)?;
            self.stale = None;
        }
        Ok(())
    }

    fn rebind(&mut self, text: &str, previous: &str, port: &mut impl Port) -> Result<(), HotkeyError> {
        let key = chord(text)?;
        self.clear_stale(port)?;
        if self.active == Some(key) {
            return if text == previous { Ok(()) } else { port.save(text) };
        }
        port.register(key)?;
        self.stale = Some(key);
        if let Err(error) = port.save(text) {
            // The original binding remains registered throughout a failed save.
            let _ = self.clear_stale(port);
            return Err(error);
        }
        if let Some(old) = self.active {
            if port.unregister(old).is_err() {
                if port.save(previous).is_ok() {
                    let _ = self.clear_stale(port);
                } else {
                    // Rollback could not be persisted. Match the durable new key;
                    // only its callbacks are effective until cleanup succeeds.
                    self.active = Some(key);
                    self.stale = Some(old);
                }
                return Err(FAILED);
            }
        }
        self.active = Some(key);
        self.stale = None;
        Ok(())
    }
}

struct Native<'a>(&'a AppHandle);
impl Port for Native<'_> {
    fn register(&mut self, key: Shortcut) -> Result<(), HotkeyError> {
        self.0.try_state::<GlobalShortcut<Wry>>().ok_or(FAILED)?.register(key).map_err(|_| CONFLICT)
    }
    fn unregister(&mut self, key: Shortcut) -> Result<(), HotkeyError> {
        self.0.try_state::<GlobalShortcut<Wry>>().ok_or(FAILED)?.unregister(key).map_err(|_| FAILED)
    }
    fn save(&mut self, text: &str) -> Result<(), HotkeyError> {
        self.0.state::<SettingsStore>().update_hotkey(text.to_owned()).map_err(|_| FAILED)
    }
}

#[derive(Default)]
pub(super) struct LauncherHotkey {
    bindings: Mutex<Bindings>,
    // The plugin calls our handler while holding its own registration lock.
    // Never acquire the binding transaction lock inside that callback.
    effective_id: AtomicU32,
    recording: AtomicBool,
    stopping: AtomicBool,
}
pub(super) struct ShowMenu(pub(super) MenuItem<Wry>);

pub(super) fn initialize(app: &AppHandle) {
    let text = app.state::<SettingsStore>().snapshot().hotkey;
    let state = app.state::<LauncherHotkey>();
    let mut bindings = state.bindings.lock().expect("hotkey bindings");
    match bindings.initialize(&text, &mut Native(app)) {
        Ok(key) => state.effective_id.store(key.id(), Ordering::Release),
        Err(_) => { let _ = app.state::<ShowMenu>().0.set_text("显示 UiPilot（快捷键不可用，请在设置中重新录制）"); }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum KeyAction { Show, Record }

impl LauncherHotkey {
    fn action(&self, id: u32, event: ShortcutState) -> Option<KeyAction> {
        if event != ShortcutState::Pressed || self.stopping.load(Ordering::Acquire)
            || self.effective_id.load(Ordering::Acquire) != id {
            return None;
        }
        Some(if self.recording.load(Ordering::Acquire) { KeyAction::Record } else { KeyAction::Show })
    }

    fn stop_once(&self) -> bool {
        if self.stopping.swap(true, Ordering::AcqRel) { return false; }
        self.effective_id.store(0, Ordering::Release);
        true
    }
}

pub(super) fn on_pressed(app: &AppHandle, shortcut: &Shortcut, event: ShortcutState) {
    let Some(state) = app.try_state::<LauncherHotkey>() else { return; };
    match state.action(shortcut.id(), event) {
        Some(KeyAction::Record) => {
            let text = app.state::<SettingsStore>().snapshot().hotkey;
            let _ = app.emit_to("main", "hotkey-recording://current", text);
        }
        Some(KeyAction::Show) => super::show(app),
        None => {}
    }
}

pub(super) fn stop(app: &AppHandle) {
    let Some(state) = app.try_state::<LauncherHotkey>() else { return; };
    if !state.stop_once() { return; }
    let Some(port) = app.try_state::<GlobalShortcut<Wry>>() else { return; };
    if let Err(error) = port.unregister_all() {
        eprintln!("UiPilot: quit could not release shortcut: {error}");
    }
}

#[tauri::command]
pub(super) fn set_hotkey_recording(window: WebviewWindow, recording: bool) -> Result<(), HotkeyError> {
    require_main(&window).map_err(|code| HotkeyError { code })?;
    window.state::<LauncherHotkey>().recording.store(recording, Ordering::Release);
    Ok(())
}

pub(super) fn end_recording(app: &AppHandle) {
    app.state::<LauncherHotkey>().recording.store(false, Ordering::Release);
}

#[tauri::command]
pub(super) async fn save_hotkey(window: WebviewWindow, hotkey: HotkeyValue) -> Result<HotkeyValue, HotkeyError> {
    require_main(&window).map_err(|code| HotkeyError { code })?;
    let app = window.app_handle().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    window.run_on_main_thread(move || {
        let state = app.state::<LauncherHotkey>();
        let result = if state.stopping.load(Ordering::Acquire) {
            Err(FAILED)
        } else {
            let previous = app.state::<SettingsStore>().snapshot().hotkey;
            let mut bindings = state.bindings.lock().expect("hotkey bindings");
            let result = bindings.rebind(&hotkey.hotkey, &previous, &mut Native(&app));
            state.effective_id.store(bindings.active.map_or(0, |key| key.id()), Ordering::Release);
            if result.is_ok() { let _ = app.state::<ShowMenu>().0.set_text("显示 UiPilot"); }
            result.map(|_| hotkey)
        };
        let _ = send.send(result);
    }).map_err(|_| FAILED)?;
    receive.await.map_err(|_| FAILED)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct TestPort { keys: Vec<Shortcut>, disk: String, fail_save: bool, fail_remove: Option<Shortcut> }
    impl Port for TestPort {
        fn register(&mut self, key: Shortcut) -> Result<(), HotkeyError> {
            if self.keys.contains(&key) { return Err(CONFLICT); }
            self.keys.push(key); Ok(())
        }
        fn unregister(&mut self, key: Shortcut) -> Result<(), HotkeyError> {
            if self.fail_remove == Some(key) { return Err(FAILED); }
            self.keys.retain(|&item| item != key); Ok(())
        }
        fn save(&mut self, text: &str) -> Result<(), HotkeyError> {
            if self.fail_save { return Err(FAILED); }
            self.disk = text.to_owned(); Ok(())
        }
    }
    #[test]
    fn failed_save_keeps_the_previous_os_registration_and_disk_value() {
        let mut state = Bindings::default();
        let mut port = TestPort::default();
        state.rebind("Alt+Space", "", &mut port).unwrap();
        let old = chord("Alt+Space").unwrap();
        port.fail_save = true;
        assert_eq!(state.rebind("Meta+K", "Alt+Space", &mut port), Err(FAILED));
        assert_eq!(port.keys, [old]);
        assert_eq!(state.active, Some(old));
        assert_eq!(port.disk, "Alt+Space");
    }
    #[test]
    fn conflict_or_native_retirement_failure_preserves_old_binding_and_permits_retry() {
        let mut state = Bindings::default();
        let mut port = TestPort::default();
        let old = chord("Alt+Space").unwrap();
        let new = chord("Meta+K").unwrap();
        state.rebind("Alt+Space", "", &mut port).unwrap();
        port.keys.push(new); // Another application has claimed this key.
        assert_eq!(state.rebind("Meta+K", "Alt+Space", &mut port), Err(CONFLICT));
        assert_eq!(state.active, Some(old));
        port.keys.retain(|key| *key != new);
        port.fail_remove = Some(old);
        assert_eq!(state.rebind("Meta+K", "Alt+Space", &mut port), Err(FAILED));
        assert_eq!(port.keys, [old]);
        assert_eq!(port.disk, "Alt+Space");
        port.fail_remove = None;
        state.rebind("Meta+K", "Alt+Space", &mut port).unwrap();
        assert_eq!(port.disk, "Meta+K"); assert_eq!(port.keys, [new]);
        assert_eq!(state.active, Some(new));
    }
    #[test]
    fn startup_conflict_leaves_no_active_binding_and_retry_only_writes_the_requested_key() {
        let key = chord("Alt+Space").unwrap();
        let mut port = TestPort { keys: vec![key], disk: "Alt+Space".into(), ..Default::default() };
        let mut state = Bindings::default();
        assert_eq!(state.initialize("Alt+Space", &mut port), Err(CONFLICT));
        assert_eq!(state.active, None);
        port.keys.clear(); // Owner of the system conflict released its shortcut.
        state.rebind("Alt+Space", "Alt+Space", &mut port).unwrap();
        assert_eq!(state.active, Some(key));
        assert_eq!(port.keys, [key]);
    }

    #[test]
    fn recording_stale_events_and_duplicate_quit_never_open_the_launcher() {
        let state = LauncherHotkey::default();
        state.effective_id.store(42, Ordering::Release);
        assert_eq!(state.action(42, ShortcutState::Pressed), Some(KeyAction::Show));
        state.recording.store(true, Ordering::Release);
        assert_eq!(state.action(42, ShortcutState::Pressed), Some(KeyAction::Record));
        assert_eq!(state.action(42, ShortcutState::Released), None);
        assert_eq!(state.action(41, ShortcutState::Pressed), None);
        assert!(state.stop_once());
        assert!(!state.stop_once());
        assert_eq!(state.action(42, ShortcutState::Pressed), None);
    }

    #[test]
    fn standalone_keys_double_taps_and_malformed_chords_are_rejected() {
        for value in ["Space", "DoubleCtrl", "DoubleAlt", "Ctrl+Shift+", "Alt+MediaPlayPause", "Alt+VolumeUp", ""] { assert_eq!(chord(value), Err(INVALID)); }
        assert_eq!(chord("Meta+K"), chord("Command+K"));
    }
}
