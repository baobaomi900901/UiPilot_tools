use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};
use tauri::{menu::MenuItem, AppHandle, Emitter, Manager, WebviewWindow, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcut, Shortcut, ShortcutState};

use crate::macos_command_tap::{CommandTap, StartError};
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

const DOUBLE_COMMAND: &str = "DoubleMeta";

#[derive(Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HotkeySnapshot {
    status: &'static str,
    configured_hotkey: String,
    effective_hotkey: Option<String>,
}
fn hotkey_snapshot(configured: String, active: Option<Gesture>, tap_active: bool, stopping: bool) -> HotkeySnapshot {
    let effective = if stopping { None } else { match active {
        Some(Gesture::DoubleCommand) if tap_active => Some(DOUBLE_COMMAND.to_owned()),
        Some(Gesture::Chord(key)) if gesture(&configured).ok() == Some(Gesture::Chord(key)) => Some(configured.clone()),
        Some(key) if gesture("Alt+Space").ok() == Some(key) => Some("Alt+Space".into()),
        _ => None,
    }};
    let status = match effective.as_ref() {
        None => "unavailable",
        Some(value) if gesture(value).ok() == gesture(&configured).ok() => "active",
        Some(_) => "fallback",
    };
    HotkeySnapshot { status, configured_hotkey: configured, effective_hotkey: effective }
}

#[tauri::command]
pub(super) async fn get_launcher_hotkey_status(window: WebviewWindow) -> Result<HotkeySnapshot, HotkeyError> {
    require_main(&window).map_err(|code| HotkeyError { code })?;
    let app = window.app_handle().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    window.run_on_main_thread(move || {
        let state = app.state::<LauncherHotkey>();
        let bindings = state.bindings.lock().expect("hotkey bindings");
        let configured = app.state::<SettingsStore>().snapshot().hotkey;
        let tap_active = state.tap.lock().expect("command tap").as_ref()
            .is_some_and(|registration| registration.enabled.load(Ordering::Acquire) && registration.tap.is_active());
        let snapshot = hotkey_snapshot(configured, bindings.active, tap_active, state.stopping.load(Ordering::Acquire));
        let _ = send.send(snapshot);
    }).map_err(|_| FAILED)?;
    receive.await.map_err(|_| FAILED)
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Gesture { Chord(Shortcut), DoubleCommand }

fn gesture(text: &str) -> Result<Gesture, HotkeyError> {
    if text == DOUBLE_COMMAND { return Ok(Gesture::DoubleCommand); }
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
    Ok(Gesture::Chord(shortcut))
}

trait Port {
    fn register(&mut self, key: Gesture) -> Result<(), HotkeyError>;
    fn unregister(&mut self, key: Gesture) -> Result<(), HotkeyError>;
    fn save(&mut self, text: &str) -> Result<(), HotkeyError>;
    fn ensure_active(&mut self, _key: Gesture) -> Result<(), HotkeyError> { Ok(()) }
}

#[derive(Default)]
struct Bindings {
    active: Option<Gesture>,
    // If native cleanup fails the stale registration is inert. Retry it before
    // admitting another change rather than accumulating system reservations.
    stale: Option<Gesture>,
}
impl Bindings {
    fn initialize(&mut self, text: &str, port: &mut impl Port) -> Result<Gesture, HotkeyError> {
        let key = gesture(text)?;
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
        let key = gesture(text)?;
        self.clear_stale(port)?;
        if self.active == Some(key) {
            port.ensure_active(key)?;
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

struct TapRegistration {
    tap: CommandTap,
    enabled: Arc<AtomicBool>,
}

struct Native<'a> { app: &'a AppHandle, request_permission: bool }

impl Native<'_> {
    fn start_tap(&self) -> Result<(), HotkeyError> {
        let app = self.app.clone();
        let enabled = Arc::new(AtomicBool::new(false));
        let gate = enabled.clone();
        let tap = CommandTap::start(self.request_permission, move || {
            if !gate.load(Ordering::Acquire) || recording(&app).is_some() { return; }
            let gate = gate.clone();
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if handle.state::<LauncherHotkey>().may_show_tap(&gate) && recording(&handle).is_none() {
                    super::show(&handle);
                }
            });
        }).map_err(|error| match error {
            StartError::InputMonitoringRequired => HotkeyError { code: "hotkeyPermissionRequired" },
            StartError::Unavailable => FAILED,
        })?;
        let state = self.app.state::<LauncherHotkey>();
        let mut slot = state.tap.lock().expect("command tap");
        if let Some(old) = slot.as_ref() { old.enabled.store(false, Ordering::Release); }
        *slot = Some(TapRegistration { tap, enabled });
        Ok(())
    }
}

