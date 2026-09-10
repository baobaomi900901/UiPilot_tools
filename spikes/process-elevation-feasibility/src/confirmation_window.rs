//! Native, synthetic-only UI experiment. No IPC, elevation, process catalog or stop API.
use super::confirmation::{Model, Outcome};
use std::{
    cell::{Cell, RefCell},
    panic::{catch_unwind, AssertUnwindSafe},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
            Gdi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_HOTLIGHT, ODS_SELECTED},
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED},
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::*,
        },
    },
};

const LIST: usize = 100;
const DETAIL: usize = 101;
const CANCEL: usize = 2;
const CONFIRM: usize = 1;
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn rgb(hex: u32) -> COLORREF {
    COLORREF((hex & 0xff00) | ((hex & 0xff) << 16) | (hex >> 16))
}
struct Brush(HBRUSH);
impl Drop for Brush {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}
struct App {
    model: RefCell<Model>,
    bg: COLORREF,
    fg: COLORREF,
    danger: COLORREF,
    background: Brush,
    border: Brush,
    danger_border: Brush,
    title: Cell<HWND>,
    list: Cell<HWND>,
    detail: Cell<HWND>,
    cancel: Cell<HWND>,
    confirm: Cell<HWND>,
}
impl App {
    fn new(dark: bool) -> Result<Self, &'static str> {
        let mut contrast = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .map_err(|_| "ThemeInspectionFailed")?;
        // Native semantic colors mirror ui-theme.css; forced colors take precedence.
        let (bg, fg, danger) = if contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0 {
            unsafe {
                (
                    COLORREF(GetSysColor(COLOR_WINDOW)),
                    COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                )
            }
        } else if dark {
            (rgb(0x07080a), rgb(0xf4f4f6), rgb(0xff6161))
        } else {
            (rgb(0xf7f7f8), rgb(0x171719), rgb(0xdc4343))
        };
        let background = Brush(unsafe { CreateSolidBrush(bg) });
        let border = Brush(unsafe { CreateSolidBrush(fg) });
        let danger_border = Brush(unsafe { CreateSolidBrush(danger) });
        if background.0.is_invalid() || border.0.is_invalid() || danger_border.0.is_invalid() {
            return Err("BrushCreationFailed");
        }
        Ok(Self {
            model: RefCell::new(Model::new()),
            bg,
            fg,
            danger,
            background,
            border,
            danger_border,
            title: Cell::default(),
            list: Cell::default(),
            detail: Cell::default(),
            cancel: Cell::default(),
            confirm: Cell::default(),
        })
    }
}

unsafe fn control(
    parent: HWND,
    class: PCWSTR,
    label: &str,
    style: u32,
    id: usize,
) -> Result<HWND, &'static str> {
    let label = text(label);
    let child = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
        0,
        0,
        1,
        1,
        Some(parent),
        Some(HMENU(id as *mut _)),
        None,
        None,
    )
    .map_err(|_| "ControlCreationFailed")?;
    SendMessageW(
        child,
        WM_SETFONT,
        Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
        Some(LPARAM(1)),
    );
    Ok(child)
}

unsafe fn initialize(hwnd: HWND, app: &App) -> Result<(), &'static str> {
    app.title.set(control(hwnd, w!("STATIC"), "仅模拟，不操作进程。范围：进程树；操作：强制终止。\r\n已冻结 256 条模拟目标；另有 2 条模拟遗漏，选择不完整。", 0, 110)?);
    app.list.set(control(
        hwnd,
        w!("LISTBOX"),
        "模拟目标列表",
        WS_TABSTOP.0
            | WS_BORDER.0
            | WS_VSCROLL.0
            | WS_HSCROLL.0
            | LBS_NOTIFY as u32
            | LBS_NOINTEGRALHEIGHT as u32,
        LIST,
    )?);
    app.detail.set(control(
        hwnd,
        w!("EDIT"),
        "请选择目标以查看完整文本；第 255、256 行包含特殊字符与长路径示例。",
        WS_TABSTOP.0
            | WS_BORDER.0
            | WS_VSCROLL.0
            | ES_MULTILINE as u32
            | ES_READONLY as u32
            | ES_AUTOVSCROLL as u32,
        DETAIL,
    )?);
    app.cancel.set(control(
        hwnd,
        w!("BUTTON"),
        "取消",
        WS_TABSTOP.0 | BS_OWNERDRAW as u32,
        CANCEL,
    )?);
    app.confirm.set(control(
        hwnd,
        w!("BUTTON"),
        "仅确认模拟",
        WS_TABSTOP.0 | BS_OWNERDRAW as u32,
        CONFIRM,
    )?);
    let rows = app.model.borrow().rows.clone();
    for row in rows {
        let row = text(&row);
        let result = SendMessageW(
            app.list.get(),
            LB_ADDSTRING,
            None,
            Some(LPARAM(row.as_ptr() as isize)),
        )
        .0;
        if result < 0 {
            return Err("ListPopulationFailed");
        }
    }
    SendMessageW(
        app.list.get(),
        LB_SETHORIZONTALEXTENT,
        Some(WPARAM(4096)),
        None,
    );
    if SendMessageW(app.list.get(), LB_GETCOUNT, None, None).0 != 256 {
        return Err("IncompletePreview");
    }
    if SetTimer(Some(hwnd), 1, 120000, None) == 0 {
        return Err("TimerCreationFailed");
    }
    layout(hwnd, app);
    Ok(())
}

