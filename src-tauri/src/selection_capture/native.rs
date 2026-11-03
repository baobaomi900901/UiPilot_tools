use super::*;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    },
    UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
    },
};

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(window: isize, process: *mut u32) -> u32;
    fn GetAsyncKeyState(key: i32) -> i16;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
}
static RUNNING: AtomicBool = AtomicBool::new(false);
const LIMIT: Duration = Duration::from_secs(5);

// Diagnostics accept only static stage labels and numeric metadata, never UI text,
// window titles, selection contents, or provider error messages.
pub(super) fn trace(stage: &'static str, value: i64) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let line = format!("[selection {millis}] {stage}={value}");
        eprintln!("{line}");
        let path =
            std::env::temp_dir().join(format!("uipilot-selection-{}.log", std::process::id()));
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = (stage, value);
}

fn diagnosed<T>(stage: &'static str, result: windows::core::Result<T>) -> windows::core::Result<T> {
    if let Err(error) = &result {
        trace(stage, i64::from(error.code().0));
    }
    result
}
pub(super) fn target() -> Result<CaptureTarget, SelectionCaptureError> {
    unsafe {
        let window = GetForegroundWindow();
        let mut process = 0;
        GetWindowThreadProcessId(window, &mut process);
        if window == 0 || process == 0 || process == GetCurrentProcessId() {
            return Err(SelectionCaptureError::TargetChanged);
        }
        Ok(CaptureTarget {
            window: window as usize,
            process,
        })
    }
}
pub(super) fn check(
    target: CaptureTarget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SelectionCaptureError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(SelectionCaptureError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(SelectionCaptureError::Timeout);
    }
    let current = self::target()?;
    if current.window != target.window || current.process != target.process {
        return Err(SelectionCaptureError::TargetChanged);
    }
    Ok(())
}
pub(super) fn capture(
    target: CaptureTarget,
    cancelled: Arc<AtomicBool>,
    history: Arc<crate::clipboard_history::ClipboardHistoryService>,
    recovery_reporter: RecoveryReporter,
) -> Result<String, SelectionCaptureError> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        trace("selection_busy", 1);
        return Err(SelectionCaptureError::Busy);
    }
    trace("capture_start_pid", i64::from(target.process));
    let started = Instant::now();
    let deadline = Instant::now() + LIMIT;
    let (tx, rx) = mpsc::sync_channel(1);
    if thread::Builder::new()
        .name("selection-capture".into())
        .spawn(move || {
            struct Reset;
            impl Drop for Reset {
                fn drop(&mut self) {
                    RUNNING.store(false, Ordering::Release);
                }
            }
            let _reset = Reset;
            let result = unsafe { run(target, &cancelled, deadline, &history) };
            // The receiver may already have hit its deadline while a Windows
            // delayed-render call was still blocked. Report compensation failures
            // from their owning worker before attempting the fallible delivery.
            complete_capture(result, &recovery_reporter, |result| {
                let _ = tx.send(result);
            });
        })
        .is_err()
    {
        RUNNING.store(false, Ordering::Release);
        return Err(SelectionCaptureError::Failed);
    }
    let result = rx
        .recv_timeout(LIMIT)
        .unwrap_or(Err(SelectionCaptureError::Timeout));
    trace(
        result
            .as_ref()
            .err()
            .map_or("selection_ok", |error| error.code()),
        started.elapsed().as_millis() as i64,
    );
    result
}
unsafe fn run(
    target: CaptureTarget,
    cancelled: &AtomicBool,
    deadline: Instant,
    history: &crate::clipboard_history::ClipboardHistoryService,
) -> Result<String, SelectionCaptureError> {
    // The foreground target was captured synchronously at hotkey admission.
    loop {
        check(target, cancelled, deadline)?;
        if [0x10, 0x11, 0x12, 0x5b, 0x5c]
            .iter()
            .all(|key| GetAsyncKeyState(*key) >= 0)
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    struct Driver<'a> {
        target: CaptureTarget,
        cancelled: &'a AtomicBool,
        deadline: Instant,
        history: &'a crate::clipboard_history::ClipboardHistoryService,
    }
    impl SelectionDriver for Driver<'_> {
        fn check(&mut self) -> Result<(), SelectionCaptureError> {
            check(self.target, self.cancelled, self.deadline)
        }
        fn selected(&mut self) -> Result<Option<String>, SelectionCaptureError> {
            unsafe { uia_selection(self.target, self.cancelled, self.deadline) }
        }
        fn copied(&mut self) -> Result<String, SelectionCaptureError> {
            super::clipboard::capture(self.target, self.cancelled, self.deadline, self.history)
        }
    }
    capture_with_driver(&mut Driver {
        target,
        cancelled,
        deadline,
        history,
    })
}
unsafe fn uia_selection(
    target: CaptureTarget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<String>, SelectionCaptureError> {
    diagnosed("com_init", CoInitializeEx(None, COINIT_MULTITHREADED).ok())
        .map_err(|_| SelectionCaptureError::Failed)?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    let automation: IUIAutomation = diagnosed(
        "uia_create",
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER),
    )
    .map_err(|_| SelectionCaptureError::Failed)?;
    check(target, cancelled, deadline)?;
    let focused = diagnosed("focused_element", automation.GetFocusedElement())
        .map_err(|_| SelectionCaptureError::Failed)?;
    let focused_process = diagnosed("focused_pid", focused.CurrentProcessId()).ok();
    if focused_process != Some(target.process as i32) {
        trace(
            "focused_pid_mismatch",
            focused_process.map_or(-1, i64::from),
        );
        return Err(SelectionCaptureError::Failed);
    }
    check(target, cancelled, deadline)?;
    // Password selections are never read.
    if diagnosed("password_property", focused.CurrentIsPassword())
        .map_err(|_| SelectionCaptureError::Invalid)?
        .as_bool()
    {
        trace("protected_control", 1);
        return Err(SelectionCaptureError::Invalid);
    }
    check(target, cancelled, deadline)?;
    trace(
        "focused_control_type",
        focused
            .CurrentControlType()
            .map_or(-1, |value| i64::from(value.0)),
    );
    let pattern: IUIAutomationTextPattern = match diagnosed(
        "text_pattern",
        focused.GetCurrentPatternAs(UIA_TextPatternId),
    ) {
        Ok(p) => p,
        Err(error) if text_pattern_missing(error.code().0) => return Ok(None),
        Err(_) => return Err(SelectionCaptureError::Failed),
    };
    check(target, cancelled, deadline)?;
    let ranges = diagnosed("get_selection", pattern.GetSelection())
        .map_err(|_| SelectionCaptureError::Failed)?;
    let count =
        diagnosed("selection_count", ranges.Length()).map_err(|_| SelectionCaptureError::Failed)?;
    trace("selection_range_count", i64::from(count));
    if !(0..=32).contains(&count) {
        return Err(SelectionCaptureError::TooLong);
    }
    let mut text = String::new();
    for index in 0..count {
        check(target, cancelled, deadline)?;
        let value = diagnosed(
            "selection_text",
            ranges
                .GetElement(index)
                .and_then(|range| range.GetText(8193)),
        )
        .map_err(|_| SelectionCaptureError::Failed)?;
        text.push_str(&String::from_utf16(&value).map_err(|_| SelectionCaptureError::Invalid)?);
        if text.len() > 8192 {
            return Err(SelectionCaptureError::TooLong);
        }
    }
    // A supported text provider with no selection returns an empty selection.
    Ok(Some(text))
}
