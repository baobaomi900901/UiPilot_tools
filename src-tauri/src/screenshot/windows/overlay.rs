use super::*;
use crate::screenshot::selection::{Direction, SelectionInput, SelectionOutcome, SelectionState};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{
            COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_FAILED, WAIT_OBJECT_0,
            WPARAM,
        },
        Graphics::Gdi::*,
        System::{
            LibraryLoader::GetModuleHandleW,
            RemoteDesktop::*,
            Threading::{GetCurrentThreadId, INFINITE},
        },
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

enum Event {
    Input(SelectionInput),
    Down(HWND, PhysicalPoint, u32),
    Up(PhysicalPoint),
    Cancel(CancelReason),
    CaptureLost,
    Tick,
    PaintFailed,
}
struct State {
    desktop: capture::Desktop,
    selection: RefCell<SelectionState>,
    palette: OverlayPalette,
    frozen: RefCell<Option<capture::Desktop>>,
    fallback: Cell<bool>,
    target: Cell<Option<crate::screenshot::probe::ProbeTarget>>,
    presented_at: Cell<Option<u32>>,
    presented_candidate: Cell<
        Option<(
            Option<PhysicalRect>,
            Option<crate::screenshot::probe::ProbeTarget>,
        )>,
    >,
    events: RefCell<VecDeque<Event>>,
    closing: Cell<bool>,
}
struct WindowContext {
    state: *const State,
    index: usize,
    cancellation: *const Cancellation,
}
struct OwnedWindow {
    hwnd: HWND,
    _context: Box<WindowContext>,
    session_notifications: bool,
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            if self.session_notifications {
                let _ = WTSUnRegisterSessionNotification(self.hwnd);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
struct WindowClass {
    name: Vec<u16>,
    instance: HINSTANCE,
}
impl WindowClass {
    fn register() -> Result<Self, NativeError> {
        unsafe {
            let instance = HINSTANCE(
                GetModuleHandleW(None)
                    .map_err(|_| NativeError::OverlayFailed)?
                    .0,
            );
            let name: Vec<u16> = format!("UiPilotScreenshot-{}", GetCurrentThreadId())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: PCWSTR(name.as_ptr()),
                hCursor: LoadCursorW(None, IDC_CROSS).map_err(|_| NativeError::OverlayFailed)?,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(NativeError::OverlayFailed);
            }
            Ok(Self { name, instance })
        }
    }
}
impl Drop for WindowClass {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterClassW(PCWSTR(self.name.as_ptr()), Some(self.instance));
        }
    }
}
struct DpiContext(DPI_AWARENESS_CONTEXT);
impl DpiContext {
    fn enter() -> Result<Self, NativeError> {
        unsafe {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            if previous.0.is_null() {
                Err(NativeError::OverlayFailed)
            } else {
                Ok(Self(previous))
            }
        }
    }
}
impl Drop for DpiContext {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}
// A separate native pump observes uncoalesced low-level mouse events while GDI
// capture blocks the overlay pump. It never swallows or synthesizes input.
#[derive(Default)]
struct MotionHistory {
    gestures: VecDeque<ObservedGesture>,
    held: bool,
    overflowed: bool,
}
struct ObservedGesture {
    time: u32,
    anchor: PhysicalPoint,
    extent: MotionExtent,
    completed: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct MotionExtent {
    min_x: i64,
    max_x: i64,
    min_y: i64,
    max_y: i64,
}
impl MotionHistory {
    fn record(&mut self, message: u32, time: u32, point: PhysicalPoint) {
        if message == WM_LBUTTONDOWN {
            // Bounded storage; one native gesture is consumed well before 128
            // new clicks can arrive during a desktop capture.
            if self.gestures.len() == 128 {
                self.gestures.pop_front();
                self.overflowed = true;
            }
            self.gestures.push_back(ObservedGesture {
                time,
                anchor: point,
                extent: MotionExtent::default(),
                completed: false,
            });
            self.held = true;
        } else if self.held && matches!(message, WM_MOUSEMOVE | WM_LBUTTONUP) {
            if let Some(gesture) = self.gestures.back_mut() {
                let dx = i64::from(point.x) - i64::from(gesture.anchor.x);
                let dy = i64::from(point.y) - i64::from(gesture.anchor.y);
                gesture.extent.min_x = gesture.extent.min_x.min(dx);
                gesture.extent.max_x = gesture.extent.max_x.max(dx);
                gesture.extent.min_y = gesture.extent.min_y.min(dy);
                gesture.extent.max_y = gesture.extent.max_y.max(dy);
            }
            if message == WM_LBUTTONUP {
                if let Some(gesture) = self.gestures.back_mut() {
                    gesture.completed = true;
                }
                self.held = false;
            }
        }
    }
    fn distance(&self, time: u32) -> Option<MotionExtent> {
        self.gestures
            .iter()
            .rev()
            .find(|g| g.time == time)
            .map(|g| g.extent)
    }
}
thread_local! {
    static MOUSE_HISTORY: RefCell<Option<std::sync::Arc<std::sync::Mutex<MotionHistory>>>> = const { RefCell::new(None) };
}
unsafe extern "system" fn observe_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        MOUSE_HISTORY.with(|slot| {
            if let Some(history) = slot.borrow().as_ref() {
                if let Ok(mut history) = history.lock() {
                    history.record(
                        wparam.0 as u32,
                        event.time,
                        PhysicalPoint {
                            x: event.pt.x,
                            y: event.pt.y,
                        },
                    );
                }
            }
        });
    }
    CallNextHookEx(None, code, wparam, lparam)
}
struct MouseObserver {
    history: std::sync::Arc<std::sync::Mutex<MotionHistory>>,
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl MouseObserver {
    fn start() -> Result<Self, NativeError> {
        let history = std::sync::Arc::new(std::sync::Mutex::new(MotionHistory::default()));
        let worker_history = history.clone();
        let (ready, receiver) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("screenshot-mouse-observer".into())
            .spawn(move || unsafe {
                let _dpi = DpiContext::enter();
                let mut message = MSG::default();
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                MOUSE_HISTORY.with(|slot| *slot.borrow_mut() = Some(worker_history));
                let instance = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(observe_mouse), instance, 0);
                match hook {
                    Ok(hook) => {
                        if ready.send(Ok(GetCurrentThreadId())).is_ok() {
                            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                                let _ = TranslateMessage(&message);
                                DispatchMessageW(&message);
                            }
                        }
                        let _ = UnhookWindowsHookEx(hook);
                    }
                    Err(_) => {
                        let _ = ready.send(Err(NativeError::OverlayFailed));
                    }
                }
                MOUSE_HISTORY.with(|slot| slot.borrow_mut().take());
            })
            .map_err(|_| NativeError::OverlayFailed)?;
        let thread_id = receiver.recv().map_err(|_| NativeError::OverlayFailed)??;
        Ok(Self {
            history,
            thread_id,
            thread: Some(thread),
        })
    }
    fn distance(&self, time: u32) -> Option<MotionExtent> {
        self.history.lock().ok()?.distance(time)
    }
    fn finish(&self, time: u32) -> Option<MotionExtent> {
        let mut history = self.history.lock().ok()?;
        let index = history.gestures.iter().position(|g| g.time == time)?;
        let gesture = history.gestures.remove(index)?;
        gesture.completed.then_some(gesture.extent)
    }
}
impl Drop for MouseObserver {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct NativeCapture;
impl CaptureApi for NativeCapture {
    fn acquire(&self, window: usize) -> bool {
        unsafe {
            let hwnd = HWND(window as *mut _);
            SetCapture(hwnd);
            GetCapture() == hwnd
        }
    }
    fn release(&self) {
        unsafe {
            let _ = ReleaseCapture();
        }
    }
}
fn cursor() -> Result<PhysicalPoint, NativeError> {
    unsafe {
        let mut point = POINT::default();
        GetPhysicalCursorPos(&mut point).map_err(|_| NativeError::OverlayFailed)?;
        Ok(PhysicalPoint {
            x: point.x,
            y: point.y,
        })
    }
}
fn bounded_cursor(point: PhysicalPoint, bounds: PhysicalRect) -> PhysicalPoint {
    PhysicalPoint {
        x: point.x.clamp(bounds.left, bounds.right),
        y: point.y.clamp(bounds.top, bounds.bottom),
    }
}
fn pointer_point(point: PhysicalPoint, state: &State) -> PhysicalPoint {
    let bounds = state.desktop.bounds;
    let mut point = bounded_cursor(point, bounds);
    if state.selection.borrow().mode == ScreenshotMode::Region
        && state.selection.borrow().anchor.is_some()
    {
        if point.x == bounds.right - 1 {
            point.x = bounds.right;
        }
        if point.y == bounds.bottom - 1 {
            point.y = bounds.bottom;
        }
    }
    point
}
fn client_message_point(lparam: LPARAM) -> PhysicalPoint {
    PhysicalPoint {
        x: (lparam.0 as u16 as i16) as i32,
        y: ((lparam.0 >> 16) as u16 as i16) as i32,
    }
}
unsafe fn message_point(hwnd: HWND, lparam: LPARAM) -> Option<PhysicalPoint> {
    // Captured messages use the owning HWND's client coordinates. Keep the
    // event's historical point even when capture blocked the message pump.
    let event_point = client_message_point(lparam);
    let mut point = POINT {
        x: event_point.x,
        y: event_point.y,
    };
    ClientToScreen(hwnd, &mut point)
        .as_bool()
        .then_some(PhysicalPoint {
            x: point.x,
            y: point.y,
        })
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowContext;
    if context.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let context = &*context;
    if context.state.is_null() {
        let cancellation = &*context.cancellation;
        match message {
            WM_WTSSESSION_CHANGE => cancellation.cancel(CancelReason::SessionChanged),
            WM_DISPLAYCHANGE | WM_DPICHANGED => cancellation.cancel(CancelReason::DisplayChanged),
            WM_QUERYENDSESSION | WM_ENDSESSION => cancellation.cancel(CancelReason::Shutdown),
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            _ => {}
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let state = &*context.state;
    let event = match message {
        WM_PAINT => {
            if paint(hwnd, state, context.index).is_err() {
                state.events.borrow_mut().push_back(Event::PaintFailed);
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEMOVE => message_point(hwnd, lparam)
            .map(|point| Event::Input(SelectionInput::Move(pointer_point(point, state)))),
        WM_LBUTTONDOWN => message_point(hwnd, lparam)
            .map(|point| Event::Down(hwnd, pointer_point(point, state), GetMessageTime() as u32)),
        WM_LBUTTONUP => {
            message_point(hwnd, lparam).map(|point| Event::Up(pointer_point(point, state)))
        }
        WM_KEYDOWN => {
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
            let input = match wparam.0 as u16 {
                v if v == VK_ESCAPE.0 => Some(SelectionInput::Escape),
                v if v == VK_SPACE.0 => Some(SelectionInput::Space),
                v if v == VK_RETURN.0 => Some(SelectionInput::Enter),
                v if v == VK_LEFT.0 => Some(SelectionInput::Arrow {
                    direction: Direction::Left,
                    shift,
                }),
                v if v == VK_RIGHT.0 => Some(SelectionInput::Arrow {
                    direction: Direction::Right,
                    shift,
                }),
                v if v == VK_UP.0 => Some(SelectionInput::Arrow {
                    direction: Direction::Up,
                    shift,
                }),
                v if v == VK_DOWN.0 => Some(SelectionInput::Arrow {
                    direction: Direction::Down,
                    shift,
                }),
                _ => None,
            };
            input.map(Event::Input)
        }
        WM_CAPTURECHANGED => Some(Event::CaptureLost),
        WM_CANCELMODE => Some(Event::Cancel(CancelReason::LostCapture)),
        WM_ACTIVATEAPP if wparam.0 == 0 => Some(Event::Cancel(CancelReason::LostCapture)),
        WM_DISPLAYCHANGE | WM_DPICHANGED => Some(Event::Cancel(CancelReason::DisplayChanged)),
        WM_WTSSESSION_CHANGE => Some(Event::Cancel(CancelReason::SessionChanged)),
        WM_CLOSE | WM_QUERYENDSESSION | WM_ENDSESSION => {
            Some(Event::Cancel(CancelReason::Shutdown))
        }
        WM_TIMER => Some(Event::Tick),
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !state.closing.get() {
                state
                    .events
                    .borrow_mut()
                    .push_back(Event::Cancel(CancelReason::LostCapture));
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        _ => return DefWindowProcW(hwnd, message, wparam, lparam),
    };
    if let Some(event) = event {
        state.events.borrow_mut().push_back(event);
    }
    // A cancellation request must not veto Windows shutdown/logoff.
    LRESULT(if message == WM_QUERYENDSESSION { 1 } else { 0 })
}

pub(super) fn run(
    mode: ScreenshotMode,
    budget: &PixelBudget,
    palette: OverlayPalette,
    excluded: &[isize],
    provider: Option<std::path::PathBuf>,
    cancellation: &Cancellation,
) -> Result<SelectionResult, NativeError> {
    let _dpi = DpiContext::enter()?;
    if let Some(reason) = cancellation.reason() {
        return Ok(SelectionResult::Cancelled(reason));
    }
    let class = WindowClass::register()?;
    // This hidden receiver is registered before capture, so lock/session/display
    // notifications arriving while GDI runs remain queued and invalidate frames.
    let receiver = unsafe {
        let context = Box::new(WindowContext {
            state: std::ptr::null(),
            index: 0,
            cancellation,
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR(class.name.as_ptr()),
            w!("UiPilot 截图会话"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(class.instance),
            Some(&*context as *const _ as *const _),
        )
        .map_err(|_| NativeError::OverlayFailed)?;
        let mut window = OwnedWindow {
            hwnd,
            _context: context,
            session_notifications: false,
        };
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
            .map_err(|_| NativeError::OverlayFailed)?;
        window.session_notifications = true;
        window
    };
    // The host hides its launcher/preview before dispatch. Refuse visible owned
    // windows rather than silently include them in the frozen desktop.
    if excluded
        .iter()
        .any(|id| unsafe { IsWindowVisible(HWND(*id as *mut _)).as_bool() })
    {
        return Err(NativeError::CaptureFailed);
    }
    let captured = capture::live_desktop(excluded);
    if let Some(reason) = cancellation.reason() {
        return Ok(SelectionResult::Cancelled(reason));
    }
    let desktop = captured?;
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, Some(receiver.hwnd), 0, 0, PM_REMOVE).as_bool() {
            DispatchMessageW(&message);
        }
    }
    if let Some(reason) = cancellation.reason() {
        return Ok(SelectionResult::Cancelled(reason));
    }
    if !capture::layout().is_ok_and(|current| current == desktop.monitors) {
        return Ok(SelectionResult::Cancelled(CancelReason::DisplayChanged));
    }
    let point = bounded_cursor(cursor()?, desktop.bounds);
    let selection = SelectionState::new(mode, point, desktop.candidates.clone());
    let state = Box::new(State {
        desktop,
        selection: RefCell::new(selection),
        palette,
        frozen: RefCell::new(None),
        fallback: Cell::new(provider.is_none()),
        target: Cell::new(None),
        presented_at: Cell::new(None),
        presented_candidate: Cell::new(None),
        events: RefCell::new(VecDeque::new()),
        closing: Cell::new(false),
    });
    let windows = collect_overlays(state.desktop.monitors.len(), |index| unsafe {
        let monitor = state.desktop.monitors[index];
        let (width, height) = monitor.rect.dimensions()?;
        let context = Box::new(WindowContext {
            state: &*state,
            index,
            cancellation,
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            PCWSTR(class.name.as_ptr()),
            w!("UiPilot 截图选择"),
            WS_POPUP,
            monitor.rect.left,
            monitor.rect.top,
            width as i32,
            height as i32,
            None,
            None,
            Some(class.instance),
            Some(&*context as *const _ as *const _),
        )
        .map_err(|_| NativeError::OverlayFailed)?;
        let mut window = OwnedWindow {
            hwnd,
            _context: context,
            session_notifications: false,
        };
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
            .map_err(|_| NativeError::OverlayFailed)?;
        window.session_notifications = true;
        Ok(window)
    })?;
    let mut probe_excluded = excluded.to_vec();
    probe_excluded.extend(windows.iter().map(|w| w.hwnd.0 as isize));
    let mut probe = provider
        .map(|path| crate::screenshot::probe::ProbeWorker::start(path, probe_excluded.clone()));
    let mut pending_probe: Option<(u64, PhysicalPoint)> = None;
    let observer = MouseObserver::start()?;
    let mut gesture_time = None;
    // Nothing is visible until the full monitor set exists. Any earlier error
    // unwinds every previously created HWND while contexts and frames still live.
    let mut capture = MouseCapture::new(NativeCapture);
    let result = (|| {
        unsafe {
            live_style(&windows)?;
            for window in &windows {
                let _ = ShowWindow(window.hwnd, SW_SHOW);
                let _ = UpdateWindow(window.hwnd);
            }
            let active = windows
                .iter()
                .find(|window| {
                    state.desktop.monitors[window._context.index]
                        .rect
                        .contains(point)
                })
                .unwrap_or(&windows[0]);
            let _ = SetForegroundWindow(active.hwnd);
            let _ = SetFocus(Some(active.hwnd));
            if GetForegroundWindow() != active.hwnd {
                return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
            }
            capture.acquire(active.hwnd.0 as usize)?;
            if SetTimer(Some(active.hwnd), 1, 50, None) == 0 {
                return Err(NativeError::OverlayFailed);
            }
        }
        loop {
            if let Some(reason) = cancellation.reason() {
                return Ok(SelectionResult::Cancelled(reason));
            }
            if observer.history.lock().map_or(true, |h| h.overflowed) {
                return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
            }
            loop {
                // End the queue borrow before calling native APIs: ReleaseCapture
                // and repaint can synchronously reenter window_proc.
                let event = state.events.borrow_mut().pop_front();
                let Some(event) = event else {
                    break;
                };
                let input = match event {
                    Event::Cancel(reason) => return Ok(SelectionResult::Cancelled(reason)),
                    Event::PaintFailed => return Err(NativeError::OverlayFailed),
                    Event::CaptureLost => {
                        if capture.active
                            && unsafe {
                                GetCapture().0.is_null()
                                    || !windows.iter().any(|window| window.hwnd == GetCapture())
                            }
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
                        }
                        continue;
                    }
                    Event::Tick => {
                        if !capture::layout().is_ok_and(|current| current == state.desktop.monitors)
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::DisplayChanged));
                        }
                        if state.frozen.borrow().is_none() {
                            let before = (state.selection.borrow().highlight(), state.target.get());
                            if state.fallback.get() || mode == ScreenshotMode::Window {
                                let fresh = capture::window_snapshot(
                                    &probe_excluded,
                                    state.desktop.bounds,
                                )?;
                                state.selection.borrow_mut().refresh_windows(fresh);
                            }
                            let point = state.selection.borrow().cursor;
                            if mode == ScreenshotMode::Region {
                                if let Some(worker) = probe.as_mut() {
                                    while let Some(reply) = worker.poll() {
                                        if reply.unavailable {
                                            state.fallback.set(true);
                                        }
                                        if pending_probe == Some((reply.sequence, reply.point))
                                            && reply.point == point
                                        {
                                            let rect = if reply.unavailable {
                                                state
                                                    .selection
                                                    .borrow()
                                                    .selected_window()
                                                    .map(|w| w.rect)
                                            } else {
                                                reply.rect
                                            };
                                            state.target.set(reply.target);
                                            state.selection.borrow_mut().set_candidate(point, rect);
                                            pending_probe = None;
                                        }
                                    }
                                    if !state.fallback.get()
                                        && pending_probe.is_none_or(|(_, p)| p != point)
                                    {
                                        pending_probe = Some((worker.request(point), point));
                                    }
                                }
                            }
                            if state.fallback.get() || mode == ScreenshotMode::Window {
                                let selected = state.selection.borrow().selected_window().cloned();
                                let target = selected.as_ref().and_then(|w| {
                                    current_window_target(w, &probe_excluded, state.desktop.bounds)
                                });
                                state.target.set(target);
                                let rect = selected.filter(|_| target.is_some()).map(|w| w.rect);
                                state.selection.borrow_mut().set_candidate(point, rect);
                            }
                            let after = (state.selection.borrow().highlight(), state.target.get());
                            if before != after || state.presented_at.get().is_none() {
                                present(&state, &windows, &probe_excluded)?;
                            }
                        }
                        continue;
                    }
                    Event::Down(_hwnd, point, time) => {
                        pending_probe = None;
                        if mode == ScreenshotMode::Window {
                            state
                                .selection
                                .borrow_mut()
                                .reduce(SelectionInput::Move(point));
                            if state.selection.borrow().selected_window().map(|w| w.id)
                                != state.target.get().map(|t| t.hwnd)
                            {
                                return Ok(SelectionResult::Cancelled(CancelReason::User));
                            }
                        }
                        if !guard_presented_down(
                            &mut state.selection.borrow_mut(),
                            point,
                            time,
                            state.presented_at.get(),
                        ) {
                            if mode == ScreenshotMode::Window {
                                return Ok(SelectionResult::Cancelled(CancelReason::User));
                            }
                            state.target.set(None);
                        }
                        if mode == ScreenshotMode::Window
                            && !state
                                .target
                                .get()
                                .is_some_and(crate::screenshot::probe::target_is_current)
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::User));
                        }
                        validate_candidate(&state);
                        gesture_time = Some(time);
                        let dpi = state
                            .desktop
                            .monitors
                            .iter()
                            .find(|m| m.rect.contains(point))
                            .map_or(96, |m| m.dpi_x);
                        unsafe {
                            state.selection.borrow_mut().set_drag_tolerance(
                                GetSystemMetricsForDpi(SM_CXDRAG, dpi),
                                GetSystemMetricsForDpi(SM_CYDRAG, dpi),
                            );
                        }
                        SelectionInput::PointerDown(point)
                    }
                    Event::Up(point) => {
                        if mode == ScreenshotMode::Window {
                            state
                                .selection
                                .borrow_mut()
                                .reduce(SelectionInput::Move(point));
                            if state.selection.borrow().selected_window().map(|w| w.id)
                                != state.target.get().map(|t| t.hwnd)
                                || !state
                                    .target
                                    .get()
                                    .is_some_and(crate::screenshot::probe::target_is_current)
                            {
                                return Ok(SelectionResult::Cancelled(CancelReason::User));
                            }
                        }
                        if !capture.active {
                            continue;
                        }
                        if let Some(time) = gesture_time.take() {
                            let Some(extent) = observer.finish(time) else {
                                return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
                            };
                            observe_extent(&state, extent);
                        }
                        SelectionInput::PointerUp(point)
                    }
                    Event::Input(input) => {
                        if let Some(time) = gesture_time {
                            if let Some(extent) = observer.distance(time) {
                                observe_extent(&state, extent);
                            }
                        }
                        if invalidates_live_probe(
                            input,
                            state.selection.borrow().cursor,
                            state.frozen.borrow().is_some(),
                        ) {
                            pending_probe = None;
                            state.target.set(None);
                        }
                        if input == SelectionInput::Enter
                            && mode == ScreenshotMode::Window
                            && (state.selection.borrow().selected_window().map(|w| w.id)
                                != state.target.get().map(|t| t.hwnd)
                                || !state
                                    .target
                                    .get()
                                    .is_some_and(crate::screenshot::probe::target_is_current))
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::User));
                        }
                        if input == SelectionInput::Enter && state.selection.borrow().hovering() {
                            validate_candidate(&state);
                        }
                        input
                    }
                };
                let unchanged =
                    if matches!(input, SelectionInput::Enter | SelectionInput::PointerUp(_)) {
                        capture::layout().is_ok_and(|current| current == state.desktop.monitors)
                    } else {
                        true
                    };
                let outcome = final_selection(&mut state.selection.borrow_mut(), input, unchanged);
                if matches!(
                    input,
                    SelectionInput::PointerDown(_) | SelectionInput::Space
                ) {
                    freeze(&state, &windows, excluded, budget, cancellation)?;
                }
                match outcome {
                    Err(reason) => return Ok(SelectionResult::Cancelled(reason)),
                    Ok(Some(rect)) => {
                        let Some(rect) = rect.intersection(state.desktop.bounds) else {
                            continue;
                        };
                        if let Some(reason) = cancellation.reason() {
                            return Ok(SelectionResult::Cancelled(reason));
                        }
                        if !windows
                            .iter()
                            .any(|window| window.hwnd == unsafe { GetForegroundWindow() })
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
                        }
                        freeze(&state, &windows, excluded, budget, cancellation)?;
                        let frozen = state.frozen.borrow();
                        let image = crate::screenshot::geometry::compose(
                            frozen
                                .as_ref()
                                .ok_or(NativeError::CaptureFailed)?
                                .frames
                                .frames(),
                            rect,
                            budget,
                        )?;
                        if let Some(reason) = cancellation.reason() {
                            return Ok(SelectionResult::Cancelled(reason));
                        }
                        if !capture::layout().is_ok_and(|current| current == state.desktop.monitors)
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::DisplayChanged));
                        }
                        return Ok(SelectionResult::Selected { rect, image });
                    }
                    Ok(None) => {}
                }
                if state.selection.borrow().hovering() && state.frozen.borrow().is_some() {
                    state.frozen.borrow_mut().take();
                    unsafe {
                        live_style(&windows)?;
                    }
                    pending_probe = None;
                }
                {
                    let mut selection = state.selection.borrow_mut();
                    selection.cursor = bounded_cursor(selection.cursor, state.desktop.bounds);
                }
                if state.frozen.borrow().is_none() {
                    present(&state, &windows, &probe_excluded)?;
                }
                unsafe {
                    for window in &windows {
                        let _ = InvalidateRect(Some(window.hwnd), None, false);
                    }
                }
            }
            unsafe {
                let wake = cancellation
                    .wake
                    .as_ref()
                    .ok_or(NativeError::OverlayFailed)?
                    .0;
                let waited = MsgWaitForMultipleObjectsEx(
                    Some(&[wake]),
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if waited == WAIT_FAILED {
                    return Err(NativeError::OverlayFailed);
                }
                if waited == WAIT_OBJECT_0 {
                    continue;
                }
                let mut message = MSG::default();
                // Dispatch one message then process the resulting event before
                // reading more, keeping capture/selection order deterministic.
                if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message == WM_QUIT {
                        return Ok(SelectionResult::Cancelled(CancelReason::Shutdown));
                    }
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    })();
    let result = cancellation
        .reason()
        .map_or(result, |reason| Ok(SelectionResult::Cancelled(reason)));
    let result = finalize_ownership(
        result,
        windows
            .iter()
            .any(|window| window.hwnd == unsafe { GetForegroundWindow() }),
    );
    state.closing.set(true);
    capture.release();
    drop(windows);
    result
}

