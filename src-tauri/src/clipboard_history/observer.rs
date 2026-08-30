use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use super::{ClipboardCapture, ClipboardHistoryError};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardReadError {
    Busy,
    Unavailable,
}

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

pub(crate) trait ClipboardReader: Send + Sync {
    fn read_capture(&self) -> Result<Option<ClipboardCapture>, ClipboardReadError>;
}

pub(crate) trait ClipboardObserver: Send + Sync {
    fn start(
        &self,
        callback: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Result<Box<dyn ClipboardObserverHandle>, ClipboardHistoryError>;
}

pub(crate) trait ClipboardObserverHandle: Send {
    fn stop(&self);
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

pub(crate) fn default_clipboard_reader() -> Arc<dyn ClipboardReader> {
    #[cfg(test)]
    {
        Arc::new(NoopClipboardReader)
    }
    #[cfg(not(test))]
    {
        Arc::new(SystemClipboardReader)
    }
}

pub(crate) fn default_clipboard_observer() -> Arc<dyn ClipboardObserver> {
    #[cfg(test)]
    {
        Arc::new(NoopClipboardObserver)
    }
    #[cfg(not(test))]
    {
        Arc::new(SystemClipboardObserver)
    }
}

struct NoopClipboardReader;

impl ClipboardReader for NoopClipboardReader {
    fn read_capture(&self) -> Result<Option<ClipboardCapture>, ClipboardReadError> {
        Ok(None)
    }
}

struct NoopClipboardObserver;

impl ClipboardObserver for NoopClipboardObserver {
    fn start(
        &self,
        _callback: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Result<Box<dyn ClipboardObserverHandle>, ClipboardHistoryError> {
        Ok(Box::new(NoopClipboardObserverHandle))
    }
}

struct NoopClipboardObserverHandle;

impl ClipboardObserverHandle for NoopClipboardObserverHandle {
    fn stop(&self) {}
}

#[cfg(windows)]
struct SystemClipboardReader;

#[cfg(windows)]
impl ClipboardReader for SystemClipboardReader {
    fn read_capture(&self) -> Result<Option<ClipboardCapture>, ClipboardReadError> {
        platform::read_system_clipboard()
    }
}

#[cfg(not(windows))]
struct SystemClipboardReader;

#[cfg(not(windows))]
impl ClipboardReader for SystemClipboardReader {
    fn read_capture(&self) -> Result<Option<ClipboardCapture>, ClipboardReadError> {
        Ok(None)
    }
}

#[derive(Default)]
struct SystemClipboardObserver;

impl ClipboardObserver for SystemClipboardObserver {
    fn start(
        &self,
        callback: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Result<Box<dyn ClipboardObserverHandle>, ClipboardHistoryError> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("uipilot-clipboard-history".into())
            .spawn(move || {
                let mut last_sequence = current_clipboard_sequence();
                while !thread_stop.load(Ordering::Acquire) {
                    thread::sleep(POLL_INTERVAL);
                    let sequence = current_clipboard_sequence();
                    if sequence != 0 && sequence != last_sequence {
                        last_sequence = sequence;
                        callback();
                    }
                }
            })
            .map_err(|_| ClipboardHistoryError::Storage)?;
        Ok(Box::new(PollingClipboardObserverHandle {
            stop,
            thread: Mutex::new(Some(handle)),
        }))
    }
}

struct PollingClipboardObserverHandle {
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl ClipboardObserverHandle for PollingClipboardObserverHandle {
    fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        let Ok(mut thread) = self.thread.lock() else {
            return;
        };
        let Some(handle) = thread.take() else {
            return;
        };
        if handle.thread().id() == thread::current().id() {
            return;
        }
        let _ = handle.join();
    }
}

impl Drop for PollingClipboardObserverHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut thread) = self.thread.lock() {
            if let Some(handle) = thread.take() {
                if handle.thread().id() == thread::current().id() {
                    return;
                }
                let _ = handle.join();
            }
        }
    }
}

#[cfg(windows)]
fn current_clipboard_sequence() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
fn current_clipboard_sequence() -> u32 {
    0
}

#[cfg(windows)]
mod platform {
    use std::{mem, path::PathBuf, ptr, slice};

    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    use windows::Win32::{
        Foundation::{HANDLE, HGLOBAL},
        Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB},
        System::{
            DataExchange::{
                CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
            },
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::Shell::{DragQueryFileW, HDROP},
    };

    use super::{
        normalize_clipboard_formats, ClipboardFormatSnapshot, ClipboardImageSnapshot,
        ClipboardReadError,
    };
    use crate::clipboard_history::ClipboardCapture;

    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;
    const CF_HDROP: u32 = 15;
    const CF_UNICODETEXT: u32 = 13;

    pub(super) fn read_system_clipboard() -> Result<Option<ClipboardCapture>, ClipboardReadError> {
        let _guard = ClipboardGuard::open()?;
        let formats = ClipboardFormatSnapshot {
            files: format_available(CF_HDROP).then(read_files).transpose()?,
            image: read_first_available_image()?,
            text: format_available(CF_UNICODETEXT)
                .then(read_text)
                .transpose()?,
        };
        Ok(normalize_clipboard_formats(formats, now_rfc3339()?))
    }

    fn read_first_available_image() -> Result<Option<ClipboardImageSnapshot>, ClipboardReadError> {
        if format_available(CF_DIBV5) {
            return read_image(CF_DIBV5);
        }
        if format_available(CF_DIB) {
            return read_image(CF_DIB);
        }
        Ok(None)
    }