unsafe fn layout(hwnd: HWND, app: &App) {
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
        return;
    }
    let width = (rect.right - 32).max(100);
    let height = rect.bottom.max(350);
    let list_height = (height - 204).max(80);
    for (child, x, y, w, h) in [
        (app.title.get(), 16, 12, width, 40),
        (app.list.get(), 16, 60, width, list_height),
        (app.detail.get(), 16, 68 + list_height, width, 80),
        (app.cancel.get(), rect.right - 240, height - 40, 96, 28),
        (app.confirm.get(), rect.right - 132, height - 40, 116, 28),
    ] {
        let _ = MoveWindow(child, x, y, w, h, true);
    }
}

unsafe fn finish(hwnd: HWND, app: &App, outcome: Outcome) {
    app.model.borrow_mut().finish(outcome);
    let _ = DestroyWindow(hwnd);
}

unsafe fn dispatch(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const App;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    // Shared reference only; no RefCell borrow is held over a reentrant Win32 call.
    let app = &*pointer;
    match message {
        WM_CREATE => {
            if initialize(hwnd, app).is_err() {
                return LRESULT(-1);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            layout(hwnd, app);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lp.0 as *mut MINMAXINFO);
            info.ptMinTrackSize.x = 640;
            info.ptMinTrackSize.y = 440;
            LRESULT(0)
        }
        DM_GETDEFID => LRESULT(((DC_HASDEFID << 16) | CANCEL as u32) as isize),
        DM_SETDEFID => LRESULT(1), // The default remains Cancel; focused button still works.
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            if id == CANCEL {
                finish(hwnd, app, Outcome::Cancelled);
            } else if id == CONFIRM {
                finish(hwnd, app, Outcome::ConfirmedSimulation);
            } else if id == LIST && (wp.0 >> 16) as u32 == LBN_SELCHANGE {
                let index = SendMessageW(app.list.get(), LB_GETCURSEL, None, None).0;
                let detail = app.model.borrow().rows.get(index as usize).cloned();
                if let Some(detail) = detail {
                    let detail = text(&detail);
                    let _ = SetWindowTextW(app.detail.get(), PCWSTR(detail.as_ptr()));
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            finish(hwnd, app, Outcome::Cancelled);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == 1 => {
            finish(hwnd, app, Outcome::TimedOut);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            FillRect(HDC(wp.0 as *mut _), &rect, app.background.0);
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT => {
            let dc = HDC(wp.0 as *mut _);
            SetTextColor(dc, app.fg);
            SetBkColor(dc, app.bg);
            LRESULT(app.background.0 .0 as isize)
        }
        WM_DRAWITEM => {
            let item = &*(lp.0 as *const DRAWITEMSTRUCT);
            if item.CtlID != CANCEL as u32 && item.CtlID != CONFIRM as u32 {
                return LRESULT(0);
            }
            FillRect(item.hDC, &item.rcItem, app.background.0);
            let danger = item.CtlID == CONFIRM as u32;
            let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
            FrameRect(
                item.hDC,
                &item.rcItem,
                if danger {
                    app.danger_border.0
                } else {
                    app.border.0
                },
            );
            if item.itemState.0 & (ODS_SELECTED.0 | ODS_HOTLIGHT.0) != 0 {
                let mut active = item.rcItem;
                active.left += 1;
                active.top += 1;
                active.right -= 1;
                active.bottom -= 1;
                FrameRect(
                    item.hDC,
                    &active,
                    if danger {
                        app.danger_border.0
                    } else {
                        app.border.0
                    },
                );
            }
            SetTextColor(
                item.hDC,
                if disabled {
                    COLORREF(GetSysColor(COLOR_GRAYTEXT))
                } else if danger {
                    app.danger
                } else {
                    app.fg
                },
            );
            SetBkMode(item.hDC, TRANSPARENT);
            let mut label: Vec<u16> = if danger { "仅确认模拟" } else { "取消" }
                .encode_utf16()
                .collect();
            let mut rect = item.rcItem;
            DrawTextW(
                item.hDC,
                &mut label,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            if item.itemState.0 & ODS_FOCUS.0 != 0 {
                rect.left += 4;
                rect.top += 4;
                rect.right -= 4;
                rect.bottom -= 4;
                let _ = DrawFocusRect(item.hDC, &rect);
            }
            LRESULT(1)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), 1);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, message, wp, lp)
        }
        _ => DefWindowProcW(hwnd, message, wp, lp),
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match catch_unwind(AssertUnwindSafe(|| dispatch(hwnd, message, wp, lp))) {
        Ok(result) => result,
        Err(_) => {
            std::process::abort();
        } // Never unwind across Win32 or continue with broken UI state.
    }
}

pub(super) fn show(dark: bool) -> Result<Outcome, &'static str> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED)
            .map_err(|_| "DpiInitializationFailed")?;
        let instance = HINSTANCE(GetModuleHandleW(None).map_err(|_| "ModuleUnavailable")?.0);
        let class = w!("UiPilotConfirmationSpecimen");
        let window_class = WNDCLASSW {
            hInstance: instance,
            lpszClassName: class,
            lpfnWndProc: Some(window_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|_| "CursorUnavailable")?,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err("WindowClassFailed");
        }
        let app = Box::new(App::new(dark)?);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class,
            w!("UiPilot 确认窗口样例 — 不执行真实操作"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            820,
            620,
            None,
            None,
            Some(instance),
            Some((&*app as *const App).cast()),
        )
        .map_err(|_| "WindowCreationFailed")?;
        let dark_flag: i32 = i32::from(dark);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark_flag as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(Some(app.cancel.get()));
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == -1 {
                let _ = DestroyWindow(hwnd);
                return Err("MessageLoopFailed");
            }
            if result == 0 {
                break;
            }
            if !IsDialogMessageW(hwnd, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let result = app.model.borrow().outcome;
        if result == Outcome::Pending {
            return Err("UnexpectedWindowExit");
        }
        Ok(result)
    }
}
