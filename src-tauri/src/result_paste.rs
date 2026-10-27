//! One explicit result paste: preflight, consume/write under admission, then hide/send without locks.
use crate::public_plugins::result_action::{ResultActionErrorCode as E, ResultActionOutcome as O};

pub(crate) trait PasteDriver {
    fn preflight(&mut self) -> Result<(), E>;
    // Returns only after every manager/admission/registry lock has been released.
    fn admit_write(&mut self) -> Result<u32, O>;
    fn hide_restore(&mut self) -> Result<(), E>;
    // Native implementation repeats target, generation, foreground, keys and sequence guards here.
    fn send(&mut self, sequence: u32) -> Result<(), E>;
}
pub(crate) fn execute(driver: &mut impl PasteDriver) -> O {
    if let Err(code) = driver.preflight() {
        return O::failed(code, false);
    }
    let sequence = match driver.admit_write() {
        Ok(sequence) => sequence,
        Err(outcome) => return outcome,
    };
    if let Err(code) = driver.hide_restore() {
        return O::failed(code, true);
    }
    match driver.send(sequence) {
        Ok(()) => O::PasteSent,
        Err(code) => O::failed(code, true),
    }
}

pub(crate) fn wait_for_keys_with(
    mut released: impl FnMut() -> bool,
    mut pause: impl FnMut(),
) -> Result<(), E> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    for attempt in 0..=50 {
        if std::time::Instant::now() > deadline {
            return Err(E::PasteInputBusy);
        }
        if released() {
            return Ok(());
        }
        if attempt < 50 {
            pause();
        }
    }
    Err(E::PasteInputBusy)
}
pub(crate) fn wait_for_keys() -> Result<(), E> {
    wait_for_keys_with(native::keys_released, || {
        std::thread::sleep(std::time::Duration::from_millis(10))
    })
}

