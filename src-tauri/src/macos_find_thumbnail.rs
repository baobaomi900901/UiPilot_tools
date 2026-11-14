//! Bounded PNG/JPEG preview of an already authorized, still-current Find result.
use super::*;
use base64::Engine;
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::{Cursor, Read};

const MAX_INPUT: u64 = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
const MAX_EDGE: u32 = 16_384;
const THUMB_EDGE: u32 = 256;

// SDK sys/resource.h: per-thread policy prevents even a racing dataless replacement
// from initiating FileProvider hydration. Restore the worker thread's previous policy.
unsafe extern "C" {
    fn getiopolicy_np(kind: i32, scope: i32) -> i32;
    fn setiopolicy_np(kind: i32, scope: i32, policy: i32) -> i32;
}
struct NoHydration(i32);
impl NoHydration {
    fn enter() -> Result<Self, FindError> {
        let old = unsafe { getiopolicy_np(3, 1) };
        if old < 0 || unsafe { setiopolicy_np(3, 1, 1) } != 0 { return Err(FindError::Denied); }
        Ok(Self(old))
    }
}
impl Drop for NoHydration {
    fn drop(&mut self) { unsafe { setiopolicy_np(3, 1, self.0); } }
}

fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.len() as u64 > MAX_INPUT { return None; }
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) { return None; }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS { return None; }
    let orientation = decoder.orientation().ok()?;
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?.thumbnail(THUMB_EDGE, THUMB_EDGE);
    image.apply_orientation(orientation);
    // Normalize to bounded 8-bit RGBA PNG; never expose original bytes or file URLs.
    let image = image::DynamicImage::ImageRgba8(image.to_rgba8());
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).ok()?;
    let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(output.into_inner()));
    (url.len() <= 524_320).then_some(url)
}

pub(crate) fn load(scope: &IndexedLocalScope, target: &AuthorizedFile, current: impl Fn() -> bool)
    -> Result<Option<String>, FindError> {
    let _no_hydration = NoHydration::enter()?;
    if target.identity.is_directory() || target.identity.size < 1 || target.identity.size as u64 > MAX_INPUT
        || !target.path.extension().and_then(|s| s.to_str()).is_some_and(|s|
            ["png", "jpg", "jpeg"].iter().any(|ext| s.eq_ignore_ascii_case(ext))) {
        return Ok(None);
    }
    // Reuse the action's full scope, mount, native-reference and admission checks.
    // This only borrows authority; it never dispatches an OS action or consumes a row.
    let bytes = submit_with(scope, target, FindAction::Default, &current, |_, _| {
        let root = target.root.current_fd()?;
        let mut file = target.identity.open_read(&root).map_err(|_| FindError::Stale)?;
        let mut bytes = Vec::new();
        (&mut file).take(MAX_INPUT + 1).read_to_end(&mut bytes).map_err(|_| FindError::Denied)?;
        target.identity.check_open_file(&file).map_err(|_| FindError::Stale)?;
        target.identity.revalidate(&root).map_err(|_| FindError::Stale)?;
        if bytes.len() as u64 > MAX_INPUT { return Err(FindError::Stale); }
        Ok(bytes)
    })??;
    if !current() { return Err(FindError::Stale); }
    let thumbnail = decode(&bytes);
    // Reject changes/revocation during decoding, and only return into the same query.
    submit_with(scope, target, FindAction::Default, current, |_, _| thumbnail)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_and_jpeg_are_bounded_and_normalized() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg] {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1200, 600, image::Rgb([80, 140, 210])));
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, format).unwrap();
            let url = decode(encoded.get_ref()).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
            let result = image::load_from_memory(&bytes).unwrap();
            assert_eq!((result.width(), result.height()), (256, 128));
        }
    }
    #[test]
    fn noisy_rgba_output_fits_the_shared_ipc_limit() {
        let mut seed = 42_u32;
        let mut pixels = vec![0; 256 * 256 * 4];
        for byte in &mut pixels {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (seed >> 24) as u8;
        }
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(256, 256, pixels).unwrap());
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(decode(encoded.get_ref()).unwrap().len() <= 524_320);
    }

    #[test]
    fn corrupt_unsupported_and_excessive_dimensions_return_no_preview() {
        assert!(decode(b"not an image").is_none());
        assert!(decode(b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_none());
        let mut bytes = Vec::new();
        let writer = png::Encoder::new(&mut bytes, MAX_EDGE + 1, 1).write_header().unwrap();
        drop(writer);
        assert!(decode(&bytes).is_none());
    }
    #[test]
    fn hydration_policy_is_restored() {
        let old = unsafe { getiopolicy_np(3, 1) };
        { let _guard = NoHydration::enter().unwrap(); assert_eq!(unsafe { getiopolicy_np(3, 1) }, 1); }
        assert_eq!(unsafe { getiopolicy_np(3, 1) }, old);
    }
}