fn recording(app: &AppHandle) -> Option<u64> {
    app.state::<Mutex<super::Presentation>>().lock().expect("presentation state").recording_owner()
}

impl Port for Native<'_> {
    fn register(&mut self, key: Gesture) -> Result<(), HotkeyError> {
        match key {
            Gesture::Chord(key) => self.app.try_state::<GlobalShortcut<Wry>>().ok_or(FAILED)?
                .register(key).map_err(|_| CONFLICT),
            Gesture::DoubleCommand => self.start_tap(),
        }
    }
    fn ensure_active(&mut self, key: Gesture) -> Result<(), HotkeyError> {
        if key == Gesture::DoubleCommand &&
            !self.app.state::<LauncherHotkey>().tap.lock().expect("command tap")
                .as_ref().is_some_and(|registration| registration.tap.is_active()) {
            self.start_tap()?;
        }
        Ok(())
    }
    fn unregister(&mut self, key: Gesture) -> Result<(), HotkeyError> {
        match key {
            Gesture::Chord(key) => self.app.try_state::<GlobalShortcut<Wry>>().ok_or(FAILED)?
                .unregister(key).map_err(|_| FAILED),
            Gesture::DoubleCommand => { self.app.state::<LauncherHotkey>().release_tap(); Ok(()) }
        }
    }
    fn save(&mut self, text: &str) -> Result<(), HotkeyError> {
        self.app.state::<SettingsStore>().update_hotkey(text.to_owned()).map_err(|_| FAILED)
    }
}

#[derive(Default)]
pub(super) struct LauncherHotkey {
    bindings: Mutex<Bindings>,
    tap: Mutex<Option<TapRegistration>>,
    // The plugin calls our handler while holding its own registration lock.
    // Never acquire the binding transaction lock inside that callback.
    effective_id: AtomicU32,
    stopping: AtomicBool,
}
pub(super) struct ShowMenu(pub(super) MenuItem<Wry>);

pub(super) fn initialize(app: &AppHandle) {
    let text = app.state::<SettingsStore>().snapshot().hotkey;
    let state = app.state::<LauncherHotkey>();
    let mut bindings = state.bindings.lock().expect("hotkey bindings");
    let mut port = Native { app, request_permission: false };
    if bindings.initialize(&text, &mut port).is_err() {
        let fallback = text == DOUBLE_COMMAND && bindings.initialize("Alt+Space", &mut port).is_ok();
        let hint = if fallback { "显示 UiPilot（双击 ⌘ 不可用，暂用 Option + 空格）" }
            else { "显示 UiPilot（快捷键不可用，请在设置中重新录制）" };
        let _ = app.state::<ShowMenu>().0.set_text(hint);
    }
    state.publish_binding(bindings.active);
}

#[derive(Debug, Eq, PartialEq)]
enum KeyAction { Show, Record(u64) }

impl LauncherHotkey {
    fn may_show_tap(&self, gate: &Arc<AtomicBool>) -> bool {
        !self.stopping.load(Ordering::Acquire) && gate.load(Ordering::Acquire) &&
            self.tap.lock().expect("command tap").as_ref().is_some_and(|tap|
                Arc::ptr_eq(gate, &tap.enabled) && tap.tap.is_active())
    }

    fn publish_binding(&self, active: Option<Gesture>) {
        let chord_id = match active { Some(Gesture::Chord(key)) => key.id(), _ => 0 };
        self.effective_id.store(chord_id, Ordering::Release);
        if let Some(registration) = self.tap.lock().expect("command tap").as_ref() {
            registration.enabled.store(active == Some(Gesture::DoubleCommand), Ordering::Release);
        }
    }

    fn release_tap(&self) {
        if let Some(old) = self.tap.lock().expect("command tap").take() {
            old.enabled.store(false, Ordering::Release);
        }
    }

    fn action(&self, id: u32, event: ShortcutState, recording: Option<u64>) -> Option<KeyAction> {
        if event != ShortcutState::Pressed || self.stopping.load(Ordering::Acquire)
            || self.effective_id.load(Ordering::Acquire) != id {
            return None;
        }
        Some(recording.map_or(KeyAction::Show, KeyAction::Record))
    }

