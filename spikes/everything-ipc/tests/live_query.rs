#![cfg(windows)]

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use everything_ipc::client::{EverythingClient, EverythingClientError};
use everything_ipc::protocol::{
    EverythingQueryResult, EverythingQuerySpec, EverythingSort, ProtocolError,
};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, IsWindow,
    PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, TranslateMessage,
    UnregisterClassW, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COPYDATA, WM_DESTROY,
    WNDCLASSW,
};

const EVERYTHING_IPC_DEFAULT_WINDOW_CLASS: &str = "EVERYTHING_TASKBAR_NOTIFICATION";
const EVERYTHING_COPYDATA_QUERY2W: usize = 18;
const QUERY2_REPLY_HWND_OFFSET: usize = 0;
const QUERY2_REPLY_MESSAGE_OFFSET: usize = 4;
const QUERY2_REQUEST_FLAGS_OFFSET: usize = 20;
const QUERY2_SORT_TYPE_OFFSET: usize = 24;
const LIST2_HEADER_LEN: usize = 20;
const REQUEST_FLAGS: u32 = 0x0000_0001 | 0x0000_0004 | 0x0000_0040 | 0x0000_0100;
const SORT_DATE_MODIFIED_DESCENDING: u32 = 14;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const QUERY_TIMEOUT: Duration = Duration::from_millis(80);
const ASSERT_STILL_PENDING: Duration = Duration::from_millis(20);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

static TEST_SERIAL: Mutex<()> = Mutex::new(());
static FAKE_STATE: OnceLock<Mutex<Option<Arc<FakeState>>>> = OnceLock::new();
static FAKE_SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
struct CapturedQuery {
    recipient_hwnd: usize,
    copydata_kind: usize,
    wparam_reply_hwnd: usize,
    encoded_reply_hwnd: u32,
    reply_copydata_message: u32,
    request_flags: u32,
    sort_type: u32,
}

struct FakeState {
    captured_tx: mpsc::Sender<CapturedQuery>,
    recipient_hwnd: AtomicUsize,
    active_request_id: Mutex<Option<u32>>,
    cancelled_request_ids: Mutex<HashSet<u32>>,
    cancellation_count: AtomicUsize,
}

struct FakeEverything {
    instance: String,
    window_class: String,
    hwnd: HWND,
    captured_rx: mpsc::Receiver<CapturedQuery>,
    stopped_rx: mpsc::Receiver<()>,
    state: Arc<FakeState>,
    thread: Option<JoinHandle<()>>,
}

impl FakeEverything {
    fn start() -> Self {
        let sequence = next_fake_session_sequence();
        let instance = format!("UiPilotIpcTest_{}_{}", std::process::id(), sequence);
        let window_class = named_instance_window_class(&instance);
        let (captured_tx, captured_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let state = Arc::new(FakeState {
            captured_tx,
            recipient_hwnd: AtomicUsize::new(0),
            active_request_id: Mutex::new(None),
            cancelled_request_ids: Mutex::new(HashSet::new()),
            cancellation_count: AtomicUsize::new(0),
        });
        *fake_state_slot().lock().expect("fake state mutex poisoned") = Some(Arc::clone(&state));
        let thread_window_class = HSTRING::from(&window_class);
        let thread_state = Arc::clone(&state);

        let thread = thread::spawn(move || {
            let result = create_fake_window(&thread_window_class);
            let (hwnd, module) = match result {
                Ok(created) => created,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    let _ = stopped_tx.send(());
                    return;
                }
            };
            thread_state
                .recipient_hwnd
                .store(hwnd_bits(hwnd), Ordering::Release);
            ready_tx
                .send(Ok(hwnd_bits(hwnd)))
                .expect("test owner dropped before fake window became ready");

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
            }
            let _ = unsafe { UnregisterClassW(&thread_window_class, Some(module)) };
            let _ = stopped_tx.send(());
        });

        let hwnd_bits = ready_rx
            .recv_timeout(CLEANUP_TIMEOUT)
            .expect("fake Everything window startup timed out")
            .expect("fake Everything window startup failed");

