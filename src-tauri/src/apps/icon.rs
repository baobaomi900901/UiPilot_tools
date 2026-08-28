use std::{os::windows::ffi::OsStrExt, path::Path};

use windows::{
    core::{Interface, Owned, PCWSTR, PWSTR},
    Win32::{
        Foundation::SIZE,
        Graphics::{
            Gdi::{HBITMAP, HPALETTE},
            Imaging::{
                CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
                IWICBitmap, IWICImagingFactory, WICBitmapEncoderNoCache, WICBitmapUseAlpha,
                WICRect,
            },
        },
        Security::Cryptography::{
            CryptBinaryToStringW, CRYPT_STRING, CRYPT_STRING_BASE64, CRYPT_STRING_NOCRLF,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx,
            StructuredStorage::IPropertyBag2, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            STREAM_SEEK_CUR,
        },
        UI::Shell::{
            IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
            SIIGBF_THUMBNAILONLY,
        },
    },
};

const ICON_EDGE: i32 = 32;
const DATA_URL_PREFIX: &str = "data:image/png;base64,";
const MAX_DATA_URL_BYTES: usize = 65_536;
const MAX_PNG_BYTES: usize = 49_134;
const THUMBNAIL_EDGE: i32 = 256;
const MAX_THUMBNAIL_DATA_URL_BYTES: usize = 524_320;
const MAX_THUMBNAIL_PNG_BYTES: usize = 393_216;

struct ComApartment;

impl ComApartment {
    fn initialize() -> Option<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .ok()?;
        Some(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) fn from_shortcut(path: &Path) -> Option<String> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None::<&IBindCtx>) }.ok()?;
    from_shell_item(&item)
}

pub(super) fn from_shell_item(item: &IShellItem) -> Option<String> {
    let factory: IShellItemImageFactory = item.cast().ok()?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: ICON_EDGE,
                cy: ICON_EDGE,
            },
            SIIGBF_ICONONLY,
        )
    }
    .ok()?;
    with_owned_bitmap(bitmap, bitmap_png_data_url)
}

pub(crate) fn thumbnail_from_path(path: &Path) -> Option<String> {
    if !is_thumbnail_candidate(path) {
        return None;
    }
    let _apartment = ComApartment::initialize()?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None::<&IBindCtx>) }.ok()?;
    let factory: IShellItemImageFactory = item.cast().ok()?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: THUMBNAIL_EDGE,
                cy: THUMBNAIL_EDGE,
            },
            SIIGBF_THUMBNAILONLY,
        )
    }
    .ok()?;
    with_owned_bitmap(bitmap, |bitmap| {
        bitmap_png_data_url_with_limits(
            bitmap,
            THUMBNAIL_EDGE,
            MAX_THUMBNAIL_PNG_BYTES,
            MAX_THUMBNAIL_DATA_URL_BYTES,
            true,
        )
    })
}

fn is_thumbnail_candidate(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bmp" | "gif" | "heic" | "jpeg" | "jpg" | "png" | "svg" | "tif" | "tiff" | "webp"
            )
        })
}

fn with_owned_bitmap<T>(bitmap: HBITMAP, encode: impl FnOnce(HBITMAP) -> Option<T>) -> Option<T> {
    let bitmap = unsafe { Owned::new(bitmap) };
    encode(*bitmap)
}

fn bitmap_png_data_url(bitmap: HBITMAP) -> Option<String> {
    bitmap_png_data_url_with_limits(bitmap, ICON_EDGE, MAX_PNG_BYTES, MAX_DATA_URL_BYTES, false)
}