fn observe_extent(state: &State, extent: MotionExtent) {
    let mut selection = state.selection.borrow_mut();
    selection.observe_motion(extent.min_x, extent.min_y);
    selection.observe_motion(extent.max_x, extent.max_y);
}

fn validate_candidate(state: &State) {
    if state.selection.borrow().hovering()
        && !state
            .target
            .get()
            .is_some_and(crate::screenshot::probe::target_is_current)
    {
        let point = state.selection.borrow().cursor;
        state.selection.borrow_mut().set_candidate(point, None);
        state.target.set(None);
    }
}

unsafe fn live_style(windows: &[OwnedWindow]) -> Result<(), NativeError> {
    for window in windows {
        SetLayeredWindowAttributes(window.hwnd, color(0xff00ff), 128, LWA_ALPHA | LWA_COLORKEY)
            .map_err(|_| NativeError::OverlayFailed)?;
    }
    Ok(())
}
fn freeze(
    state: &State,
    windows: &[OwnedWindow],
    excluded: &[isize],
    budget: &PixelBudget,
    cancellation: &Cancellation,
) -> Result<(), NativeError> {
    if state.frozen.borrow().is_some() {
        return Ok(());
    }
    // Keep every HWND visible and continuously captured while alpha zero removes
    // decorations. Queued input remains owned through the compositor barrier.
    unsafe {
        for window in windows {
            SetLayeredWindowAttributes(window.hwnd, COLORREF(0), 0, LWA_ALPHA)
                .map_err(|_| NativeError::OverlayFailed)?;
        }
    }
    let mut ignored = excluded.to_vec();
    ignored.extend(windows.iter().map(|w| w.hwnd.0 as isize));
    let desktop = capture::desktop(&ignored, budget, cancellation)?;
    if desktop.monitors != state.desktop.monitors {
        cancellation.cancel(CancelReason::DisplayChanged);
        return Err(NativeError::CaptureFailed);
    }
    *state.frozen.borrow_mut() = Some(desktop);
    unsafe {
        for window in windows {
            SetLayeredWindowAttributes(window.hwnd, COLORREF(0), 255, LWA_ALPHA)
                .map_err(|_| NativeError::OverlayFailed)?;
            let _ = InvalidateRect(Some(window.hwnd), None, false);
            let _ = UpdateWindow(window.hwnd);
        }
    }
    Ok(())
}

