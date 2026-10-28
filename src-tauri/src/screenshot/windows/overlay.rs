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
    Down(HWND, PhysicalPoint),
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
    if state.selection.borrow().mode == ScreenshotMode::Region {
        if point.x == bounds.right - 1 {
            point.x = bounds.right;
        }
        if point.y == bounds.bottom - 1 {
            point.y = bounds.bottom;
        }
    }
    point
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
        WM_MOUSEMOVE => cursor()
            .ok()
            .map(|point| Event::Input(SelectionInput::Move(pointer_point(point, state)))),
        WM_LBUTTONDOWN => cursor()
            .ok()
            .map(|point| Event::Down(hwnd, pointer_point(point, state))),
        WM_LBUTTONUP => cursor()
            .ok()
            .map(|point| Event::Up(pointer_point(point, state))),
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
        desktop,
        selection: RefCell::new(selection),
        palette,
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
            if SetTimer(Some(active.hwnd), 1, 150, None) == 0 {
                return Err(NativeError::OverlayFailed);
            }
        }
        loop {
            if let Some(reason) = cancellation.reason() {
                return Ok(SelectionResult::Cancelled(reason));
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
                        continue;
                    }
                    Event::Down(hwnd, point) => {
                        capture.acquire(hwnd.0 as usize)?;
                        SelectionInput::PointerDown(point)
                    }
                    Event::Up(point) => {
                        if !capture.active {
                            continue;
                        }
                        capture.release();
                        SelectionInput::PointerUp(point)
                    }
                    Event::Input(input) => input,
                };
                let unchanged =
                    if matches!(input, SelectionInput::Enter | SelectionInput::PointerUp(_)) {
                        capture::layout().is_ok_and(|current| current == state.desktop.monitors)
                    } else {
                        true
                    };
                let outcome = final_selection(&mut state.selection.borrow_mut(), input, unchanged);
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
                        let image = crate::screenshot::geometry::compose(
                            state.desktop.frames.frames(),
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
                {
                    let mut selection = state.selection.borrow_mut();
                    selection.cursor = bounded_cursor(selection.cursor, state.desktop.bounds);
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
unsafe fn paint(hwnd: HWND, state: &State, index: usize) -> Result<(), NativeError> {
    let paint = Paint::begin(hwnd)?;
    let dc = paint.dc;
    let monitor = state.desktop.monitors[index];
    let frame = &state.desktop.frames.frames()[index];
    let selection = state.selection.borrow();
    let (width, height) = monitor.rect.dimensions()?;
    frozen(dc, frame)?;
    capture::shade(
        dc,
        RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        },
        state.palette.mask,
        state.palette.mask_opacity,
    )?;
    let selected = match selection.mode {
        ScreenshotMode::Region => selection.region(),
        ScreenshotMode::Window => selection.selected_window().map(|window| window.rect),
    };
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
    if selection.mode == ScreenshotMode::Region && monitor.rect.contains(selection.cursor) {
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
        Some((width, height)) => format!("{width} × {height} px   Enter 确认 · Esc 取消"),
        None => match selection.mode {
            ScreenshotMode::Region => "拖动选择 · Space 设置起点 · Esc 取消".into(),
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
