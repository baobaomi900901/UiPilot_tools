//! PNG encoding and same-directory atomic commits. Callers reserve workspace first.
use super::{geometry::rgba_len, model::ScreenshotError};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

pub(crate) fn encoding_workspace_bytes(width: u32, height: u32) -> Result<usize, ScreenshotError> {
    let raw = rgba_len(width, height)?;
    // png 0.18 streaming encoding: output Vec + 3 rows + 64KiB IDAT
    // buffer. Fast's fdeflate compressor has no image-sized heap buffers.
    // Reserve 16 filtered rows instead: this also covers save validation's
    // unfiltering buffer growth (4-row shift threshold, Vec doubling), scratch
    // rows, and 2MiB for fixed inflate/codec/chunk storage and alignment.
    let rows = (width as usize)
        .checked_mul(4)
        .and_then(|n| n.checked_add(1))
        .and_then(|n| n.checked_mul(16))
        .ok_or(ScreenshotError::ResourceLimit)?;
    png_capacity(raw)?
        .checked_add(rows)
        .and_then(|n| n.checked_add(2 * 1024 * 1024))
        .ok_or(ScreenshotError::ResourceLimit)
}
fn png_capacity(raw: usize) -> Result<usize, ScreenshotError> {
    raw.checked_mul(2)
        .and_then(|n| n.checked_add(65536))
        .ok_or(ScreenshotError::ResourceLimit)
}
pub(crate) fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, ScreenshotError> {
    if rgba_len(width, height)? != rgba.len() {
        return Err(ScreenshotError::InvalidPixels);
    }
    let capacity = png_capacity(rgba.len())?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ScreenshotError::AllocationFailed)?;
    {
        let mut encoder = png::Encoder::new(
            BoundedWriter {
                bytes: &mut bytes,
                limit: capacity,
            },
            width,
            height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder
            .write_header()
            .map_err(|_| ScreenshotError::EncodingFailed)?;
        {
            let mut stream = writer
                .stream_writer_with_size(65536)
                .map_err(|_| ScreenshotError::EncodingFailed)?;
            stream
                .write_all(rgba)
                .map_err(|_| ScreenshotError::EncodingFailed)?;
            stream
                .finish()
                .map_err(|_| ScreenshotError::EncodingFailed)?;
        }
        writer
            .finish()
            .map_err(|_| ScreenshotError::EncodingFailed)?;
    }
    Ok(bytes)
}
struct BoundedWriter<'a> {
    bytes: &'a mut Vec<u8>,
    limit: usize,
}
impl Write for BoundedWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() > self.limit - self.bytes.len() {
            return Err(io::Error::other("PNG size limit"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
/// Accepts an already completely encoded PNG. Cancellation never calls this function.
pub(crate) fn save_png(path: &Path, png: &[u8]) -> Result<(), ScreenshotError> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .map_err(|_| ScreenshotError::EncodingFailed)?;
    while reader
        .next_row()
        .map_err(|_| ScreenshotError::EncodingFailed)?
        .is_some()
    {}
    reader
        .finish()
        .map_err(|_| ScreenshotError::EncodingFailed)?;
    save_with(path, png, replace)
}
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn save_with(
    path: &Path,
    png: &[u8],
    commit: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), ScreenshotError> {
    save_with_io(
        path,
        png,
        |file, bytes| file.write_all(bytes).and_then(|_| file.sync_all()),
        commit,
    )
}
fn save_with_io(
    path: &Path,
    png: &[u8],
    mut write: impl FnMut(&mut std::fs::File, &[u8]) -> io::Result<()>,
    mut commit: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), ScreenshotError> {
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (mut file, temp) = loop {
        let candidate = directory.join(format!(
            ".uipilot-screenshot-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (file, TempFile(candidate)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ScreenshotError::SaveFailed),
        }
    };
    let written = write(&mut file, png);
    drop(file);
    // Close before the temp guard cleans up, including Windows write failures.
    written.map_err(|_| ScreenshotError::SaveFailed)?;
    commit(&temp.0, path).map_err(|_| ScreenshotError::SaveFailed)
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let from = from
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::other)
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_shallow_workspace_covers_output_and_streaming_rows() {
        let width = 1_000_000;
        let row = width as usize * 4;
        let output_capacity = 2 * row + 65536;
        // png's streaming path keeps previous/current/filtered scanlines and
        // its bounded IDAT chunk alongside the fully reserved output Vec.
        assert!(encoding_workspace_bytes(width, 1).unwrap() >= output_capacity + 3 * row + 65536);
    }
    #[test]
    fn png_roundtrip_and_reject_bad_dimensions() {
        let png = encode_png(1, 1, &[1, 2, 3, 255]).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let mut reader = png::Decoder::new(std::io::Cursor::new(&png))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut rgba).unwrap();
        assert_eq!(rgba, [1, 2, 3, 255]);
        assert!(encode_png(1, 2, &[1, 2, 3, 255]).is_err());
    }
    #[test]
    fn truncated_png_does_not_replace_existing_file() {
        let target =
            std::env::temp_dir().join(format!("uipilot-truncated-{}.png", std::process::id()));
        std::fs::write(&target, b"original").unwrap();
        assert!(save_png(&target, b"\x89PNG\r\n\x1a\n").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        std::fs::remove_file(target).unwrap();
    }
    #[test]
    fn failed_partial_write_keeps_target_and_removes_temp() {
        let dir = std::env::temp_dir().join(format!("uipilot-output-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("existing.png");
        std::fs::write(&target, b"original").unwrap();
        let result = save_with_io(
            &target,
            b"candidate",
            |file, _| {
                file.write_all(b"partial")?;
                Err(io::Error::other("injected write failure"))
            },
            |_, _| panic!("commit must not run"),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_commit_keeps_target_and_cleans_only_owned_temp() {
        let dir = std::env::temp_dir().join(format!("uipilot-output-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("中文 image.png");
        let unrelated = dir.join("other.tmp");
        std::fs::write(&target, b"original").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();
        assert!(
            save_with(&target, b"candidate", |_, _| Err(std::io::Error::other(
                "injected"
            )))
            .is_err()
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn save_creates_then_replaces_unicode_target() {
        let dir =
            std::env::temp_dir().join(format!("uipilot-output-success-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("截图 test.png");
        let png = encode_png(1, 1, &[1, 2, 3, 255]).unwrap();
        save_png(&target, &png).unwrap();
        save_png(&target, &png).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), png);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
