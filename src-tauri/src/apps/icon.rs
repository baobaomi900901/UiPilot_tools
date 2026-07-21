use std::{os::windows::ffi::OsStrExt, path::Path};

use windows::{
    core::{Interface, Owned, PCWSTR, PWSTR},
    Win32::{
        Foundation::SIZE,
        Graphics::{
            Gdi::{HBITMAP, HPALETTE},
            Imaging::{
                CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
                IWICImagingFactory, WICBitmapEncoderNoCache, WICBitmapUseAlpha,
            },
        },
        Security::Cryptography::{
            CryptBinaryToStringW, CRYPT_STRING, CRYPT_STRING_BASE64, CRYPT_STRING_NOCRLF,
        },
        System::Com::{
            CoCreateInstance, IBindCtx, StructuredStorage::IPropertyBag2, CLSCTX_INPROC_SERVER,
            STREAM_SEEK_CUR,
        },
        UI::Shell::{
            IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
        },
    },
};

const ICON_EDGE: i32 = 32;
const DATA_URL_PREFIX: &str = "data:image/png;base64,";
const MAX_DATA_URL_BYTES: usize = 65_536;
const MAX_PNG_BYTES: usize = 49_134;

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

fn with_owned_bitmap<T>(bitmap: HBITMAP, encode: impl FnOnce(HBITMAP) -> Option<T>) -> Option<T> {
    let bitmap = unsafe { Owned::new(bitmap) };
    encode(*bitmap)
}

fn bitmap_png_data_url(bitmap: HBITMAP) -> Option<String> {
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let source =
        unsafe { factory.CreateBitmapFromHBITMAP(bitmap, HPALETTE::default(), WICBitmapUseAlpha) }
            .ok()?;
    let bytes = vec![0_u8; MAX_PNG_BYTES];
    let written = {
        let stream = unsafe { factory.CreateStream() }.ok()?;
        unsafe { stream.InitializeFromMemory(&bytes) }.ok()?;
        let encoder =
            unsafe { factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()) }.ok()?;
        unsafe { encoder.Initialize(&*stream, WICBitmapEncoderNoCache) }.ok()?;
        let mut frame = None;
        unsafe { encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()) }.ok()?;
        let frame = frame?;
        unsafe { frame.Initialize(None::<&IPropertyBag2>) }.ok()?;
        unsafe { frame.SetSize(ICON_EDGE as u32, ICON_EDGE as u32) }.ok()?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        unsafe { frame.SetPixelFormat(&mut format) }.ok()?;
        if format != GUID_WICPixelFormat32bppBGRA {
            return None;
        }
        unsafe { frame.WriteSource(&source, std::ptr::null()) }.ok()?;
        unsafe { frame.Commit() }.ok()?;
        unsafe { encoder.Commit() }.ok()?;
        let mut written = 0_u64;
        unsafe { stream.Seek(0, STREAM_SEEK_CUR, Some(&mut written)) }.ok()?;
        usize::try_from(written)
            .ok()
            .filter(|length| (1..=MAX_PNG_BYTES).contains(length))?
    };
    png_data_url(&bytes[..written])
}

fn png_data_url(png: &[u8]) -> Option<String> {
    if png.is_empty() || png.len() > MAX_PNG_BYTES {
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
    (result.len() <= MAX_DATA_URL_BYTES).then_some(result)
}

#[cfg(test)]
mod tests {
    use std::{mem::size_of, path::Path};

    use windows::Win32::Graphics::Gdi::{CreateBitmap, GetObjectW, BITMAP, HGDIOBJ};

    use super::{from_shortcut, png_data_url, with_owned_bitmap, MAX_PNG_BYTES};

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
}
