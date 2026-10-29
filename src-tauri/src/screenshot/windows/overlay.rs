use super::*;
#[path = "editor.rs"]
mod editor;
#[path = "pin.rs"]
pub(super) mod pin;
#[path = "toolbar.rs"]
mod toolbar;
use crate::screenshot::desktop_output::{self, OutputGate, OutputKind, OutputSuccess};
use crate::screenshot::editing::{self, Handle};
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
        UI::{
            Controls::DRAWITEMSTRUCT, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*,
        },
    },
};

enum Event {
    Input(SelectionInput),
    Down(HWND, PhysicalPoint, u32),
    Up(PhysicalPoint),
    Cancel(CancelReason),
    CaptureLost,
    Tick,
    ProbeReady,
    PaintFailed,
    Key(u16, bool, bool),
    Button(editor::Button),
    ButtonFocused(editor::Button),
    OutputReady,
}
const WM_PROBE_READY: u32 = WM_APP + 19;
const WM_OUTPUT_READY: u32 = WM_APP + 20;
struct State {
    desktop: Arc<capture::Desktop>,
    budget: PixelBudget,
    selection: RefCell<SelectionState>,
    editor: RefCell<Option<editor::Editor>>,
    palette: OverlayPalette,
    locked: Cell<bool>,
    fallback: Cell<bool>,
    target: Cell<Option<crate::screenshot::probe::ProbeTarget>>,
    scope: Cell<Option<crate::screenshot::probe::ProbeScope>>,
    presented_at: Cell<Option<u32>>,
    presented_candidate: Cell<
        Option<(
            Option<PhysicalRect>,
            Option<crate::screenshot::probe::ProbeTarget>,
            Option<crate::screenshot::probe::ProbeScope>,
            bool,
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
    next_id: u64,
}
struct ObservedGesture {
    id: u64,
    claimed: bool,
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
    fn record(&mut self, message: u32, _time: u32, point: PhysicalPoint) {
        if message == WM_LBUTTONDOWN {
            // Bounded storage; one native gesture is consumed well before 128
            // new clicks can arrive during a desktop capture.
            if self.gestures.len() == 128 {
                self.gestures.pop_front();
                self.overflowed = true;
            }
            self.gestures.push_back(ObservedGesture {
                id: self.next_id,
                claimed: false,
                anchor: point,
                extent: MotionExtent::default(),
                completed: false,
            });
            self.next_id = self.next_id.checked_add(1).unwrap_or_else(|| {
                self.overflowed = true;
                0
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
    fn distance(&self, id: u64) -> Option<MotionExtent> {
        self.gestures
            .iter()
            .rev()
            .find(|g| g.id == id)
            .map(|g| g.extent)
    }
    fn claim(&mut self, point: PhysicalPoint) -> Option<u64> {
        // The hook completes before Windows dispatches the corresponding button
        // message. Pair in native event order, not by two queues' timestamps.
        let gesture = self.gestures.iter_mut().find(|g| !g.claimed)?;
        if gesture.anchor != point {
            return None;
        }
        gesture.claimed = true;
        Some(gesture.id)
    }
    fn completed(&mut self, id: u64) -> Option<MotionExtent> {
        let index = self.gestures.iter().position(|g| g.id == id)?;
        let gesture = self.gestures.remove(index)?;
        gesture.completed.then_some(gesture.extent)
    }
}
thread_local! {
    static MOUSE_HISTORY: RefCell<Option<MouseHistoryContext>> = const { RefCell::new(None) };
}
struct MouseHistoryContext {
    history: std::sync::Arc<std::sync::Mutex<MotionHistory>>,
    owner_thread: u32,
    windows: Vec<isize>,
}
unsafe extern "system" fn observe_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        MOUSE_HISTORY.with(|slot| {
            if let Some(context) = slot.borrow().as_ref() {
                if wparam.0 as u32 == WM_LBUTTONDOWN {
                    let mut info = GUITHREADINFO {
                        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                        ..Default::default()
                    };
                    if GetGUIThreadInfo(context.owner_thread, &mut info).is_err()
                        || !context.windows.contains(&(info.hwndCapture.0 as isize))
                    {
                        return;
                    }
                }
                if let Ok(mut history) = context.history.lock() {
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
    fn start(windows: Vec<isize>) -> Result<Self, NativeError> {
        let owner_thread = unsafe { GetCurrentThreadId() };
        let history = std::sync::Arc::new(std::sync::Mutex::new(MotionHistory::default()));
        let worker_history = history.clone();
        let (ready, receiver) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("screenshot-mouse-observer".into())
            .spawn(move || unsafe {
                let _dpi = DpiContext::enter();
                let mut message = MSG::default();
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                MOUSE_HISTORY.with(|slot| {
                    *slot.borrow_mut() = Some(MouseHistoryContext {
                        history: worker_history,
                        owner_thread,
                        windows,
                    })
                });
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
    fn claim(&self, point: PhysicalPoint) -> Option<u64> {
        self.history.lock().ok()?.claim(point)
    }
    fn distance(&self, id: u64) -> Option<MotionExtent> {
        self.history.lock().ok()?.distance(id)
    }
    fn finish(&self, id: u64) -> Option<MotionExtent> {
        self.history.lock().ok()?.completed(id)
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
    if let Some(editor) = state.editor.borrow().as_ref() {
        return editor.pointer_point(point);
    }
    let bounds = state.desktop.bounds;
    let mut point = bounded_cursor(point, bounds);
    let endpoint = state.selection.borrow().mode == ScreenshotMode::Region
        && state.selection.borrow().anchor.is_some();
    if endpoint {
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
        WM_COMMAND
            if toolbar::from_id(wparam.0 & 0xffff).is_some()
                && (wparam.0 >> 16) & 0xffff == BN_CLICKED as usize =>
        {
            Some(Event::Button(toolbar::from_id(wparam.0 & 0xffff).unwrap()))
        }
        WM_COMMAND
            if toolbar::from_id(wparam.0 & 0xffff).is_some()
                && (wparam.0 >> 16) & 0xffff == BN_SETFOCUS as usize =>
        {
            Some(Event::ButtonFocused(
                toolbar::from_id(wparam.0 & 0xffff).unwrap(),
            ))
        }
        WM_DRAWITEM if toolbar::from_id(wparam.0).is_some() => {
            let outputting = state.editor.borrow().as_ref().is_some_and(|e| e.outputting);
            if toolbar::draw(
                state.palette,
                outputting,
                &*(lparam.0 as *const DRAWITEMSTRUCT),
            )
            .is_err()
            {
                state.events.borrow_mut().push_back(Event::PaintFailed);
            }
            return LRESULT(1);
        }
        WM_SETCURSOR if state.editor.borrow().is_some() => {
            if let Ok(point) = cursor() {
                update_editor_cursor(state, point);
            }
            return LRESULT(1);
        }
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
            .map(|point| Event::Down(hwnd, point, GetMessageTime() as u32)),
        WM_LBUTTONUP => {
            message_point(hwnd, lparam).map(|point| Event::Up(pointer_point(point, state)))
        }
        WM_KEYDOWN => {
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
            if state.editor.borrow().is_some() {
                if !editor::accepts_key(wparam.0 as u16, lparam.0 & (1 << 30) != 0) {
                    return LRESULT(0);
                }
                state.events.borrow_mut().push_back(Event::Key(
                    wparam.0 as u16,
                    shift,
                    GetKeyState(VK_CONTROL.0 as i32) < 0,
                ));
                return LRESULT(0);
            }
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
        WM_CAPTURECHANGED => {
            crate::screenshot::probe::trace(format!("native capture changed to {}", lparam.0));
            Some(Event::CaptureLost)
        }
        WM_CANCELMODE => {
            crate::screenshot::probe::trace("native WM_CANCELMODE");
            Some(Event::Cancel(CancelReason::LostCapture))
        }
        WM_ACTIVATEAPP if wparam.0 == 0 => {
            crate::screenshot::probe::trace("native app deactivated");
            Some(Event::Cancel(CancelReason::LostCapture))
        }
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

fn editor_monitors(state: &State) -> Vec<(PhysicalRect, u32)> {
    state
        .desktop
        .monitors
        .iter()
        .map(|m| (m.rect, m.dpi_x))
        .collect()
}

unsafe fn update_editor_cursor(state: &State, point: PhysicalPoint) {
    let editor = state.editor.borrow();
    let Some(editor) = editor.as_ref() else {
        return;
    };
    let handle = if editor.outputting {
        None
    } else {
        editor.geometry.active_handle().or_else(|| {
            editing::hit_handle_on_monitors(editor.geometry.rect(), point, &editor_monitors(state))
        })
    };
    let id = match handle {
        Some(Handle::NorthWest | Handle::SouthEast) => IDC_SIZENWSE,
        Some(Handle::NorthEast | Handle::SouthWest) => IDC_SIZENESW,
        Some(Handle::North | Handle::South) => IDC_SIZENS,
        Some(Handle::East | Handle::West) => IDC_SIZEWE,
        None => IDC_ARROW,
    };
    if let Ok(cursor) = LoadCursorW(None, id) {
        SetCursor(Some(cursor));
    }
}

fn sync_editor(
    state: &State,
    windows: &[OwnedWindow],
    buttons: &[toolbar::Toolbar],
    focus: bool,
) -> Result<(), NativeError> {
    let (rect, hidden, outputting, focused) = {
        let editor = state.editor.borrow();
        let Some(editor) = editor.as_ref() else {
            return Ok(());
        };
        (
            editing::save_button_rect(editor.geometry.rect(), &editor_monitors(state), 112, 28),
            editor.geometry.resizing(),
            editor.outputting,
            editor.focus,
        )
    };
    unsafe {
        for (index, toolbar) in buttons.iter().enumerate() {
            let monitor = state.desktop.monitors[index];
            let visible = !hidden
                && monitor.rect.contains(PhysicalPoint {
                    x: rect.left,
                    y: rect.top,
                });
            for (button_index, button) in toolbar.buttons.iter().enumerate() {
                if visible {
                    button.theme(state.palette);
                    let _ = EnableWindow(button.hwnd, !outputting);
                    let left = rect.left + (rect.right - rect.left) * button_index as i32 / 4;
                    let right =
                        rect.left + (rect.right - rect.left) * (button_index + 1) as i32 / 4;
                    SetWindowPos(
                        button.hwnd,
                        None,
                        left - monitor.rect.left,
                        rect.top - monitor.rect.top,
                        right - left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    )
                    .map_err(|_| NativeError::OverlayFailed)?;
                    if focus
                        && !outputting
                        && focused == editor::Focus::Button(editor::Button::ALL[button_index])
                    {
                        let _ = SetFocus(Some(button.hwnd));
                    }
                    let _ = InvalidateRect(Some(button.hwnd), None, false);
                } else {
                    let _ = ShowWindow(button.hwnd, SW_HIDE);
                }
            }
            if focus && !outputting {
                if let editor::Focus::Handle(handle) = focused {
                    let center =
                        handle.center(state.editor.borrow().as_ref().unwrap().geometry.rect());
                    if monitor.rect.contains(PhysicalPoint {
                        x: center.x.clamp(monitor.rect.left, monitor.rect.right - 1),
                        y: center.y.clamp(monitor.rect.top, monitor.rect.bottom - 1),
                    }) && (monitor.rect.contains(center)
                        || index == 0
                            && !state
                                .desktop
                                .monitors
                                .iter()
                                .any(|m| m.rect.contains(center)))
                    {
                        let _ = SetFocus(Some(windows[index].hwnd));
                    }
                }
            }
            let _ = InvalidateRect(Some(windows[index].hwnd), None, false);
            let _ = UpdateWindow(windows[index].hwnd);
        }
    }
    Ok(())
}

struct OutputFlight {
    gate: Arc<OutputGate>,
    result: std::sync::mpsc::Receiver<Result<OutputSuccess, String>>,
}
fn start_output(
    state: &State,
    cancellation: &Cancellation,
    desktop_directory: Option<std::path::PathBuf>,
    action: editor::Action,
    pins: Arc<pin::PinManager>,
) -> Result<Option<OutputFlight>, NativeError> {
    let rect = {
        let mut editor = state.editor.borrow_mut();
        let Some(editor) = editor.as_mut() else {
            return Ok(None);
        };
        if !editor.begin_output(action) {
            return Ok(None);
        }
        editor.geometry.rect()
    };
    let gate = Arc::new(OutputGate::new());
    cancellation.register_output(gate.clone());
    let worker_gate = gate.clone();
    let desktop = state.desktop.clone();
    let budget = state.budget.clone();
    let thread_id = unsafe { GetCurrentThreadId() };
    let (sender, result) = std::sync::mpsc::channel();
    let spawn = std::thread::Builder::new()
        .name("screenshot-output".into())
        .spawn(move || {
            let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if worker_gate.is_cancelled() {
                    return Err("Cancelled".into());
                }
                let image =
                    crate::screenshot::geometry::compose(desktop.frames.frames(), rect, &budget)
                        .map_err(|e| e.to_string())?;
                if action == editor::Action::Pin {
                    return pins
                        .publish(image, rect, &budget, worker_gate)
                        .map(|_| OutputSuccess::Pinned);
                }
                desktop_output::perform(
                    if action == editor::Action::Copy {
                        OutputKind::Copy
                    } else {
                        OutputKind::SaveDesktop
                    },
                    image,
                    &budget,
                    desktop_directory.ok_or_else(|| "DesktopUnavailable".into()),
                    worker_gate,
                )
            }))
            .unwrap_or_else(|_| Err("OutputFailed".into()));
            let _ = sender.send(output);
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_OUTPUT_READY, WPARAM(0), LPARAM(0));
            }
        });
    if spawn.is_err() {
        state
            .editor
            .borrow_mut()
            .as_mut()
            .unwrap()
            .failed("OutputFailed");
        return Ok(None);
    }
    Ok(Some(OutputFlight { gate, result }))
}

pub(super) fn run(
    mode: ScreenshotMode,
    budget: &PixelBudget,
    palette: OverlayPalette,
    excluded: &[isize],
    provider: Option<std::path::PathBuf>,
    desktop_directory: Option<std::path::PathBuf>,
    pins: Arc<pin::PinManager>,
    cancellation: &Cancellation,
) -> Result<SelectionResult, NativeError> {
    crate::screenshot::probe::trace(format!("selection starting mode={mode:?}"));
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
    let captured = capture::desktop(excluded, budget, cancellation);
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
        budget: budget.clone(),
        desktop: Arc::new(desktop),
        selection: RefCell::new(selection),
        editor: RefCell::new(None),
        palette,
        locked: Cell::new(false),
        fallback: Cell::new(true),
        target: Cell::new(None),
        scope: Cell::new(None),
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            PCWSTR(class.name.as_ptr()),
            w!("UiPilot 截图选择"),
            WS_POPUP | WS_CLIPCHILDREN,
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
    let mut probe_disabled = provider.is_none();
    let overlay_thread = unsafe { GetCurrentThreadId() };
    let mut probe = provider.map(|path| {
        crate::screenshot::probe::ProbeWorker::start(path, probe_excluded.clone(), move || unsafe {
            let _ = PostThreadMessageW(overlay_thread, WM_PROBE_READY, WPARAM(0), LPARAM(0));
        })
    });
    let mut pending_probe: Option<(u64, PhysicalPoint)> = None;
    let mut applied_sequence = 0;
    let mut observer = Some(MouseObserver::start(
        windows.iter().map(|w| w.hwnd.0 as isize).collect(),
    )?);
    let buttons = collect_overlays(windows.len(), |index| unsafe {
        toolbar::Toolbar::new(windows[index].hwnd)
    })?;
    let mut output: Option<OutputFlight> = None;
    let mut deferred_cancel = false;
    let mut gesture_id = None;
    // Nothing is visible until the full monitor set exists. Any earlier error
    // unwinds every previously created HWND while contexts and frames still live.
    let mut capture = MouseCapture::new(NativeCapture);
    let result = (|| {
        unsafe {
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
        present(&state, &windows, &probe_excluded)?;
        if let Some(worker) = probe.as_ref().filter(|_| mode == ScreenshotMode::Region) {
            pending_probe = Some((worker.request(point), point));
        }
        loop {
            if let Some(flight) = &output {
                let completed = match flight.result.try_recv() {
                    Ok(result) => Some(result),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Some(Err("OutputFailed".into()))
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                };
                if let Some(completed) = completed {
                    output = None;
                    match completed {
                        Ok(output) => return Ok(SelectionResult::Completed { output }),
                        Err(_) if cancellation.reason().is_some() => {
                            return Ok(SelectionResult::Cancelled(cancellation.reason().unwrap()))
                        }
                        Err(error) => {
                            state.editor.borrow_mut().as_mut().unwrap().failed(&error);
                            sync_editor(&state, &windows, &buttons, true)?;
                        }
                    }
                }
            }
            if let Some(reason) = cancellation.reason() {
                if output.as_ref().is_some_and(|flight| !flight.gate.cancel()) {
                    // Publication already owns the gate. Wait through the native
                    // pump for its actual result; cancellation cannot undo it.
                    deferred_cancel = true;
                    capture.release();
                } else {
                    return Ok(SelectionResult::Cancelled(reason));
                }
            }
            if observer
                .as_ref()
                .is_some_and(|observer| observer.history.lock().map_or(true, |h| h.overflowed))
            {
                return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
            }
            loop {
                // End the queue borrow before calling native APIs: ReleaseCapture
                // and repaint can synchronously reenter window_proc.
                let event = state.events.borrow_mut().pop_front();
                let Some(event) = event else {
                    break;
                };
                if state.editor.borrow().is_some() {
                    let mut action = editor::Action::None;
                    let mut changed = false;
                    let mut focus = false;
                    match event {
                        Event::Cancel(reason) => cancellation.cancel(reason),
                        Event::PaintFailed => cancellation.cancel(CancelReason::LostCapture),
                        Event::CaptureLost => {
                            if capture.active
                                && unsafe {
                                    GetCapture().0.is_null()
                                        || !windows.iter().any(|w| w.hwnd == GetCapture())
                                }
                            {
                                cancellation.cancel(CancelReason::LostCapture);
                            }
                        }
                        Event::Tick => {
                            if !capture::layout()
                                .is_ok_and(|current| current == state.desktop.monitors)
                            {
                                cancellation.cancel(CancelReason::DisplayChanged);
                            }
                            if !windows
                                .iter()
                                .any(|w| w.hwnd == unsafe { GetForegroundWindow() })
                            {
                                cancellation.cancel(CancelReason::LostCapture);
                            }
                        }
                        Event::ProbeReady | Event::OutputReady => {}
                        Event::Down(hwnd, point, _) => {
                            changed = state
                                .editor
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .down(point, &editor_monitors(&state));
                            if changed {
                                // Editing releases persistent capture so the
                                // accessible child BUTTON can receive a click.
                                // Dragging alone reacquires overlay capture.
                                unsafe {
                                    let _ = SetFocus(Some(hwnd));
                                }
                                capture.acquire(hwnd.0 as usize)?;
                                focus = true;
                            }
                        }
                        Event::Up(point) => {
                            changed = state.editor.borrow_mut().as_mut().unwrap().up(point);
                            capture.release();
                            focus = changed;
                            unsafe {
                                update_editor_cursor(&state, point);
                            }
                        }
                        Event::Input(SelectionInput::Move(point)) => {
                            changed = state.editor.borrow_mut().as_mut().unwrap().moved(point);
                            // Capture suppresses normal WM_SETCURSOR delivery;
                            // update flipped directions during the drag too.
                            unsafe {
                                update_editor_cursor(&state, point);
                            }
                        }
                        Event::Key(key, shift, ctrl) => {
                            action = state
                                .editor
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .key(key, shift, ctrl);
                            focus = true;
                        }
                        Event::Button(button) => action = button.action(),
                        Event::ButtonFocused(button) => {
                            state
                                .editor
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .focus_button(button);
                            changed = true;
                        }
                        Event::Input(SelectionInput::Escape) => action = editor::Action::Cancel,
                        Event::Input(_) => {}
                    }
                    match action {
                        editor::Action::Cancel => cancellation.cancel(CancelReason::User),
                        editor::Action::Save | editor::Action::Copy | editor::Action::Pin => {
                            if cancellation.reason().is_none() {
                                if !capture::layout()
                                    .is_ok_and(|current| current == state.desktop.monitors)
                                {
                                    cancellation.cancel(CancelReason::DisplayChanged);
                                } else if output.is_none() {
                                    output = start_output(
                                        &state,
                                        cancellation,
                                        desktop_directory.clone(),
                                        action,
                                        pins.clone(),
                                    )?;
                                    changed = true;
                                }
                            }
                        }
                        editor::Action::Changed => changed = true,
                        editor::Action::None => {}
                    }
                    if changed && sync_editor(&state, &windows, &buttons, focus).is_err() {
                        // Once a job owns commit authorization, only its actual
                        // mailbox result may decide success. UI failure requests
                        // cancellation and keeps pumping until that decision.
                        cancellation.cancel(CancelReason::LostCapture);
                    }
                    continue;
                }
                let input = match event {
                    Event::Key(_, _, _)
                    | Event::Button(_)
                    | Event::ButtonFocused(_)
                    | Event::OutputReady => continue,
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
                    Event::Tick | Event::ProbeReady => {
                        if !capture::layout().is_ok_and(|current| current == state.desktop.monitors)
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::DisplayChanged));
                        }
                        if !state.locked.get() {
                            let before = (
                                state.selection.borrow().highlight(),
                                state.target.get(),
                                state.scope.get(),
                                state.fallback.get(),
                            );
                            let point = state.selection.borrow().cursor;
                            if mode == ScreenshotMode::Region {
                                if let Some(worker) = probe.as_mut() {
                                    while let Some(reply) = worker.poll() {
                                        if reply.unavailable {
                                            probe_disabled = true;
                                            state.fallback.set(true);
                                        }
                                        if reply_applies(
                                            pending_probe,
                                            applied_sequence,
                                            reply.sequence,
                                            reply.point,
                                            reply.rect,
                                            point,
                                            reply.scope.is_some_and(|scope| {
                                                crate::screenshot::probe::scope_is_current(
                                                    scope,
                                                    point,
                                                    &probe_excluded,
                                                )
                                            }),
                                        ) {
                                            let rect = reply.rect;
                                            state.fallback.set(rect.is_none());
                                            state.target.set(reply.target);
                                            state.scope.set(reply.scope);
                                            state.selection.borrow_mut().set_candidate(point, rect);
                                            applied_sequence = reply.sequence;
                                        }
                                        finish_probe_request(
                                            &mut pending_probe,
                                            &mut applied_sequence,
                                            reply.sequence,
                                            reply.point,
                                        );
                                    }
                                    if !probe_disabled
                                        && pending_probe.is_none_or(|(_, p)| p != point)
                                    {
                                        pending_probe = Some((worker.request(point), point));
                                    }
                                }
                            }
                            if mode == ScreenshotMode::Window {
                                let selected = state.selection.borrow().selected_window().cloned();
                                let target = selected.as_ref().and_then(|w| {
                                    current_window_target(w, &probe_excluded, state.desktop.bounds)
                                });
                                state.target.set(target);
                                let rect = selected.filter(|_| target.is_some()).map(|w| w.rect);
                                state.selection.borrow_mut().set_candidate(point, rect);
                            }
                            refresh_window_fallback(&state, &probe_excluded)?;
                            let after = (
                                state.selection.borrow().highlight(),
                                state.target.get(),
                                state.scope.get(),
                                state.fallback.get(),
                            );
                            if before != after || state.presented_at.get().is_none() {
                                present(&state, &windows, &probe_excluded)?;
                            }
                        }
                        continue;
                    }
                    Event::Down(_hwnd, point, time) => {
                        gesture_id = observer.as_ref().unwrap().claim(point);
                        if gesture_id.is_none() {
                            crate::screenshot::probe::trace(
                                "mouse observer missing ordered gesture",
                            );
                            return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
                        }
                        let point = pointer_point(point, &state);
                        crate::screenshot::probe::trace(format!(
                            "pointer down highlight={:?}",
                            state.selection.borrow().highlight()
                        ));
                        if let Some((sequence, _)) = pending_probe.take() {
                            applied_sequence = applied_sequence.max(sequence);
                        }
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
                            state.scope.set(None);
                        }
                        if mode == ScreenshotMode::Window
                            && !state
                                .target
                                .get()
                                .is_some_and(crate::screenshot::probe::target_is_current)
                        {
                            return Ok(SelectionResult::Cancelled(CancelReason::User));
                        }
                        validate_candidate(&state, point, &probe_excluded);
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
                        if let Some(id) = gesture_id.take() {
                            let Some(extent) = observer.as_ref().unwrap().finish(id) else {
                                crate::screenshot::probe::trace(format!(
                                    "mouse observer missing completed gesture id={id}"
                                ));
                                return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
                            };
                            observe_extent(&state, extent);
                        }
                        SelectionInput::PointerUp(point)
                    }
                    Event::Input(input) => {
                        if let Some(id) = gesture_id {
                            if let Some(extent) = observer.as_ref().unwrap().distance(id) {
                                observe_extent(&state, extent);
                            }
                        }
                        if invalidates_live_probe(
                            input,
                            state.selection.borrow().cursor,
                            state.locked.get(),
                        ) {
                            if !matches!(input, SelectionInput::Move(_)) {
                                if let Some((sequence, _)) = pending_probe {
                                    applied_sequence = applied_sequence.max(sequence);
                                }
                            }
                            pending_probe = None;
                            if !matches!(input, SelectionInput::Move(p)
                                if state.selection.borrow().highlight().is_some_and(|r| r.contains(p)))
                            {
                                state.target.set(None);
                                state.scope.set(None);
                            }
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
                            let point = state.selection.borrow().cursor;
                            validate_candidate(&state, point, &probe_excluded);
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
                    state.locked.set(true);
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
                            crate::screenshot::probe::trace(
                                "foreground lost before frame composition",
                            );
                            return Ok(SelectionResult::Cancelled(CancelReason::LostCapture));
                        }
                        *state.editor.borrow_mut() =
                            editor::Editor::new(rect, state.desktop.bounds);
                        if state.editor.borrow().is_none() {
                            return Err(NativeError::OverlayFailed);
                        }
                        // Geometry is now owned by the frozen editing session,
                        // independent of live window positions or late hits.
                        pending_probe = None;
                        state.target.set(None);
                        state.scope.set(None);
                        drop(probe.take());
                        drop(observer.take());
                        gesture_id = None;
                        capture.release();
                        sync_editor(&state, &windows, &buttons, true)?;
                        continue;
                    }
                    Ok(None) => {}
                }
                if state.selection.borrow().hovering() && state.locked.get() {
                    state.locked.set(false);
                    state.presented_candidate.set(None);
                    pending_probe = None;
                }
                {
                    let mut selection = state.selection.borrow_mut();
                    selection.cursor = bounded_cursor(selection.cursor, state.desktop.bounds);
                }
                if !state.locked.get() {
                    if state.selection.borrow().hovering() && !probe_disabled {
                        let point = state.selection.borrow().cursor;
                        if let Some(worker) = &probe {
                            if pending_probe.is_none_or(|(_, requested)| requested != point) {
                                pending_probe = Some((worker.request(point), point));
                            }
                        }
                    }
                    present(&state, &windows, &probe_excluded)?;
                } else {
                    unsafe {
                        for window in &windows {
                            let _ = InvalidateRect(Some(window.hwnd), None, false);
                        }
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
                    Some(if deferred_cancel {
                        &[]
                    } else {
                        std::slice::from_ref(&wake)
                    }),
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if waited == WAIT_FAILED {
                    if output.is_some() {
                        cancellation.cancel(CancelReason::LostCapture);
                        continue;
                    }
                    return Err(NativeError::OverlayFailed);
                }
                if waited == WAIT_OBJECT_0 && !deferred_cancel {
                    continue;
                }
                let mut message = MSG::default();
                // Dispatch one message then process the resulting event before
                // reading more, keeping capture/selection order deterministic.
                if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message == WM_PROBE_READY {
                        state.events.borrow_mut().push_back(Event::ProbeReady);
                        continue;
                    }
                    if message.message == WM_OUTPUT_READY {
                        state.events.borrow_mut().push_back(Event::OutputReady);
                        continue;
                    }
                    if message.message == WM_QUIT {
                        cancellation.cancel(CancelReason::Shutdown);
                        continue;
                    }
                    // Child BUTTON focus retains the editor keyboard contract.
                    // Do not translate Tab/Enter into button-specific behavior.
                    if message.message == WM_KEYDOWN && state.editor.borrow().is_some() {
                        if !editor::accepts_key(
                            message.wParam.0 as u16,
                            message.lParam.0 & (1 << 30) != 0,
                        ) {
                            continue;
                        }
                        state.events.borrow_mut().push_back(Event::Key(
                            message.wParam.0 as u16,
                            GetKeyState(VK_SHIFT.0 as i32) < 0,
                            GetKeyState(VK_CONTROL.0 as i32) < 0,
                        ));
                        continue;
                    }
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    })();
    let result = finish_cancel(result, cancellation.reason());
    let result = finalize_ownership(
        result,
        windows
            .iter()
            .any(|window| window.hwnd == unsafe { GetForegroundWindow() }),
    );
    match &result {
        Ok(SelectionResult::Completed { output }) => {
            crate::screenshot::probe::trace(format!("selection output {output:?}"))
        }
        Ok(SelectionResult::Selected { rect, .. }) => {
            crate::screenshot::probe::trace(format!("selection captured {rect:?}"))
        }
        Ok(SelectionResult::Cancelled(reason)) => {
            crate::screenshot::probe::trace(format!("selection cancelled {reason:?}"))
        }
        Err(error) => crate::screenshot::probe::trace(format!("selection failed {error:?}")),
    }
    state.closing.set(true);
    capture.release();
    drop(buttons);
    drop(windows);
    result
}

fn observe_extent(state: &State, extent: MotionExtent) {
    let mut selection = state.selection.borrow_mut();
    selection.observe_motion(extent.min_x, extent.min_y);
    selection.observe_motion(extent.max_x, extent.max_y);
}

fn validate_candidate(state: &State, point: PhysicalPoint, excluded: &[isize]) {
    if state.selection.borrow().hovering()
        && (!state
            .target
            .get()
            .is_some_and(crate::screenshot::probe::target_is_current)
            || !state.scope.get().map_or(state.fallback.get(), |scope| {
                crate::screenshot::probe::scope_is_current(scope, point, excluded)
            }))
    {
        let point = state.selection.borrow().cursor;
        state.selection.borrow_mut().set_candidate(point, None);
        state.target.set(None);
        state.scope.set(None);
    }
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

fn finish_cancel(
    result: Result<SelectionResult, NativeError>,
    reason: Option<CancelReason>,
) -> Result<SelectionResult, NativeError> {
    // A successfully published file or clipboard change is irreversible. Its
    // actual completion wins over cleanup-time cancellation and focus changes.
    match result {
        Ok(SelectionResult::Completed { .. }) => result,
        other => reason.map_or(other, |reason| Ok(SelectionResult::Cancelled(reason))),
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
// Draw the frame, shade, hole and border off-screen, then publish one complete
// image. Painting these layers directly exposes a bright desktop between them.
struct BackBuffer {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    _reservation: crate::screenshot::geometry::BudgetReservation,
}
impl BackBuffer {
    unsafe fn new(
        screen: HDC,
        width: u32,
        height: u32,
        budget: &PixelBudget,
    ) -> Result<Self, NativeError> {
        let reservation = budget.reserve(crate::screenshot::geometry::rgba_len(width, height)?)?;
        let dc = CreateCompatibleDC(Some(screen));
        if dc.0.is_null() {
            return Err(NativeError::OverlayFailed);
        }
        let mut bits = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(
            Some(screen),
            &capture::bitmap_info(width as i32, height as i32),
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        ) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                let _ = DeleteDC(dc);
                return Err(NativeError::OverlayFailed);
            }
        };
        let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
        if previous.0.is_null() || previous.0 as isize == -1 {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            return Err(NativeError::OverlayFailed);
        }
        Ok(Self {
            dc,
            bitmap,
            previous,
            _reservation: reservation,
        })
    }
}
impl Drop for BackBuffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
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
fn reply_applies(
    pending: Option<(u64, PhysicalPoint)>,
    applied: u64,
    sequence: u64,
    point: PhysicalPoint,
    rect: Option<PhysicalRect>,
    cursor: PhysicalPoint,
    scope_current: bool,
) -> bool {
    let Some((latest, _)) = pending else {
        return false;
    };
    sequence > applied
        && sequence <= latest
        && (rect.is_none() || scope_current)
        && (rect.is_some_and(|rect| rect.contains(cursor))
            || (sequence == latest && point == cursor))
}
fn finish_probe_request(
    pending: &mut Option<(u64, PhysicalPoint)>,
    applied: &mut u64,
    sequence: u64,
    point: PhysicalPoint,
) {
    // Completion is independent of presentation acceptance: a stale-scope hit
    // must free the latest request slot so a stationary cursor can be reprobed.
    if *pending == Some((sequence, point)) {
        *pending = None;
        *applied = (*applied).max(sequence);
    }
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
fn guard_presented_down(
    selection: &mut SelectionState,
    _point: PhysicalPoint,
    time: u32,
    presented: Option<u32>,
) -> bool {
    let accepted = input_after_presentation(time, presented);
    if !accepted {
        selection.set_candidate(selection.cursor, None);
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
fn refresh_window_fallback(state: &State, excluded: &[isize]) -> Result<(), NativeError> {
    if state.locked.get() || !state.selection.borrow().hovering() {
        return Ok(());
    }
    let fresh = capture::window_snapshot(excluded, state.desktop.bounds)?;
    let point = state.selection.borrow().cursor;
    if !same_visible_window(&state.desktop.candidates, &fresh, point) {
        state.selection.borrow_mut().set_candidate(point, None);
        state.target.set(None);
        state.scope.set(None);
        state.fallback.set(true);
        return Ok(());
    }
    let candidate = state.selection.borrow().selected_window().cloned();
    if !state.fallback.get()
        && state.selection.borrow().highlight().is_some()
        && state
            .scope
            .get()
            .is_some_and(|scope| crate::screenshot::probe::scope_is_current(scope, point, excluded))
        && state.target.get().is_some_and(|target| {
            candidate
                .as_ref()
                .is_some_and(|window| window.id == target.hwnd)
                && crate::screenshot::probe::target_is_current(target)
        })
    {
        return Ok(());
    }
    let target = candidate
        .as_ref()
        .and_then(|window| current_window_target(window, excluded, state.desktop.bounds));
    let rect = candidate.filter(|_| target.is_some()).map(|w| w.rect);
    state.target.set(target);
    state.scope.set(None);
    state.fallback.set(true);
    let point = state.selection.borrow().cursor;
    state
        .selection
        .borrow_mut()
        .set_probe_candidate(point, None, rect);
    Ok(())
}
fn same_visible_window(
    captured: &[WindowCandidate],
    current: &[WindowCandidate],
    point: PhysicalPoint,
) -> bool {
    let top = |windows: &[WindowCandidate]| {
        windows
            .iter()
            .filter(|w| w.rect.contains(point))
            .min_by_key(|w| w.z_order)
            .map(|w| (w.id, w.rect))
    };
    top(captured).is_some() && top(captured) == top(current)
}
fn present(state: &State, windows: &[OwnedWindow], excluded: &[isize]) -> Result<(), NativeError> {
    refresh_window_fallback(state, excluded)?;
    // Keyboard cycling and live pointer movement publish identity with geometry,
    // without waiting for a subsequent 50ms hover tick. Frozen locks stay owned.
    if !state.locked.get() && state.selection.borrow().mode == ScreenshotMode::Window {
        state
            .target
            .set(presented_window_target(&state.selection.borrow(), |w| {
                current_window_target(w, excluded, state.desktop.bounds)
            }));
    }
    let candidate = (
        state.selection.borrow().highlight(),
        state.target.get(),
        state.scope.get(),
        state.fallback.get(),
    );
    let changed = presentation_changed(state.presented_candidate.get(), candidate);
    if !changed {
        return Ok(());
    }
    if changed {
        state.presented_at.set(None);
        crate::screenshot::probe::trace(format!(
            "present candidate={:?} window_fallback={}",
            candidate.0,
            state.fallback.get()
        ));
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
    let buffer = render_overlay(paint.dc, state, index)?;
    let (width, height) = state.desktop.monitors[index].rect.dimensions()?;
    BitBlt(
        paint.dc,
        0,
        0,
        width as i32,
        height as i32,
        Some(buffer.dc),
        0,
        0,
        SRCCOPY,
    )
    .map_err(|_| NativeError::OverlayFailed)?;
    Ok(())
}

/// The native paint path and controlled synthetic-frame fixture share this
/// compositor. Editing controls never modify the immutable capture frames.
unsafe fn render_overlay(
    reference_dc: HDC,
    state: &State,
    index: usize,
) -> Result<BackBuffer, NativeError> {
    let monitor = state.desktop.monitors[index];
    let frame = &state.desktop.frames.frames()[index];
    let selection = state.selection.borrow();
    let editor = state.editor.borrow();
    let editor = editor.as_ref();
    let (width, height) = monitor.rect.dimensions()?;
    let buffer = BackBuffer::new(reference_dc, width, height, &state.budget)?;
    let dc = buffer.dc;
    frozen(dc, frame)?;
    if editor.is_some() || selection.highlight().is_some() || state.locked.get() {
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
    }
    let selected = editor
        .map(|editor| editor.geometry.rect())
        .or_else(|| selection.highlight());
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
            frozen(dc, frame)?;
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
    if editor.is_none()
        && selection.mode == ScreenshotMode::Region
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
        right: (px(if editor.is_some() { 540.0 } else { 324.0 })).min(width as i32),
        bottom: (px(44.0)).min(height as i32),
    };
    fill(dc, label, state.palette.surface)?;
    let text = if let Some(editor) = editor {
        let (width, height) = editor.geometry.rect().dimensions()?;
        if let Some(error) = editor.error {
            format!("{width} × {height} px · {error}")
        } else if editor.outputting {
            format!(
                "{width} × {height} px · {} · Esc 取消",
                match editor.output_action {
                    Some(editor::Action::Pin) => "贴图中…",
                    Some(editor::Action::Copy) => "复制中…",
                    _ => "保存中…",
                }
            )
        } else {
            format!("{width} × {height} px · Tab 切换 · Enter 执行 · Ctrl+C 复制 · Esc 取消")
        }
    } else {
        match selected.and_then(|rect| rect.dimensions().ok()) {
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
                    "移动选择窗口或元素 · 拖动框选 · Esc 取消".into()
                }
                ScreenshotMode::Region => "单击元素 / 拖动选择 · Space 起点 · Esc 取消".into(),
                ScreenshotMode::Window => "选择窗口 · 方向键切换 · Esc 取消".into(),
            },
        }
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
    SetTextColor(
        dc,
        color(if editor.is_some_and(|editor| editor.error.is_some()) {
            state.palette.destructive
        } else {
            state.palette.foreground
        }),
    );
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
    if let Some(editor) = editor {
        draw_handles(dc, state, editor, monitor.rect)?;
    }
    Ok(buffer)
}

unsafe fn draw_handles(
    dc: HDC,
    state: &State,
    editor: &editor::Editor,
    monitor: PhysicalRect,
) -> Result<(), NativeError> {
    let rect = editor.geometry.rect();
    for handle in Handle::ALL {
        let center = handle.center(rect);
        let dpi = state
            .desktop
            .monitors
            .iter()
            .find(|m| m.rect.contains(center))
            .or_else(|| {
                state.desktop.monitors.iter().min_by_key(|m| {
                    let dx = i64::from(center.x)
                        - i64::from(center.x.clamp(m.rect.left, m.rect.right - 1));
                    let dy = i64::from(center.y)
                        - i64::from(center.y.clamp(m.rect.top, m.rect.bottom - 1));
                    dx * dx + dy * dy
                })
            })
            .map_or(96, |m| m.dpi_x);
        let visual = handle.visual_rect(rect, dpi);
        let Some(visible) = visual.intersection(monitor) else {
            continue;
        };
        let local = RECT {
            left: visible.left - monitor.left,
            top: visible.top - monitor.top,
            right: visible.right - monitor.left,
            bottom: visible.bottom - monitor.top,
        };
        fill(dc, local, state.palette.primary_foreground)?;
        let border = brush(state.palette.selection_border)?;
        FrameRect(dc, &local, HBRUSH(border.0 .0));
        if editor.focus == editor::Focus::Handle(handle) {
            let thickness = ((2 * dpi + 48) / 96).max(1) as i32;
            let focus = RECT {
                left: local.left - thickness,
                top: local.top - thickness,
                right: local.right + thickness,
                bottom: local.bottom + thickness,
            };
            FrameRect(dc, &focus, HBRUSH(border.0 .0));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn completed_publication_survives_cleanup_cancellation_and_foreground_loss() {
        let saved = std::path::PathBuf::from("actual-desktop-result.png");
        let result = finish_cancel(
            Ok(SelectionResult::Completed {
                output: OutputSuccess::Saved(saved.clone()),
            }),
            Some(CancelReason::Requested),
        );
        assert!(
            matches!(finalize_ownership(result, false), Ok(SelectionResult::Completed { output: OutputSuccess::Saved(path) }) if path == saved)
        );
    }
    #[test]
    fn unsuccessful_selection_still_obeys_cleanup_cancellation() {
        assert!(matches!(
            finish_cancel(
                Err(NativeError::CaptureFailed),
                Some(CancelReason::SessionChanged)
            ),
            Ok(SelectionResult::Cancelled(CancelReason::SessionChanged))
        ));
    }
    #[test]
    fn hidden_native_toolbar_has_accessible_names_routes_actions_and_renders_both_themes() {
        // All messages target this fixture's hidden HWND; no input is injected
        // into the desktop, provider or any application belonging to the user.
        let _dpi = DpiContext::enter().unwrap();
        let cancellation = Cancellation::default();
        let class = WindowClass::register().unwrap();
        let bounds = PhysicalRect {
            left: 0,
            top: 0,
            right: 400,
            bottom: 300,
        };
        let budget = PixelBudget::new(16 * 1024 * 1024);
        let mut image = BudgetedImage::allocate(400, 300, &budget).unwrap();
        for (index, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
            let x = index % 400;
            let y = index / 400;
            pixel.copy_from_slice(&[120 + (x / 4) as u8, 110 + (y / 3) as u8, 180, 255]);
        }
        let frames = crate::screenshot::geometry::FrameSet::new(vec![(bounds, image)]).unwrap();
        let mut state = Box::new(State {
            desktop: Arc::new(capture::Desktop {
                monitors: vec![capture::Monitor {
                    rect: bounds,
                    dpi_x: 96,
                    dpi_y: 96,
                }],
                frames,
                candidates: vec![],
                bounds,
            }),
            budget,
            selection: RefCell::new(SelectionState::new(
                ScreenshotMode::Region,
                PhysicalPoint { x: 0, y: 0 },
                vec![],
            )),
            editor: RefCell::new(editor::Editor::new(
                PhysicalRect {
                    left: 50,
                    top: 50,
                    right: 200,
                    bottom: 180,
                },
                bounds,
            )),
            palette: OverlayPalette {
                foreground: 0xffffff,
                surface: 0x202020,
                selection_border: 0x888888,
                mask: 0,
                mask_opacity: 128,
                primary: 0xffffff,
                primary_foreground: 0,
                destructive: 0xff0000,
            },
            locked: Cell::new(true),
            fallback: Cell::new(false),
            target: Cell::new(None),
            scope: Cell::new(None),
            presented_at: Cell::new(None),
            presented_candidate: Cell::new(None),
            events: RefCell::new(VecDeque::new()),
            closing: Cell::new(false),
        });
        let context = Box::new(WindowContext {
            state: &*state,
            index: 0,
            cancellation: &cancellation,
        });
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.name.as_ptr()),
                w!("UiPilot screenshot test fixture"),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                400,
                300,
                None,
                None,
                Some(class.instance),
                Some(&*context as *const _ as *const _),
            )
            .unwrap()
        };
        let window = OwnedWindow {
            hwnd,
            _context: context,
            session_notifications: false,
        };
        let toolbar = unsafe { toolbar::Toolbar::new(hwnd).unwrap() };
        for (index, button) in toolbar.buttons.iter().enumerate() {
            let action = editor::Button::ALL[index];
            let mut caption = [0u16; 32];
            let length = unsafe { GetWindowTextW(button.hwnd, &mut caption) };
            assert_eq!(
                String::from_utf16_lossy(&caption[..length as usize]),
                toolbar::name(action)
            );
            assert_ne!(
                unsafe { GetWindowLongPtrW(button.hwnd, GWL_STYLE) } as u32 & WS_TABSTOP.0,
                0
            );
            unsafe {
                SendMessageW(button.hwnd, BM_CLICK, None, None);
            }
            assert!(state
                .events
                .borrow_mut()
                .drain(..)
                .any(|event| matches!(event, Event::Button(b) if b == action)));
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_COMMAND,
                    Some(WPARAM(toolbar::id(action) | ((BN_SETFOCUS as usize) << 16))),
                    Some(LPARAM(button.hwnd.0 as isize)),
                );
            }
            assert!(
                matches!(state.events.borrow_mut().pop_front(), Some(Event::ButtonFocused(b)) if b == action)
            );
            unsafe {
                assert_eq!(
                    SendMessageW(
                        button.tooltip,
                        windows::Win32::UI::Controls::TTM_GETTOOLCOUNT,
                        None,
                        None
                    )
                    .0,
                    1
                );
            }
        }
        unsafe {
            SendMessageW(
                hwnd,
                WM_KEYDOWN,
                Some(WPARAM(VK_TAB.0 as usize)),
                Some(LPARAM(0)),
            );
        }
        assert!(matches!(
            state.events.borrow_mut().pop_front(),
            Some(Event::Key(9, _, _))
        ));
        let edge_rect = PhysicalRect {
            left: 50,
            top: 50,
            right: 399,
            bottom: 299,
        };
        for down in [
            PhysicalPoint { x: 399, y: 174 },
            PhysicalPoint { x: 224, y: 299 },
            PhysicalPoint { x: 399, y: 299 },
        ] {
            *state.editor.borrow_mut() = editor::Editor::new(edge_rect, bounds);
            assert!(state
                .editor
                .borrow_mut()
                .as_mut()
                .unwrap()
                .down(down, &editor_monitors(&state)));
            let up = pointer_point(down, &state);
            state.editor.borrow_mut().as_mut().unwrap().up(up);
            assert_eq!(
                state.editor.borrow().as_ref().unwrap().geometry.rect(),
                edge_rect,
                "native down/map/up must preserve a stationary edge click"
            );
        }
        let original_rect = PhysicalRect {
            left: 50,
            top: 50,
            right: 200,
            bottom: 180,
        };
        *state.editor.borrow_mut() = editor::Editor::new(original_rect, bounds);
        assert!(state
            .editor
            .borrow_mut()
            .as_mut()
            .unwrap()
            .down(PhysicalPoint { x: 200, y: 115 }, &editor_monitors(&state)));
        assert_eq!(
            pointer_point(PhysicalPoint { x: 399, y: 299 }, &state),
            PhysicalPoint { x: 400, y: 300 },
            "captured editing drag reaches half-open desktop edges"
        );
        let up = pointer_point(PhysicalPoint { x: 399, y: 115 }, &state);
        state.editor.borrow_mut().as_mut().unwrap().up(up);
        assert_eq!(
            state
                .editor
                .borrow()
                .as_ref()
                .unwrap()
                .geometry
                .rect()
                .right,
            400
        );
        *state.editor.borrow_mut() = editor::Editor::new(original_rect, bounds);
        for (name, foreground, surface, primary, primary_foreground, selection_border) in [
            ("light", 0x18191a, 0xf8f8f9, 0x000000, 0xffffff, 0x18191a),
            ("dark", 0xffffff, 0x171919, 0xffffff, 0x000000, 0xffffff),
        ] {
            state.palette = OverlayPalette {
                foreground,
                surface,
                primary,
                primary_foreground,
                selection_border,
                mask: 0,
                mask_opacity: 128,
                destructive: 0xc62828,
            };
            unsafe {
                let buffer = render_overlay(HDC::default(), &state, 0).unwrap();
                let editor = state.editor.borrow();
                let editor = editor.as_ref().unwrap();
                for handle in Handle::ALL {
                    let center = handle.center(editor.geometry.rect());
                    assert_eq!(
                        GetPixel(buffer.dc, center.x, center.y),
                        color(primary_foreground),
                        "all eight native handle centers are visible in {name}"
                    );
                }
                let button_rect = editing::save_button_rect(
                    editor.geometry.rect(),
                    &editor_monitors(&state),
                    112,
                    28,
                );
                for (index, button) in toolbar.buttons.iter().enumerate() {
                    let left = button_rect.left + 28 * index as i32;
                    let item = DRAWITEMSTRUCT {
                        CtlID: toolbar::id(editor::Button::ALL[index]) as u32,
                        hwndItem: button.hwnd,
                        hDC: buffer.dc,
                        itemState: windows::Win32::UI::Controls::ODS_FOCUS,
                        rcItem: RECT {
                            left,
                            top: button_rect.top,
                            right: left + 28,
                            bottom: button_rect.bottom,
                        },
                        ..Default::default()
                    };
                    toolbar::draw(state.palette, false, &item).unwrap();
                    let background = primary_foreground;
                    assert_eq!(
                        GetPixel(buffer.dc, left + 3, button_rect.top + 14),
                        color(background),
                        "every action uses the same monochrome background in {name}"
                    );
                    for y in button_rect.top..button_rect.bottom {
                        for x in left..left + 28 {
                            let rgb = GetPixel(buffer.dc, x, y).0;
                            assert_eq!(rgb & 255, (rgb >> 8) & 255, "no colored action in {name}");
                            assert_eq!(rgb & 255, (rgb >> 16) & 255, "no colored action in {name}");
                        }
                    }
                    assert_ne!(
                        GetPixel(buffer.dc, left + 14, button_rect.top),
                        color(background),
                        "all action focus rings must be visible in {name}"
                    );
                    let mut different = 0;
                    for y in button_rect.top + 6..button_rect.bottom - 6 {
                        for x in left + 6..left + 22 {
                            if GetPixel(buffer.dc, x, y) != color(background) {
                                different += 1;
                            }
                        }
                    }
                    assert!(
                        different > 15,
                        "canonical icon must be actually painted for action {index} in {name}"
                    );
                    let item = DRAWITEMSTRUCT {
                        itemState: if index == 2 {
                            windows::Win32::UI::Controls::ODS_FOCUS
                        } else {
                            windows::Win32::UI::Controls::ODS_DEFAULT
                        },
                        ..item
                    };
                    toolbar::draw(state.palette, false, &item).unwrap();
                }
                // Bright selection and shaded outside are composed from the
                // fixture's frozen frame, using the production native renderer.
                assert_eq!(GetPixel(buffer.dc, 100, 100), color(0x918fb4));
                assert_ne!(GetPixel(buffer.dc, 350, 250), color(0xcf_c1_b4));
                let _ = GdiFlush();
                let mut bitmap = BITMAP::default();
                assert_ne!(
                    GetObjectW(
                        HGDIOBJ(buffer.bitmap.0),
                        std::mem::size_of::<BITMAP>() as i32,
                        Some((&mut bitmap as *mut BITMAP).cast())
                    ),
                    0
                );
                let bits = std::slice::from_raw_parts(bitmap.bmBits.cast::<u8>(), 400 * 300 * 4);
                let mut rgba = bits.to_vec();
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                    pixel[3] = 255;
                }
                let png = crate::screenshot::output::encode_png(400, 300, &rgba).unwrap();
                let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "../.superpowers/sdd/toolbar-pin/native-editor-{name}.png"
                ));
                std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
                std::fs::write(artifact, png).unwrap();
            }
        }
        let temporary = std::env::temp_dir().join(format!(
            "uipilot-overlay-output-{}-{}",
            std::process::id(),
            unsafe { GetCurrentThreadId() }
        ));
        std::fs::create_dir_all(&temporary).unwrap();
        let output = start_output(
            &state,
            &cancellation,
            Some(temporary.clone()),
            editor::Action::Save,
            Arc::new(pin::PinManager::default()),
        )
        .unwrap()
        .unwrap();
        let success = output
            .result
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let OutputSuccess::Saved(path) = success else {
            panic!("save fixture should publish a PNG")
        };
        let expected = crate::screenshot::geometry::compose(
            state.desktop.frames.frames(),
            state.editor.borrow().as_ref().unwrap().geometry.rect(),
            &state.budget,
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(),crate::screenshot::output::encode_png(expected.width(),expected.height(),&expected.image().pixels).unwrap(),"output contains frozen pixels only, excluding handles, status label and real child button");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(temporary).unwrap();
        state.closing.set(true);
        drop(toolbar);
        drop(window);
    }
    #[test]
    fn rejected_latest_completion_requeues_without_consuming_a_newer_request() {
        let point = PhysicalPoint { x: 20, y: 20 };
        let mut pending = Some((3, point));
        let mut applied = 1;
        finish_probe_request(&mut pending, &mut applied, 2, point);
        assert_eq!(pending, Some((3, point)));
        assert_eq!(applied, 1);
        finish_probe_request(&mut pending, &mut applied, 3, point);
        assert!(
            pending.is_none(),
            "latest completion must allow a fresh request even when its hit was rejected"
        );
        assert_eq!(applied, 3);
    }
    #[test]
    fn overlapping_native_sibling_scope_rejects_historical_hit_but_same_scope_accepts() {
        let queried = PhysicalPoint { x: 20, y: 20 };
        let cursor = PhysicalPoint { x: 40, y: 20 };
        let rect = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        assert!(!reply_applies(
            Some((3, cursor)),
            1,
            2,
            queried,
            Some(rect),
            cursor,
            false
        ));
        assert!(reply_applies(
            Some((3, cursor)),
            1,
            2,
            queried,
            Some(rect),
            cursor,
            true
        ));
        assert!(!reply_applies(
            Some((3, cursor)),
            1,
            3,
            cursor,
            Some(rect),
            cursor,
            false
        ));
    }
    #[test]
    fn moving_within_completed_hit_does_not_wait_for_an_exact_stationary_point() {
        let queried = PhysicalPoint { x: 20, y: 20 };
        let cursor = PhysicalPoint { x: 24, y: 22 };
        let rect = PhysicalRect {
            left: 10,
            top: 10,
            right: 30,
            bottom: 30,
        };
        assert!(reply_applies(
            Some((3, cursor)),
            1,
            2,
            queried,
            Some(rect),
            cursor,
            true
        ));
        assert!(!reply_applies(
            Some((3, cursor)),
            2,
            2,
            queried,
            Some(rect),
            cursor,
            true
        ));
        assert!(!reply_applies(
            Some((3, cursor)),
            1,
            2,
            queried,
            None,
            cursor,
            true
        ));
        assert!(!reply_applies(
            None,
            1,
            2,
            queried,
            Some(rect),
            cursor,
            true
        ));
        assert!(!reply_applies(
            Some((3, cursor)),
            1,
            4,
            queried,
            Some(rect),
            cursor,
            true
        ));
        let outside = PhysicalPoint { x: 30, y: 22 };
        assert!(!reply_applies(
            Some((3, outside)),
            1,
            2,
            queried,
            Some(rect),
            outside,
            true
        ));
        assert!(reply_applies(
            Some((3, cursor)),
            2,
            3,
            cursor,
            None,
            cursor,
            true
        ));
    }
    #[test]
    fn captured_candidates_reject_live_movement_and_changed_occlusion() {
        let point = PhysicalPoint { x: 20, y: 20 };
        let rect = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let original = vec![WindowCandidate {
            id: 1,
            rect,
            z_order: 0,
        }];
        assert!(same_visible_window(&original, &original, point));
        let moved = vec![WindowCandidate {
            id: 1,
            rect: PhysicalRect { left: 10, ..rect },
            z_order: 0,
        }];
        assert!(!same_visible_window(&original, &moved, point));
        let popup = vec![
            WindowCandidate {
                id: 2,
                rect,
                z_order: 0,
            },
            WindowCandidate {
                id: 1,
                rect,
                z_order: 1,
            },
        ];
        assert!(!same_visible_window(&original, &popup, point));
        assert!(!same_visible_window(&popup, &original, point));
        assert!(!same_visible_window(&original, &[], point));
    }
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
    fn claim_native_gesture_without_relying_on_message_timestamp() {
        let mut h = MotionHistory::default();
        let p = PhysicalPoint { x: 10, y: 20 };
        h.record(WM_LBUTTONDOWN, 100, p);
        h.record(WM_MOUSEMOVE, 101, PhysicalPoint { x: 30, y: 20 });
        h.record(WM_LBUTTONUP, 102, p);
        let id = h.claim(p).expect("queued down claims native gesture");
        assert_eq!(h.completed(id).unwrap().max_x, 20);
        h.record(WM_LBUTTONDOWN, 100, p);
        h.record(WM_LBUTTONUP, 103, p);
        let next = h.claim(p).unwrap();
        assert_ne!(id, next);
        assert!(h.completed(next).is_some());
    }
    #[test]
    fn claim_never_skips_an_unmatched_native_press() {
        let mut h = MotionHistory::default();
        h.record(WM_LBUTTONDOWN, 100, PhysicalPoint { x: 10, y: 20 });
        assert!(h.claim(PhysicalPoint { x: 11, y: 20 }).is_none());
        let id = h.claim(PhysicalPoint { x: 10, y: 20 }).unwrap();
        assert!(h.completed(id).is_none());
    }
    #[test]
    fn separate_native_history_preserves_excursion_and_original_down() {
        let mut h = MotionHistory::default();
        let p = PhysicalPoint { x: -100, y: -100 };
        h.record(WM_LBUTTONDOWN, 10, p);
        h.record(WM_MOUSEMOVE, 11, PhysicalPoint { x: -80, y: -130 });
        h.record(WM_MOUSEMOVE, 12, p);
        h.record(WM_LBUTTONUP, 13, p);
        h.record(WM_LBUTTONDOWN, 14, PhysicalPoint { x: 100, y: 100 });
        let first = h.claim(p).unwrap();
        let second = h.claim(PhysicalPoint { x: 100, y: 100 }).unwrap();
        assert_eq!(
            h.distance(first),
            Some(MotionExtent {
                min_x: 0,
                max_x: 20,
                min_y: -30,
                max_y: 0
            })
        );
        assert_eq!(h.distance(second), Some(MotionExtent::default()));
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
        let observer = MouseObserver::start(vec![]).expect("native observer should install");
        assert_ne!(observer.thread_id, 0);
        assert!(observer.history.lock().is_ok());
        drop(observer);
    }
}
