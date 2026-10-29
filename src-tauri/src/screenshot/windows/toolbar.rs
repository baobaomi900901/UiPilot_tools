//! Accessible native BUTTONs; only the owner-drawn face uses the Lucide masks.
use super::{editor::Button, *};
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
        Button::Cancel => "终止截图",
        Button::Pin => "贴图",
        Button::Save => "保存",
        Button::Copy => "复制",
    }
}
fn icon(button: Button) -> &'static [u8; 4096] {
    match button {
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
        let mut caption: Vec<u16> = name(button).encode_utf16().chain(Some(0)).collect();
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
            Some(HMENU(id(button) as *mut _)),
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
            Some(WPARAM(color(palette.surface).0 as usize)),
            None,
        );
        SendMessageW(
            self.tooltip,
            TTM_SETTIPTEXTCOLOR,
            Some(WPARAM(color(palette.foreground).0 as usize)),
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
        Ok(Self { buttons })
    }
}
pub(super) unsafe fn draw(
    palette: OverlayPalette,
    outputting: bool,
    item: &DRAWITEMSTRUCT,
) -> Result<(), NativeError> {
    let Some(button) = from_id(item.CtlID as usize) else {
        return Ok(());
    };
    let dpi = GetDpiForWindow(item.hwndItem).max(96);
    let scale = |dip: i32| ((dip * dpi as i32 + 48) / 96).max(1);
    let primary = button == Button::Save && !outputting;
    let background = if primary {
        palette.primary
    } else {
        palette.surface
    };
    let foreground = if primary {
        palette.primary_foreground
    } else if button == Button::Cancel {
        palette.destructive
    } else {
        palette.foreground
    };
    fill(item.hDC, item.rcItem, palette.surface)?;
    let region = GdiObject(HGDIOBJ(
        CreateRoundRectRgn(
            item.rcItem.left,
            item.rcItem.top,
            item.rcItem.right + 1,
            item.rcItem.bottom + 1,
            scale(12),
            scale(12),
        )
        .0,
    ));
    let surface = brush(background)?;
    if FillRgn(item.hDC, HRGN(region.0 .0), HBRUSH(surface.0 .0)).0 == 0 {
        return Err(NativeError::OverlayFailed);
    }
    if item.itemState.0 & (ODS_FOCUS.0 | ODS_SELECTED.0) != 0 {
        let border = brush(foreground)?;
        if FrameRgn(
            item.hDC,
            HRGN(region.0 .0),
            HBRUSH(border.0 .0),
            scale(2),
            scale(2),
        )
        .0 == 0
        {
            return Err(NativeError::OverlayFailed);
        }
    }
    // Preblend the antialiased canonical mask against this button's surface;
    // GDI never needs a runtime SVG parser or an unbounded image allocation.
    let mut pixels = [0u8; 64 * 64 * 4];
    for (alpha, pixel) in icon(button).iter().zip(pixels.chunks_exact_mut(4)) {
        let alpha = u32::from(*alpha) * if outputting { 110 } else { 255 } / 255;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_ids_names_and_embedded_canonical_icons_are_distinct() {
        assert_eq!(Button::ALL.map(name), ["终止截图", "贴图", "保存", "复制"]);
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
        assert_eq!(from_id(104), None);
    }
}