    fn stop_once(&self) -> bool {
        if self.stopping.swap(true, Ordering::AcqRel) { return false; }
        self.effective_id.store(0, Ordering::Release);
        true
    }
}

pub(super) fn on_pressed(app: &AppHandle, shortcut: &Shortcut, event: ShortcutState) {
    let Some(state) = app.try_state::<LauncherHotkey>() else { return; };
    let owner = app.state::<Mutex<super::Presentation>>().lock().expect("presentation state").recording_owner();
    match state.action(shortcut.id(), event, owner) {
        Some(KeyAction::Record(id)) => {
            let mut text = app.state::<SettingsStore>().snapshot().hotkey;
            if text == DOUBLE_COMMAND { text = "Alt+Space".into(); }
            let _ = app.emit_to("main", "hotkey-recording://current", serde_json::json!({"invocationId": id.to_string(), "hotkey": text}));
        }
        Some(KeyAction::Show) => super::show(app),
        None => {}
    }
}

pub(super) fn stop(app: &AppHandle) {
    let Some(state) = app.try_state::<LauncherHotkey>() else { return; };
    if !state.stop_once() { return; }
    state.release_tap();
    let Some(port) = app.try_state::<GlobalShortcut<Wry>>() else { return; };
    if let Err(error) = port.unregister_all() {
        eprintln!("UiPilot: quit could not release shortcut: {error}");
    }
}

