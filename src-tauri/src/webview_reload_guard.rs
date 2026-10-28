//! Block browser reload accelerators without changing application commands or navigation.

pub(crate) fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("reload-guard")
        .js_init_script_on_all_frames(include_str!("webview_reload_guard.js"))
        .on_webview_ready(|webview| {
            #[cfg(windows)]
            if webview
                .with_webview(|platform| {
                    if install_native_guard(platform.controller()).is_err() {
                        eprintln!("UiPilot: native reload shortcut guard unavailable; document guard remains active");
                    }
                })
                .is_err()
            {
                eprintln!("UiPilot: could not dispatch native reload shortcut guard installation");
            }
            #[cfg(not(windows))]
            let _ = webview;
        })
        .build()
}

#[cfg(any(windows, test))]
fn is_reload_key(key: u32, ctrl: bool, alt: bool, windows: bool) -> bool {
    // VK_F5, VK_BROWSER_REFRESH and Ctrl+R, including Shift/hard-reload variants.
    !alt && !windows && (matches!(key, 0x74 | 0xA8) || (ctrl && key == 0x52))
}

#[cfg(windows)]
fn install_native_guard(
    controller: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
) -> windows::core::Result<()> {
    use webview2_com::{
        AcceleratorKeyPressedEventHandler,
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        },
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
    };

    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(|_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        let mut key = 0;
        unsafe {
            args.KeyEventKind(&mut kind)?;
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                return Ok(());
            }
            args.VirtualKey(&mut key)?;
            if is_reload_key(
                key,
                GetKeyState(i32::from(VK_CONTROL.0)) < 0,
                GetKeyState(i32::from(VK_MENU.0)) < 0,
                GetKeyState(i32::from(VK_LWIN.0)) < 0 || GetKeyState(i32::from(VK_RWIN.0)) < 0,
            ) {
                args.SetHandled(true)?;
            }
        }
        Ok(())
    }));
    // The controller owns this subscription until its WebView is destroyed.
    let mut token = 0;
    unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_only_browser_reload_accelerators() {
        for ctrl in [false, true] {
            assert!(is_reload_key(0x74, ctrl, false, false));
            assert!(is_reload_key(0xA8, ctrl, false, false));
        }
        assert!(is_reload_key(0x52, true, false, false));
        assert!(!is_reload_key(0x52, false, false, false));
        for key in [0x52, 0x74, 0xA8] {
            assert!(!is_reload_key(key, true, true, false));
            assert!(!is_reload_key(key, true, false, true));
        }
        for key in [0x54, 0x43, 0x56, 0x58, 0x41, 0x53, 0x46, 0x7B, 0x09] {
            assert!(!is_reload_key(key, true, false, false));
        }
    }
}
