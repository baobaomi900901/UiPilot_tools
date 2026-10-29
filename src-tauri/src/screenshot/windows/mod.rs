mod capture;
mod overlay;

use super::{
    geometry::{BudgetedImage, PixelBudget},
    model::*,
};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{CreateEventW, SetEvent},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeError {
    Pixels(ScreenshotError),
    CaptureFailed,
    OverlayFailed,
}
impl From<ScreenshotError> for NativeError {
    fn from(value: ScreenshotError) -> Self {
        Self::Pixels(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CancelReason {
    User = 1,
    Requested,
    DisplayChanged,
    SessionChanged,
    LostCapture,
    Shutdown,
}
#[derive(Debug)]
pub enum SelectionResult {
    Selected {
        rect: PhysicalRect,
        image: BudgetedImage,
    },
    Cancelled(CancelReason),
}
/// RGB values correspond to host foreground, surface, selection-border and
/// backdrop/mask tokens. The caller resolves the current Light/Dark theme.
#[derive(Debug, Clone, Copy)]
pub struct OverlayPalette {
    pub foreground: u32,
    pub surface: u32,
    pub selection_border: u32,
    pub mask: u32,
    pub mask_opacity: u8,
}
struct WakeEvent(HANDLE);
// Kernel events support cross-thread SetEvent/waits. The Arc keeps this handle
// alive until both caller and message worker have finished using it.
unsafe impl Send for WakeEvent {}
unsafe impl Sync for WakeEvent {}
impl Drop for WakeEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
#[derive(Default)]
struct Cancellation {
    reason: AtomicU8,
    wake: Option<WakeEvent>,
}
impl Cancellation {
    fn cancel(&self, reason: CancelReason) {
        let _ = self
            .reason
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire);
        if let Some(wake) = &self.wake {
            unsafe {
                let _ = SetEvent(wake.0);
            }
        }
    }
    fn reason(&self) -> Option<CancelReason> {
        match self.reason.load(Ordering::Acquire) {
            1 => Some(CancelReason::User),
            2 => Some(CancelReason::Requested),
            3 => Some(CancelReason::DisplayChanged),
            4 => Some(CancelReason::SessionChanged),
            5 => Some(CancelReason::LostCapture),
            6 => Some(CancelReason::Shutdown),
            _ => None,
        }
    }
}
pub struct SelectionHandle {
    cancellation: Arc<Cancellation>,
    thread: Option<JoinHandle<()>>,
}
impl SelectionHandle {
    pub fn cancel(&self, reason: CancelReason) {
        self.cancellation.cancel(reason);
    }
    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(|thread| thread.is_finished())
    }
    /// Only call on a shutdown/background worker; joining waits for native cleanup.
    pub fn join(mut self) -> std::thread::Result<()> {
        self.cancel(CancelReason::Shutdown);
        self.thread.take().map_or(Ok(()), |thread| thread.join())
    }
}
impl Drop for SelectionHandle {
    fn drop(&mut self) {
        self.cancel(CancelReason::Requested);
    }
}

/// Returns immediately. All capture, HWND ownership and cropping stay on the
/// dedicated message thread; completion runs there after every overlay is gone.
pub fn start_selection(
    mode: ScreenshotMode,
    budget: PixelBudget,
    palette: OverlayPalette,
    excluded_windows: Vec<isize>,
    provider: Option<std::path::PathBuf>,
    complete: impl FnOnce(Result<SelectionResult, NativeError>) + Send + 'static,
) -> Result<SelectionHandle, NativeError> {
    let cancellation = Arc::new(Cancellation {
        reason: AtomicU8::new(0),
        wake: Some(WakeEvent(unsafe {
            CreateEventW(None, true, false, None).map_err(|_| NativeError::OverlayFailed)?
        })),
    });
    let worker_cancel = cancellation.clone();
    let thread = std::thread::Builder::new()
        .name("screenshot-selection".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                overlay::run(
                    mode,
                    &budget,
                    palette,
                    &excluded_windows,
                    provider,
                    &worker_cancel,
                )
            }))
            .unwrap_or(Err(NativeError::OverlayFailed));
            complete(result);
        })
        .map_err(|_| NativeError::OverlayFailed)?;
    Ok(SelectionHandle {
        cancellation,
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_before_worker_queue_exists_is_retained_and_first_reason_wins() {
        let cancellation = Cancellation::default();
        cancellation.cancel(CancelReason::SessionChanged);
        cancellation.cancel(CancelReason::Requested);
        assert_eq!(cancellation.reason(), Some(CancelReason::SessionChanged));
    }
}
