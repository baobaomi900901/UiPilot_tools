use std::{mem, path::PathBuf, slice};

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
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_V,
    };

    const CF_HDROP: u32 = 15;
    const CF_UNICODETEXT: u32 = 13;
    use crate::clipboard_image::{write_payloads, ClipboardPayload};

    pub(super) fn write_record(
        record: &ClipboardHistoryRecord,
    ) -> Result<ClipboardHistoryPasteWrite, ClipboardHistoryPasteError> {
        let (prepared, kind) = match &record.payload {
            ClipboardHistoryRecordPayload::Text { text } => (
                vec![ClipboardPayload::standard(
                    CF_UNICODETEXT,
                    text_clipboard_bytes(text)?,
                )],
                ClipboardHistoryPasteWrite::Text,
            ),
            ClipboardHistoryRecordPayload::Image { png, .. } => {
                crate::clipboard_image::copy_png(png)
                    .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
                return Ok(ClipboardHistoryPasteWrite::Image);
            }
            ClipboardHistoryRecordPayload::Files { paths } => (
                vec![ClipboardPayload::standard(
                    CF_HDROP,
                    file_drop_clipboard_bytes(paths)?,
                )],
                ClipboardHistoryPasteWrite::Files,
            ),
        };
        write_payloads(prepared).map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        Ok(kind)
    }

    #[cfg(test)]
    pub(super) fn check_text_and_file_payloads() {
        let text = text_clipboard_bytes("中文 text").unwrap();
        assert_eq!(
            text,
            "中文 text\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let paths = vec![PathBuf::from("C:\\中文 path\\image.png")];
        let files = file_drop_clipboard_bytes(&paths).unwrap();
        assert_eq!(u32::from_le_bytes(files[..4].try_into().unwrap()), 20);
        assert_eq!(
            &files[20..],
            "C:\\中文 path\\image.png\0\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert!(file_drop_clipboard_bytes(&[]).is_err());
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

#[cfg(all(test, windows))]
mod payload_tests {
    use super::*;
    #[test]
    fn text_and_file_payloads_keep_unicode_and_terminators() {
        platform::check_text_and_file_payloads();
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
