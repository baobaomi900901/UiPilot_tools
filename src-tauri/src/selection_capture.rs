//! One explicitly requested selection. Never logs captured text or clipboard data.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionCaptureError {
    Empty,
    Invalid,
    TooLong,
    TargetChanged,
    Timeout,
    Busy,
    Unsupported,
    Cancelled,
    Failed,
    ClipboardBackup,
    ClipboardRestore,
    ClipboardChanged,
    HistoryUnavailable,
    HistoryCleanup,
}
impl SelectionCaptureError {
    pub(crate) fn recovery_failure(self) -> bool {
        matches!(self, Self::ClipboardRestore | Self::HistoryCleanup)
    }
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Empty => "selection_empty",
            Self::Invalid => "selection_invalid",
            Self::TooLong => "selection_too_long",
            Self::TargetChanged => "selection_target_changed",
            Self::Timeout => "selection_timeout",
            Self::Busy => "selection_busy",
            Self::Unsupported => "selection_unsupported",
            Self::Cancelled => "selection_cancelled",
            Self::Failed => "selection_failed",
            Self::ClipboardBackup => "selection_clipboard_backup",
            Self::ClipboardRestore => "selection_clipboard_restore",
            Self::ClipboardChanged => "selection_clipboard_changed",
            Self::HistoryUnavailable => "selection_history_unavailable",
            Self::HistoryCleanup => "selection_history_cleanup",
        }
    }
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Empty => "未选中文字，请选择文字后重试",
            Self::Invalid => "选中文字包含不支持的字符",
            Self::TooLong => "选中文字超过 8192 字节，请缩短选区",
            Self::TargetChanged => "前台窗口已改变，请重新选择文字",
            Self::Timeout => "读取选中文字超时，请重试",
            Self::Busy => "选区读取仍在进行，请稍后重试",
            Self::Unsupported => "当前应用不支持读取选中文字，请手动输入翻译文字",
            Self::Cancelled => "读取选中文字已取消",
            Self::Failed => "无法读取选中文字，请手动输入",
            Self::ClipboardBackup => "无法完整备份当前剪贴板，已停止复制取词，请手动输入",
            Self::ClipboardRestore => "原剪贴板未能完整恢复，请检查剪贴板内容",
            Self::ClipboardChanged => "取词期间剪贴板或输入已改变，未覆盖当前剪贴板，请重试",
            Self::HistoryUnavailable => "无法访问 Windows 剪贴板历史，已停止复制取词，请手动输入",
            Self::HistoryCleanup => "临时取词记录未能完成清理，请检查 Win+V 历史",
        }
    }
}
fn text_pattern_missing(code: i32) -> bool {
    // GetCurrentPatternAs maps S_OK with a null pattern to an HRESULT(0)
    // error. Only explicit absence authorizes input injection; provider/COM
    // failures, including access denied and an unavailable element, fail closed.
    matches!(code as u32, 0 | 0x80004002 | 0x80040204)
}
fn validate(text: String) -> Result<String, SelectionCaptureError> {
    if text.contains('\0') {
        Err(SelectionCaptureError::Invalid)
    } else if text.len() > 8192 {
        Err(SelectionCaptureError::TooLong)
    } else if text.trim().is_empty() {
        Err(SelectionCaptureError::Empty)
    } else {
        Ok(text)
    }
}
trait SelectionDriver {
    fn check(&mut self) -> Result<(), SelectionCaptureError>;
    fn selected(&mut self) -> Result<Option<String>, SelectionCaptureError>;
    fn copied(&mut self) -> Result<String, SelectionCaptureError> {
        Err(SelectionCaptureError::Unsupported)
    }
}
fn capture_with_driver(driver: &mut impl SelectionDriver) -> Result<String, SelectionCaptureError> {
    driver.check()?;
    let selected = driver.selected();
    driver.check()?;
    match selected {
        Ok(Some(text)) => validate(text),
        Ok(None) => {
            let copied = driver.copied();
            let current = driver.check();
            if copied.as_ref().is_err_and(|error| error.recovery_failure()) {
                return copied;
            }
            current?;
            copied.and_then(validate)
        }
        Err(error) => Err(error),
    }
}

