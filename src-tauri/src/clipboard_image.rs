/// Conservative temporary peak, including decoded pixels, DIBs and native allocations.
pub(crate) fn clipboard_workspace_bytes(png_len: usize, width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?
        .checked_mul(8)?
        .checked_add(png_len.checked_mul(3)?)?
        .checked_add(1024 * 1024)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardImageError {
    InvalidImage,
    Busy,
    WriteFailed,
}

fn publish_prepared<T, G>(
    prepared: Vec<T>,
    open: impl FnMut() -> Result<G, ClipboardImageError>,
    mut empty: impl FnMut() -> Result<(), ClipboardImageError>,
    mut set: impl FnMut(T) -> Result<(), ClipboardImageError>,
    wait: impl FnMut(),
) -> Result<(), ClipboardImageError> {
    let _guard = retry_open(open, wait)?;
    empty()?;
    for payload in prepared {
        set(payload)?;
    }
    Ok(())
}

fn retry_open<T>(
    mut open: impl FnMut() -> Result<T, ClipboardImageError>,
    mut wait: impl FnMut(),
) -> Result<T, ClipboardImageError> {
    for attempt in 0..5 {
        match open() {
            Err(ClipboardImageError::Busy) if attempt < 4 => wait(),
            result => return result,
        }
    }
    unreachable!()
}

pub(crate) fn copy_png(png: &[u8]) -> Result<(), ClipboardImageError> {
    #[cfg(windows)]
    {
        let payloads = platform::image_clipboard_payloads(png)
            .map_err(|_| ClipboardImageError::InvalidImage)?;
        platform::write_payloads(payloads)
    }
    #[cfg(not(windows))]
    {
        let _ = png;
        Err(ClipboardImageError::WriteFailed)
    }
}

#[cfg(windows)]
pub(crate) use platform::{write_payloads, ClipboardPayload};

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{io::Cursor, mem, ptr};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
            System::{
                DataExchange::{
                    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
                    SetClipboardData,
                },
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
            },
        },
    };
    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    const BITMAPINFOHEADER_SIZE: usize = 40;
    const BITMAPV5HEADER_SIZE: usize = 124;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum ClipboardFormat {
        Standard(u32),
        RegisteredPng,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct ClipboardPayload {
        format: ClipboardFormat,
        bytes: Vec<u8>,
    }

    impl ClipboardPayload {
        pub(crate) fn standard(format: u32, bytes: Vec<u8>) -> Self {
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

    struct ClipboardGuard(HWND);

    impl ClipboardGuard {
        fn open() -> Result<Self, ClipboardImageError> {
            // EmptyClipboard requires a non-null owner for SetClipboardData.
            let owner = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    windows::core::w!("STATIC"),
                    windows::core::w!(""),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    None,
                    None,
                )
            }
            .map_err(|_| ClipboardImageError::WriteFailed)?;
            unsafe { OpenClipboard(Some(owner)) }
                .map(|()| Self(owner))
                .map_err(|e| {
                    let _ = unsafe { DestroyWindow(owner) };
                    if e.code() == windows::core::HRESULT::from_win32(5) {
                        ClipboardImageError::Busy
                    } else {
                        ClipboardImageError::WriteFailed
                    }
                })
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    struct MovableMemory {
        handle: HGLOBAL,
    }

    impl MovableMemory {
        fn copy_from_bytes(bytes: &[u8]) -> Result<Self, ClipboardImageError> {
            if bytes.is_empty() {
                return Err(ClipboardImageError::WriteFailed);
            }
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
                .map_err(|_| ClipboardImageError::WriteFailed)?;
            let ptr = unsafe { GlobalLock(handle) };
            if ptr.is_null() {
                let _ = unsafe { GlobalFree(Some(handle)) };
                return Err(ClipboardImageError::WriteFailed);
            }
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            }
            let _ = unsafe { GlobalUnlock(handle) };
            Ok(Self { handle })
        }
    }

    impl Drop for MovableMemory {
        fn drop(&mut self) {
            let _ = unsafe { GlobalFree(Some(self.handle)) };
        }
    }
    fn set_clipboard_data(format: u32, memory: MovableMemory) -> Result<(), ClipboardImageError> {
        unsafe { SetClipboardData(format, Some(HANDLE(memory.handle.0))) }
            .map_err(|_| ClipboardImageError::WriteFailed)?;
        mem::forget(memory); // Windows owns it only after SetClipboardData succeeds.
        Ok(())
    }

    pub(crate) fn write_payloads(
        prepared: Vec<ClipboardPayload>,
    ) -> Result<(), ClipboardImageError> {
        let prepared = prepared
            .into_iter()
            .map(resolve_clipboard_payload)
            .collect::<Result<Vec<_>, _>>()?;
        // Allocate every native block before clearing the clipboard.
        let prepared = prepared
            .into_iter()
            .map(|p| Ok((p.format, MovableMemory::copy_from_bytes(&p.bytes)?)))
            .collect::<Result<Vec<_>, ClipboardImageError>>()?;
        publish_prepared(
            prepared,
            ClipboardGuard::open,
            || unsafe { EmptyClipboard() }.map_err(|_| ClipboardImageError::WriteFailed),
            |(format, memory)| set_clipboard_data(format, memory),
            || std::thread::sleep(std::time::Duration::from_millis(10)),
        )
    }

    fn resolve_clipboard_payload(
        payload: ClipboardPayload,
    ) -> Result<ResolvedClipboardPayload, ClipboardImageError> {
        Ok(ResolvedClipboardPayload {
            format: match payload.format {
                ClipboardFormat::Standard(format) => format,
                ClipboardFormat::RegisteredPng => png_clipboard_format()?,
            },
            bytes: payload.bytes,
        })
    }

    fn png_clipboard_format() -> Result<u32, ClipboardImageError> {
        let name = ['P' as u16, 'N' as u16, 'G' as u16, 0];
        let format = unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) };
        (format != 0)
            .then_some(format)
            .ok_or(ClipboardImageError::WriteFailed)
    }

    pub(crate) fn image_clipboard_payloads(
        png: &[u8],
    ) -> Result<Vec<ClipboardPayload>, ClipboardImageError> {
        let image = decode_png_rgba(png)?;
        Ok(vec![
            ClipboardPayload::registered_png(copy_bytes(png)?),
            ClipboardPayload::standard(CF_DIBV5, dibv5_clipboard_bytes(&image)?),
            ClipboardPayload::standard(CF_DIB, dib_clipboard_bytes(&image)?),
        ])
    }

    struct DecodedImage {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    }

    fn decode_png_rgba(png: &[u8]) -> Result<DecodedImage, ClipboardImageError> {
        let decoder = png::Decoder::new(Cursor::new(png));
        let mut reader = decoder
            .read_info()
            .map_err(|_| ClipboardImageError::WriteFailed)?;
        let output_size = reader
            .output_buffer_size()
            .ok_or(ClipboardImageError::WriteFailed)?;
        let mut bytes = zeroed(output_size)?;
        let info = reader
            .next_frame(&mut bytes)
            .map_err(|_| ClipboardImageError::WriteFailed)?;
        reader
            .finish()
            .map_err(|_| ClipboardImageError::WriteFailed)?;
        let used = info.buffer_size();
        bytes.truncate(used);
        let rgba = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgba, png::BitDepth::Eight) => bytes,
            (png::ColorType::Rgb, png::BitDepth::Eight) => {
                let pixel_count = info
                    .width
                    .checked_mul(info.height)
                    .and_then(|pixels| usize::try_from(pixels).ok())
                    .ok_or(ClipboardImageError::WriteFailed)?;
                if bytes.len() != pixel_count.saturating_mul(3) {
                    return Err(ClipboardImageError::WriteFailed);
                }
                let mut rgba = Vec::new();
                rgba.try_reserve_exact(
                    pixel_count
                        .checked_mul(4)
                        .ok_or(ClipboardImageError::WriteFailed)?,
                )
                .map_err(|_| ClipboardImageError::WriteFailed)?;
                for rgb in bytes.chunks_exact(3) {
                    rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
                }
                rgba
            }
            _ => return Err(ClipboardImageError::WriteFailed),
        };
        let expected = info
            .width
            .checked_mul(info.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(ClipboardImageError::WriteFailed)?;
        if rgba.len() != expected || info.width == 0 || info.height == 0 {
            return Err(ClipboardImageError::WriteFailed);
        }
        Ok(DecodedImage {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    fn dibv5_clipboard_bytes(image: &DecodedImage) -> Result<Vec<u8>, ClipboardImageError> {
        let width = i32::try_from(image.width).map_err(|_| ClipboardImageError::WriteFailed)?;
        let height = i32::try_from(image.height).map_err(|_| ClipboardImageError::WriteFailed)?;
        let stride = image_stride(image.width, 32)?;
        let image_size = stride
            .checked_mul(
                usize::try_from(image.height).map_err(|_| ClipboardImageError::WriteFailed)?,
            )
            .ok_or(ClipboardImageError::WriteFailed)?;
        let image_size_u32 =
            u32::try_from(image_size).map_err(|_| ClipboardImageError::WriteFailed)?;
        let mut bytes = zeroed(
            BITMAPV5HEADER_SIZE
                .checked_add(image_size)
                .ok_or(ClipboardImageError::WriteFailed)?,
        )?;
        let width_usize =
            usize::try_from(image.width).map_err(|_| ClipboardImageError::WriteFailed)?;
        let height_usize =
            usize::try_from(image.height).map_err(|_| ClipboardImageError::WriteFailed)?;
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
                .ok_or(ClipboardImageError::WriteFailed)?;
            let target_row = BITMAPV5HEADER_SIZE
                .checked_add(
                    y.checked_mul(stride)
                        .ok_or(ClipboardImageError::WriteFailed)?,
                )
                .ok_or(ClipboardImageError::WriteFailed)?;
            write_bgra_row(
                &image.rgba[source_row..source_row + width_usize * 4],
                &mut bytes[target_row..target_row + stride],
                true,
            );
        }
        Ok(bytes)
    }

    fn dib_clipboard_bytes(image: &DecodedImage) -> Result<Vec<u8>, ClipboardImageError> {
        let width = i32::try_from(image.width).map_err(|_| ClipboardImageError::WriteFailed)?;
        let height = i32::try_from(image.height).map_err(|_| ClipboardImageError::WriteFailed)?;
        let stride = image_stride(image.width, 24)?;
        let image_size = stride
            .checked_mul(
                usize::try_from(image.height).map_err(|_| ClipboardImageError::WriteFailed)?,
            )
            .ok_or(ClipboardImageError::WriteFailed)?;
        let image_size_u32 =
            u32::try_from(image_size).map_err(|_| ClipboardImageError::WriteFailed)?;
        let mut bytes = zeroed(
            BITMAPINFOHEADER_SIZE
                .checked_add(image_size)
                .ok_or(ClipboardImageError::WriteFailed)?,
        )?;
        write_u32(&mut bytes, 0, BITMAPINFOHEADER_SIZE as u32);
        write_i32(&mut bytes, 4, width);
        write_i32(&mut bytes, 8, height);
        write_u16(&mut bytes, 12, 1);
        write_u16(&mut bytes, 14, 24);
        write_u32(&mut bytes, 16, BI_RGB);
        write_u32(&mut bytes, 20, image_size_u32);
        let width_usize =
            usize::try_from(image.width).map_err(|_| ClipboardImageError::WriteFailed)?;
        let height_usize =
            usize::try_from(image.height).map_err(|_| ClipboardImageError::WriteFailed)?;
        for target_y in 0..height_usize {
            let source_y = height_usize - 1 - target_y;
            let source_row = source_y
                .checked_mul(width_usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or(ClipboardImageError::WriteFailed)?;
            let target_row = BITMAPINFOHEADER_SIZE
                .checked_add(
                    target_y
                        .checked_mul(stride)
                        .ok_or(ClipboardImageError::WriteFailed)?,
                )
                .ok_or(ClipboardImageError::WriteFailed)?;
            write_bgra_row(
                &image.rgba[source_row..source_row + width_usize * 4],
                &mut bytes[target_row..target_row + stride],
                false,
            );
        }
        Ok(bytes)
    }

    fn zeroed(size: usize) -> Result<Vec<u8>, ClipboardImageError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| ClipboardImageError::WriteFailed)?;
        bytes.resize(size, 0);
        Ok(bytes)
    }
    fn copy_bytes(source: &[u8]) -> Result<Vec<u8>, ClipboardImageError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(source.len())
            .map_err(|_| ClipboardImageError::WriteFailed)?;
        bytes.extend_from_slice(source);
        Ok(bytes)
    }

    fn image_stride(width: u32, bits_per_pixel: usize) -> Result<usize, ClipboardImageError> {
        let width = usize::try_from(width).map_err(|_| ClipboardImageError::WriteFailed)?;
        width
            .checked_mul(bits_per_pixel)
            .and_then(|bits| bits.checked_add(31))
            .map(|bits| (bits / 32) * 4)
            .ok_or(ClipboardImageError::WriteFailed)
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
                Err(ClipboardImageError::WriteFailed)
            );
        }

        #[test]
        fn truncated_iend_is_rejected_before_clipboard_access() {
            let mut png = encode_test_png(1, 1, &[1, 2, 3, 255]);
            png.truncate(png.len() - 1);
            // Stop at the pure preparation boundary in the red phase: never open
            // the real clipboard even if a malformed image is wrongly accepted.
            assert!(image_clipboard_payloads(&png).is_err());
            assert_eq!(copy_png(&png), Err(ClipboardImageError::InvalidImage));
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_format_failure_stops_and_closes_guard() {
        use std::{cell::RefCell, rc::Rc};
        struct Guard(Rc<RefCell<Vec<i32>>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.borrow_mut().push(99);
            }
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let result = publish_prepared(
            vec![1, 2, 3],
            || Ok(Guard(events.clone())),
            || {
                events.borrow_mut().push(0);
                Ok(())
            },
            |value| {
                events.borrow_mut().push(value);
                if value == 2 {
                    Err(ClipboardImageError::WriteFailed)
                } else {
                    Ok(())
                }
            },
            || {},
        );
        assert_eq!(result, Err(ClipboardImageError::WriteFailed));
        assert_eq!(*events.borrow(), [0, 1, 2, 99]);
    }
    #[test]
    fn invalid_png_is_rejected_before_native_access() {
        assert_eq!(copy_png(b"invalid"), Err(ClipboardImageError::InvalidImage));
    }
    #[test]
    fn busy_retry_is_bounded_and_does_not_retry_other_errors() {
        let mut attempts = 0;
        assert_eq!(
            retry_open(
                || {
                    attempts += 1;
                    Err::<(), _>(ClipboardImageError::Busy)
                },
                || {}
            ),
            Err(ClipboardImageError::Busy)
        );
        assert_eq!(attempts, 5);
        attempts = 0;
        assert_eq!(
            retry_open(
                || {
                    attempts += 1;
                    Err::<(), _>(ClipboardImageError::WriteFailed)
                },
                || {}
            ),
            Err(ClipboardImageError::WriteFailed)
        );
        assert_eq!(attempts, 1);
        attempts = 0;
        assert!(retry_open(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(ClipboardImageError::Busy)
                } else {
                    Ok(())
                }
            },
            || {}
        )
        .is_ok());
        assert_eq!(attempts, 3);
    }
}
