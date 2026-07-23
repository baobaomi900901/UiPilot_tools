#![cfg(windows)]

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, IsWindow, KillTimer, PostMessageW, PostQuitMessage, RegisterClassW,
    SendMessageTimeoutW, SetTimer, SetWindowLongPtrW, TranslateMessage, UnregisterClassW,
    CREATESTRUCTW, GWLP_USERDATA, MSG, SMTO_ABORTIFHUNG, SMTO_ERRORONEXIT, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COPYDATA, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_TIMER,
    WNDCLASSW,
};

use crate::client::EverythingClientError;
use crate::protocol::{
    decode_list2_payload, encode_query2, EverythingQueryResult, EverythingQuerySpec,
    QueryReplyRoute,
};

const EVERYTHING_COPYDATA_QUERY2W: usize = 18;
const WORKER_WAKE_MESSAGE: u32 = WM_APP + 0x31;
const WORKER_TIMER_ID: usize = 1;
const WORKER_TIMER_INTERVAL_MS: u32 = 5;
const MAX_REPLY_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_SEND_TIMEOUT: Duration = Duration::from_secs(1);

static REPLY_CLASS_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) enum WorkerCommand {
    Query {
        spec: EverythingQuerySpec,
        response_tx: mpsc::Sender<Result<EverythingQueryResult, EverythingClientError>>,
    },
    Shutdown,
}

pub(crate) struct WorkerParts {
    pub(crate) command_tx: mpsc::Sender<WorkerCommand>,
    pub(crate) reply_hwnd_bits: usize,
    pub(crate) join: JoinHandle<()>,
}

struct ReplyEnvelope {
    request_id: u32,
    payload: Vec<u8>,
}

struct WindowBinding {
    everything_hwnd_bits: usize,
    active_request_ids: Arc<Mutex<HashSet<u32>>>,
    envelope_tx: mpsc::Sender<ReplyEnvelope>,
}

struct PendingQuery {
    deadline: Instant,
    response_tx: mpsc::Sender<Result<EverythingQueryResult, EverythingClientError>>,
}

struct WorkerState {
    everything_hwnd: HWND,
    reply_hwnd: HWND,
    reply_hwnd_u32: u32,
    command_rx: mpsc::Receiver<WorkerCommand>,
    envelope_rx: mpsc::Receiver<ReplyEnvelope>,
    active_request_ids: Arc<Mutex<HashSet<u32>>>,
    pending: HashMap<u32, PendingQuery>,
    next_request_id: Option<u32>,
}

pub(crate) fn spawn_worker(
    everything_hwnd_bits: usize,
    startup_timeout: Duration,
    closed: Arc<AtomicBool>,
) -> Result<WorkerParts, EverythingClientError> {
    if everything_hwnd_bits == 0 {
        return Err(EverythingClientError::IpcUnavailable);
    }
    let (command_tx, command_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let startup_cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&startup_cancelled);
    let worker_closed = Arc::clone(&closed);
    let join = thread::spawn(move || {
        run_worker(
            everything_hwnd_bits,
            command_rx,
            ready_tx,
            worker_cancelled,
            worker_closed,
        );
    });

    match ready_rx.recv_timeout(startup_timeout) {
        Ok(Ok(reply_hwnd_bits)) => Ok(WorkerParts {
            command_tx,
            reply_hwnd_bits,
            join,
        }),
        Ok(Err(error)) => {
            closed.store(true, Ordering::Release);
            let _ = join.join();
            Err(error)
        }
        Err(_) => {
            startup_cancelled.store(true, Ordering::Release);
            closed.store(true, Ordering::Release);
            let _ = join.join();
            Err(EverythingClientError::IpcUnavailable)
        }
    }
}

