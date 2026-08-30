use std::{mem, path::PathBuf, ptr, slice};

use crate::lifecycle::ClipboardPasteTarget;

use super::{ClipboardHistoryPasteError, ClipboardHistoryRecord, ClipboardHistoryRecordPayload};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardHistoryPasteWrite {
    Text,
    Image,
    Files,
}

pub(crate) trait ClipboardHistoryPasteDriver {
    fn write_record(
        &self,
        record: &ClipboardHistoryRecord,
    ) -> Result<ClipboardHistoryPasteWrite, ClipboardHistoryPasteError>;

    fn send_ctrl_v(&self, target: ClipboardPasteTarget) -> bool;
}

pub(crate) fn paste_clipboard_history_record(
    record: &ClipboardHistoryRecord,
) -> Result<ClipboardHistoryPasteWrite, ClipboardHistoryPasteError> {
    platform::write_record(record)
}

pub(crate) fn send_ctrl_v_to_foreground_target(target: ClipboardPasteTarget) -> bool {
    platform::send_ctrl_v(target)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{HANDLE, HGLOBAL},
            System::{
                DataExchange::{
                    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
                    SetClipboardData,
                },
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            },
            UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                VIRTUAL_KEY, VK_CONTROL, VK_V,
            },
        },
    };

    const CF_HDROP: u32 = 15;
    const CF_UNICODETEXT: u32 = 13;

    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open() -> Result<Self, ClipboardHistoryPasteError> {
            unsafe { OpenClipboard(None) }
                .map(|()| Self)
                .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    struct MovableMemory {
        handle: HGLOBAL,
    }

    impl MovableMemory {
        fn copy_from_bytes(bytes: &[u8]) -> Result<Self, ClipboardHistoryPasteError> {
            if bytes.is_empty() {
                return Err(ClipboardHistoryPasteError::ClipboardWriteFailed);
            }
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
                .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
            let ptr = unsafe { GlobalLock(handle) };
            if ptr.is_null() {
                let _ = unsafe { GlobalUnlock(handle) };
                return Err(ClipboardHistoryPasteError::ClipboardWriteFailed);
            }
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            }
            let _ = unsafe { GlobalUnlock(handle) };
            Ok(Self { handle })
        }

        fn into_handle(self) -> HANDLE {
            let handle = self.handle;
            mem::forget(self);
            HANDLE(handle.0)
        }
    }

    fn set_clipboard_data(
        format: u32,
        memory: MovableMemory,
    ) -> Result<(), ClipboardHistoryPasteError> {
        unsafe { SetClipboardData(format, Some(memory.into_handle())) }
            .map(|_| ())
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)
    }

    pub(super) fn write_record(
        record: &ClipboardHistoryRecord,
    ) -> Result<ClipboardHistoryPasteWrite, ClipboardHistoryPasteError> {
        let prepared = match &record.payload {
            ClipboardHistoryRecordPayload::Text { text } => (
                CF_UNICODETEXT,
                text_clipboard_bytes(text)?,
                ClipboardHistoryPasteWrite::Text,
            ),
            ClipboardHistoryRecordPayload::Image { png, .. } => {
                let format = png_clipboard_format()?;
                (format, png.clone(), ClipboardHistoryPasteWrite::Image)
            }
            ClipboardHistoryRecordPayload::Files { paths } => (
                CF_HDROP,
                file_drop_clipboard_bytes(paths)?,
                ClipboardHistoryPasteWrite::Files,
            ),
        };
        let _guard = ClipboardGuard::open()?;
        unsafe { EmptyClipboard() }
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        set_clipboard_data(prepared.0, MovableMemory::copy_from_bytes(&prepared.1)?)?;
        Ok(prepared.2)
    }

    fn png_clipboard_format() -> Result<u32, ClipboardHistoryPasteError> {
        let name = ['P' as u16, 'N' as u16, 'G' as u16, 0];
        let format = unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) };
        (format != 0)
            .then_some(format)
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)
    }

    fn text_clipboard_bytes(text: &str) -> Result<Vec<u8>, ClipboardHistoryPasteError> {
        let mut words = text.encode_utf16().collect::<Vec<_>>();
        words.push(0);
        utf16_bytes(&words)
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct DropFilesHeader {
        p_files: u32,
        pt_x: i32,
        pt_y: i32,
        f_nc: i32,
        f_wide: i32,
    }

    fn file_drop_clipboard_bytes(paths: &[PathBuf]) -> Result<Vec<u8>, ClipboardHistoryPasteError> {
        if paths.is_empty() {
            return Err(ClipboardHistoryPasteError::RecordUnavailable);
        }
        let header = DropFilesHeader {
            p_files: mem::size_of::<DropFilesHeader>() as u32,
            pt_x: 0,
            pt_y: 0,
            f_nc: 0,
            f_wide: 1,
        };
        let mut bytes = unsafe {
            slice::from_raw_parts(
                (&header as *const DropFilesHeader).cast::<u8>(),
                mem::size_of::<DropFilesHeader>(),
            )
            .to_vec()
        };
        let mut words = Vec::new();
        for path in paths {
            let Some(path) = path.to_str() else {
                return Err(ClipboardHistoryPasteError::RecordUnavailable);
            };
            words.extend(path.encode_utf16());
            words.push(0);
        }
        words.push(0);
        bytes.extend(utf16_bytes(&words)?);
        Ok(bytes)
    }

    fn utf16_bytes(words: &[u16]) -> Result<Vec<u8>, ClipboardHistoryPasteError> {
        let byte_len = words
            .len()
            .checked_mul(mem::size_of::<u16>())
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let bytes = unsafe { slice::from_raw_parts(words.as_ptr().cast::<u8>(), byte_len) };
        Ok(bytes.to_vec())
    }

    pub(super) fn send_ctrl_v(_target: ClipboardPasteTarget) -> bool {
        let inputs = [
            key_input(VK_CONTROL, false),
            key_input(VK_V, false),
            key_input(VK_V, true),
            key_input(VK_CONTROL, true),
        ];
        unsafe { SendInput(&inputs, mem::size_of::<INPUT>() as i32) == inputs.len() as u32 }
    }

    fn key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: if key_up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub(super) fn write_record(
        _record: &ClipboardHistoryRecord,
    ) -> Result<ClipboardHistoryPasteWrite, ClipboardHistoryPasteError> {
        Err(ClipboardHistoryPasteError::ClipboardWriteFailed)
    }

    pub(super) fn send_ctrl_v(_target: ClipboardPasteTarget) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_driver_trait_keeps_native_effects_behind_a_narrow_boundary() {
        struct FakeDriver;

        impl ClipboardHistoryPasteDriver for FakeDriver {
            fn write_record(
                &self,
                record: &ClipboardHistoryRecord,
            ) -> Result<ClipboardHistoryPasteWrite, ClipboardHistoryPasteError> {
                match record.payload {
                    ClipboardHistoryRecordPayload::Text { .. } => {
                        Ok(ClipboardHistoryPasteWrite::Text)
                    }
                    _ => Err(ClipboardHistoryPasteError::RecordUnavailable),
                }
            }

            fn send_ctrl_v(&self, _target: ClipboardPasteTarget) -> bool {
                true
            }
        }

        let record = ClipboardHistoryRecord {
            id: "1".into(),
            captured_at: "2026-08-30T01:00:00Z".into(),
            payload: ClipboardHistoryRecordPayload::Text {
                text: "secret".into(),
            },
        };
        assert_eq!(
            FakeDriver.write_record(&record),
            Ok(ClipboardHistoryPasteWrite::Text)
        );
        assert!(FakeDriver.send_ctrl_v(ClipboardPasteTarget {
            show_generation: 1,
            hwnd: 2,
            pid: 3
        }));
    }
}
