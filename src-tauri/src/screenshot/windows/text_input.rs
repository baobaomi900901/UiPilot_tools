//! Native multiline EDIT owns IME and editing shortcuts; committed text owns a shared RGBA raster.
use crate::screenshot::{
    geometry::{rgba_len, BudgetedImage, PixelBudget},
    model::{PhysicalPoint, PhysicalRect},
};
use std::{cell::Cell, sync::Arc};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        UI::{
            Input::KeyboardAndMouse::SetFocus,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const SUBCLASS_ID: usize = 0x55495458;
const HOST_SUBCLASS_ID: usize = 0x55495448;
const MAX_TEXT_UNITS: usize = 32_768;
const EM_SETRECTNP: u32 = 0x00b4;
const EM_SETLIMITTEXT: u32 = 0x00c5;
const EM_SETMARGINS: u32 = 0x00d3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyRoute {
    Native,
    Submit,
    Cancel,
}

/// Only these two keys leave the EDIT, and never during IME composition.
pub(super) fn route_key(composing: bool, message: u32, key: u16, ctrl: bool) -> KeyRoute {
    if composing || message != WM_KEYDOWN || key == 0xe5 {
        return KeyRoute::Native;
    }
    match (key, ctrl) {
        (0x0d, true) => KeyRoute::Submit,
        (0x1b, _) => KeyRoute::Cancel,
        _ => KeyRoute::Native,
    }
}

struct EditState {
    composing: Cell<bool>,
    edit: Cell<HWND>,
    origin: PhysicalPoint,
    color: [u8; 4],
    background: Arc<BudgetedImage>,
    background_rect: PhysicalRect,
}

pub(super) struct TextInput {
    hwnd: HWND,
    host: HWND,
    font: Font,
    state: Box<EditState>,
    clip: PhysicalRect,
    bounds: PhysicalRect,
    dpi: u32,
    font_size_dip: u32,
}

impl TextInput {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create(
        parent: HWND,
        origin: PhysicalPoint,
        clip: PhysicalRect,
        dpi: u32,
        font_size_dip: u32,
        color: [u8; 4],
        initial: &str,
        background: Arc<BudgetedImage>,
        background_rect: PhysicalRect,
    ) -> Result<Self, String> {
        clip.dimensions().map_err(|e| e.to_string())?;
        if !clip.contains(origin) || initial.encode_utf16().count() > MAX_TEXT_UNITS {
            return Err("文字输入范围无效".into());
        }
        if background_rect.dimensions().map_err(|e| e.to_string())?
            != (background.width(), background.height())
        {
            return Err("文字背景范围无效".into());
        }
        let font = Font::new(dpi, font_size_dip)?;
        let width = (clip.right - origin.x).min(scale(360, dpi));
        let height = (clip.bottom - origin.y).min(scale(140, dpi));
        let bounds = PhysicalRect {
            left: origin.x,
            top: origin.y,
            right: origin.x + width,
            bottom: origin.y + height,
        };
        let mut state = Box::new(EditState {
            composing: Cell::new(false),
            edit: Cell::new(HWND::default()),
            origin,
            color,
            background,
            background_rect,
        });
        // An owned popup is independent of the owner monitor's client clip. Its EDIT
        // therefore has one stable wrap width even when physical bounds cross displays.
        let host = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("STATIC"),
                None,
                WS_POPUP | WS_CLIPCHILDREN,
                origin.x,
                origin.y,
                width,
                height,
                Some(parent),
                None,
                None,
                None,
            )
            .map_err(|_| "无法创建跨屏文字窗口".to_string())?
        };
        unsafe {
            if !SetWindowSubclass(
                host,
                Some(host_proc),
                HOST_SUBCLASS_ID,
                (&mut *state as *mut EditState) as usize,
            )
            .as_bool()
            {
                let _ = DestroyWindow(host);
                return Err("无法初始化跨屏文字窗口".into());
            }
        }
        let initial: Vec<u16> = initial
            .replace('\n', "\r\n")
            .replace("\r\r\n", "\r\n")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let hwnd = match unsafe {
            CreateWindowExW(
                WS_EX_TRANSPARENT,
                w!("EDIT"),
                PCWSTR(initial.as_ptr()),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
                0,
                0,
                width,
                height,
                Some(host),
                None,
                None,
                None,
            )
        } {
            Ok(hwnd) => hwnd,
            Err(_) => {
                unsafe {
                    let _ = DestroyWindow(host);
                }
                return Err("无法创建文字输入框".into());
            }
        };
        state.edit.set(hwnd);
        unsafe {
            if !SetWindowSubclass(
                hwnd,
                Some(edit_proc),
                SUBCLASS_ID,
                (&mut *state as *mut EditState) as usize,
            )
            .as_bool()
            {
                let _ = DestroyWindow(hwnd);
                let _ = DestroyWindow(host);
                return Err("无法初始化文字输入框".into());
            }
            let _ = SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 .0 as usize)),
                Some(LPARAM(1)),
            );
            let _ = SendMessageW(
                hwnd,
                EM_SETLIMITTEXT,
                Some(WPARAM(MAX_TEXT_UNITS)),
                Some(LPARAM(0)),
            );
            let _ = SendMessageW(hwnd, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM(0)));
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let _ = SendMessageW(
                hwnd,
                EM_SETRECTNP,
                Some(WPARAM(0)),
                Some(LPARAM((&rect as *const RECT) as isize)),
            );
            // Editing existing text starts at the end, without selecting/replacing its content.
            let _ = SendMessageW(hwnd, 0x00b1, Some(WPARAM(usize::MAX)), Some(LPARAM(-1)));
        }
        let input = Self {
            hwnd,
            host,
            font,
            state,
            clip,
            bounds,
            dpi,
            font_size_dip,
        };
        if unsafe { IsWindowVisible(parent) }.as_bool() {
            unsafe {
                let _ = ShowWindow(host, SW_SHOWNOACTIVATE);
            }
            input.focus();
        }
        Ok(input)
    }
    pub(super) fn host_hwnd(&self) -> HWND {
        self.host
    }
    pub(super) fn owns_foreground(&self, hwnd: HWND) -> bool {
        hwnd == self.host || hwnd == self.hwnd || unsafe { GetAncestor(hwnd, GA_ROOT) } == self.host
    }
    /// Refocus a retained input after failed submission without activating unrelated apps.
    pub(super) fn focus(&self) {
        unsafe {
            let foreground = GetForegroundWindow();
            if !IsWindowVisible(self.host).as_bool()
                || foreground.is_invalid()
                || GetWindowThreadProcessId(foreground, None)
                    != GetWindowThreadProcessId(self.host, None)
            {
                return;
            }
            let _ = SetForegroundWindow(self.host);
            let _ = SetFocus(Some(self.hwnd));
        }
    }
    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub(super) fn owns_message(&self, hwnd: HWND) -> bool {
        hwnd == self.hwnd
    }
    pub(super) fn origin(&self) -> PhysicalPoint {
        self.state.origin
    }
    pub(super) fn clip(&self) -> PhysicalRect {
        self.clip
    }
    pub(super) fn bounds(&self) -> PhysicalRect {
        self.bounds
    }
    pub(super) fn color(&self) -> [u8; 4] {
        self.state.color
    }
    pub(super) fn font_size_dip(&self) -> u32 {
        self.font_size_dip
    }
    pub(super) fn is_composing(&self) -> bool {
        self.state.composing.get()
    }
    pub(super) fn contains_screen_point(&self, point: PhysicalPoint) -> bool {
        self.bounds.contains(point)
    }
    pub(super) fn route_key(&self, message: u32, key: u16, ctrl: bool) -> KeyRoute {
        route_key(self.is_composing(), message, key, ctrl)
    }
    pub(super) fn content(&self) -> Result<String, String> {
        let len = unsafe { GetWindowTextLengthW(self.hwnd) };
        if len < 0 || len as usize > MAX_TEXT_UNITS {
            return Err("文字内容过长".into());
        }
        let mut text = Vec::new();
        text.try_reserve_exact(len as usize + 1)
            .map_err(|_| "无法读取文字".to_string())?;
        text.resize(len as usize + 1, 0);
        let read = unsafe { GetWindowTextW(self.hwnd, &mut text) };
        let text =
            String::from_utf16(&text[..read as usize]).map_err(|_| "文字编码无效".to_string())?;
        Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
    }
    pub(super) fn rasterize(&self, budget: &PixelBudget) -> Result<Arc<BudgetedImage>, String> {
        rasterize_text(
            &self.content()?,
            self.dpi,
            self.font_size_dip,
            self.state.color,
            (
                (self.bounds.right - self.bounds.left) as u32,
                (self.clip.bottom - self.state.origin.y) as u32,
            ),
            budget,
        )
    }
    /// Shared native color hook; the popup host handles its child's WM_CTLCOLOREDIT itself.
    pub(super) fn control_colors(&self, hwnd: HWND, dc: HDC) -> Option<HBRUSH> {
        if hwnd != self.hwnd {
            return None;
        }
        Some(unsafe { control_colors(&self.state, dc) })
    }
}
impl Drop for TextInput {
    fn drop(&mut self) {
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(edit_proc), SUBCLASS_ID);
            let _ = DestroyWindow(self.hwnd);
            let _ = RemoveWindowSubclass(self.host, Some(host_proc), HOST_SUBCLASS_ID);
            let _ = DestroyWindow(self.host);
        }
        // The HWND releases its selected font before Font drops.
        let _ = &self.font;
    }
}

