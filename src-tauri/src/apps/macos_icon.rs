//! Decode local application artwork without creating a window or launching an app.
use base64::{engine::general_purpose::STANDARD, Engine};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    data::{CFData, CFDataCreateMutable, CFDataRef, CFMutableDataRef},
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use objc2::{rc::Retained, AnyThread};
use objc2_app_kit::{NSBitmapImageRep, NSWorkspace};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::{path::Path, ptr};

use super::{ICON_LIMIT, MAX_PNG_BYTES};

const EDGE: usize = 64; // 28-point launcher icon, including Retina displays.
const MAX_SOURCE_EDGE: i64 = 4096;
const MAX_REPRESENTATIONS: usize = 64;

// Public ImageIO/CoreGraphics headers. All Create/Copy results are retained
// CF objects and are released by the wrappers before returning to the caller.
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: CFDataRef, options: CFDictionaryRef) -> CFTypeRef;
    fn CGImageSourceGetType(source: CFTypeRef) -> CFStringRef;
    fn CGImageSourceGetCount(source: CFTypeRef) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: CFTypeRef, index: usize, options: CFDictionaryRef) -> CFDictionaryRef;
    fn CGImageSourceCreateThumbnailAtIndex(source: CFTypeRef, index: usize, options: CFDictionaryRef) -> CFTypeRef;
    fn CGImageDestinationCreateWithData(data: CFMutableDataRef, kind: CFStringRef, count: usize, options: CFDictionaryRef) -> CFTypeRef;
    fn CGImageDestinationAddImage(destination: CFTypeRef, image: CFTypeRef, options: CFDictionaryRef);
    fn CGImageDestinationFinalize(destination: CFTypeRef) -> bool;
    static kCGImageSourceShouldCache: CFStringRef;
    static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
    static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    static kCGImagePropertyPixelWidth: CFStringRef;
    static kCGImagePropertyPixelHeight: CFStringRef;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: CFTypeRef) -> usize;
    fn CGImageGetHeight(image: CFTypeRef) -> usize;
}

unsafe fn owned(value: CFTypeRef) -> Option<CFType> {
    if value.is_null() { None } else { Some(unsafe { CFType::wrap_under_create_rule(value) }) }
}

fn complete_icns(bytes: &[u8]) -> bool {
    if bytes.len() < 8 || bytes[..4] != *b"icns"
        || u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize != bytes.len() { return false; }
    let mut remaining = &bytes[8..];
    let mut count = 0;
    while !remaining.is_empty() {
        if remaining.len() < 8 || count >= 128 { return false; }
        let size = u32::from_be_bytes(remaining[4..8].try_into().unwrap()) as usize;
        if size < 8 || size > remaining.len() { return false; }
        remaining = &remaining[size..];
        count += 1;
    }
    count > 0
}

pub(super) fn asset(path: &Path) -> Option<String> {
    let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path.to_str()?));
    // Ask for a single raster representation. TIFFRepresentation on the entire
    // NSImage can expand dozens of resolutions/appearance variants into 35+ MB.
    let mut rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(EDGE as f64, EDGE as f64));
    // SAFETY: the proposed rect is live, no context or hints are passed, and the
    // retained CGImage stays alive throughout the dimension check and conversion.
    let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
    let raw = Retained::as_ptr(&cg).cast();
    let (width, height) = unsafe { (CGImageGetWidth(raw), CGImageGetHeight(raw)) };
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) { return None; }
    let bitmap = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg);
    let data = bitmap.TIFFRepresentation()?;
    if data.len() > ICON_LIMIT { return None; }
    decode(&data.to_vec())
}

