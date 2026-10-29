//! Accessible native BUTTONs; only the owner-drawn face uses the Lucide masks.
use super::{
    editor::{Button, Editor, Tool},
    *,
};
use windows::core::PWSTR;
use windows::Win32::UI::Controls::*;

pub(super) fn id(button: Button) -> usize {
    100 + Button::ALL.iter().position(|b| *b == button).unwrap()
}
pub(super) fn from_id(id: usize) -> Option<Button> {
    id.checked_sub(100)
        .and_then(|i| Button::ALL.get(i))
        .copied()
}
pub(super) fn name(button: Button) -> &'static str {
    match button {
        Button::Mosaic => "马赛克",
        Button::Text => "文字",
        Button::Rectangle => "实心矩形",
        Button::Undo => "撤销 (Ctrl+Z)",
        Button::Redo => "重做 (Ctrl+Y)",
        Button::Cancel => "终止截图",
        Button::Pin => "贴图",
        Button::Save => "保存",
        Button::Copy => "复制",
    }
}
fn icon(button: Button) -> &'static [u8; 4096] {
    match button {
        Button::Mosaic => include_bytes!("../../../resources/screenshot-icons/grid-2x2.alpha"),
        Button::Text => include_bytes!("../../../resources/screenshot-icons/type.alpha"),
        Button::Rectangle => include_bytes!("../../../resources/screenshot-icons/square.alpha"),
        Button::Undo => include_bytes!("../../../resources/screenshot-icons/undo-2.alpha"),
        Button::Redo => include_bytes!("../../../resources/screenshot-icons/redo-2.alpha"),
        Button::Cancel => include_bytes!("../../../resources/screenshot-icons/x.alpha"),
        Button::Pin => include_bytes!("../../../resources/screenshot-icons/pin.alpha"),
        Button::Save => include_bytes!("../../../resources/screenshot-icons/save.alpha"),
        Button::Copy => include_bytes!("../../../resources/screenshot-icons/copy.alpha"),
    }
}
pub(super) struct NativeButton {
    pub hwnd: HWND,
    pub(super) tooltip: HWND,
    _caption: Vec<u16>,
}
impl NativeButton {
    unsafe fn new(parent: HWND, button: Button) -> Result<Self, NativeError> {
        Self::named(parent, id(button), name(button))
    }
    unsafe fn named(parent: HWND, control_id: usize, name: &str) -> Result<Self, NativeError> {
        let mut caption: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            PCWSTR(caption.as_ptr()),
            WS_CHILD | WS_TABSTOP | WINDOW_STYLE((BS_OWNERDRAW | BS_NOTIFY) as u32),
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(control_id as *mut _)),
            None,
            None,
        )
        .map_err(|_| NativeError::OverlayFailed)?;
        let mut owned = Self {
            hwnd,
            tooltip: HWND::default(),
            _caption: vec![],
        };
        owned.tooltip = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            TOOLTIPS_CLASSW,
            None,
            WS_POPUP | WINDOW_STYLE((TTS_ALWAYSTIP | TTS_NOPREFIX) as u32),
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            None,
            None,
        )
        .map_err(|_| NativeError::OverlayFailed)?;
        let info = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: parent,
            uId: hwnd.0 as usize,
            lpszText: PWSTR(caption.as_mut_ptr()),
            ..Default::default()
        };
        if SendMessageW(
            owned.tooltip,
            TTM_ADDTOOLW,
            None,
            Some(LPARAM(&info as *const _ as isize)),
        )
        .0 == 0
        {
            return Err(NativeError::OverlayFailed);
        }
        owned._caption = caption;
        // The classic tooltip color messages must remain effective in both
        // host themes instead of inheriting the desktop's unrelated theme.
        let _ = SetWindowTheme(owned.tooltip, w!(""), w!(""));
        Ok(owned)
    }
    pub unsafe fn theme(&self, palette: OverlayPalette) {
        SendMessageW(
            self.tooltip,
            TTM_SETTIPBKCOLOR,
            Some(WPARAM(color(palette.primary_foreground).0 as usize)),
            None,
        );
        SendMessageW(
            self.tooltip,
            TTM_SETTIPTEXTCOLOR,
            Some(WPARAM(color(palette.primary).0 as usize)),
            None,
        );
    }
}
impl Drop for NativeButton {
    fn drop(&mut self) {
        unsafe {
            if !self.tooltip.0.is_null() {
                let _ = DestroyWindow(self.tooltip);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
pub(super) struct Toolbar {
    pub buttons: Vec<NativeButton>,
    pub properties: Vec<NativeButton>,
}
impl Toolbar {
    pub unsafe fn new(parent: HWND) -> Result<Self, NativeError> {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_WIN95_CLASSES,
        })
        .ok()
        .map_err(|_| NativeError::OverlayFailed)?;
        let buttons = Button::ALL
            .into_iter()
            .map(|b| NativeButton::new(parent, b))
            .collect::<Result<_, _>>()?;
        let properties = Property::ALL
            .into_iter()
            .map(|p| NativeButton::named(parent, p.id(), p.name()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            buttons,
            properties,
        })
    }
}
fn selected_background(palette: OverlayPalette) -> u32 {
    let base = (palette.primary_foreground >> 16) & 255;
    let foreground = (palette.primary >> 16) & 255;
    let gray = (base * 224 + foreground * 31 + 127) / 255;
    gray * 0x010101
}
pub(super) unsafe fn draw(
    palette: OverlayPalette,
    outputting: bool,
    keyboard_focus: bool,
    item: &DRAWITEMSTRUCT,
) -> Result<(), NativeError> {
    draw_at_dpi(
        palette,
        outputting,
        keyboard_focus,
        item,
        GetDpiForWindow(item.hwndItem).max(96),
    )
}
pub(super) unsafe fn draw_at_dpi(
    palette: OverlayPalette,
    outputting: bool,
    keyboard_focus: bool,
    item: &DRAWITEMSTRUCT,
    dpi: u32,
) -> Result<(), NativeError> {
    let Some(button) = from_id(item.CtlID as usize) else {
        return Ok(());
    };
    let scale = |dip: i32| ((dip * dpi as i32 + 48) / 96).max(1);
    // All actions share a borderless face. Keyboard navigation alone gets a
    // short underline, never a frame around the icon.
    let background = if item.itemState.0 & ODS_SELECTED.0 != 0 {
        selected_background(palette)
    } else {
        palette.primary_foreground
    };
    let foreground = palette.primary;
    fill(item.hDC, item.rcItem, background)?;
    // Preblend the antialiased canonical mask against this button's surface;
    // GDI never needs a runtime SVG parser or an unbounded image allocation.
    let mut pixels = [0u8; 64 * 64 * 4];
    for (alpha, pixel) in icon(button).iter().zip(pixels.chunks_exact_mut(4)) {
        let alpha = u32::from(*alpha)
            * if outputting || item.itemState.0 & ODS_DISABLED.0 != 0 {
                90
            } else {
                255
            }
            / 255;
        for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
            pixel[channel] = ((((foreground >> shift) & 255) * alpha
                + ((background >> shift) & 255) * (255 - alpha)
                + 127)
                / 255) as u8;
        }
        pixel[3] = 255;
    }
    let size = scale(18)
        .min(item.rcItem.right - item.rcItem.left)
        .min(item.rcItem.bottom - item.rcItem.top);
    let _saved = SavedDc::new(item.hDC)?;
    SetStretchBltMode(item.hDC, HALFTONE);
    if StretchDIBits(
        item.hDC,
        (item.rcItem.left + item.rcItem.right - size) / 2,
        (item.rcItem.top + item.rcItem.bottom - size) / 2,
        size,
        size,
        0,
        0,
        64,
        64,
        Some(pixels.as_ptr().cast()),
        &capture::bitmap_info(64, 64),
        DIB_RGB_COLORS,
        SRCCOPY,
    ) == 0
    {
        return Err(NativeError::OverlayFailed);
    }
    if keyboard_focus && item.itemState.0 & ODS_FOCUS.0 != 0 {
        fill(
            item.hDC,
            RECT {
                left: item.rcItem.left + scale(6),
                right: item.rcItem.right - scale(6),
                top: item.rcItem.bottom - scale(3),
                bottom: item.rcItem.bottom - scale(1),
            },
            foreground,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_ids_names_and_embedded_canonical_icons_are_distinct() {
        assert_eq!(
            Button::ALL.map(name),
            [
                "马赛克",
                "文字",
                "实心矩形",
                "撤销 (Ctrl+Z)",
                "重做 (Ctrl+Y)",
                "终止截图",
                "贴图",
                "保存",
                "复制"
            ]
        );
        for (index, button) in Button::ALL.into_iter().enumerate() {
            assert_eq!(from_id(id(button)), Some(button));
            assert_eq!(id(button), 100 + index);
            let mask = icon(button);
            assert_eq!(mask.len(), 64 * 64);
            assert!(mask.iter().any(|a| *a > 0));
            assert!(mask.iter().any(|a| *a > 0 && *a < 255));
            for other in Button::ALL.into_iter().skip(index + 1) {
                assert_ne!(mask, icon(other));
            }
        }
        assert_eq!(from_id(109), None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Property {
    Brush(u32),
    Font(u32),
    Color(usize),
}
impl Property {
    pub const COLORS: [[u8; 4]; 5] = [
        [239, 68, 68, 255],
        [250, 204, 21, 255],
        [59, 130, 246, 255],
        [0, 0, 0, 255],
        [255, 255, 255, 255],
    ];
    pub const ALL: [Self; 11] = [
        Self::Brush(12),
        Self::Brush(24),
        Self::Brush(40),
        Self::Font(16),
        Self::Font(20),
        Self::Font(28),
        Self::Color(0),
        Self::Color(1),
        Self::Color(2),
        Self::Color(3),
        Self::Color(4),
    ];
    pub fn id(self) -> usize {
        200 + Self::ALL.iter().position(|p| *p == self).unwrap()
    }
    pub fn from_id(id: usize) -> Option<Self> {
        id.checked_sub(200).and_then(|i| Self::ALL.get(i)).copied()
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Brush(12) => "小号笔刷",
            Self::Brush(24) => "中号笔刷",
            Self::Brush(_) => "大号笔刷",
            Self::Font(16) => "16 DIP 字号",
            Self::Font(20) => "20 DIP 字号",
            Self::Font(_) => "28 DIP 字号",
            Self::Color(0) => "红色",
            Self::Color(1) => "黄色",
            Self::Color(2) => "蓝色",
            Self::Color(3) => "黑色",
            Self::Color(_) => "白色",
        }
    }
    pub fn visible(self, tool: Option<Tool>) -> bool {
        match self {
            Self::Brush(_) => tool == Some(Tool::Mosaic),
            Self::Font(_) => tool == Some(Tool::Text),
            Self::Color(_) => matches!(tool, Some(Tool::Text | Tool::Rectangle)),
        }
    }
    pub fn selected(self, e: &Editor) -> bool {
        match self {
            Self::Brush(v) => v == e.brush_size,
            Self::Font(v) => v == e.font_size,
            Self::Color(i) => Self::COLORS[i] == e.color,
        }
    }
    pub fn apply(self, e: &mut Editor) {
        match self {
            Self::Brush(v) => e.brush_size = v,
            Self::Font(v) => e.font_size = v,
            Self::Color(i) => e.color = Self::COLORS[i],
        }
    }
}
pub(super) unsafe fn draw_property(
    palette: OverlayPalette,
    property: Property,
    selected: bool,
    item: &DRAWITEMSTRUCT,
) -> Result<(), NativeError> {
    draw_property_at_dpi(
        palette,
        property,
        selected,
        item,
        GetDpiForWindow(item.hwndItem).max(96),
    )
}
pub(super) unsafe fn draw_property_at_dpi(
    palette: OverlayPalette,
    property: Property,
    selected: bool,
    item: &DRAWITEMSTRUCT,
    dpi: u32,
) -> Result<(), NativeError> {
    let scale = |v: i32| (v * dpi as i32 + 48) / 96;
    let font = CreateFontW(
        -scale(13),
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
        ANTIALIASED_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Microsoft YaHei UI"),
    );
    if font.0.is_null() {
        return Err(NativeError::OverlayFailed);
    }
    let font = GdiObject(HGDIOBJ(font.0));
    let _saved = SavedDc::new(item.hDC)?;
    SelectObject(item.hDC, font.0);
    let background = palette.primary_foreground;
    fill(item.hDC, item.rcItem, background)?;
    let inset = scale(4);
    let face = RECT {
        left: item.rcItem.left + inset,
        top: item.rcItem.top + inset,
        right: item.rcItem.right - inset,
        bottom: item.rcItem.bottom - inset,
    };
    match property {
        Property::Color(i) => {
            let c = Property::COLORS[i];
            fill(
                item.hDC,
                face,
                u32::from(c[2]) | (u32::from(c[1]) << 8) | (u32::from(c[0]) << 16),
            )?;
        }
        _ => {
            let label = match property {
                Property::Brush(12) => "小",
                Property::Brush(24) => "中",
                Property::Brush(_) => "大",
                Property::Font(16) => "16",
                Property::Font(20) => "20",
                _ => "28",
            };
            let mut text: Vec<u16> = label.encode_utf16().collect();
            let mut face = face;
            SetBkMode(item.hDC, TRANSPARENT);
            SetTextColor(item.hDC, color(palette.primary));
            DrawTextW(
                item.hDC,
                &mut text,
                &mut face,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }
    if selected || item.itemState.0 & ODS_FOCUS.0 != 0 {
        fill(
            item.hDC,
            RECT {
                left: face.left,
                right: face.right,
                top: item.rcItem.bottom - scale(3),
                bottom: item.rcItem.bottom - scale(1),
            },
            palette.primary,
        )?;
    }
    Ok(())
}