#[tauri::command]
pub(super) fn set_hotkey_recording(window: WebviewWindow, recording: bool, invocation_id: String) -> Result<(), HotkeyError> {
    require_main(&window).map_err(|code| HotkeyError { code })?;
    let stale = || HotkeyError { code: "staleRequest" };
    let id = invocation_id.parse().map_err(|_| stale())?;
    let state = window.state::<Mutex<super::Presentation>>();
    if !state.lock().expect("presentation state").set_recording(id, recording) { return Err(stale()); }
    Ok(())
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
            let result = bindings.rebind(&hotkey.hotkey, &previous, &mut Native { app: &app, request_permission: true });
            state.publish_binding(bindings.active);
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
    struct TestPort {
        keys: Vec<Gesture>,
        disk: String,
        store: Option<SettingsStore>,
        deny_tap: bool,
        fail_save: bool,
        fail_remove: Option<Gesture>,
    }
    impl Port for TestPort {
        fn register(&mut self, key: Gesture) -> Result<(), HotkeyError> {
            if key == Gesture::DoubleCommand && self.deny_tap {
                return Err(HotkeyError { code: "hotkeyPermissionRequired" });
            }
            if self.keys.contains(&key) { return Err(CONFLICT); }
            self.keys.push(key); Ok(())
        }
        fn unregister(&mut self, key: Gesture) -> Result<(), HotkeyError> {
            if self.fail_remove == Some(key) { return Err(FAILED); }
            self.keys.retain(|&item| item != key); Ok(())
        }
        fn save(&mut self, text: &str) -> Result<(), HotkeyError> {
            if self.fail_save { return Err(FAILED); }
            if let Some(store) = &self.store {
                store.update_hotkey(text.to_owned()).map_err(|_| FAILED)?;
            }
            self.disk = text.to_owned(); Ok(())
        }
    }

    #[test]
    fn status_reports_actual_binding_not_the_saved_double_command_preference() {
        let chord = gesture("Alt+Space").unwrap();
        assert_eq!(hotkey_snapshot("DoubleMeta".into(), Some(chord), false, false), HotkeySnapshot {
            status: "fallback", configured_hotkey: "DoubleMeta".into(), effective_hotkey: Some("Alt+Space".into()),
        });
        for (active, alive, stopped) in [(None, false, false), (Some(Gesture::DoubleCommand), false, false), (Some(chord), true, true)] {
            let state = hotkey_snapshot("DoubleMeta".into(), active, alive, stopped);
            assert_eq!(state.status, "unavailable"); assert_eq!(state.effective_hotkey, None);
        }
        assert_eq!(hotkey_snapshot("DoubleMeta".into(), Some(Gesture::DoubleCommand), true, false).status, "active");
        assert_eq!(hotkey_snapshot("Alt+Space".into(), Some(chord), false, false).status, "active");
    }

    #[test]
    fn cold_start_reads_the_last_committed_chord_after_conflict_and_retry() {
        use crate::settings::ThemePreference;
        use std::fs;

        let dir = std::env::temp_dir().canonicalize().unwrap()
            .join(format!("uipilot-mac-hotkey-restart-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let settings = SettingsStore::load(&dir).unwrap();
        settings.update_hotkey("Alt+Space".into()).unwrap();
        settings.set_theme_preference_with_revision(ThemePreference::Dark).unwrap();

        let candidate = gesture("Meta+K").unwrap();
        let mut port = TestPort { store: Some(settings), keys: vec![candidate], ..Default::default() };
        let mut bindings = Bindings::default();
        let original = bindings.initialize("Alt+Space", &mut port).unwrap();
        let path = dir.join("settings.json");
        let before = fs::read(&path).unwrap();
        assert_eq!(bindings.rebind("Meta+K", "Alt+Space", &mut port), Err(CONFLICT));
        assert_eq!(bindings.active, Some(original));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(SettingsStore::load(&dir).unwrap().snapshot().hotkey, "Alt+Space");

        port.keys.retain(|key| *key != candidate);
        bindings.rebind("Meta+K", "Alt+Space", &mut port).unwrap();
        assert_eq!(port.keys, [candidate]);
        drop(port);
        let restored = SettingsStore::load(&dir).unwrap().snapshot();
        assert_eq!(restored.theme, ThemePreference::Dark);
        assert_eq!(restored.hotkey, "Meta+K");
        let mut restarted = Bindings::default();
        let mut fresh_port = TestPort::default();
        assert_eq!(restarted.initialize(&restored.hotkey, &mut fresh_port), Ok(candidate));
        assert_eq!(fresh_port.keys, [candidate]);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_save_keeps_the_previous_os_registration_and_disk_value() {
        let mut state = Bindings::default();
        let mut port = TestPort::default();
        state.rebind("Alt+Space", "", &mut port).unwrap();
        let old = gesture("Alt+Space").unwrap();
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
        let old = gesture("Alt+Space").unwrap();
        let new = gesture("Meta+K").unwrap();
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
        let key = gesture("Alt+Space").unwrap();
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
        assert_eq!(state.action(42, ShortcutState::Pressed, None), Some(KeyAction::Show));
        assert_eq!(state.action(42, ShortcutState::Pressed, Some(1)), Some(KeyAction::Record(1)));
        assert_eq!(state.action(42, ShortcutState::Released, Some(1)), None);
        assert_eq!(state.action(41, ShortcutState::Pressed, Some(1)), None);
        assert!(state.stop_once());
        assert!(!state.stop_once());
        assert_eq!(state.action(42, ShortcutState::Pressed, Some(1)), None);
    }

    #[test]
    fn double_command_requires_successful_admission_and_save_before_retiring_old_key() {
        let mut port = TestPort::default();
        let mut bindings = Bindings::default();
        let old = gesture("Alt+Space").unwrap();
        bindings.rebind("Alt+Space", "", &mut port).unwrap();
        port.deny_tap = true;
        assert_eq!(bindings.rebind(DOUBLE_COMMAND, "Alt+Space", &mut port),
            Err(HotkeyError { code: "hotkeyPermissionRequired" }));
        assert_eq!(port.keys, [old]);
        assert_eq!(port.disk, "Alt+Space");
        port.deny_tap = false;
        port.fail_save = true;
        assert_eq!(bindings.rebind(DOUBLE_COMMAND, "Alt+Space", &mut port), Err(FAILED));
        assert_eq!(port.keys, [old]);
        port.fail_save = false;
        bindings.rebind(DOUBLE_COMMAND, "Alt+Space", &mut port).unwrap();
        assert_eq!(port.keys, [Gesture::DoubleCommand]);
        assert_eq!(port.disk, DOUBLE_COMMAND);
        bindings.rebind("Alt+Space", DOUBLE_COMMAND, &mut port).unwrap();
        assert_eq!(port.keys, [old]);
    }

    #[test]
    fn unsupported_double_taps_and_malformed_chords_are_rejected() {
        for value in ["Space", "DoubleCtrl", "DoubleAlt", "Ctrl+Shift+", "Alt+MediaPlayPause", "Alt+VolumeUp", ""] { assert_eq!(gesture(value), Err(INVALID)); }
        assert_eq!(gesture("Meta+K"), gesture("Command+K"));
    }
}