pub(crate) fn preflight(
    visible: bool,
    main_foreground: bool,
    lease_current: bool,
    target_valid: bool,
    released: bool,
) -> Result<(), E> {
    if !visible || !main_foreground || !lease_current || !target_valid {
        return Err(E::PasteTargetUnavailable);
    }
    if !released {
        return Err(E::PasteInputBusy);
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct SendGuard {
    pub(crate) generation_current: bool,
    pub(crate) target_valid: bool,
    pub(crate) foreground_matches: bool,
    pub(crate) keys_released: bool,
    pub(crate) sequence: u32,
}
fn send_once_with(guard: SendGuard, expected: u32, send: impl FnOnce() -> u32) -> Result<(), E> {
    if !guard.generation_current
        || !guard.target_valid
        || !guard.foreground_matches
        || !guard.keys_released
        || expected == 0
        || guard.sequence != expected
    {
        return Err(E::PasteFailed);
    }
    if send() == 4 {
        Ok(())
    } else {
        Err(E::PasteFailed)
    }
}
trait ClipboardDriver {
    fn open(&mut self) -> Result<(), ()>;
    fn set(&mut self, expected: &[u16]) -> Result<(), ()>;
    fn close(&mut self) -> Result<(), ()>;
    fn verify(&mut self, expected: &[u16]) -> Result<u32, ()>;
}
fn write_text_with(text: &str, driver: &mut impl ClipboardDriver) -> Result<u32, ()> {
    if text.is_empty() || text.len() > 8192 || text.contains('\0') {
        return Err(());
    }
    let expected: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    driver.open()?;
    driver.set(&expected)?;
    driver.close()?;
    // CloseClipboard may synthesize text formats. Read only our just-written text,
    // with its exact owner, after a single reopen; never read or restore prior data.
    driver.open()?;
    let sequence = driver.verify(&expected)?;
    driver.close()?;
    if sequence == 0 {
        return Err(());
    }
    Ok(sequence)
}

pub(crate) mod native {
    use super::*;
    use crate::lifecycle::ClipboardPasteTarget;
    use std::{ffi::c_void, mem, ptr};
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
                GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
            Threading::GetCurrentProcessId,
        },
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
                KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
                VK_SHIFT, VK_SPACE, VK_V,
            },
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsWindow},
        },
    };
    pub(crate) fn keys_released() -> bool {
        [
            VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_RETURN, VK_SPACE,
        ]
        .into_iter()
        .all(|key| unsafe { GetAsyncKeyState(key.0 as i32) } as u16 & 0x8000 == 0)
    }
    pub(crate) fn main_is_foreground(hwnd: isize) -> bool {
        hwnd != 0 && unsafe { GetForegroundWindow() }.0 as isize == hwnd
    }
    pub(crate) fn target_valid(target: ClipboardPasteTarget) -> bool {
        let hwnd = HWND(target.hwnd as *mut c_void);
        let mut pid = 0;
        target.hwnd != 0
            && unsafe { IsWindow(Some(hwnd)) }.as_bool()
            && unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) } != 0
            && pid == target.pid
            && pid != unsafe { GetCurrentProcessId() }
    }
    struct Clipboard(bool);
    impl Clipboard {
        fn open(hwnd: HWND) -> Result<Self, ()> {
            unsafe { OpenClipboard(Some(hwnd)) }
                .map(|_| Self(true))
                .map_err(|_| ())
        }
    }
    impl Clipboard {
        fn close(mut self) -> Result<(), ()> {
            self.0 = false; // Never retry an uncertain close, including in Drop.
            unsafe { CloseClipboard() }.map_err(|_| ())
        }
    }
    impl Drop for Clipboard {
        fn drop(&mut self) {
            if self.0 {
                let _ = unsafe { CloseClipboard() };
            }
        }
    }
    struct Memory(HGLOBAL);
    impl Drop for Memory {
        fn drop(&mut self) {
            let _ = unsafe { GlobalFree(Some(self.0)) };
        }
    }
    struct NativeClipboard {
        hwnd: HWND,
        guard: Option<Clipboard>,
    }
    impl ClipboardDriver for NativeClipboard {
        fn open(&mut self) -> Result<(), ()> {
            self.guard = Some(Clipboard::open(self.hwnd)?);
            Ok(())
        }
        fn set(&mut self, expected: &[u16]) -> Result<(), ()> {
            let bytes = std::mem::size_of_val(expected);
            let memory = Memory(unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }.map_err(|_| ())?);
            let address = unsafe { GlobalLock(memory.0) };
            if address.is_null() {
                return Err(());
            }
            unsafe {
                ptr::copy_nonoverlapping(expected.as_ptr(), address.cast::<u16>(), expected.len());
            }
            let _ = unsafe { GlobalUnlock(memory.0) };
            unsafe { EmptyClipboard() }.map_err(|_| ())?;
            unsafe { SetClipboardData(13, Some(HANDLE(memory.0 .0))) }.map_err(|_| ())?;
            mem::forget(memory);
            Ok(())
        }
        fn close(&mut self) -> Result<(), ()> {
            self.guard.take().ok_or(())?.close()
        }
        fn verify(&mut self, expected: &[u16]) -> Result<u32, ()> {
            if unsafe { GetClipboardOwner() }.map_err(|_| ())? != self.hwnd {
                return Err(());
            }
            let data = unsafe { GetClipboardData(13) }.map_err(|_| ())?;
            let global = HGLOBAL(data.0);
            if unsafe { GlobalSize(global) } < std::mem::size_of_val(expected) {
                return Err(());
            }
            let address = unsafe { GlobalLock(global) };
            if address.is_null() {
                return Err(());
            }
            let matches =
                unsafe { std::slice::from_raw_parts(address.cast::<u16>(), expected.len()) }
                    == expected;
            let _ = unsafe { GlobalUnlock(global) };
            if !matches {
                return Err(());
            }
            Ok(unsafe { GetClipboardSequenceNumber() })
        }
    }
    pub(crate) fn write_text(main: isize, text: &str) -> Result<u32, ()> {
        if main == 0 {
            return Err(());
        }
        write_text_with(
            text,
            &mut NativeClipboard {
                hwnd: HWND(main as *mut c_void),
                guard: None,
            },
        )
    }
    pub(crate) fn send(
        target: ClipboardPasteTarget,
        sequence: u32,
        generation_current: bool,
    ) -> Result<(), E> {
        let guard = SendGuard {
            generation_current,
            target_valid: target_valid(target),
            foreground_matches: main_is_foreground(target.hwnd),
            keys_released: keys_released(),
            sequence: unsafe { GetClipboardSequenceNumber() },
        };
        let key = |key: VIRTUAL_KEY, up| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        };
        let batch = [
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ];
        send_once_with(guard, sequence, || unsafe {
            SendInput(&batch, mem::size_of::<INPUT>() as i32)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Driver {
        steps: Vec<&'static str>,
        fail: Option<&'static str>,
        locked: bool,
        consumed: bool,
    }
    impl PasteDriver for Driver {
        fn preflight(&mut self) -> Result<(), E> {
            self.steps.push("preflight");
            if self.fail == Some("preflight") {
                Err(E::PasteTargetUnavailable)
            } else {
                Ok(())
            }
        }
        fn admit_write(&mut self) -> Result<u32, O> {
            self.steps.push("claim");
            if self.fail == Some("admission") {
                return Err(O::failed(E::StaleRequest, false));
            }
            self.consumed = true;
            self.locked = true;
            self.steps.push("write");
            self.locked = false;
            if self.fail == Some("write") {
                Err(O::failed(E::ClipboardWriteFailed, true))
            } else {
                Ok(42)
            }
        }
        fn hide_restore(&mut self) -> Result<(), E> {
            assert!(!self.locked);
            self.steps.push("hide");
            if self.fail == Some("hide") {
                Err(E::PasteFailed)
            } else {
                Ok(())
            }
        }
        fn send(&mut self, sequence: u32) -> Result<(), E> {
            assert!(!self.locked);
            assert_eq!(sequence, 42);
            self.steps.push("send");
            if self.fail == Some("send") {
                Err(E::PasteFailed)
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn result_paste_order_and_commit_failures() {
        for (fail, steps, consumed) in [
            (
                None,
                vec!["preflight", "claim", "write", "hide", "send"],
                true,
            ),
            (Some("preflight"), vec!["preflight"], false),
            (Some("admission"), vec!["preflight", "claim"], false),
            (Some("write"), vec!["preflight", "claim", "write"], true),
            (
                Some("hide"),
                vec!["preflight", "claim", "write", "hide"],
                true,
            ),
            (
                Some("send"),
                vec!["preflight", "claim", "write", "hide", "send"],
                true,
            ),
        ] {
            let mut driver = Driver {
                fail,
                ..Default::default()
            };
            let outcome = execute(&mut driver);
            assert_eq!(driver.steps, steps);
            assert_eq!(driver.consumed, consumed);
            match outcome {
                O::PasteSent => assert!(fail.is_none()),
                O::Failed {
                    source_consumed, ..
                } => assert_eq!(source_consumed, consumed),
                _ => panic!("wrong outcome"),
            }
        }
    }
    #[test]
    fn result_paste_wait_is_bounded_and_has_no_effect() {
        let mut pauses = 0;
        assert_eq!(
            wait_for_keys_with(|| false, || pauses += 1),
            Err(E::PasteInputBusy)
        );
        assert_eq!(pauses, 50);
        let mut observations = 0;
        assert_eq!(
            wait_for_keys_with(
                || {
                    observations += 1;
                    observations == 3
                },
                || {}
            ),
            Ok(())
        );
        assert_eq!(observations, 3);
    }
    #[test]
    fn result_paste_final_guards_and_partial_input_never_retry() {
        let valid = SendGuard {
            generation_current: true,
            target_valid: true,
            foreground_matches: true,
            keys_released: true,
            sequence: 42,
        };
        for guard in [
            SendGuard {
                generation_current: false,
                ..valid
            },
            SendGuard {
                target_valid: false,
                ..valid
            },
            SendGuard {
                foreground_matches: false,
                ..valid
            },
            SendGuard {
                keys_released: false,
                ..valid
            },
            SendGuard {
                sequence: 43,
                ..valid
            },
            SendGuard {
                sequence: 0,
                ..valid
            },
        ] {
            assert_eq!(
                send_once_with(guard, 42, || panic!("must not send")),
                Err(E::PasteFailed)
            );
        }
        for inserted in 0..=4 {
            let mut calls = 0;
            let result = send_once_with(valid, 42, || {
                calls += 1;
                inserted
            });
            assert_eq!(result.is_ok(), inserted == 4);
            assert_eq!(calls, 1);
        }
    }
    struct ClipboardMock {
        steps: Vec<&'static str>,
        fail_at: usize,
        sequence: u32,
    }
    impl ClipboardMock {
        fn step(&mut self, name: &'static str) -> Result<(), ()> {
            self.steps.push(name);
            if self.steps.len() == self.fail_at {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    impl ClipboardDriver for ClipboardMock {
        fn open(&mut self) -> Result<(), ()> {
            self.step("open")
        }
        fn close(&mut self) -> Result<(), ()> {
            self.step("close")
        }
        fn set(&mut self, expected: &[u16]) -> Result<(), ()> {
            assert_eq!(expected, [0x6c49, 0x5b57, 0]);
            self.step("set")
        }
        fn verify(&mut self, expected: &[u16]) -> Result<u32, ()> {
            assert_eq!(expected, [0x6c49, 0x5b57, 0]);
            self.step("verify")?;
            Ok(self.sequence)
        }
    }
    #[test]
    fn result_paste_clipboard_transaction_closes_writer_then_verifies_once() {
        let all = ["open", "set", "close", "open", "verify", "close"];
        for fail_at in 0..=6 {
            let mut driver = ClipboardMock {
                steps: vec![],
                fail_at,
                sequence: 42,
            };
            let result = write_text_with("汉字", &mut driver);
            assert_eq!(result.is_ok(), fail_at == 0);
            assert_eq!(
                driver.steps,
                if fail_at == 0 {
                    &all[..]
                } else {
                    &all[..fail_at]
                }
            );
        }
        let mut driver = ClipboardMock {
            steps: vec![],
            fail_at: 0,
            sequence: 0,
        };
        assert_eq!(write_text_with("汉字", &mut driver), Err(()));
        for text in ["", "x\0y"] {
            driver.steps.clear();
            assert_eq!(write_text_with(text, &mut driver), Err(()));
            assert!(driver.steps.is_empty());
        }
    }

    #[test]
    fn result_paste_queued_preflight_rejects_new_show_focus_and_closed_target() {
        for flags in [
            [false, true, true, true, true],
            [true, false, true, true, true],
            [true, true, false, true, true],
            [true, true, true, false, true],
            [true, true, true, true, false],
        ] {
            assert!(preflight(flags[0], flags[1], flags[2], flags[3], flags[4]).is_err());
        }
        assert_eq!(preflight(true, true, true, true, true), Ok(()));
    }
}