fn finalize_ownership(
    result: Result<SelectionResult, NativeError>,
    owns_foreground: bool,
) -> Result<SelectionResult, NativeError> {
    match result {
        Ok(SelectionResult::Cancelled(_)) | Ok(SelectionResult::Selected { .. })
            if !owns_foreground =>
        {
            Ok(SelectionResult::Cancelled(CancelReason::LostCapture))
        }
        other => other,
    }
}

fn collect_overlays<T>(
    count: usize,
    mut create: impl FnMut(usize) -> Result<T, NativeError>,
) -> Result<Vec<T>, NativeError> {
    let mut windows = Vec::new();
    windows
        .try_reserve_exact(count)
        .map_err(|_| ScreenshotError::AllocationFailed)?;
    for index in 0..count {
        windows.push(create(index)?);
    }
    Ok(windows)
}
trait CaptureApi {
    fn acquire(&self, window: usize) -> bool;
    fn release(&self);
}
struct MouseCapture<A: CaptureApi> {
    api: A,
    active: bool,
}
impl<A: CaptureApi> MouseCapture<A> {
    fn new(api: A) -> Self {
        Self { api, active: false }
    }
    fn acquire(&mut self, window: usize) -> Result<(), NativeError> {
        self.release();
        self.active = self.api.acquire(window);
        if self.active {
            Ok(())
        } else {
            Err(NativeError::OverlayFailed)
        }
    }
    fn release(&mut self) {
        if self.active {
            self.active = false;
            self.api.release();
        }
    }
}
impl<A: CaptureApi> Drop for MouseCapture<A> {
    fn drop(&mut self) {
        self.release();
    }
}
fn final_selection(
    state: &mut SelectionState,
    input: SelectionInput,
    layout_unchanged: bool,
) -> Result<Option<PhysicalRect>, CancelReason> {
    if !layout_unchanged {
        return Err(CancelReason::DisplayChanged);
    }
    match state.reduce(input) {
        SelectionOutcome::Continue => Ok(None),
        SelectionOutcome::Selected(rect) => Ok(Some(rect)),
        SelectionOutcome::Cancelled => Err(CancelReason::User),
    }
}