fn bitmap_png_data_url_with_limits(
    bitmap: HBITMAP,
    edge: i32,
    max_png_bytes: usize,
    max_data_url_bytes: usize,
    trim_transparency: bool,
) -> Option<String> {
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let source =
        unsafe { factory.CreateBitmapFromHBITMAP(bitmap, HPALETTE::default(), WICBitmapUseAlpha) }
            .ok()?;
    let mut width = 0_u32;
    let mut height = 0_u32;
    unsafe { source.GetSize(&mut width, &mut height) }.ok()?;
    if width == 0 || height == 0 || width > edge as u32 || height > edge as u32 {
        return None;
    }
    let crop = trim_transparency
        .then(|| alpha_crop_rect(&source, width, height))
        .flatten();
    let (output_width, output_height) = crop.as_ref().map_or((width, height), |rect| {
        (rect.Width as u32, rect.Height as u32)
    });
    let mut bytes = vec![0_u8; max_png_bytes];
    let buffer_length = u32::try_from(bytes.len()).ok()?;
    let written = {
        let stream = unsafe { factory.CreateStream() }.ok()?;
        // SAFETY: the pointer comes from the mutable Vec allocation WIC writes into. The Vec is
        // neither moved, resized, nor accessed until frame, encoder, and stream drop at block end.
        unsafe {
            (Interface::vtable(&stream).InitializeFromMemory)(
                Interface::as_raw(&stream),
                bytes.as_mut_ptr().cast_const(),
                buffer_length,
            )
        }
        .ok()
        .ok()?;
        let encoder =
            unsafe { factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()) }.ok()?;
        unsafe { encoder.Initialize(&*stream, WICBitmapEncoderNoCache) }.ok()?;
        let mut frame = None;
        unsafe { encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()) }.ok()?;
        let frame = frame?;
        unsafe { frame.Initialize(None::<&IPropertyBag2>) }.ok()?;
        unsafe { frame.SetSize(output_width, output_height) }.ok()?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        unsafe { frame.SetPixelFormat(&mut format) }.ok()?;
        if format != GUID_WICPixelFormat32bppBGRA {
            return None;
        }
        let crop = crop.as_ref().map_or(std::ptr::null(), std::ptr::from_ref);
        unsafe { frame.WriteSource(&source, crop) }.ok()?;
        unsafe { frame.Commit() }.ok()?;
        unsafe { encoder.Commit() }.ok()?;
        let mut written = 0_u64;
        unsafe { stream.Seek(0, STREAM_SEEK_CUR, Some(&mut written)) }.ok()?;
        usize::try_from(written)
            .ok()
            .filter(|length| (1..=max_png_bytes).contains(length))?
    };
    png_data_url_with_limits(&bytes[..written], max_png_bytes, max_data_url_bytes)
}

