use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
#[cfg(not(windows))]
use std::time::Duration;

use crate::protocol::{EverythingQueryResult, EverythingQuerySpec, ProtocolError};

pub const MAX_OUTSTANDING_QUERIES: usize = 32;

pub(crate) struct QueryPermit {
    outstanding: Arc<AtomicUsize>,
}

impl Drop for QueryPermit {
    fn drop(&mut self) {
        self.outstanding.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn try_acquire_query_permit(outstanding: &Arc<AtomicUsize>) -> Option<QueryPermit> {
    let mut current = outstanding.load(Ordering::Acquire);
    loop {
        if current >= MAX_OUTSTANDING_QUERIES {
            return None;
        }
        match outstanding.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                return Some(QueryPermit {
                    outstanding: Arc::clone(outstanding),
                });
            }
            Err(observed) => current = observed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EverythingClientError {
    InvalidInstance,
    ConnectionTimedOut,
    IpcUnavailable,
    IpcOwnerMismatch,
    IpcSendFailed,
    Overloaded,
    RequestIdExhausted,
    Protocol(ProtocolError),
    QueryTimedOut,
    ClientClosed,
}

fn select_owned_window(
    candidates: &[(usize, u32)],
    expected_pid: u32,
) -> Result<Option<usize>, EverythingClientError> {
    match candidates {
        [] => Ok(None),
        [(window, actual_pid)] if expected_pid != 0 && *actual_pid == expected_pid => {
            Ok(Some(*window))
        }
        _ => Err(EverythingClientError::IpcOwnerMismatch),
    }
}

#[cfg(test)]
mod owned_window_tests {
    use super::*;

    #[test]
    fn owned_window_selection_requires_one_exact_pid() {
        assert_eq!(select_owned_window(&[], 42), Ok(None));
        assert_eq!(select_owned_window(&[(7, 42)], 42), Ok(Some(7)));
        assert_eq!(
            select_owned_window(&[(7, 41)], 42),
            Err(EverythingClientError::IpcOwnerMismatch)
        );
        assert_eq!(
            select_owned_window(&[(7, 42), (8, 43)], 42),
            Err(EverythingClientError::IpcOwnerMismatch)
        );
        assert_eq!(
            select_owned_window(&[(7, 42)], 0),
            Err(EverythingClientError::IpcOwnerMismatch)
        );
    }
}

impl fmt::Display for EverythingClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInstance => "invalid Everything instance",
            Self::ConnectionTimedOut => "Everything IPC connection timed out",
            Self::IpcUnavailable => "Everything IPC is unavailable",
            Self::IpcOwnerMismatch => "Everything IPC window owner does not match",
            Self::IpcSendFailed => "Everything IPC send failed",
            Self::Overloaded => "Everything IPC query capacity is exhausted",
            Self::RequestIdExhausted => "Everything IPC request id exhausted",
            Self::Protocol(_) => "Everything IPC protocol error",
            Self::QueryTimedOut => "Everything query timed out",
            Self::ClientClosed => "Everything IPC client is closed",
        })
    }
}