struct Paint {
    hwnd: HWND,
    dc: HDC,
    paint: PAINTSTRUCT,
}
impl Paint {
    unsafe fn begin(hwnd: HWND) -> Result<Self, NativeError> {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        if dc.0.is_null() {
            return Err(NativeError::OverlayFailed);
        }
        Ok(Self { hwnd, dc, paint })
    }
}
impl Drop for Paint {
    fn drop(&mut self) {
        unsafe {
            let _ = EndPaint(self.hwnd, &self.paint);
        }
    }
}
struct GdiObject(HGDIOBJ);
impl Drop for GdiObject {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}
struct SavedDc(HDC, i32);
impl SavedDc {
    unsafe fn new(dc: HDC) -> Result<Self, NativeError> {
        let saved = SaveDC(dc);
        if saved == 0 {
            Err(NativeError::OverlayFailed)
        } else {
            Ok(Self(dc, saved))
        }
    }
}
impl Drop for SavedDc {
    fn drop(&mut self) {
        unsafe {
            let _ = RestoreDC(self.0, self.1);
        }
    }
}
fn color(rgb: u32) -> COLORREF {
    COLORREF(((rgb >> 16) & 255) | (rgb & 0xff00) | ((rgb & 255) << 16))
}
unsafe fn brush(rgb: u32) -> Result<GdiObject, NativeError> {
    let brush = CreateSolidBrush(color(rgb));
    if brush.0.is_null() {
        Err(NativeError::OverlayFailed)
    } else {
        Ok(GdiObject(HGDIOBJ(brush.0)))
    }
}
unsafe fn fill(dc: HDC, rect: RECT, rgb: u32) -> Result<(), NativeError> {
    let brush = brush(rgb)?;
    if FillRect(dc, &rect, HBRUSH(brush.0 .0)) == 0 {
        Err(NativeError::OverlayFailed)
    } else {
        Ok(())
    }
}
unsafe fn frozen(dc: HDC, frame: &MonitorFrame) -> Result<(), NativeError> {
    let header = BITMAPV4HEADER {
        bV4Size: std::mem::size_of::<BITMAPV4HEADER>() as u32,
        bV4Width: frame.image.width as i32,
        bV4Height: -(frame.image.height as i32),
        bV4Planes: 1,
        bV4BitCount: 32,
        bV4V4Compression: BI_BITFIELDS,
        bV4RedMask: 0x000000ff,
        bV4GreenMask: 0x0000ff00,
        bV4BlueMask: 0x00ff0000,
        bV4AlphaMask: 0xff000000,
        ..Default::default()
    };
    if SetDIBitsToDevice(
        dc,
        0,
        0,
        frame.image.width,
        frame.image.height,
        0,
        0,
        0,
        frame.image.height,
        frame.image.pixels.as_ptr().cast(),
        &header as *const _ as *const BITMAPINFO,
        DIB_RGB_COLORS,
    ) == 0
    {
        return Err(NativeError::OverlayFailed);
    }
    Ok(())
}
fn presentation_changed<T: PartialEq>(previous: Option<T>, current: T) -> bool {
    previous.as_ref() != Some(&current)
}
fn presented_window_target<T>(
    selection: &SelectionState,
    snapshot: impl FnOnce(&WindowCandidate) -> Option<T>,
) -> Option<T> {
    selection.selected_window().and_then(snapshot)
}
fn invalidates_live_probe(input: SelectionInput, cursor: PhysicalPoint, frozen: bool) -> bool {
    !frozen
        && (matches!(input, SelectionInput::Arrow { .. } | SelectionInput::Space)
            || matches!(input, SelectionInput::Move(p) if p != cursor))
}
fn live_background(rect: Option<PhysicalRect>) -> u32 {
    if rect.is_some() {
        0
    } else {
        0xff00ff
    }
}
fn guard_presented_down(
    selection: &mut SelectionState,
    point: PhysicalPoint,
    time: u32,
    presented: Option<u32>,
) -> bool {
    let accepted = input_after_presentation(time, presented);
    if !accepted {
        selection.set_candidate(point, None);
    }
    accepted
}
fn input_after_presentation(time: u32, presented: Option<u32>) -> bool {
    presented.is_some_and(|stamp| (time.wrapping_sub(stamp) as i32) > 0)
}
fn current_window_target(
    w: &WindowCandidate,
    excluded: &[isize],
    bounds: PhysicalRect,
) -> Option<crate::screenshot::probe::ProbeTarget> {
    let target = crate::screenshot::probe::snapshot_target(w.id)?;
    let fresh = capture::window_snapshot(excluded, bounds).ok()?;
    if fresh
        .iter()
        .any(|current| current.id == w.id && current.rect == w.rect)
        && crate::screenshot::probe::target_is_current(target)
    {
        Some(target)
    } else {
        None
    }
}
fn present(state: &State, windows: &[OwnedWindow], excluded: &[isize]) -> Result<(), NativeError> {
    // Keyboard cycling and live pointer movement publish identity with geometry,
    // without waiting for a subsequent 50ms hover tick. Frozen locks stay owned.
    if state.frozen.borrow().is_none() && state.selection.borrow().mode == ScreenshotMode::Window {
        state
            .target
            .set(presented_window_target(&state.selection.borrow(), |w| {
                current_window_target(w, excluded, state.desktop.bounds)
            }));
    }
    let candidate = (state.selection.borrow().highlight(), state.target.get());
    let changed = presentation_changed(state.presented_candidate.get(), candidate);
    if changed {
        state.presented_at.set(None);
    }
    unsafe {
        for window in windows {
            let _ = InvalidateRect(Some(window.hwnd), None, false);
            let _ = UpdateWindow(window.hwnd);
        }
        windows::Win32::Graphics::Dwm::DwmFlush().map_err(|_| NativeError::OverlayFailed)?;
        if changed {
            state.presented_candidate.set(Some(candidate));
            state.presented_at.set(Some(
                windows::Win32::System::SystemInformation::GetTickCount(),
            ));
        }
    }
    Ok(())
}
unsafe fn paint(hwnd: HWND, state: &State, index: usize) -> Result<(), NativeError> {
    let paint = Paint::begin(hwnd)?;
    let dc = paint.dc;
    let monitor = state.desktop.monitors[index];
    let frozen_desktop = state.frozen.borrow();
    let frame = frozen_desktop.as_ref().map(|d| &d.frames.frames()[index]);
    let selection = state.selection.borrow();
    let (width, height) = monitor.rect.dimensions()?;
    if let Some(frame) = frame {
        frozen(dc, frame)?;
        capture::shade(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            },
            0,
            128,
        )?;
    } else {
        fill(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            },
            live_background(selection.highlight()),
        )?;
    }
    let selected = selection.highlight();
    if let Some(selected) = selected.and_then(|selected| selected.intersection(monitor.rect)) {
        let local = RECT {
            left: selected.left - monitor.rect.left,
            top: selected.top - monitor.rect.top,
            right: selected.right - monitor.rect.left,
            bottom: selected.bottom - monitor.rect.top,
        };
        {
            let _saved = SavedDc::new(dc)?;
            IntersectClipRect(dc, local.left, local.top, local.right, local.bottom);
            if let Some(frame) = frame {
                frozen(dc, frame)?;
            } else {
                fill(dc, local, 0xff00ff)?;
            }
        }
        let border = brush(state.palette.selection_border)?;
        if FrameRect(dc, &local, HBRUSH(border.0 .0)) == 0 {
            return Err(NativeError::OverlayFailed);
        }
        let inner = RECT {
            left: local.left + 1,
            top: local.top + 1,
            right: local.right - 1,
            bottom: local.bottom - 1,
        };
        if inner.right > inner.left && inner.bottom > inner.top {
            FrameRect(dc, &inner, HBRUSH(border.0 .0));
        }
    }
    if selection.mode == ScreenshotMode::Region
        && !selection.hovering()
        && monitor.rect.contains(selection.cursor)
    {
        let x = selection.cursor.x - monitor.rect.left;
        let y = selection.cursor.y - monitor.rect.top;
        // The keyboard crosshair is independent of the system pointer.
        fill(
            dc,
            RECT {
                left: x.saturating_sub(9),
                top: y.saturating_sub(1),
                right: x.saturating_add(10),
                bottom: y.saturating_add(2),
            },
            state.palette.surface,
        )?;
        fill(
            dc,
            RECT {
                left: x.saturating_sub(1),
                top: y.saturating_sub(9),
                right: x.saturating_add(2),
                bottom: y.saturating_add(10),
            },
            state.palette.surface,
        )?;
        fill(
            dc,
            RECT {
                left: x.saturating_sub(8),
                top: y,
                right: x.saturating_add(9),
                bottom: y.saturating_add(1),
            },
            state.palette.foreground,
        )?;
        fill(
            dc,
            RECT {
                left: x,
                top: y.saturating_sub(8),
                right: x.saturating_add(1),
                bottom: y.saturating_add(9),
            },
            state.palette.foreground,
        )?;
    }
    let scale = monitor.dpi_x as f64 / 96.0;
    let px = |value: f64| (value * scale).round().clamp(1.0, i32::MAX as f64) as i32;
    let mut label = RECT {
        left: px(12.0),
        top: px(12.0),
        right: (px(324.0)).min(width as i32),
        bottom: (px(44.0)).min(height as i32),
    };
    fill(dc, label, state.palette.surface)?;
    let text = match selected.and_then(|rect| rect.dimensions().ok()) {
        Some((width, height)) => format!(
            "{} {width} × {height} px · Enter 确认 · Esc 取消",
            if state.fallback.get() && selection.hovering() {
                "窗口候选"
            } else {
                ""
            }
        ),
        None => match selection.mode {
            ScreenshotMode::Region if state.fallback.get() => {
                "元素识别不可用 · 拖动选择 · Esc 取消".into()
            }
            ScreenshotMode::Region => "单击元素 / 拖动选择 · Space 起点 · Esc 取消".into(),
            ScreenshotMode::Window => "选择窗口 · 方向键切换 · Esc 取消".into(),
        },
    };
    let font = CreateFontW(
        -px(12.0),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Inter"),
    );
    if font.0.is_null() {
        return Err(NativeError::OverlayFailed);
    }
    let font = GdiObject(HGDIOBJ(font.0));
    let _saved = SavedDc::new(dc)?;
    SelectObject(dc, font.0);
    SetTextColor(dc, color(state.palette.foreground));
    SetBkMode(dc, TRANSPARENT);
    label.left += px(8.0);
    label.right -= px(8.0);
    let mut text: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        dc,
        &mut text,
        &mut label,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn unchanged_presentation_keeps_seen_candidate_selectable_during_repaint() {
        let a = (Some(42), Some((0, 0, 100, 100)));
        let b = (Some(43), Some((10, 10, 110, 110)));
        let mut stamp = Some(90);
        // A already seen; mouse move within A requests harmless repaint at 101.
        if presentation_changed(Some(a), a) {
            stamp = Some(101);
        }
        assert!(input_after_presentation(100, stamp));
        if presentation_changed(Some(a), b) {
            stamp = Some(102);
        }
        assert!(!input_after_presentation(100, stamp));
        assert!(presentation_changed(None, a));
        assert!(presentation_changed(Some(a), (None, None)));
        assert!(!presentation_changed(
            Some((None, None)),
            (None::<i32>, None::<(i32, i32, i32, i32)>)
        ));
    }
    #[test]
    fn window_arrow_then_enter_uses_new_presented_identity_without_tick() {
        let p = PhysicalPoint { x: 20, y: 20 };
        let a = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let b = PhysicalRect {
            left: 200,
            top: 0,
            right: 300,
            bottom: 100,
        };
        let mut selection = SelectionState::new(
            ScreenshotMode::Window,
            p,
            vec![
                WindowCandidate {
                    id: 42,
                    rect: a,
                    z_order: 0,
                },
                WindowCandidate {
                    id: 43,
                    rect: b,
                    z_order: 1,
                },
            ],
        );
        selection.reduce(SelectionInput::Arrow {
            direction: Direction::Right,
            shift: false,
        });
        let target = presented_window_target(&selection, |candidate| Some(candidate.id));
        assert_eq!(target, Some(43));
        assert_eq!(
            selection.reduce(SelectionInput::Enter),
            SelectionOutcome::Selected(b)
        );
    }
    #[test]
    fn frozen_window_jitter_preserves_locked_native_target() {
        let p = PhysicalPoint { x: 20, y: 20 };
        let r = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let mut selection = SelectionState::new(
            ScreenshotMode::Window,
            p,
            vec![WindowCandidate {
                id: 42,
                rect: r,
                z_order: 0,
            }],
        );
        let mut target = Some(42);
        selection.reduce(SelectionInput::PointerDown(p));
        let jitter = SelectionInput::Move(PhysicalPoint { x: 21, y: 20 });
        if invalidates_live_probe(jitter, selection.cursor, true) {
            target = None;
        }
        selection.reduce(jitter);
        assert_eq!(target, Some(42));
        assert_eq!(
            selection.reduce(SelectionInput::PointerUp(PhysicalPoint { x: 21, y: 20 })),
            SelectionOutcome::Selected(r)
        );
    }
    #[test]
    fn live_no_hit_clears_mask() {
        assert_eq!(live_background(None), 0xff00ff);
    }
    #[test]
    fn queued_down_before_candidate_presentation_cannot_lock_it() {
        assert!(!input_after_presentation(100, Some(101)));
        assert!(!input_after_presentation(101, Some(101)));
        assert!(input_after_presentation(102, Some(101)));
        assert!(input_after_presentation(1, Some(u32::MAX)));
    }
    #[test]
    fn tick_reply_then_queued_down_before_paint_never_selects_unseen_rect() {
        let p = PhysicalPoint { x: 20, y: 20 };
        let a = PhysicalRect {
            left: 0,
            top: 0,
            right: 50,
            bottom: 50,
        };
        let b = PhysicalRect {
            left: 10,
            top: 10,
            right: 100,
            bottom: 100,
        };
        let mut selection = SelectionState::new(ScreenshotMode::Region, p, vec![]);
        selection.set_candidate(p, Some(a)); // A presented at tick 90.
        selection.set_candidate(p, Some(b)); // Tick accepts reply, down already queued at 100.
                                             // Synchronous paint/compositor finish at 101, before native queue dispatch.
        assert!(!guard_presented_down(&mut selection, p, 100, Some(101)));
        selection.reduce(SelectionInput::PointerDown(p));
        assert_eq!(
            selection.reduce(SelectionInput::PointerUp(p)),
            SelectionOutcome::Continue
        );
        selection.set_candidate(p, Some(b));
        assert!(guard_presented_down(&mut selection, p, 103, Some(102)));
        selection.reduce(SelectionInput::PointerDown(p));
        assert_eq!(
            selection.reduce(SelectionInput::PointerUp(p)),
            SelectionOutcome::Selected(b)
        );
    }
    #[test]
    fn foreground_loss_during_compose_discards_selected_pixels_before_teardown() {
        let budget = PixelBudget::new(4);
        let result = finalize_ownership(
            Ok(SelectionResult::Selected {
                rect: PhysicalRect {
                    left: 0,
                    top: 0,
                    right: 1,
                    bottom: 1,
                },
                image: BudgetedImage::allocate(1, 1, &budget).unwrap(),
            }),
            false,
        );
        assert!(matches!(
            result,
            Ok(SelectionResult::Cancelled(CancelReason::LostCapture))
        ));
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn keyboard_endpoint_can_reach_half_open_desktop_edge() {
        let bounds = PhysicalRect {
            left: -10,
            top: -5,
            right: 10,
            bottom: 5,
        };
        assert_eq!(
            bounded_cursor(PhysicalPoint { x: 10, y: 5 }, bounds),
            PhysicalPoint { x: 10, y: 5 }
        );
    }
    #[test]
    fn second_overlay_failure_destroys_first_and_never_leaves_half_overlay_set() {
        struct Owned(Rc<Cell<usize>>);
        impl Drop for Owned {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }
        let live = Rc::new(Cell::new(0));
        let result = collect_overlays(2, |index| {
            if index == 1 {
                return Err(NativeError::OverlayFailed);
            }
            live.set(live.get() + 1);
            Ok(Owned(live.clone()))
        });
        assert!(result.is_err());
        assert_eq!(live.get(), 0);
    }
    #[test]
    fn abnormal_scope_exit_releases_mouse_capture_once() {
        struct Api(Rc<Cell<usize>>);
        impl CaptureApi for Api {
            fn acquire(&self, _: usize) -> bool {
                self.0.set(self.0.get() + 1);
                true
            }
            fn release(&self) {
                self.0.set(self.0.get() - 1);
            }
        }
        let live = Rc::new(Cell::new(0));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut capture = MouseCapture::new(Api(live.clone()));
            capture.acquire(1).unwrap();
            panic!("injected native destruction");
        }));
        assert!(result.is_err());
        assert_eq!(live.get(), 0);
    }
    #[test]
    fn layout_change_after_input_selection_returns_only_cancellation() {
        let mut state =
            SelectionState::new(ScreenshotMode::Region, PhysicalPoint { x: 0, y: 0 }, vec![]);
        state.reduce(SelectionInput::Space);
        state.reduce(SelectionInput::Move(PhysicalPoint { x: 10, y: 10 }));
        assert_eq!(
            final_selection(&mut state, SelectionInput::Enter, false),
            Err(CancelReason::DisplayChanged)
        );
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    #[test]
    fn separate_native_history_preserves_excursion_and_original_down() {
        let mut h = MotionHistory::default();
        let p = PhysicalPoint { x: -100, y: -100 };
        h.record(WM_LBUTTONDOWN, 10, p);
        h.record(WM_MOUSEMOVE, 11, PhysicalPoint { x: -80, y: -130 });
        h.record(WM_MOUSEMOVE, 12, p);
        h.record(WM_LBUTTONUP, 13, p);
        h.record(WM_LBUTTONDOWN, 14, PhysicalPoint { x: 100, y: 100 });
        assert_eq!(
            h.distance(10),
            Some(MotionExtent {
                min_x: 0,
                max_x: 20,
                min_y: -30,
                max_y: 0
            })
        );
        assert_eq!(h.distance(14), Some(MotionExtent::default()));
    }
    #[test]
    fn history_overflow_cancels_instead_of_reverting_to_click() {
        let mut h = MotionHistory::default();
        for time in 0..129 {
            h.record(WM_LBUTTONDOWN, time, PhysicalPoint { x: 0, y: 0 });
        }
        assert!(h.overflowed);
    }
    #[test]
    fn captured_message_keeps_negative_historical_client_coordinates() {
        let encoded = (((-200i16 as u16 as u32) << 16) | (-100i16 as u16 as u32)) as isize;
        assert_eq!(
            client_message_point(LPARAM(encoded)),
            PhysicalPoint { x: -100, y: -200 }
        );
    }
}

#[cfg(test)]
mod native_observer_tests {
    use super::*;
    #[test]
    fn observer_native_pump_starts_and_releases_hook_without_input_synthesis() {
        let observer = MouseObserver::start().expect("native observer should install");
        assert_ne!(observer.thread_id, 0);
        assert!(observer.history.lock().is_ok());
        drop(observer);
    }
}