pub(crate) fn wake_worker(reply_hwnd_bits: usize) -> Result<(), EverythingClientError> {
    if reply_hwnd_bits == 0 {
        return Err(EverythingClientError::ClientClosed);
    }
    unsafe {
        PostMessageW(
            Some(hwnd_from_bits(reply_hwnd_bits)),
            WORKER_WAKE_MESSAGE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .map_err(|_| EverythingClientError::IpcSendFailed)
}

fn run_worker(
    everything_hwnd_bits: usize,
    command_rx: mpsc::Receiver<WorkerCommand>,
    ready_tx: mpsc::Sender<Result<usize, EverythingClientError>>,
    startup_cancelled: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
) {
    let everything_hwnd = hwnd_from_bits(everything_hwnd_bits);
    let (envelope_tx, envelope_rx) = mpsc::channel();
    let active_request_ids = Arc::new(Mutex::new(HashSet::new()));
    let binding = Box::new(WindowBinding {
        everything_hwnd_bits,
        active_request_ids: Arc::clone(&active_request_ids),
        envelope_tx,
    });
    let class_name = HSTRING::from(format!(
        "UiPilotEverythingReply_{}_{}",
        std::process::id(),
        next_reply_class_sequence()
    ));
    let created = create_reply_window(&class_name, binding.as_ref());
    let (reply_hwnd, instance) = match created {
        Ok(created) => created,
        Err(error) => {
            closed.store(true, Ordering::Release);
            let _ = ready_tx.send(Err(error));
            drain_commands_closed(&command_rx);
            return;
        }
    };
    let reply_hwnd_bits = hwnd_bits(reply_hwnd);
    let reply_hwnd_u32 = match u32::try_from(reply_hwnd_bits) {
        Ok(value) if value != 0 => value,
        _ => {
            destroy_reply_window(reply_hwnd, &class_name, instance);
            closed.store(true, Ordering::Release);
            let _ = ready_tx.send(Err(EverythingClientError::IpcUnavailable));
            drain_commands_closed(&command_rx);
            return;
        }
    };
    let timer_id = unsafe {
        SetTimer(
            Some(reply_hwnd),
            WORKER_TIMER_ID,
            WORKER_TIMER_INTERVAL_MS,
            None,
        )
    };
    if timer_id == 0 {
        destroy_reply_window(reply_hwnd, &class_name, instance);
        closed.store(true, Ordering::Release);
        let _ = ready_tx.send(Err(EverythingClientError::IpcUnavailable));
        drain_commands_closed(&command_rx);
        return;
    }
    if startup_cancelled.load(Ordering::Acquire) || ready_tx.send(Ok(reply_hwnd_bits)).is_err() {
        unsafe {
            let _ = KillTimer(Some(reply_hwnd), WORKER_TIMER_ID);
        }
        destroy_reply_window(reply_hwnd, &class_name, instance);
        closed.store(true, Ordering::Release);
        drain_commands_closed(&command_rx);
        return;
    }

    let mut state = WorkerState {
        everything_hwnd,
        reply_hwnd,
        reply_hwnd_u32,
        command_rx,
        envelope_rx,
        active_request_ids,
        pending: HashMap::new(),
        next_request_id: Some(1),
    };
    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if status.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if !unsafe { IsWindow(Some(reply_hwnd)).as_bool() } {
            break;
        }
        if state.drain_commands() {
            unsafe {
                let _ = DestroyWindow(reply_hwnd);
            }
            break;
        }
        state.drain_envelopes();
        state.expire_queries();
    }

    closed.store(true, Ordering::Release);
    state.close_all();
    unsafe {
        let _ = KillTimer(Some(reply_hwnd), WORKER_TIMER_ID);
        if IsWindow(Some(reply_hwnd)).as_bool() {
            let _ = DestroyWindow(reply_hwnd);
        }
    }
    drop(state);
    drop(binding);
    unsafe {
        let _ = UnregisterClassW(&class_name, Some(instance));
    }
}

impl WorkerState {
    fn drain_commands(&mut self) -> bool {
        loop {
            match self.command_rx.try_recv() {
                Ok(WorkerCommand::Query { spec, response_tx }) => {
                    self.start_query(spec, response_tx);
                }
                Ok(WorkerCommand::Shutdown) => return true,
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => return true,
            }
        }
    }

    fn start_query(
        &mut self,
        spec: EverythingQuerySpec,
        response_tx: mpsc::Sender<Result<EverythingQueryResult, EverythingClientError>>,
    ) {
        if spec.deadline <= Instant::now() {
            let _ = response_tx.send(Err(EverythingClientError::QueryTimedOut));
            return;
        }
        let request_id = match self.next_request_id {
            Some(request_id) => request_id,
            None => {
                let _ = response_tx.send(Err(EverythingClientError::RequestIdExhausted));
                return;
            }
        };
        self.next_request_id = request_id.checked_add(1);
        let encoded = match encode_query2(
            &spec,
            QueryReplyRoute {
                reply_hwnd: self.reply_hwnd_u32,
                reply_copydata_message: request_id,
            },
        ) {
            Ok(encoded) => encoded,
            Err(error) => {
                let _ = response_tx.send(Err(EverythingClientError::Protocol(error)));
                return;
            }
        };
        let registered = self
            .active_request_ids
            .lock()
            .map(|mut active_request_ids| active_request_ids.insert(request_id))
            .unwrap_or(false);
        if !registered {
            let _ = response_tx.send(Err(EverythingClientError::IpcUnavailable));
            return;
        }
        self.pending.insert(
            request_id,
            PendingQuery {
                deadline: spec.deadline,
                response_tx,
            },
        );
        if !send_query2(
            self.everything_hwnd,
            self.reply_hwnd,
            &encoded,
            spec.deadline,
        ) {
            self.remove_active_request(request_id);
            if let Some(pending) = self.pending.remove(&request_id) {
                let _ = pending
                    .response_tx
                    .send(Err(EverythingClientError::IpcSendFailed));
            }
        }
    }

    fn drain_envelopes(&mut self) {
        while let Ok(envelope) = self.envelope_rx.try_recv() {
            let Some(pending) = self.pending.remove(&envelope.request_id) else {
                continue;
            };
            if pending.deadline <= Instant::now() {
                let _ = pending
                    .response_tx
                    .send(Err(EverythingClientError::QueryTimedOut));
                continue;
            }
            let result =
                decode_list2_payload(&envelope.payload).map_err(EverythingClientError::Protocol);
            let _ = pending.response_tx.send(result);
        }
    }

    fn expire_queries(&mut self) {
        let now = Instant::now();
        let expired: Vec<u32> = self
            .pending
            .iter()
            .filter_map(|(request_id, pending)| (pending.deadline <= now).then_some(*request_id))
            .collect();
        for request_id in expired {
            self.remove_active_request(request_id);
            if let Some(pending) = self.pending.remove(&request_id) {
                let _ = pending
                    .response_tx
                    .send(Err(EverythingClientError::QueryTimedOut));
            }
        }
    }

    fn close_all(&mut self) {
        if let Ok(mut active_request_ids) = self.active_request_ids.lock() {
            active_request_ids.clear();
        }
        for (_, pending) in self.pending.drain() {
            let _ = pending
                .response_tx
                .send(Err(EverythingClientError::ClientClosed));
        }
        drain_commands_closed(&self.command_rx);
    }

    fn remove_active_request(&self, request_id: u32) {
        if let Ok(mut active_request_ids) = self.active_request_ids.lock() {
            active_request_ids.remove(&request_id);
        }
    }
}

fn drain_commands_closed(command_rx: &mpsc::Receiver<WorkerCommand>) {
    while let Ok(command) = command_rx.try_recv() {
        if let WorkerCommand::Query { response_tx, .. } = command {
            let _ = response_tx.send(Err(EverythingClientError::ClientClosed));
        }
    }
}

fn send_query2(everything_hwnd: HWND, reply_hwnd: HWND, encoded: &[u8], deadline: Instant) -> bool {
    let Ok(cb_data) = u32::try_from(encoded.len()) else {
        return false;
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    let send_timeout = remaining.min(MAX_SEND_TIMEOUT);
    let timeout_ms = send_timeout.as_millis().clamp(1, u32::MAX as u128) as u32;
    let copydata = COPYDATASTRUCT {
        dwData: EVERYTHING_COPYDATA_QUERY2W,
        cbData: cb_data,
        lpData: encoded.as_ptr().cast_mut().cast::<c_void>(),
    };
    unsafe {
        SendMessageTimeoutW(
            everything_hwnd,
            WM_COPYDATA,
            WPARAM(hwnd_bits(reply_hwnd)),
            LPARAM((&copydata as *const COPYDATASTRUCT) as isize),
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            timeout_ms,
            None,
        )
        .0 != 0
    }
}

fn create_reply_window(
    class_name: &HSTRING,
    binding: &WindowBinding,
) -> Result<(HWND, HINSTANCE), EverythingClientError> {
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|_| EverythingClientError::IpcUnavailable)?;
    let instance = HINSTANCE(module.0);
    let window_class = WNDCLASSW {
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        lpfnWndProc: Some(reply_window_proc),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(EverythingClientError::IpcUnavailable);
    }
    let title = HSTRING::from("UiPilot Everything IPC reply");
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            &title,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            Some((binding as *const WindowBinding).cast::<c_void>()),
        )
    };
    match created {
        Ok(hwnd) => Ok((hwnd, instance)),
        Err(_) => {
            unsafe {
                let _ = UnregisterClassW(class_name, Some(instance));
            }
            Err(EverythingClientError::IpcUnavailable)
        }
    }
}