unsafe fn control_colors(state: &EditState, dc: HDC) -> HBRUSH {
    let _ = SetTextColor(dc, rgb(state.color));
    let _ = SetBkMode(dc, TRANSPARENT);
    HBRUSH(GetStockObject(HOLLOW_BRUSH).0)
}
unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let state = &*(data as *const EditState);
    match message {
        WM_CTLCOLOREDIT if HWND(lparam.0 as *mut _) == state.edit.get() => {
            return LRESULT(control_colors(state, HDC(wparam.0 as *mut _)).0 as isize);
        }
        WM_ERASEBKGND => {
            paint_background(state, HDC(wparam.0 as *mut _));
            return LRESULT(1);
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            if !dc.is_invalid() {
                paint_background(state, dc);
            }
            let _ = EndPaint(hwnd, &paint);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(host_proc), HOST_SUBCLASS_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let state = &*(data as *const EditState);
    match message {
        WM_IME_STARTCOMPOSITION => state.composing.set(true),
        WM_IME_ENDCOMPOSITION => state.composing.set(false),
        WM_ERASEBKGND => {
            paint_background(state, HDC(wparam.0 as *mut _));
            return LRESULT(1);
        }
        WM_PAINT => {
            // EDIT sometimes redraws only glyph damage with a hollow brush. Repaint its full
            // background first so deletions and IME preedit never leave old glyphs behind.
            let _ = InvalidateRect(Some(hwnd), None, false);
            let dc = GetDC(Some(hwnd));
            if !dc.is_invalid() {
                paint_background(state, dc);
                let _ = ReleaseDC(Some(hwnd), dc);
            }
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID);
        }
        _ => {}
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    if matches!(
        message,
        WM_CHAR
            | WM_KEYDOWN
            | WM_IME_COMPOSITION
            | WM_CUT
            | WM_PASTE
            | WM_CLEAR
            | WM_UNDO
            | WM_MOUSEWHEEL
    ) {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    result
}

#[repr(C)]
struct RgbaBitmapInfo {
    header: BITMAPINFOHEADER,
    masks: [u32; 3],
}
unsafe fn paint_background(state: &EditState, dc: HDC) {
    let image = state.background.image();
    let info = RgbaBitmapInfo {
        header: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: image.width as i32,
            biHeight: -(image.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_BITFIELDS.0,
            ..Default::default()
        },
        // The captured buffer is RGBA, rather than Win32's default BGRA.
        masks: [0x000000ff, 0x0000ff00, 0x00ff0000],
    };
    let _ = StretchDIBits(
        dc,
        state.background_rect.left - state.origin.x,
        state.background_rect.top - state.origin.y,
        image.width as i32,
        image.height as i32,
        0,
        0,
        image.width as i32,
        image.height as i32,
        Some(image.pixels.as_ptr().cast()),
        (&info as *const RgbaBitmapInfo).cast(),
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}
fn rgb(color: [u8; 4]) -> COLORREF {
    COLORREF(u32::from(color[0]) | (u32::from(color[1]) << 8) | (u32::from(color[2]) << 16))
}
fn scale(dip: u32, dpi: u32) -> i32 {
    ((u64::from(dip) * u64::from(dpi.clamp(48, 768)) + 48) / 96).min(i32::MAX as u64) as i32
}
struct Font(HFONT);
impl Font {
    fn new(dpi: u32, size: u32) -> Result<Self, String> {
        if !(8..=144).contains(&size) {
            return Err("文字字号无效".into());
        }
        let make = |face: PCWSTR| unsafe {
            CreateFontW(
                -scale(size, dpi),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                DEFAULT_PITCH.0 as u32 | FF_SWISS.0 as u32,
                face,
            )
        };
        let inter = Self(make(w!("Inter")));
        if inter.0.is_invalid() {
            return Err("无法创建文字字体".into());
        }
        let dc = MemoryDc::new()?;
        let selected = Selected::new(dc.0, inter.0.into());
        let mut actual = [0u16; 64];
        let count = unsafe { GetTextFaceW(dc.0, Some(&mut actual)) }.max(0) as usize;
        let actual = String::from_utf16_lossy(&actual[..count])
            .trim_end_matches('\0')
            .to_owned();
        drop(selected);
        if actual.eq_ignore_ascii_case("Inter") {
            // GDI font linking supplies CJK glyphs using the installed system fallback.
            return Ok(inter);
        }
        let fallback = make(w!("Microsoft YaHei UI"));
        if fallback.is_invalid() {
            Err("无法创建中文字体".into())
        } else {
            Ok(Self(fallback))
        }
    }
}
impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}
struct MemoryDc(HDC);
impl MemoryDc {
    fn new() -> Result<Self, String> {
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            Err("无法创建文字绘图上下文".into())
        } else {
            Ok(Self(dc))
        }
    }
}
impl Drop for MemoryDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}
struct Selected {
    dc: HDC,
    old: HGDIOBJ,
}
impl Selected {
    fn new(dc: HDC, object: HGDIOBJ) -> Self {
        Self {
            dc,
            old: unsafe { SelectObject(dc, object) },
        }
    }
}
impl Drop for Selected {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.old);
        }
    }
}
struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