pub(super) fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > ICON_LIMIT { return None; }
    if bytes.starts_with(b"icns") && !complete_icns(bytes) { return None; }
    // SAFETY: only public APIs, live CF objects and valid dictionary types are
    // passed. No graphics context, application activation or GUI is involved.
    unsafe {
        let data = CFData::from_buffer(bytes);
        let cache = CFDictionary::from_CFType_pairs(&[(
            CFString::wrap_under_get_rule(kCGImageSourceShouldCache), CFBoolean::false_value().as_CFType(),
        )]);
        let source = owned(CGImageSourceCreateWithData(data.as_concrete_TypeRef(), cache.as_concrete_TypeRef()))?;
        let kind = CGImageSourceGetType(source.as_CFTypeRef());
        if kind.is_null() || !matches!(CFString::wrap_under_get_rule(kind).to_string().as_str(),
            "com.apple.icns" | "public.png" | "public.jpeg" | "public.jpeg-2000" | "public.tiff") { return None; }
        let count = CGImageSourceGetCount(source.as_CFTypeRef());
        if count == 0 || count > MAX_REPRESENTATIONS { return None; }
        let mut best = None;
        for index in 0..count {
            let raw = CGImageSourceCopyPropertiesAtIndex(source.as_CFTypeRef(), index, cache.as_concrete_TypeRef());
            if raw.is_null() { continue; }
            let properties = CFDictionary::<CFString, CFType>::wrap_under_create_rule(raw);
            let dimension = |key| properties.find(&CFString::wrap_under_get_rule(key))
                .and_then(|v| v.downcast::<CFNumber>()).and_then(|v| v.to_i64());
            let (Some(width), Some(height)) = (dimension(kCGImagePropertyPixelWidth), dimension(kCGImagePropertyPixelHeight)) else { continue; };
            if !(1..=MAX_SOURCE_EDGE).contains(&width) || !(1..=MAX_SOURCE_EDGE).contains(&height) { continue; }
            let area = width * height;
            if best.is_none_or(|(_, size)| area > size) { best = Some((index, area)); }
        }
        let (index, _) = best?;
        let options = CFDictionary::from_CFType_pairs(&[
            (CFString::wrap_under_get_rule(kCGImageSourceCreateThumbnailFromImageAlways), CFBoolean::true_value().as_CFType()),
            (CFString::wrap_under_get_rule(kCGImageSourceThumbnailMaxPixelSize), CFNumber::from(EDGE as i32).as_CFType()),
            (CFString::wrap_under_get_rule(kCGImageSourceShouldCache), CFBoolean::false_value().as_CFType()),
        ]);
        let thumbnail = owned(CGImageSourceCreateThumbnailAtIndex(source.as_CFTypeRef(), index, options.as_concrete_TypeRef()))?;
        if !(1..=EDGE).contains(&CGImageGetWidth(thumbnail.as_CFTypeRef()))
            || !(1..=EDGE).contains(&CGImageGetHeight(thumbnail.as_CFTypeRef())) { return None; }
        let output = CFDataCreateMutable(ptr::null(), 0);
        if output.is_null() { return None; }
        // Retain mutable storage privately; read only after Finalize completes.
        let output_owner = CFData::wrap_under_create_rule(output);
        let png = CFString::from_static_string("public.png");
        let destination = owned(CGImageDestinationCreateWithData(output, png.as_concrete_TypeRef(), 1, ptr::null()))?;
        CGImageDestinationAddImage(destination.as_CFTypeRef(), thumbnail.as_CFTypeRef(), ptr::null());
        if !CGImageDestinationFinalize(destination.as_CFTypeRef()) || output_owner.len() as usize > MAX_PNG_BYTES { return None; }
        Some(format!("data:image/png;base64,{}", STANDARD.encode(output_owner.bytes())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(width, height, image::Rgba([36, 128, 220, 127])));
        let mut output = std::io::Cursor::new(Vec::new());
        image.write_to(&mut output, image::ImageFormat::Png).unwrap();
        output.into_inner()
    }

    #[test]
    fn oversized_dimensions_are_rejected_and_rgba_is_preserved() {
        assert!(decode(&png(MAX_SOURCE_EDGE as u32 + 1, 1)).is_none());
        let url = decode(&png(512, 256)).unwrap();
        let result = STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let result = image::load_from_memory(&result).unwrap().to_rgba8();
        assert_eq!(result.dimensions(), (64, 32));
        let pixel = result.get_pixel(32, 16).0;
        for (actual, expected) in pixel.into_iter().zip([36_u8, 128, 220, 127]) {
            assert!(actual.abs_diff(expected) <= 2, "alpha and color must survive normalization");
        }
    }

    #[test]
    fn malformed_icns_and_unsupported_content_are_rejected() {
        for bytes in [b"icns\0\0\0\x10ic08\0\0\0\0".as_slice(), b"icns\0\0\0\x10ic08\0\0\0\x20",
            b"icns\0\0\0\x08", b"<svg/>", b"not an icon"] {
            assert!(decode(bytes).is_none());
        }
        assert!(decode(&vec![0; ICON_LIMIT + 1]).is_none());
    }
}