        Self {
            instance,
            window_class,
            hwnd: hwnd_from_bits(hwnd_bits),
            captured_rx,
            stopped_rx,
            state,
            thread: Some(thread),
        }
    }

    fn capture(&self) -> CapturedQuery {
        let query = self
            .captured_rx
            .recv_timeout(CLEANUP_TIMEOUT)
            .expect("client did not send Query2 before cleanup deadline");
        assert_eq!(query.recipient_hwnd, hwnd_bits(self.hwnd));
        query
    }

    fn connect_client(&self) -> Arc<EverythingClient> {
        assert!(!self.instance.is_empty());
        assert_ne!(self.window_class, EVERYTHING_IPC_DEFAULT_WINDOW_CLASS);
        assert_eq!(
            self.window_class,
            named_instance_window_class(&self.instance)
        );
        Arc::new(
            EverythingClient::connect(&self.instance, CONNECT_TIMEOUT)
                .expect("client should find the isolated fake Everything instance"),
        )
    }

    fn send_valid_empty_reply(&self, query: &CapturedQuery) {
        if self.state.finish_reply(query.reply_copydata_message) {
            self.send_empty_reply_from(self.hwnd, query.reply_copydata_message, query);
        }
    }

    fn send_null_empty_reply(&self, query: &CapturedQuery) {
        if !self.state.finish_reply(query.reply_copydata_message) {
            return;
        }
        let copydata = COPYDATASTRUCT {
            dwData: query.reply_copydata_message as usize,
            cbData: 0,
            lpData: std::ptr::null_mut(),
        };
        unsafe {
            SendMessageW(
                hwnd_from_u32(query.encoded_reply_hwnd),
                WM_COPYDATA,
                Some(WPARAM(hwnd_bits(self.hwnd))),
                Some(LPARAM((&copydata as *const COPYDATASTRUCT) as isize)),
            );
        }
    }

    fn send_empty_reply_from(&self, source_hwnd: HWND, request_id: u32, query: &CapturedQuery) {
        let payload = empty_list2_payload(query.request_flags, query.sort_type);
        let copydata = COPYDATASTRUCT {
            dwData: request_id as usize,
            cbData: payload.len() as u32,
            lpData: payload.as_ptr().cast_mut().cast::<c_void>(),
        };
        let reply_hwnd = hwnd_from_u32(query.encoded_reply_hwnd);
        unsafe {
            SendMessageW(
                reply_hwnd,
                WM_COPYDATA,
                Some(WPARAM(hwnd_bits(source_hwnd))),
                Some(LPARAM((&copydata as *const COPYDATASTRUCT) as isize)),
            );
        }
    }

    fn send_unknown_reply_with_invalid_payload(&self, request_id: u32, query: &CapturedQuery) {
        let copydata = COPYDATASTRUCT {
            dwData: request_id as usize,
            cbData: 1,
            lpData: std::ptr::dangling_mut::<c_void>(),
        };
        unsafe {
            SendMessageW(
                hwnd_from_u32(query.encoded_reply_hwnd),
                WM_COPYDATA,
                Some(WPARAM(hwnd_bits(self.hwnd))),
                Some(LPARAM((&copydata as *const COPYDATASTRUCT) as isize)),
            );
        }
    }

    fn assert_no_query_after_return(&self, timeout: Duration) {
        assert!(matches!(
            self.captured_rx.recv_timeout(timeout),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    }

    fn stop_client_window(&self, reply_hwnd: u32) {
        unsafe {
            PostMessageW(
                Some(hwnd_from_u32(reply_hwnd)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
            .expect("failed to stop client reply window");
        }
    }

    fn mismatched_source(&self) -> HWND {
        hwnd_from_bits(hwnd_bits(self.hwnd).wrapping_add(1))
    }

    fn cancellation_count(&self) -> usize {
        self.state.cancellation_count.load(Ordering::Acquire)
    }
}

impl FakeState {
    fn record_query(&self, request_id: u32) {
        let previous = self
            .active_request_id
            .lock()
            .expect("fake active request mutex poisoned")
            .replace(request_id);
        if let Some(previous) = previous {
            self.cancelled_request_ids
                .lock()
                .expect("fake cancelled request mutex poisoned")
                .insert(previous);
            self.cancellation_count.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn finish_reply(&self, request_id: u32) -> bool {
        if self
            .cancelled_request_ids
            .lock()
            .expect("fake cancelled request mutex poisoned")
            .remove(&request_id)
        {
            return false;
        }
        let mut active_request_id = self
            .active_request_id
            .lock()
            .expect("fake active request mutex poisoned");
        if *active_request_id == Some(request_id) {
            *active_request_id = None;
        }
        true
    }
}

impl Drop for FakeEverything {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if self.stopped_rx.recv_timeout(CLEANUP_TIMEOUT).is_ok() {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
        *fake_state_slot().lock().expect("fake state mutex poisoned") = None;
    }
}

fn fake_state_slot() -> &'static Mutex<Option<Arc<FakeState>>> {
    FAKE_STATE.get_or_init(|| Mutex::new(None))
}

fn next_fake_session_sequence() -> u64 {
    let mut current = FAKE_SESSION_SEQUENCE.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .expect("fake Everything session sequence exhausted");
        match FAKE_SESSION_SEQUENCE.compare_exchange_weak(
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

fn named_instance_window_class(instance: &str) -> String {
    assert!(!instance.is_empty());
    format!("{EVERYTHING_IPC_DEFAULT_WINDOW_CLASS}_({instance})")
}

fn create_fake_window(class_name: &HSTRING) -> Result<(HWND, HINSTANCE), String> {
    let instance = register_fake_window_class(class_name)?;
    let title = HSTRING::from(format!(
        "UiPilot Everything IPC fake {}",
        std::process::id()
    ));
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
            None,
        )
    };
    match created {
        Ok(hwnd) => Ok((hwnd, instance)),
        Err(error) => {
            let _ = unsafe { UnregisterClassW(class_name, Some(instance)) };
            Err(error.to_string())
        }
    }
}

fn register_fake_window_class(class_name: &HSTRING) -> Result<HINSTANCE, String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let instance = HINSTANCE(module.0);
    let window_class = WNDCLASSW {
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        lpfnWndProc: Some(fake_window_proc),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        Err("failed to register fake Everything IPC window class".to_owned())
    } else {
        Ok(instance)
    }
}

unsafe extern "system" fn fake_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COPYDATA => {
            if let Some(state) = fake_state_slot()
                .lock()
                .expect("fake state mutex poisoned")
                .as_ref()
                .cloned()
            {
                if state.recipient_hwnd.load(Ordering::Acquire) == hwnd_bits(hwnd) {
                    if let Some(query) = capture_query(hwnd, wparam, lparam) {
                        state.record_query(query.reply_copydata_message);
                        let _ = state.captured_tx.send(query);
                    }
                }
            }
            LRESULT(1)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn capture_query(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<CapturedQuery> {
    let copydata = (lparam.0 as *const COPYDATASTRUCT).as_ref()?;
    let payload =
        std::slice::from_raw_parts(copydata.lpData.cast::<u8>(), copydata.cbData as usize);
    Some(CapturedQuery {
        recipient_hwnd: hwnd_bits(hwnd),
        copydata_kind: copydata.dwData,
        wparam_reply_hwnd: wparam.0,
        encoded_reply_hwnd: read_u32(payload, QUERY2_REPLY_HWND_OFFSET)?,
        reply_copydata_message: read_u32(payload, QUERY2_REPLY_MESSAGE_OFFSET)?,
        request_flags: read_u32(payload, QUERY2_REQUEST_FLAGS_OFFSET)?,
        sort_type: read_u32(payload, QUERY2_SORT_TYPE_OFFSET)?,
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let field = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(field.try_into().ok()?))
}

fn empty_list2_payload(request_flags: u32, sort_type: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(LIST2_HEADER_LEN);
    for value in [0, 0, 0, request_flags, sort_type] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload
}

fn query_spec(timeout: Duration) -> EverythingQuerySpec {
    EverythingQuerySpec {
        search: "needle".encode_utf16().collect(),
        offset: 0,
        max_results: 200,
        request_flags: REQUEST_FLAGS,
        sort: EverythingSort::DateModifiedDescending,
        deadline: Instant::now() + timeout,
    }
}

fn spawn_query(
    client: Arc<EverythingClient>,
    timeout: Duration,
) -> mpsc::Receiver<Result<EverythingQueryResult, EverythingClientError>> {
    let (result_tx, result_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = result_tx.send(client.query(query_spec(timeout)));
    });
    result_rx
}

fn assert_query_route(query: &CapturedQuery) {
    assert_eq!(query.copydata_kind, EVERYTHING_COPYDATA_QUERY2W);
    assert_ne!(query.encoded_reply_hwnd, 0);
    assert_eq!(query.wparam_reply_hwnd as u32, query.encoded_reply_hwnd);
    assert_ne!(query.reply_copydata_message, 0);
    assert_eq!(query.request_flags, REQUEST_FLAGS);
    assert_eq!(query.sort_type, SORT_DATE_MODIFIED_DESCENDING);
}

fn expect_empty_result(
    receiver: &mpsc::Receiver<Result<EverythingQueryResult, EverythingClientError>>,
) {
    let result = receiver
        .recv_timeout(CLEANUP_TIMEOUT)
        .expect("query did not finish before cleanup deadline")
        .expect("query failed");
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());
}

fn assert_window_destroyed(hwnd: u32) {
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    while Instant::now() < deadline {
        if !unsafe { IsWindow(Some(hwnd_from_u32(hwnd))).as_bool() } {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("client reply window survived the cleanup deadline");
}

fn hwnd_bits(hwnd: HWND) -> usize {
    hwnd.0 as usize
}

fn hwnd_from_bits(bits: usize) -> HWND {
    HWND(bits as *mut c_void)
}

fn hwnd_from_u32(bits: u32) -> HWND {
    hwnd_from_bits(bits as usize)
}

#[test]
fn null_empty_copydata_payload_is_admitted_then_rejected_by_protocol() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();
    let result = spawn_query(Arc::clone(&client), QUERY_TIMEOUT);
    let query = fake.capture();

    fake.send_null_empty_reply(&query);

    assert_eq!(
        result
            .recv_timeout(CLEANUP_TIMEOUT)
            .expect("null-empty reply did not complete"),
        Err(EverythingClientError::Protocol(
            ProtocolError::PayloadTooShort
        ))
    );
}

#[test]
fn concurrent_calls_wait_for_active_reply_without_peer_cancellation() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();
    let first_result = spawn_query(Arc::clone(&client), Duration::from_millis(500));
    let first = fake.capture();

    let second_result = spawn_query(Arc::clone(&client), Duration::from_millis(500));
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);
    assert_eq!(fake.cancellation_count(), 0);

    fake.send_valid_empty_reply(&first);
    expect_empty_result(&first_result);
    let second = fake.capture();
    fake.send_valid_empty_reply(&second);
    expect_empty_result(&second_result);
    assert_eq!(fake.cancellation_count(), 0);
}

#[test]
fn queued_call_expires_at_its_original_deadline_before_dispatch() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();
    let first_result = spawn_query(Arc::clone(&client), Duration::from_millis(500));
    let first = fake.capture();
    let queued_result = spawn_query(Arc::clone(&client), Duration::from_millis(35));

    assert_eq!(
        queued_result
            .recv_timeout(Duration::from_millis(250))
            .expect("queued query did not honor its original deadline"),
        Err(EverythingClientError::QueryTimedOut)
    );
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);
    assert_eq!(fake.cancellation_count(), 0);

    fake.send_valid_empty_reply(&first);
    expect_empty_result(&first_result);
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);
}