pub(crate) type RecoveryReporter = Arc<dyn Fn(SelectionCaptureError) + Send + Sync>;
fn complete_capture(
    result: Result<String, SelectionCaptureError>,
    reporter: &RecoveryReporter,
    deliver: impl FnOnce(Result<String, SelectionCaptureError>),
) {
    if let Err(error) = &result {
        if error.recovery_failure() {
            reporter(*error);
        }
    }
    deliver(result);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureTarget {
    window: usize,
    process: u32,
}
pub(crate) fn capture_target() -> Result<CaptureTarget, SelectionCaptureError> {
    #[cfg(windows)]
    {
        native::target()
    }
    #[cfg(not(windows))]
    {
        Err(SelectionCaptureError::Unsupported)
    }
}
pub(crate) fn target_is_current(target: CaptureTarget) -> bool {
    capture_target()
        .map(|current| current.window == target.window && current.process == target.process)
        .unwrap_or(false)
}
pub(crate) fn capture_selection_for(
    target: CaptureTarget,
    cancelled: Arc<AtomicBool>,
    history: Arc<crate::clipboard_history::ClipboardHistoryService>,
    recovery_reporter: RecoveryReporter,
) -> Result<String, SelectionCaptureError> {
    #[cfg(windows)]
    {
        native::capture(target, cancelled, history, recovery_reporter)
    }
    #[cfg(not(windows))]
    {
        let _ = (target, cancelled, history, recovery_reporter);
        Err(SelectionCaptureError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_worker_reports_recovery_once_even_after_receiver_has_expired() {
        use std::sync::{mpsc, Mutex};
        for error in [
            SelectionCaptureError::ClipboardRestore,
            SelectionCaptureError::HistoryCleanup,
        ] {
            let reports = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&reports);
            let reporter: RecoveryReporter =
                Arc::new(move |error| seen.lock().unwrap().push(error));
            let (tx, rx) = mpsc::channel();
            drop(rx);
            let seen = Arc::clone(&reports);
            let reporter_worker = Arc::clone(&reporter);
            std::thread::spawn(move || {
                complete_capture(Err(error), &reporter_worker, |result| {
                    assert_eq!(*seen.lock().unwrap(), [error]);
                    assert!(tx.send(result).is_err());
                });
            })
            .join()
            .unwrap();
            assert_eq!(*reports.lock().unwrap(), [error]);
            for ordinary in [
                Ok("private text".into()),
                Err(SelectionCaptureError::Cancelled),
                Err(SelectionCaptureError::Timeout),
            ] {
                complete_capture(ordinary, &reporter, |_| {});
            }
            assert_eq!(*reports.lock().unwrap(), [error]);
        }
    }
    struct Driver {
        checks: usize,
        fail_at: usize,
        error: SelectionCaptureError,
        selected: Option<String>,
        reads: usize,
        copies: usize,
        copied: Result<String, SelectionCaptureError>,
        selected_error: Option<SelectionCaptureError>,
    }
    impl SelectionDriver for Driver {
        fn check(&mut self) -> Result<(), SelectionCaptureError> {
            self.checks += 1;
            if self.checks == self.fail_at {
                Err(self.error)
            } else {
                Ok(())
            }
        }
        fn selected(&mut self) -> Result<Option<String>, SelectionCaptureError> {
            self.reads += 1;
            if let Some(error) = self.selected_error {
                return Err(error);
            }
            Ok(self.selected.take())
        }
        fn copied(&mut self) -> Result<String, SelectionCaptureError> {
            self.copies += 1;
            self.copied.clone()
        }
    }
    fn driver() -> Driver {
        Driver {
            checks: 0,
            fail_at: 0,
            error: SelectionCaptureError::TargetChanged,
            selected: Some("selected".into()),
            reads: 0,
            copies: 0,
            copied: Err(SelectionCaptureError::Unsupported),
            selected_error: None,
        }
    }
    #[test]
    fn provider_failures_are_not_missing_patterns_and_never_copy() {
        for code in [0, 0x80004002u32 as i32, 0x80040204u32 as i32] {
            assert!(text_pattern_missing(code));
        }
        for code in [
            0x80070005u32 as i32,
            0x80040201u32 as i32,
            0x80004005u32 as i32,
        ] {
            assert!(!text_pattern_missing(code));
        }
        let mut d = driver();
        d.selected_error = Some(SelectionCaptureError::Failed);
        assert_eq!(
            capture_with_driver(&mut d),
            Err(SelectionCaptureError::Failed)
        );
        assert_eq!(d.copies, 0);
    }
    #[test]
    fn recovery_failures_survive_post_copy_cancel_target_and_deadline_checks() {
        for recovery in [
            SelectionCaptureError::ClipboardRestore,
            SelectionCaptureError::HistoryCleanup,
        ] {
            assert!(recovery.recovery_failure());
            for rejection in [
                SelectionCaptureError::Cancelled,
                SelectionCaptureError::TargetChanged,
                SelectionCaptureError::Timeout,
            ] {
                let mut d = driver();
                d.selected = None;
                d.copied = Err(recovery);
                d.fail_at = 3;
                d.error = rejection;
                assert_eq!(capture_with_driver(&mut d), Err(recovery));
                d = driver();
                d.selected = None;
                d.copied = Ok("private".into());
                d.fail_at = 3;
                d.error = rejection;
                assert_eq!(capture_with_driver(&mut d), Err(rejection));
            }
        }
    }
    #[test]
    fn selection_copy_fallback_only_for_missing_pattern() {
        let mut d = driver();
        d.copied = Ok(" copied 中文 ".into());
        assert_eq!(capture_with_driver(&mut d), Ok("selected".into()));
        assert_eq!(d.copies, 0);
        d.selected = Some("".into());
        assert_eq!(
            capture_with_driver(&mut d),
            Err(SelectionCaptureError::Empty)
        );
        assert_eq!(d.copies, 0);
        d.selected = None;
        assert_eq!(capture_with_driver(&mut d), Ok(" copied 中文 ".into()));
        assert_eq!(d.copies, 1);
    }
    #[test]
    fn selection_copy_rechecks_target_before_and_after_fallback() {
        for (fail_at, copies) in [(2, 0), (3, 1)] {
            let mut d = driver();
            d.selected = None;
            d.copied = Ok("copied".into());
            d.fail_at = fail_at;
            assert_eq!(
                capture_with_driver(&mut d),
                Err(SelectionCaptureError::TargetChanged)
            );
            assert_eq!(d.copies, copies);
        }
    }
    #[test]
    fn selection_copy_validates_after_transaction_finishes() {
        for text in ["a\0b".to_string(), "é".repeat(4097), "  ".to_string()] {
            let expected = validate(text.clone());
            let mut d = driver();
            d.selected = None;
            d.copied = Ok(text);
            assert_eq!(capture_with_driver(&mut d), expected);
            assert_eq!(d.copies, 1);
        }
    }
    #[test]
    fn changed_target_before_read_does_not_read() {
        let mut d = driver();
        d.fail_at = 1;
        assert_eq!(
            capture_with_driver(&mut d),
            Err(SelectionCaptureError::TargetChanged)
        );
        assert_eq!(d.reads, 0);
    }
    #[test]
    fn changed_target_during_read_discards_text() {
        let mut d = driver();
        d.fail_at = 2;
        assert_eq!(
            capture_with_driver(&mut d),
            Err(SelectionCaptureError::TargetChanged)
        );
        assert_eq!(d.reads, 1);
    }
    #[test]
    fn cancellation_and_timeout_discard_late_text() {
        for error in [
            SelectionCaptureError::Cancelled,
            SelectionCaptureError::Timeout,
        ] {
            let mut d = driver();
            d.fail_at = 2;
            d.error = error;
            assert_eq!(capture_with_driver(&mut d), Err(error));
        }
    }
    #[test]
    fn absent_provider_fails_closed_without_an_input_driver() {
        let mut d = driver();
        d.selected = None;
        assert_eq!(
            capture_with_driver(&mut d),
            Err(SelectionCaptureError::Unsupported)
        );
        assert_eq!(d.reads, 1);
    }
    #[test]
    fn empty_selection_is_rejected() {
        let mut d = driver();
        d.selected = Some(" \n\t".into());
        assert_eq!(
            capture_with_driver(&mut d),
            Err(SelectionCaptureError::Empty)
        );
    }
    #[test]
    fn validates_without_modifying_unicode_or_spaces() {
        let text = "  中文 🦀\nsecond  ".to_owned();
        assert_eq!(validate(text.clone()), Ok(text));
        assert_eq!(validate("a\0b".into()), Err(SelectionCaptureError::Invalid));
        assert!(validate("é".repeat(4096)).is_ok());
        assert_eq!(
            validate("é".repeat(4097)),
            Err(SelectionCaptureError::TooLong)
        );
    }
}

#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod windows_history;