fn destroy_reply_window(hwnd: HWND, class_name: &HSTRING, instance: HINSTANCE) {
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        let _ = UnregisterClassW(class_name, Some(instance));
    }
}

fn next_reply_class_sequence() -> u64 {
    let mut current = REPLY_CLASS_SEQUENCE.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .expect("reply window class sequence exhausted");
        match REPLY_CLASS_SEQUENCE.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

unsafe extern "system" fn reply_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            LRESULT(1)
        }
        WM_COPYDATA => handle_copydata(hwnd, wparam, lparam),
        WORKER_WAKE_MESSAGE | WM_TIMER => LRESULT(0),
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn handle_copydata(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let binding_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowBinding;
    let Some(binding) = binding_ptr.as_ref() else {
        return LRESULT(0);
    };
    if wparam.0 != binding.everything_hwnd_bits {
        return LRESULT(0);
    }
    let Some(copydata) = (lparam.0 as *const COPYDATASTRUCT).as_ref() else {
        return LRESULT(0);
    };
    let Ok(request_id) = u32::try_from(copydata.dwData) else {
        return LRESULT(0);
    };
    let Ok(mut active_request_ids) = binding.active_request_ids.lock() else {
        return LRESULT(0);
    };
    if !active_request_ids.contains(&request_id) {
        return LRESULT(0);
    }
    let payload_len = copydata.cbData as usize;
    if payload_len > MAX_REPLY_PAYLOAD_BYTES || (payload_len != 0 && copydata.lpData.is_null()) {
        active_request_ids.remove(&request_id);
        let _ = binding.envelope_tx.send(ReplyEnvelope {
            request_id,
            payload: Vec::new(),
        });
        return LRESULT(1);
    }
    let payload = std::slice::from_raw_parts(copydata.lpData.cast::<u8>(), payload_len).to_vec();
    active_request_ids.remove(&request_id);
    let _ = binding.envelope_tx.send(ReplyEnvelope {
        request_id,
        payload,
    });
    LRESULT(1)
}

fn hwnd_bits(hwnd: HWND) -> usize {
    hwnd.0 as usize
}

fn hwnd_from_bits(bits: usize) -> HWND {
    HWND(bits as *mut c_void)
}
