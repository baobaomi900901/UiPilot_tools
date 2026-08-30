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
    use std::io::Cursor;

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

    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;
    const CF_HDROP: u32 = 15;
    const CF_UNICODETEXT: u32 = 13;
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    const BITMAPINFOHEADER_SIZE: usize = 40;
    const BITMAPV5HEADER_SIZE: usize = 124;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ClipboardFormat {
        Standard(u32),
        RegisteredPng,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ClipboardPayload {
        format: ClipboardFormat,
        bytes: Vec<u8>,
    }

    impl ClipboardPayload {
        fn standard(format: u32, bytes: Vec<u8>) -> Self {
            Self {
                format: ClipboardFormat::Standard(format),
                bytes,
            }
        }

        fn registered_png(bytes: Vec<u8>) -> Self {
            Self {
                format: ClipboardFormat::RegisteredPng,
                bytes,
            }
        }
    }

    struct ResolvedClipboardPayload {
        format: u32,
        bytes: Vec<u8>,
    }

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
        let (prepared, kind) = match &record.payload {
            ClipboardHistoryRecordPayload::Text { text } => (
                vec![ClipboardPayload::standard(
                    CF_UNICODETEXT,
                    text_clipboard_bytes(text)?,
                )],
                ClipboardHistoryPasteWrite::Text,
            ),
            ClipboardHistoryRecordPayload::Image { png, .. } => (
                image_clipboard_payloads(png)?,
                ClipboardHistoryPasteWrite::Image,
            ),
            ClipboardHistoryRecordPayload::Files { paths } => (
                vec![ClipboardPayload::standard(
                    CF_HDROP,
                    file_drop_clipboard_bytes(paths)?,
                )],
                ClipboardHistoryPasteWrite::Files,
            ),
        };
        let prepared = prepared
            .into_iter()
            .map(resolve_clipboard_payload)
            .collect::<Result<Vec<_>, _>>()?;
        let _guard = ClipboardGuard::open()?;
        unsafe { EmptyClipboard() }
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        for payload in prepared {
            set_clipboard_data(
                payload.format,
                MovableMemory::copy_from_bytes(&payload.bytes)?,
            )?;
        }
        Ok(kind)
    }

    fn resolve_clipboard_payload(
        payload: ClipboardPayload,
    ) -> Result<ResolvedClipboardPayload, ClipboardHistoryPasteError> {
        Ok(ResolvedClipboardPayload {
            format: match payload.format {
                ClipboardFormat::Standard(format) => format,
                ClipboardFormat::RegisteredPng => png_clipboard_format()?,
            },
            bytes: payload.bytes,
        })
    }

    fn png_clipboard_format() -> Result<u32, ClipboardHistoryPasteError> {
        let name = ['P' as u16, 'N' as u16, 'G' as u16, 0];
        let format = unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) };
        (format != 0)
            .then_some(format)
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)
    }

    fn image_clipboard_payloads(
        png: &[u8],
    ) -> Result<Vec<ClipboardPayload>, ClipboardHistoryPasteError> {
        let image = decode_png_rgba(png)?;
        Ok(vec![
            ClipboardPayload::registered_png(png.to_vec()),
            ClipboardPayload::standard(CF_DIBV5, dibv5_clipboard_bytes(&image)?),
            ClipboardPayload::standard(CF_DIB, dib_clipboard_bytes(&image)?),
        ])
    }

    struct DecodedImage {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    }

    fn decode_png_rgba(png: &[u8]) -> Result<DecodedImage, ClipboardHistoryPasteError> {
        let decoder = png::Decoder::new(Cursor::new(png));
        let mut reader = decoder
            .read_info()
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let output_size = reader
            .output_buffer_size()
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let mut bytes = vec![0; output_size];
        let info = reader
            .next_frame(&mut bytes)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let used = info.buffer_size();
        bytes.truncate(used);
        let rgba = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgba, png::BitDepth::Eight) => bytes,
            (png::ColorType::Rgb, png::BitDepth::Eight) => {
                let pixel_count = info
                    .width
                    .checked_mul(info.height)
                    .and_then(|pixels| usize::try_from(pixels).ok())
                    .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
                if bytes.len() != pixel_count.saturating_mul(3) {
                    return Err(ClipboardHistoryPasteError::ClipboardWriteFailed);
                }
                let mut rgba = Vec::with_capacity(pixel_count * 4);
                for rgb in bytes.chunks_exact(3) {
                    rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
                }
                rgba
            }
            _ => return Err(ClipboardHistoryPasteError::ClipboardWriteFailed),
        };
        let expected = info
            .width
            .checked_mul(info.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        if rgba.len() != expected || info.width == 0 || info.height == 0 {
            return Err(ClipboardHistoryPasteError::ClipboardWriteFailed);
        }
        Ok(DecodedImage {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    fn dibv5_clipboard_bytes(image: &DecodedImage) -> Result<Vec<u8>, ClipboardHistoryPasteError> {
        let width = i32::try_from(image.width)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let height = i32::try_from(image.height)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let stride = image_stride(image.width, 32)?;
        let image_size = stride
            .checked_mul(
                usize::try_from(image.height)
                    .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?,
            )
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let image_size_u32 = u32::try_from(image_size)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let mut bytes = vec![0; BITMAPV5HEADER_SIZE + image_size];
        let width_usize = usize::try_from(image.width)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let height_usize = usize::try_from(image.height)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        write_u32(&mut bytes, 0, BITMAPV5HEADER_SIZE as u32);
        write_i32(&mut bytes, 4, width);
        write_i32(&mut bytes, 8, -height);
        write_u16(&mut bytes, 12, 1);
        write_u16(&mut bytes, 14, 32);
        write_u32(&mut bytes, 16, BI_BITFIELDS);
        write_u32(&mut bytes, 20, image_size_u32);
        write_u32(&mut bytes, 40, 0x00ff_0000);
        write_u32(&mut bytes, 44, 0x0000_ff00);
        write_u32(&mut bytes, 48, 0x0000_00ff);
        write_u32(&mut bytes, 52, 0xff00_0000);
        for y in 0..height_usize {
            let source_row = y
                .checked_mul(width_usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
            let target_row = BITMAPV5HEADER_SIZE
                .checked_add(
                    y.checked_mul(stride)
                        .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?,
                )
                .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
            write_bgra_row(
                &image.rgba[source_row..source_row + width_usize * 4],
                &mut bytes[target_row..target_row + stride],
                true,
            );
        }
        Ok(bytes)
    }

    fn dib_clipboard_bytes(image: &DecodedImage) -> Result<Vec<u8>, ClipboardHistoryPasteError> {
        let width = i32::try_from(image.width)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let height = i32::try_from(image.height)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let stride = image_stride(image.width, 24)?;
        let image_size = stride
            .checked_mul(
                usize::try_from(image.height)
                    .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?,
            )
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let image_size_u32 = u32::try_from(image_size)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let mut bytes = vec![0; BITMAPINFOHEADER_SIZE + image_size];
        write_u32(&mut bytes, 0, BITMAPINFOHEADER_SIZE as u32);
        write_i32(&mut bytes, 4, width);
        write_i32(&mut bytes, 8, height);
        write_u16(&mut bytes, 12, 1);
        write_u16(&mut bytes, 14, 24);
        write_u32(&mut bytes, 16, BI_RGB);
        write_u32(&mut bytes, 20, image_size_u32);
        let width_usize = usize::try_from(image.width)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        let height_usize = usize::try_from(image.height)
            .map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        for target_y in 0..height_usize {
            let source_y = height_usize - 1 - target_y;
            let source_row = source_y
                .checked_mul(width_usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
            let target_row = BITMAPINFOHEADER_SIZE
                .checked_add(
                    target_y
                        .checked_mul(stride)
                        .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?,
                )
                .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)?;
            write_bgra_row(
                &image.rgba[source_row..source_row + width_usize * 4],
                &mut bytes[target_row..target_row + stride],
                false,
            );
        }
        Ok(bytes)
    }

    fn image_stride(
        width: u32,
        bits_per_pixel: usize,
    ) -> Result<usize, ClipboardHistoryPasteError> {
        let width =
            usize::try_from(width).map_err(|_| ClipboardHistoryPasteError::ClipboardWriteFailed)?;
        width
            .checked_mul(bits_per_pixel)
            .and_then(|bits| bits.checked_add(31))
            .map(|bits| (bits / 32) * 4)
            .ok_or(ClipboardHistoryPasteError::ClipboardWriteFailed)
    }

    fn write_bgra_row(source_rgba: &[u8], target: &mut [u8], include_alpha: bool) {
        let bytes_per_pixel = if include_alpha { 4 } else { 3 };
        for (index, rgba) in source_rgba.chunks_exact(4).enumerate() {
            let offset = index * bytes_per_pixel;
            target[offset..offset + 3].copy_from_slice(&[rgba[2], rgba[1], rgba[0]]);
            if include_alpha {
                target[offset + 3] = rgba[3];
            }
        }
    }

    fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn image_clipboard_payloads_publish_png_dibv5_and_compatible_dib() {
            let rgba = [
                255, 0, 0, 128, 0, 255, 0, 255, 0, 0, 255, 0, 0, 0, 0, 255, 255, 255, 255, 255, 1,
                2, 3, 255,
            ];
            let png = encode_test_png(3, 2, &rgba);

            let payloads = image_clipboard_payloads(&png).unwrap();

            assert_eq!(
                payloads
                    .iter()
                    .map(|payload| payload.format)
                    .collect::<Vec<_>>(),
                vec![
                    ClipboardFormat::RegisteredPng,
                    ClipboardFormat::Standard(CF_DIBV5),
                    ClipboardFormat::Standard(CF_DIB),
                ]
            );
            assert_eq!(payloads[0].bytes, png);

            let dibv5 = &payloads[1].bytes;
            assert_eq!(le_u32(dibv5, 0), 124);
            assert_eq!(le_i32(dibv5, 4), 3);
            assert_eq!(le_i32(dibv5, 8), -2);
            assert_eq!(le_u16(dibv5, 12), 1);
            assert_eq!(le_u16(dibv5, 14), 32);
            assert_eq!(le_u32(dibv5, 16), BI_BITFIELDS);
            assert_eq!(le_u32(dibv5, 20), 24);
            assert_eq!(le_u32(dibv5, 40), 0x00ff_0000);
            assert_eq!(le_u32(dibv5, 44), 0x0000_ff00);
            assert_eq!(le_u32(dibv5, 48), 0x0000_00ff);
            assert_eq!(le_u32(dibv5, 52), 0xff00_0000);
            assert_eq!(&dibv5[124..128], &[0, 0, 255, 128]);
            assert_eq!(&dibv5[128..132], &[0, 255, 0, 255]);

            let dib = &payloads[2].bytes;
            assert_eq!(le_u32(dib, 0), 40);
            assert_eq!(le_i32(dib, 4), 3);
            assert_eq!(le_i32(dib, 8), 2);
            assert_eq!(le_u16(dib, 12), 1);
            assert_eq!(le_u16(dib, 14), 24);
            assert_eq!(le_u32(dib, 16), BI_RGB);
            assert_eq!(le_u32(dib, 20), 24);
            assert_eq!(dib.len(), 40 + 24);
            assert_eq!(&dib[40..52], &[0, 0, 0, 255, 255, 255, 3, 2, 1, 0, 0, 0]);
            assert_eq!(&dib[52..64], &[0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 0, 0]);
        }

        #[test]
        fn invalid_png_fails_before_clipboard_is_opened() {
            assert_eq!(
                image_clipboard_payloads(b"not a png"),
                Err(ClipboardHistoryPasteError::ClipboardWriteFailed)
            );
        }

        fn encode_test_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, width, height);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().unwrap();
                writer.write_image_data(rgba).unwrap();
            }
            bytes
        }

        fn le_u16(bytes: &[u8], offset: usize) -> u16 {
            u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
        }

        fn le_u32(bytes: &[u8], offset: usize) -> u32 {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        }

        fn le_i32(bytes: &[u8], offset: usize) -> i32 {
            i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
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
