//! Pure format selection; no system clipboard access or image validation.
use std::path::PathBuf;

use super::ClipboardCapture;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClipboardFormatSnapshot {
    pub(crate) files: Option<Vec<PathBuf>>,
    pub(crate) image: Option<ClipboardImageSnapshot>,
    pub(crate) text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClipboardImageSnapshot {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn normalize_clipboard_formats(
    formats: ClipboardFormatSnapshot,
    captured_at: impl Into<String>,
) -> Option<ClipboardCapture> {
    let captured_at = captured_at.into();
    if let Some(paths) = formats.files.filter(|paths| !paths.is_empty()) {
        return Some(ClipboardCapture::files(paths, captured_at));
    }
    if let Some(image) = formats.image {
        return Some(ClipboardCapture::image(
            image.rgba,
            image.width,
            image.height,
            captured_at,
        ));
    }
    formats
        .text
        .map(|text| ClipboardCapture::text(text, captured_at))
}

