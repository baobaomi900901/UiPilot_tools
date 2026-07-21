use std::sync::{Arc, Mutex, OnceLock};

use tauri::AppHandle;

#[cfg(all(not(test), not(feature = "test-instrumentation")))]
use crate::double_tap::DoubleTapDetector;
use crate::{double_tap::TapKey, hotkey::DoubleTapModifier};

struct HookState {
    #[cfg(all(not(test), not(feature = "test-instrumentation")))]
    detector: DoubleTapDetector,
    #[cfg(all(not(test), not(feature = "test-instrumentation")))]
    target: DoubleTapModifier,
    #[cfg(all(not(test), not(feature = "test-instrumentation")))]
    on_match: Arc<dyn Fn() + Send + Sync>,
}

static HOOK_STATE: OnceLock<Mutex<Option<HookState>>> = OnceLock::new();

fn hook_state() -> &'static Mutex<Option<HookState>> {
    HOOK_STATE.get_or_init(|| Mutex::new(None))
}

fn tap_key_from_vk(vk: u32) -> TapKey {
    match vk {
        0x11 | 0xA2 | 0xA3 => TapKey::Ctrl,
        0x12 | 0xA4 | 0xA5 => TapKey::Alt,
        _ => TapKey::Other,
    }
}

#[cfg(all(not(test), not(feature = "test-instrumentation")))]
unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use std::time::Instant;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    if code >= 0 {
        let message = wparam.0 as u32;
        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            let info = *(lparam.0 as *const KBDLLHOOKSTRUCT);
            let key = tap_key_from_vk(info.vkCode);
            let now = Instant::now();
            let mut state = hook_state().lock().expect("hook state lock poisoned");
            if let Some(inner) = state.as_mut() {
                if let Some(matched) = inner.detector.on_key_down(key, now) {
                    if matched == inner.target {
                        let callback = Arc::clone(&inner.on_match);
                        drop(state);
                        callback();
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[derive(Debug)]
pub(crate) struct HotkeyHook {
    #[cfg(all(not(test), not(feature = "test-instrumentation")))]
    handle: isize,
}

impl HotkeyHook {
    pub(crate) fn install(
        _app: &AppHandle,
        modifier: DoubleTapModifier,
        on_match: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, ()> {
        {
            let mut state = hook_state().lock().map_err(|_| ())?;
            *state = Some(HookState {
                #[cfg(all(not(test), not(feature = "test-instrumentation")))]
                detector: DoubleTapDetector::default(),
                #[cfg(all(not(test), not(feature = "test-instrumentation")))]
                target: modifier,
                #[cfg(all(not(test), not(feature = "test-instrumentation")))]
                on_match,
            });
        }

        #[cfg(any(test, feature = "test-instrumentation"))]
        {
            let _ = (_app, modifier, on_match);
            Ok(Self {})
        }

        #[cfg(all(not(test), not(feature = "test-instrumentation")))]
        {
            use windows::Win32::{
                Foundation::HINSTANCE,
                UI::WindowsAndMessaging::{SetWindowsHookExW, WH_KEYBOARD_LL},
            };
            let handle = match unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(keyboard_hook_proc),
                    Some(HINSTANCE::default()),
                    0,
                )
            } {
                Ok(handle) => handle,
                Err(_) => {
                    if let Ok(mut state) = hook_state().lock() {
                        *state = None;
                    }
                    return Err(());
                }
            };
            Ok(Self {
                handle: handle.0 as isize,
            })
        }
    }

    pub(crate) fn uninstall(self) {
        self.uninstall_internal();
    }

    fn uninstall_internal(self) {
        #[cfg(all(not(test), not(feature = "test-instrumentation")))]
        {
            use windows::Win32::UI::WindowsAndMessaging::{UnhookWindowsHookEx, HHOOK};
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(self.handle as *mut _));
            }
        }
        if let Ok(mut state) = hook_state().lock() {
            *state = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_key_mapping_recognizes_ctrl_and_alt_variants() {
        assert_eq!(tap_key_from_vk(0xA2), TapKey::Ctrl);
        assert_eq!(tap_key_from_vk(0xA5), TapKey::Alt);
        assert_eq!(tap_key_from_vk(0x41), TapKey::Other);
    }
}
