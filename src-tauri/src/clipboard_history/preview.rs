use sha2::{Digest, Sha256};

use super::model::{ClipboardHistoryError, MAX_THUMBNAIL_PNG_BYTES, THUMBNAIL_MAX_EDGE};

const DATA_URL_PREFIX: &str = "data:image/png;base64,";

pub(super) struct PreparedImage {
    pub(super) png: Vec<u8>,
    pub(super) thumbnail_data_url: String,
    pub(super) thumbnail_width: u32,
    pub(super) thumbnail_height: u32,
}

pub(super) fn text_preview(text: &str) -> String {
    let mut preview = String::new();
    let mut pending_space = false;
    let mut saw_content = false;

    for character in text.chars() {
        if character.is_whitespace() {
            if saw_content {
                pending_space = true;
            }
            continue;
        }
        if pending_space && preview.chars().count() < 120 {
            preview.push(' ');
        }
        pending_space = false;
        saw_content = true;
        if preview.chars().count() == 120 {
            break;
        }
        preview.push(character);
    }

    preview
}

pub(super) fn text_fingerprint(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"text\0");
    hasher.update(text.as_bytes());
    hex_digest(hasher.finalize().as_slice())
}

pub(super) fn image_fingerprint(width: u32, height: u32, rgba: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"image\0rgba8\0");
    hasher.update(width.to_le_bytes());
    hasher.update(height.to_le_bytes());
    hasher.update(rgba);
    hex_digest(hasher.finalize().as_slice())
}

pub(super) fn files_fingerprint(paths: &[std::path::PathBuf]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"files\0");
    for path in paths {
        hasher.update(normalize_windows_path(path).as_bytes());
        hasher.update(b"\0");
    }
    hex_digest(hasher.finalize().as_slice())
}

pub(super) fn prepare_image(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<PreparedImage, ClipboardHistoryError> {
    validate_rgba(width, height, rgba)?;
    let png = encode_rgba_png(width, height, rgba)?;
    let (thumbnail_width, thumbnail_height, thumbnail_rgba) = thumbnail_pixels(width, height, rgba);
    let mut candidate_width = thumbnail_width;
    let mut candidate_height = thumbnail_height;
    let mut candidate_rgba = thumbnail_rgba;
    loop {
        let thumbnail_png = encode_rgba_png(candidate_width, candidate_height, &candidate_rgba)?;
        if thumbnail_png.len() <= MAX_THUMBNAIL_PNG_BYTES
            || candidate_width == 1
            || candidate_height == 1
        {
            return Ok(PreparedImage {
                png,
                thumbnail_data_url: format!("{DATA_URL_PREFIX}{}", base64_encode(&thumbnail_png)),
                thumbnail_width: candidate_width,
                thumbnail_height: candidate_height,
            });
        }
        let next_width = ((candidate_width as f32) * 0.85).floor().max(1.0) as u32;
        let next_height = ((candidate_height as f32) * 0.85).floor().max(1.0) as u32;
        if next_width == candidate_width && next_height == candidate_height {
            return Err(ClipboardHistoryError::InvalidCapture);
        }
        candidate_rgba = resize_rgba_nearest(
            &candidate_rgba,
            candidate_width,
            candidate_height,
            next_width,
            next_height,
        );
        candidate_width = next_width;
        candidate_height = next_height;
    }
}

pub(crate) fn decode_data_url_for_test(value: &str) -> Vec<u8> {
    let payload = value
        .strip_prefix(DATA_URL_PREFIX)
        .expect("expected PNG data URL");
    base64_decode(payload)
}

fn validate_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<(), ClipboardHistoryError> {
    if width == 0 || height == 0 {
        return Err(ClipboardHistoryError::InvalidCapture);
    }
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .map(|bytes| bytes as usize)
        .ok_or(ClipboardHistoryError::InvalidCapture)?;
    if rgba.len() != expected {
        return Err(ClipboardHistoryError::InvalidCapture);
    }
    Ok(())
}

fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, ClipboardHistoryError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|_| ClipboardHistoryError::InvalidCapture)?;
        writer
            .write_image_data(rgba)
            .map_err(|_| ClipboardHistoryError::InvalidCapture)?;
    }
    Ok(bytes)
}

fn thumbnail_pixels(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let long_edge = width.max(height);
    if long_edge <= THUMBNAIL_MAX_EDGE {
        return (width, height, rgba.to_vec());
    }
    let scaled_width =
        ((width as u64 * THUMBNAIL_MAX_EDGE as u64) / long_edge as u64).max(1) as u32;
    let scaled_height =
        ((height as u64 * THUMBNAIL_MAX_EDGE as u64) / long_edge as u64).max(1) as u32;
    (
        scaled_width,
        scaled_height,
        resize_rgba_nearest(rgba, width, height, scaled_width, scaled_height),
    )
}

fn resize_rgba_nearest(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    let mut output = vec![0; target_width as usize * target_height as usize * 4];
    for target_y in 0..target_height {
        let source_y = (target_y as u64 * source_height as u64 / target_height as u64) as usize;
        for target_x in 0..target_width {
            let source_x = (target_x as u64 * source_width as u64 / target_width as u64) as usize;
            let source_offset = (source_y * source_width as usize + source_x) * 4;
            let target_offset = (target_y as usize * target_width as usize + target_x as usize) * 4;
            output[target_offset..target_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
    output
}

fn normalize_windows_path(path: &std::path::Path) -> String {
    let mut value = path.as_os_str().to_string_lossy().replace('/', "\\");
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }
    value.to_uppercase()
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(third & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn base64_decode(value: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let mut buffer = [0_u8; 4];
    let mut filled = 0;
    for byte in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        let decoded = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => 64,
            _ => panic!("invalid base64"),
        };
        buffer[filled] = decoded;
        filled += 1;
        if filled == 4 {
            let combined = ((buffer[0] as u32) << 18)
                | ((buffer[1] as u32) << 12)
                | (if buffer[2] == 64 {
                    0
                } else {
                    (buffer[2] as u32) << 6
                })
                | if buffer[3] == 64 { 0 } else { buffer[3] as u32 };
            output.push((combined >> 16) as u8);
            if buffer[2] != 64 {
                output.push((combined >> 8) as u8);
            }
            if buffer[3] != 64 {
                output.push(combined as u8);
            }
            filled = 0;
        }
    }
    output
}