    fn format_available(format: u32) -> bool {
        unsafe { IsClipboardFormatAvailable(format).is_ok() }
    }

    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open() -> Result<Self, ClipboardReadError> {
            unsafe { OpenClipboard(None) }.map_err(|_| ClipboardReadError::Busy)?;
            Ok(Self)
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    fn read_text() -> Result<String, ClipboardReadError> {
        let memory = GlobalClipboardMemory::for_format(CF_UNICODETEXT)?;
        let words = memory.as_u16_slice();
        let nul = words
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(words.len());
        Ok(String::from_utf16_lossy(&words[..nul]))
    }

    fn read_files() -> Result<Vec<PathBuf>, ClipboardReadError> {
        let handle =
            unsafe { GetClipboardData(CF_HDROP) }.map_err(|_| ClipboardReadError::Unavailable)?;
        let hdrop = HDROP(handle.0);
        let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            let len = unsafe { DragQueryFileW(hdrop, index, None) };
            let mut buffer = vec![0_u16; len.saturating_add(1) as usize];
            let copied = unsafe { DragQueryFileW(hdrop, index, Some(&mut buffer)) };
            if copied == 0 && len != 0 {
                return Err(ClipboardReadError::Unavailable);
            }
            buffer.truncate(copied as usize);
            paths.push(PathBuf::from(String::from_utf16_lossy(&buffer)));
        }
        Ok(paths)
    }

    fn read_image(format: u32) -> Result<Option<ClipboardImageSnapshot>, ClipboardReadError> {
        let memory = GlobalClipboardMemory::for_format(format)?;
        Ok(decode_dib(memory.bytes()))
    }

    struct GlobalClipboardMemory {
        hglobal: HGLOBAL,
        ptr: *mut core::ffi::c_void,
        size: usize,
    }

    impl GlobalClipboardMemory {
        fn for_format(format: u32) -> Result<Self, ClipboardReadError> {
            let handle =
                unsafe { GetClipboardData(format) }.map_err(|_| ClipboardReadError::Unavailable)?;
            Self::from_handle(handle)
        }

        fn from_handle(handle: HANDLE) -> Result<Self, ClipboardReadError> {
            let hglobal = HGLOBAL(handle.0);
            let ptr = unsafe { GlobalLock(hglobal) };
            if ptr.is_null() {
                return Err(ClipboardReadError::Unavailable);
            }
            let size = unsafe { GlobalSize(hglobal) };
            if size == 0 {
                let _ = unsafe { GlobalUnlock(hglobal) };
                return Err(ClipboardReadError::Unavailable);
            }
            Ok(Self { hglobal, ptr, size })
        }

        fn bytes(&self) -> &[u8] {
            unsafe { slice::from_raw_parts(self.ptr.cast::<u8>(), self.size) }
        }

        fn as_u16_slice(&self) -> &[u16] {
            unsafe { slice::from_raw_parts(self.ptr.cast::<u16>(), self.size / 2) }
        }
    }

    impl Drop for GlobalClipboardMemory {
        fn drop(&mut self) {
            let _ = unsafe { GlobalUnlock(self.hglobal) };
        }
    }

    fn decode_dib(bytes: &[u8]) -> Option<ClipboardImageSnapshot> {
        if bytes.len() < mem::size_of::<BITMAPINFOHEADER>() {
            return None;
        }
        let header = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<BITMAPINFOHEADER>()) };
        if header.biPlanes != 1
            || header.biWidth <= 0
            || header.biHeight == 0
            || header.biCompression != BI_RGB.0
            || !matches!(header.biBitCount, 24 | 32)
        {
            return None;
        }
        let header_len = usize::try_from(header.biSize).ok()?;
        if header_len < mem::size_of::<BITMAPINFOHEADER>() || header_len > bytes.len() {
            return None;
        }
        let width = u32::try_from(header.biWidth).ok()?;
        let height = header.biHeight.unsigned_abs();
        let bytes_per_pixel = usize::from(header.biBitCount / 8);
        let width_usize = usize::try_from(width).ok()?;
        let height_usize = usize::try_from(height).ok()?;
        let stride = ((width_usize.checked_mul(usize::from(header.biBitCount))? + 31) / 32)
            .checked_mul(4)?;
        let pixel_len = stride.checked_mul(height_usize)?;
        let pixel_end = header_len.checked_add(pixel_len)?;
        if pixel_end > bytes.len() {
            return None;
        }
        let pixels = &bytes[header_len..pixel_end];
        let alpha_meaningful = header.biBitCount == 32
            && (0..height_usize).any(|row| {
                let row_start = row * stride;
                (0..width_usize).any(|column| pixels[row_start + column * bytes_per_pixel + 3] != 0)
            });
        let mut rgba = vec![0_u8; width_usize.checked_mul(height_usize)?.checked_mul(4)?];
        for dest_y in 0..height_usize {
            let source_y = if header.biHeight > 0 {
                height_usize - 1 - dest_y
            } else {
                dest_y
            };
            let source_row = source_y * stride;
            for x in 0..width_usize {
                let source = source_row + x * bytes_per_pixel;
                let dest = (dest_y * width_usize + x) * 4;
                rgba[dest] = pixels[source + 2];
                rgba[dest + 1] = pixels[source + 1];
                rgba[dest + 2] = pixels[source];
                rgba[dest + 3] = if header.biBitCount == 32 && alpha_meaningful {
                    pixels[source + 3]
                } else {
                    0xff
                };
            }
        }
        Some(ClipboardImageSnapshot {
            rgba,
            width,
            height,
        })
    }

    fn now_rfc3339() -> Result<String, ClipboardReadError> {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| ClipboardReadError::Unavailable)
    }
}