#[test]
fn reply_window_lifecycle_and_message_ids_are_stable() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();

    let first_result = spawn_query(Arc::clone(&client), QUERY_TIMEOUT);
    let first = fake.capture();
    assert_query_route(&first);
    fake.send_valid_empty_reply(&first);
    expect_empty_result(&first_result);

    let second_result = spawn_query(Arc::clone(&client), QUERY_TIMEOUT);
    let second = fake.capture();
    assert_query_route(&second);
    assert!(second.reply_copydata_message > first.reply_copydata_message);
    let third_result = spawn_query(Arc::clone(&client), QUERY_TIMEOUT);
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);
    fake.send_valid_empty_reply(&second);
    expect_empty_result(&second_result);
    let third = fake.capture();
    assert_query_route(&third);
    assert!(third.reply_copydata_message > second.reply_copydata_message);
    fake.send_valid_empty_reply(&third);
    expect_empty_result(&third_result);

    let fourth_result = spawn_query(Arc::clone(&client), QUERY_TIMEOUT);
    let fourth = fake.capture();
    assert!(fourth.reply_copydata_message > third.reply_copydata_message);
    fake.send_valid_empty_reply(&fourth);
    expect_empty_result(&fourth_result);
    assert_eq!(fake.cancellation_count(), 0);

    let reply_hwnd = fourth.encoded_reply_hwnd;
    drop(client);
    assert_window_destroyed(reply_hwnd);
}