impl std::error::Error for EverythingClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Protocol(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, GetWindowThreadProcessId,
    };

    use super::{
        select_owned_window, try_acquire_query_permit, EverythingClientError,
        EverythingQueryResult, EverythingQuerySpec,
    };
    use crate::window::{self, WorkerCommand};

    const EVERYTHING_IPC_DEFAULT_WINDOW_CLASS: &str = "EVERYTHING_TASKBAR_NOTIFICATION";
    const FIND_WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(5);

    pub struct EverythingClient {
        command_tx: mpsc::SyncSender<WorkerCommand>,
        shutdown_tx: mpsc::SyncSender<()>,
        reply_hwnd_bits: usize,
        everything_hwnd_bits: usize,
        closed: Arc<AtomicBool>,
        outstanding_queries: Arc<AtomicUsize>,
        worker: Mutex<Option<JoinHandle<()>>>,
    }

    impl EverythingClient {
        pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

        pub fn connect(instance: &str, timeout: Duration) -> Result<Self, EverythingClientError> {
            let class_name = everything_window_class(instance)?;
            let started = Instant::now();
            let everything_hwnd_bits = find_everything_window(&class_name, timeout)?;
            Self::connect_to_window(
                everything_hwnd_bits,
                timeout.saturating_sub(started.elapsed()),
            )
        }

        pub fn connect_owned(
            instance: &str,
            expected_pid: u32,
            timeout: Duration,
        ) -> Result<Self, EverythingClientError> {
            let class_name = everything_window_class(instance)?;
            let started = Instant::now();
            let everything_hwnd_bits =
                find_owned_everything_window(&class_name, expected_pid, timeout)?;
            Self::connect_to_window(
                everything_hwnd_bits,
                timeout.saturating_sub(started.elapsed()),
            )
        }

        fn connect_to_window(
            everything_hwnd_bits: usize,
            worker_timeout: Duration,
        ) -> Result<Self, EverythingClientError> {
            let closed = Arc::new(AtomicBool::new(false));
            let outstanding_queries = Arc::new(AtomicUsize::new(0));
            let worker =
                window::spawn_worker(everything_hwnd_bits, worker_timeout, Arc::clone(&closed))?;
            Ok(Self {
                command_tx: worker.command_tx,
                shutdown_tx: worker.shutdown_tx,
                reply_hwnd_bits: worker.reply_hwnd_bits,
                everything_hwnd_bits,
                closed,
                outstanding_queries,
                worker: Mutex::new(Some(worker.join)),
            })
        }

        pub fn database_loaded(&self, timeout: Duration) -> Result<bool, EverythingClientError> {
            if self.closed.load(Ordering::Acquire) {
                return Err(EverythingClientError::ClientClosed);
            }
            window::database_loaded(self.everything_hwnd_bits, timeout)
        }

        pub fn query(
            &self,
            query: EverythingQuerySpec,
        ) -> Result<EverythingQueryResult, EverythingClientError> {
            if self.closed.load(Ordering::Acquire) {
                return Err(EverythingClientError::ClientClosed);
            }
            let permit = try_acquire_query_permit(&self.outstanding_queries)
                .ok_or(EverythingClientError::Overloaded)?;
            let (response_tx, response_rx) = mpsc::channel();
            match self.command_tx.try_send(WorkerCommand {
                spec: query,
                response_tx,
                permit,
            }) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) => {
                    return Err(EverythingClientError::Overloaded);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(EverythingClientError::ClientClosed);
                }
            }
            let _ = window::wake_worker(self.reply_hwnd_bits);
            response_rx
                .recv()
                .unwrap_or(Err(EverythingClientError::ClientClosed))
        }

        pub fn close(&self) {
            self.closed.store(true, Ordering::Release);
            let _ = self.shutdown_tx.try_send(());
            let _ = window::wake_worker(self.reply_hwnd_bits);
            let mut worker = self.worker.lock().expect("worker mutex poisoned");
            if let Some(join) = worker.take() {
                let _ = join.join();
            }
        }
    }

    impl Drop for EverythingClient {
        fn drop(&mut self) {
            self.close();
        }
    }

    fn everything_window_class(instance: &str) -> Result<HSTRING, EverythingClientError> {
        if instance.contains('\0') {
            return Err(EverythingClientError::InvalidInstance);
        }
        if instance.is_empty() {
            Ok(HSTRING::from(EVERYTHING_IPC_DEFAULT_WINDOW_CLASS))
        } else {
            Ok(HSTRING::from(format!(
                "{EVERYTHING_IPC_DEFAULT_WINDOW_CLASS}_({instance})"
            )))
        }
    }

    fn find_everything_window(
        class_name: &HSTRING,
        timeout: Duration,
    ) -> Result<usize, EverythingClientError> {
        let started = Instant::now();
        loop {
            if let Ok(hwnd) = unsafe { FindWindowW(class_name, PCWSTR::null()) } {
                let bits = hwnd.0 as usize;
                if bits != 0 {
                    return Ok(bits);
                }
            }
            if started.elapsed() >= timeout {
                return Err(EverythingClientError::ConnectionTimedOut);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            std::thread::sleep(FIND_WINDOW_POLL_INTERVAL.min(remaining));
        }
    }

    fn find_owned_everything_window(
        class_name: &HSTRING,
        expected_pid: u32,
        timeout: Duration,
    ) -> Result<usize, EverythingClientError> {
        if expected_pid == 0 {
            return Err(EverythingClientError::IpcOwnerMismatch);
        }
        let started = Instant::now();
        loop {
            let mut candidates = Vec::new();
            let mut after = None;
            while let Ok(hwnd) = unsafe { FindWindowExW(None, after, class_name, PCWSTR::null()) } {
                let bits = hwnd.0 as usize;
                let mut actual_pid = 0;
                if bits == 0
                    || unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual_pid)) } == 0
                    || actual_pid == 0
                {
                    return Err(EverythingClientError::IpcUnavailable);
                }
                candidates.push((bits, actual_pid));
                after = Some(hwnd);
            }
            if let Some(window) = select_owned_window(&candidates, expected_pid)? {
                return Ok(window);
            }
            if started.elapsed() >= timeout {
                return Err(EverythingClientError::ConnectionTimedOut);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            std::thread::sleep(FIND_WINDOW_POLL_INTERVAL.min(remaining));
        }
    }
}

#[cfg(windows)]
pub use imp::EverythingClient;

#[cfg(not(windows))]
pub struct EverythingClient;

#[cfg(not(windows))]
impl EverythingClient {
    pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

    pub fn connect(_instance: &str, _timeout: Duration) -> Result<Self, EverythingClientError> {
        Err(EverythingClientError::IpcUnavailable)
    }

    pub fn connect_owned(
        _instance: &str,
        _expected_pid: u32,
        _timeout: Duration,
    ) -> Result<Self, EverythingClientError> {
        Err(EverythingClientError::IpcUnavailable)
    }

    pub fn database_loaded(&self, _timeout: Duration) -> Result<bool, EverythingClientError> {
        Err(EverythingClientError::IpcUnavailable)
    }

    pub fn close(&self) {}

    pub fn query(
        &self,
        _query: EverythingQuerySpec,
    ) -> Result<EverythingQueryResult, EverythingClientError> {
        Err(EverythingClientError::IpcUnavailable)
    }
}