pub(super) fn rasterize_text(
    text: &str,
    dpi: u32,
    font_size_dip: u32,
    color: [u8; 4],
    maximum: (u32, u32),
    budget: &PixelBudget,
) -> Result<Arc<BudgetedImage>, String> {
    if text.trim().is_empty()
        || text.len() > 65_536
        || maximum.0 == 0
        || maximum.1 == 0
        || maximum.0 > i32::MAX as u32
        || maximum.1 > i32::MAX as u32
    {
        return Err("文字内容或范围无效".into());
    }
    let font = Font::new(dpi, font_size_dip)?;
    let dc = MemoryDc::new()?;
    let _font_selection = Selected::new(dc.0, font.0.into());
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    let flags = DT_NOPREFIX | DT_EXPANDTABS | DT_WORDBREAK | DT_EDITCONTROL;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: maximum.0 as i32,
        bottom: 0,
    };
    let measured = unsafe { DrawTextW(dc.0, &mut utf16, &mut rect, flags | DT_CALCRECT) };
    if measured <= 0 {
        return Err("无法排版文字".into());
    }
    let width = maximum.0;
    let height = (rect.bottom.max(1) as u32).min(maximum.1);
    // Both the final RGBA image and temporary DIB mask hold leases before either allocates.
    let mut image = BudgetedImage::allocate(width, height, budget).map_err(|e| e.to_string())?;
    let mask_len = rgba_len(width, height).map_err(|e| e.to_string())?;
    let _mask_lease = budget.reserve(mask_len).map_err(|e| e.to_string())?;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = std::ptr::null_mut();
    let bitmap = Bitmap(
        unsafe { CreateDIBSection(Some(dc.0), &info, DIB_RGB_COLORS, &mut pixels, None, 0) }
            .map_err(|_| "无法创建文字栅格".to_string())?,
    );
    let _bitmap_selection = Selected::new(dc.0, bitmap.0.into());
    let mask = unsafe { std::slice::from_raw_parts_mut(pixels.cast::<u8>(), mask_len) };
    mask.fill(0);
    unsafe {
        let _ = SetTextColor(dc.0, COLORREF(0x00ffffff));
        let _ = SetBkColor(dc.0, COLORREF(0));
        let _ = SetBkMode(dc.0, TRANSPARENT);
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            // Describe the complete layout and let the DIB clip it. DT_EDITCONTROL
            // otherwise suppresses a partially visible final line at the creation clip.
            bottom: rect.bottom,
        };
        let _ = DrawTextW(dc.0, &mut utf16, &mut bounds, flags);
        let _ = GdiFlush();
    }
    for (pixel, coverage) in image
        .pixels_mut()
        .chunks_exact_mut(4)
        .zip(mask.chunks_exact(4))
    {
        let alpha = coverage[0].max(coverage[1]).max(coverage[2]);
        pixel.copy_from_slice(&[color[0], color[1], color[2], alpha]);
    }
    Ok(Arc::new(image))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_edit_shortcuts_and_ime_keys_remain_native() {
        for key in [0x1b, 0x0d, 0x43, 0x53, 0x5a, 0x59, 0xe5] {
            assert_eq!(route_key(true, WM_KEYDOWN, key, true), KeyRoute::Native);
        }
        assert_eq!(route_key(false, WM_KEYDOWN, 0x0d, true), KeyRoute::Submit);
        assert_eq!(route_key(false, WM_KEYDOWN, 0x1b, false), KeyRoute::Cancel);
        assert_eq!(route_key(false, WM_KEYDOWN, 0x0d, false), KeyRoute::Native);
        for key in [0x43, 0x53, 0x5a, 0x59] {
            assert_eq!(route_key(false, WM_KEYDOWN, key, true), KeyRoute::Native);
        }
        assert_eq!(route_key(false, WM_KEYUP, 0x1b, false), KeyRoute::Native);
    }
    #[test]
    fn chinese_multiline_raster_is_straight_rgba_and_budget_owned() {
        let budget = PixelBudget::new(8 * 1024 * 1024);
        let text = "中文测试\n第二行";
        let image = rasterize_text(text, 96, 20, [210, 25, 40, 255], (300, 150), &budget).unwrap();
        assert!(image.height() >= 40);
        assert!(image.image().pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert!(image.image().pixels.chunks_exact(4).any(|p| p[3] > 0));
        for pixel in image.image().pixels.chunks_exact(4).filter(|p| p[3] != 0) {
            assert_eq!(&pixel[..3], &[210, 25, 40]);
        }
        assert_eq!(budget.used(), image.image().pixels.len());
        let shared = Arc::clone(&image);
        drop(image);
        assert!(budget.used() > 0);
        drop(shared);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn raster_dpi_scales_and_failure_releases_scratch_budget() {
        let budget = PixelBudget::new(8 * 1024 * 1024);
        let low =
            rasterize_text("中文\nline", 96, 20, [255, 0, 0, 255], (600, 300), &budget).unwrap();
        let high =
            rasterize_text("中文\nline", 192, 20, [255, 0, 0, 255], (600, 300), &budget).unwrap();
        // Font hinting rounds each line independently; verify physical growth within one pixel per line.
        assert!((i64::from(high.height()) - 2 * i64::from(low.height())).abs() <= 4);
        let tiny = PixelBudget::new(1);
        assert!(rasterize_text("预算", 96, 20, [255, 0, 0, 255], (300, 150), &tiny).is_err());
        assert_eq!(tiny.used(), 0);
        assert!(rasterize_text("", 96, 20, [255, 0, 0, 255], (300, 150), &budget).is_err());
    }
    #[test]
    fn hidden_native_edit_preserves_multiline_and_tracks_ime_without_global_shortcuts() {
        struct Hidden(HWND);
        impl Drop for Hidden {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let parent = Hidden(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                None,
                WS_POPUP,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        let budget = PixelBudget::new(8 * 1024 * 1024);
        let background = Arc::new(BudgetedImage::allocate(800, 600, &budget).unwrap());
        let clip = PhysicalRect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let input = TextInput::create(
            parent.0,
            PhysicalPoint { x: 20, y: 20 },
            clip,
            96,
            20,
            [255, 0, 0, 255],
            "中文\n第二行",
            background,
            clip,
        )
        .unwrap();
        assert_eq!(input.content().unwrap(), "中文\n第二行");
        assert_eq!(
            input.bounds(),
            PhysicalRect {
                left: 20,
                top: 20,
                right: 380,
                bottom: 160
            }
        );
        assert!(input.owns_message(input.hwnd()));
        assert!(input.contains_screen_point(PhysicalPoint { x: 21, y: 21 }));
        unsafe {
            let _ = SendMessageW(input.hwnd(), WM_IME_STARTCOMPOSITION, None, None);
        }
        assert!(input.is_composing());
        assert_eq!(input.route_key(WM_KEYDOWN, 0x1b, false), KeyRoute::Native);
        assert_eq!(input.route_key(WM_KEYDOWN, 0x0d, true), KeyRoute::Native);
        unsafe {
            let _ = SendMessageW(input.hwnd(), WM_IME_ENDCOMPOSITION, None, None);
        }
        assert!(!input.is_composing());
        assert_eq!(input.route_key(WM_KEYDOWN, 0x0d, true), KeyRoute::Submit);
        let raster = input.rasterize(&budget).unwrap();
        assert!(raster.image().pixels.chunks_exact(4).any(|p| p[3] > 0));
        // A hidden parent never becomes visible or receives synthetic desktop input.
        assert!(!unsafe { IsWindowVisible(parent.0) }.as_bool());
    }
    #[test]
    fn transparent_background_uses_rgba_masks_and_physical_origin() {
        let budget = PixelBudget::new(1024);
        let mut background = BudgetedImage::allocate(2, 2, &budget).unwrap();
        background.pixels_mut().copy_from_slice(&[
            255, 0, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255, 12, 34, 56, 255,
        ]);
        let state = EditState {
            composing: Cell::new(false),
            edit: Cell::new(HWND::default()),
            origin: PhysicalPoint { x: -99, y: -19 },
            color: [255, 0, 0, 255],
            background: Arc::new(background),
            background_rect: PhysicalRect {
                left: -100,
                top: -20,
                right: -98,
                bottom: -18,
            },
        };
        let dc = MemoryDc::new().unwrap();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 2,
                biHeight: -2,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = std::ptr::null_mut();
        let bitmap = Bitmap(unsafe {
            CreateDIBSection(Some(dc.0), &info, DIB_RGB_COLORS, &mut pixels, None, 0).unwrap()
        });
        let _selected = Selected::new(dc.0, bitmap.0.into());
        let rendered = unsafe { std::slice::from_raw_parts_mut(pixels.cast::<u8>(), 16) };
        rendered.fill(0);
        unsafe {
            paint_background(&state, dc.0);
            let _ = GdiFlush();
        }
        // Child origin (-99,-19) sees source bottom-right RGB(12,34,56).
        assert_eq!(&rendered[..3], &[56, 34, 12]);
    }
    #[test]
    fn creation_clip_keeps_partial_glyph_rows_instead_of_suppressing_last_line() {
        let budget = PixelBudget::new(8 * 1024 * 1024);
        let full = rasterize_text("中文", 96, 20, [255, 0, 0, 255], (300, 150), &budget).unwrap();
        let clipped = rasterize_text("中文", 96, 20, [255, 0, 0, 255], (300, 12), &budget).unwrap();
        assert_eq!(clipped.height(), 12);
        assert_eq!(clipped.image().pixels, full.image().pixels[..300 * 12 * 4]);
        assert!(clipped.image().pixels.chunks_exact(4).any(|p| p[3] > 0));
    }
    #[test]
    fn hidden_popup_text_crosses_owner_seam_and_reopens_with_identical_raster() {
        struct Hidden(HWND);
        impl Drop for Hidden {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let make = |x| {
            Hidden(unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    None,
                    WS_POPUP,
                    x,
                    -100,
                    400,
                    300,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            })
        };
        let first = make(-400);
        let second = make(0);
        let foreground = unsafe { GetForegroundWindow() };
        let budget = PixelBudget::new(8 * 1024 * 1024);
        let clip = PhysicalRect {
            left: -400,
            top: -100,
            right: 400,
            bottom: 200,
        };
        let background = Arc::new(BudgetedImage::allocate(800, 300, &budget).unwrap());
        let origin = PhysicalPoint { x: -100, y: -80 };
        let create = |owner| {
            TextInput::create(
                owner,
                origin,
                clip,
                96,
                20,
                [255, 0, 0, 255],
                "跨屏中文文字，保持原始换行宽度。长句测试跨屏编辑不会裁切。",
                Arc::clone(&background),
                clip,
            )
            .unwrap()
        };
        let original = create(first.0);
        let root = unsafe { GetAncestor(original.hwnd(), GA_ROOT) };
        assert_eq!(root, original.host_hwnd());
        assert_ne!(root, first.0);
        assert_eq!(unsafe { GetWindow(root, GW_OWNER).unwrap() }, first.0);
        assert!(original.owns_foreground(root));
        assert!(original.owns_foreground(original.hwnd()));
        assert!(!original.owns_foreground(second.0));
        let mut host_rect = RECT::default();
        unsafe {
            GetWindowRect(root, &mut host_rect).unwrap();
        }
        assert_eq!((host_rect.left, host_rect.right), (-100, 260));
        assert!(host_rect.left < 0 && host_rect.right > 0);
        let mut child_rect = RECT::default();
        unsafe {
            GetWindowRect(original.hwnd(), &mut child_rect).unwrap();
        }
        assert_eq!(host_rect, child_rect);
        assert!(!unsafe { IsWindowVisible(root) }.as_bool());
        original.focus();
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        let dc = MemoryDc::new().unwrap();
        let brush = unsafe {
            SendMessageW(
                root,
                WM_CTLCOLOREDIT,
                Some(WPARAM(dc.0 .0 as usize)),
                Some(LPARAM(original.hwnd().0 as isize)),
            )
        };
        assert_ne!(brush.0, 0);
        assert_eq!(unsafe { GetTextColor(dc.0) }, rgb([255, 0, 0, 255]));
        assert_eq!(unsafe { GetBkMode(dc.0) }, TRANSPARENT.0 as i32);
        let raster = original.rasterize(&budget).unwrap();
        let reopened = create(second.0);
        assert_eq!(reopened.bounds(), original.bounds());
        assert_eq!(reopened.rasterize(&budget).unwrap().image(), raster.image());
        assert_eq!(
            unsafe { GetAncestor(reopened.hwnd(), GA_ROOT) },
            reopened.host_hwnd()
        );
        let child = original.hwnd();
        drop(original);
        assert!(!unsafe { IsWindow(Some(root)) }.as_bool());
        assert!(!unsafe { IsWindow(Some(child)) }.as_bool());
        assert!(unsafe { IsWindow(Some(reopened.host_hwnd())) }.as_bool());
    }
}