fn alpha_crop_rect(source: &IWICBitmap, width: u32, height: u32) -> Option<WICRect> {
    if unsafe { source.GetPixelFormat() }.ok()? != GUID_WICPixelFormat32bppBGRA {
        return None;
    }
    let stride = width.checked_mul(4)?;
    let length = usize::try_from(stride.checked_mul(height)?).ok()?;
    let mut pixels = vec![0_u8; length];
    unsafe { source.CopyPixels(std::ptr::null(), stride, &mut pixels) }.ok()?;

    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    for y in 0..height {
        for x in 0..width {
            let alpha = usize::try_from(y.checked_mul(stride)?.checked_add(x.checked_mul(4)?)?)
                .ok()?
                .checked_add(3)?;
            if pixels[alpha] == 0 {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if min_x == width || (min_x == 0 && min_y == 0 && max_x == width - 1 && max_y == height - 1) {
        return None;
    }
    Some(WICRect {
        X: min_x as i32,
        Y: min_y as i32,
        Width: (max_x - min_x + 1) as i32,
        Height: (max_y - min_y + 1) as i32,
    })
}

#[cfg(test)]
fn png_data_url(png: &[u8]) -> Option<String> {
    png_data_url_with_limits(png, MAX_PNG_BYTES, MAX_DATA_URL_BYTES)
}

fn png_data_url_with_limits(
    png: &[u8],
    max_png_bytes: usize,
    max_data_url_bytes: usize,
) -> Option<String> {
    if png.is_empty() || png.len() > max_png_bytes {
        return None;
    }
    let flags = CRYPT_STRING(CRYPT_STRING_BASE64.0 | CRYPT_STRING_NOCRLF);
    let mut length = 0_u32;
    if !unsafe { CryptBinaryToStringW(png, flags, None, &mut length) }.as_bool() {
        return None;
    }
    let mut encoded = vec![0_u16; length as usize];
    if !unsafe { CryptBinaryToStringW(png, flags, Some(PWSTR(encoded.as_mut_ptr())), &mut length) }
        .as_bool()
    {
        return None;
    }
    let end = encoded
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(encoded.len());
    let payload = String::from_utf16(&encoded[..end]).ok()?;
    let result = format!("{DATA_URL_PREFIX}{payload}");
    (result.len() <= max_data_url_bytes).then_some(result)
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, mem::size_of, path::Path};

    use windows::Win32::{
        Graphics::Gdi::{CreateBitmap, GetObjectW, BITMAP, HGDIOBJ},
        Security::Cryptography::{CryptStringToBinaryW, CRYPT_STRING_BASE64},
    };

    use super::{
        from_shortcut, is_thumbnail_candidate, png_data_url, thumbnail_from_path,
        with_owned_bitmap, MAX_PNG_BYTES,
    };

    fn decode_data_url(value: &str) -> Vec<u8> {
        let payload = value.strip_prefix(super::DATA_URL_PREFIX).unwrap();
        let payload = payload.encode_utf16().collect::<Vec<_>>();
        let mut length = 0;
        unsafe {
            CryptStringToBinaryW(&payload, CRYPT_STRING_BASE64, None, &mut length, None, None)
        }
        .unwrap();
        let mut decoded = vec![0; length as usize];
        unsafe {
            CryptStringToBinaryW(
                &payload,
                CRYPT_STRING_BASE64,
                Some(decoded.as_mut_ptr()),
                &mut length,
                None,
                None,
            )
        }
        .unwrap();
        decoded.truncate(length as usize);
        decoded
    }

    #[test]
    fn native_base64_is_bounded_and_has_no_line_breaks() {
        let accepted = png_data_url(&vec![0x5a; MAX_PNG_BYTES]).unwrap();
        assert!(accepted.starts_with("data:image/png;base64,"));
        assert!(accepted.len() <= 65_536);
        assert!(!accepted.contains('\r'));
        assert!(!accepted.contains('\n'));
        assert!(png_data_url(&[]).is_none());
        assert!(png_data_url(&vec![0x5a; MAX_PNG_BYTES + 1]).is_none());
    }

    #[test]
    fn owned_bitmap_is_deleted_after_success_and_failure() {
        for succeeds in [true, false] {
            let bitmap = unsafe { CreateBitmap(1, 1, 1, 32, None) };
            assert!(!bitmap.is_invalid());
            let raw = bitmap;
            let result = with_owned_bitmap(bitmap, |_| succeeds.then_some("encoded"));
            assert_eq!(result, succeeds.then_some("encoded"));
            let mut description = BITMAP::default();
            assert_eq!(
                unsafe {
                    GetObjectW(
                        HGDIOBJ(raw.0),
                        size_of::<BITMAP>() as i32,
                        Some((&mut description as *mut BITMAP).cast()),
                    )
                },
                0
            );
        }
    }

    #[test]
    fn missing_shortcut_has_no_icon() {
        assert_eq!(from_shortcut(Path::new(r"Z:\missing\UiPilot.lnk")), None);
    }

    #[test]
    fn thumbnail_candidates_match_the_find_image_extensions() {
        for extension in [
            "bmp", "gif", "heic", "jpeg", "jpg", "png", "svg", "tif", "tiff", "webp",
        ] {
            assert!(is_thumbnail_candidate(Path::new(&format!(
                "image.{extension}"
            ))));
            assert!(is_thumbnail_candidate(Path::new(&format!(
                "image.{}",
                extension.to_uppercase()
            ))));
        }
        for path in ["folder", "report.pdf", "video.mp4", "archive.zip"] {
            assert!(!is_thumbnail_candidate(Path::new(path)));
        }
    }

    #[test]
    fn shell_thumbnail_is_returned_as_a_bounded_png_data_url() {
        let path =
            std::env::temp_dir().join(format!("uipilot-find-thumbnail-{}.png", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .unwrap();
        drop(writer);

        let thumbnail = thumbnail_from_path(&path);
        let _ = std::fs::remove_file(&path);
        let thumbnail = thumbnail.expect("Windows Shell must thumbnail a valid PNG");
        assert!(thumbnail.starts_with("data:image/png;base64,"));
        assert!(thumbnail.len() <= super::MAX_THUMBNAIL_DATA_URL_BYTES);
    }

    #[test]
    fn shell_thumbnail_trims_asymmetric_transparent_padding() {
        let path = std::env::temp_dir().join(format!(
            "uipilot-find-thumbnail-padding-{}.png",
            std::process::id()
        ));
        let mut rgba = vec![0_u8; 4 * 4 * 4];
        for y in 2..4 {
            for x in 0..2 {
                let offset = (y * 4 + x) * 4;
                rgba[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, 4, 4);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&rgba).unwrap();
        drop(writer);

        let thumbnail =
            thumbnail_from_path(&path).expect("Windows Shell must thumbnail a valid PNG");
        let _ = std::fs::remove_file(&path);
        let mut reader = png::Decoder::new(Cursor::new(decode_data_url(&thumbnail)))
            .read_info()
            .unwrap();
        let mut output = vec![0_u8; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut output).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let width = info.width as usize;
        let height = info.height as usize;
        let mut bounds = (width, height, 0_usize, 0_usize);
        for y in 0..height {
            for x in 0..width {
                if output[(y * width + x) * 4 + 3] != 0 {
                    bounds.0 = bounds.0.min(x);
                    bounds.1 = bounds.1.min(y);
                    bounds.2 = bounds.2.max(x);
                    bounds.3 = bounds.3.max(y);
                }
            }
        }
        assert_eq!(bounds, (0, 0, width - 1, height - 1));
    }
}