#[test]
fn source_hwnd_and_dwdata_must_match_the_pending_request() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();
    let first_result = spawn_query(Arc::clone(&client), Duration::from_millis(500));
    let first = fake.capture();
    let second_result = spawn_query(Arc::clone(&client), Duration::from_millis(500));
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);

    let unknown_id = first.reply_copydata_message.wrapping_add(10_000);
    fake.send_unknown_reply_with_invalid_payload(unknown_id, &first);
    fake.send_empty_reply_from(fake.hwnd, unknown_id, &first);
    fake.send_empty_reply_from(
        fake.mismatched_source(),
        first.reply_copydata_message,
        &first,
    );
    assert!(matches!(
        first_result.recv_timeout(ASSERT_STILL_PENDING),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(matches!(
        second_result.recv_timeout(ASSERT_STILL_PENDING),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    fake.send_valid_empty_reply(&first);
    expect_empty_result(&first_result);
    let second = fake.capture();
    fake.send_valid_empty_reply(&second);
    expect_empty_result(&second_result);
    assert_eq!(fake.cancellation_count(), 0);
}

#[test]
fn timeout_and_late_reply_do_not_pollute_the_next_request() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();
    assert_eq!(
        EverythingClient::DEFAULT_QUERY_TIMEOUT,
        Duration::from_secs(1)
    );

    let timed_out_result = spawn_query(Arc::clone(&client), Duration::from_millis(35));
    let timed_out_query = fake.capture();
    assert_eq!(
        timed_out_result
            .recv_timeout(CLEANUP_TIMEOUT)
            .expect("timed query did not complete"),
        Err(EverythingClientError::QueryTimedOut)
    );

    let next_result = spawn_query(Arc::clone(&client), Duration::from_millis(500));
    let next_query = fake.capture();
    assert!(next_query.reply_copydata_message > timed_out_query.reply_copydata_message);
    fake.send_valid_empty_reply(&timed_out_query);
    assert!(matches!(
        next_result.recv_timeout(ASSERT_STILL_PENDING),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    fake.send_valid_empty_reply(&next_query);
    expect_empty_result(&next_result);
}

#[test]
fn worker_exit_fails_all_pending_requests_with_a_fixed_error() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let fake = FakeEverything::start();
    let client = fake.connect_client();

    let receivers: Vec<_> = (0..3)
        .map(|_| spawn_query(Arc::clone(&client), Duration::from_secs(1)))
        .collect();
    let active_query = fake.capture();
    let reply_hwnd = active_query.encoded_reply_hwnd;
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);

    fake.stop_client_window(reply_hwnd);
    assert_eq!(
        client.query(query_spec(QUERY_TIMEOUT)),
        Err(EverythingClientError::ClientClosed)
    );
    fake.assert_no_query_after_return(ASSERT_STILL_PENDING);
    for receiver in receivers {
        assert_eq!(
            receiver
                .recv_timeout(CLEANUP_TIMEOUT)
                .expect("pending query was not failed during client shutdown"),
            Err(EverythingClientError::ClientClosed)
        );
    }
    drop(client);
    assert_window_destroyed(reply_hwnd);
}

#[test]
fn thousand_queries_have_no_crosstalk_timeout_leak_or_id_reuse() {
    let _serial = TEST_SERIAL.lock().expect("test serial mutex poisoned");
    let gate_deadline = Instant::now() + Duration::from_secs(30);
    let fake = FakeEverything::start();
    let client = fake.connect_client();
    let mut seen_ids = HashSet::new();
    let mut last_id = 0;

    for _ in 0..500 {
        let remaining = gate_deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "sequential gate deadline elapsed");
        let result = spawn_query(Arc::clone(&client), remaining.min(Duration::from_secs(5)));
        let query = fake.capture();
        assert!(query.reply_copydata_message > last_id);
        assert!(seen_ids.insert(query.reply_copydata_message));
        last_id = query.reply_copydata_message;
        fake.send_valid_empty_reply(&query);
        expect_empty_result(&result);
    }

    let remaining = gate_deadline.saturating_duration_since(Instant::now());
    assert!(!remaining.is_zero(), "concurrent gate deadline elapsed");
    let receivers: Vec<_> = (0..500)
        .map(|_| spawn_query(Arc::clone(&client), remaining.min(Duration::from_secs(10))))
        .collect();
    for _ in 0..500 {
        let query = fake.capture();
        assert!(query.reply_copydata_message > last_id);
        assert!(seen_ids.insert(query.reply_copydata_message));
        last_id = query.reply_copydata_message;
        fake.send_valid_empty_reply(&query);
    }
    for receiver in &receivers {
        expect_empty_result(receiver);
    }
    assert_eq!(seen_ids.len(), 1_000);
    assert_eq!(fake.cancellation_count(), 0);

    let final_result = spawn_query(Arc::clone(&client), QUERY_TIMEOUT);
    let final_query = fake.capture();
    assert!(final_query.reply_copydata_message > last_id);
    assert!(seen_ids.insert(final_query.reply_copydata_message));
    fake.send_valid_empty_reply(&final_query);
    expect_empty_result(&final_result);

    let reply_hwnd = final_query.encoded_reply_hwnd;
    drop(client);
    assert_window_destroyed(reply_hwnd);
}
