use super::*;
use crate::screenshot::geometry::FrameSet;
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::{
            Dwm::{DwmFlush, DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::*,
        },
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::*,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Monitor {
    pub rect: PhysicalRect,
    pub dpi_x: u32,
    pub dpi_y: u32,
}
pub(super) struct Desktop {
    pub monitors: Vec<Monitor>,
    pub frames: FrameSet,
    pub candidates: Vec<WindowCandidate>,
    pub bounds: PhysicalRect,
}

fn physical(rect: RECT) -> PhysicalRect {
    PhysicalRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}
pub(super) fn layout() -> Result<Vec<Monitor>, NativeError> {
    struct Enumeration {
        monitors: Vec<Monitor>,
        failed: bool,
    }
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let state = &mut *(data.0 as *mut Enumeration);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let (mut dpi_x, mut dpi_y) = (0, 0);
        if !GetMonitorInfoW(monitor, &mut info).as_bool()
            || GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err()
            || physical(info.rcMonitor).dimensions().is_err()
        {
            state.failed = true;
            return BOOL(0);
        }
        if state.monitors.try_reserve(1).is_err() {
            state.failed = true;
            return BOOL(0);
        }
        state.monitors.push(Monitor {
            rect: physical(info.rcMonitor),
            dpi_x,
            dpi_y,
        });
        BOOL(1)
    }
    let mut state = Enumeration {
        monitors: vec![],
        failed: false,
    };
    unsafe {
        if !EnumDisplayMonitors(
            None,
            None,
            Some(visit),
            LPARAM(&mut state as *mut _ as isize),
        )
        .as_bool()
            || state.failed
            || state.monitors.is_empty()
        {
            return Err(NativeError::CaptureFailed);
        }
    }
    state.monitors.sort_by_key(|m| {
        (
            m.rect.left,
            m.rect.top,
            m.rect.right,
            m.rect.bottom,
            m.dpi_x,
            m.dpi_y,
        )
    });
    Ok(state.monitors)
}
pub(super) fn desktop(
    excluded: &[isize],
    budget: &PixelBudget,
    cancellation: &Cancellation,
) -> Result<Desktop, NativeError> {
    // The host disabled exit transitions while hiding its windows and waited
    // for that change to be presented. Flush again at the actual read boundary;
    // a flush alone cannot finish an ongoing window exit animation.
    unsafe {
        DwmFlush().map_err(|_| NativeError::CaptureFailed)?;
    }
    let monitors = layout()?;
    let bounds = PhysicalRect {
        left: monitors.iter().map(|m| m.rect.left).min().unwrap(),
        top: monitors.iter().map(|m| m.rect.top).min().unwrap(),
        right: monitors.iter().map(|m| m.rect.right).max().unwrap(),
        bottom: monitors.iter().map(|m| m.rect.bottom).max().unwrap(),
    };
    bounds.dimensions()?;
    let candidates = window_snapshot(excluded, bounds)?;
    let frames = collect_frames(
        &mut WindowsSource(cancellation),
        &monitors.iter().map(|m| m.rect).collect::<Vec<_>>(),
        budget,
    )?;
    Ok(Desktop {
        monitors,
        frames,
        candidates,
        bounds,
    })
}
pub(super) fn window_snapshot(
    excluded: &[isize],
    bounds: PhysicalRect,
) -> Result<Vec<WindowCandidate>, NativeError> {
    struct Enumeration<'a> {
        excluded: &'a [isize],
        bounds: PhysicalRect,
        candidates: Vec<WindowCandidate>,
        failed: bool,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let state = &mut *(data.0 as *mut Enumeration<'_>);
        if state.excluded.contains(&(hwnd.0 as isize))
            || !IsWindowVisible(hwnd).as_bool()
            || IsIconic(hwnd).as_bool()
        {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4).is_ok()
            && cloaked != 0
        {
            return BOOL(1);
        }
        let mut rect = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_err()
            && GetWindowRect(hwnd, &mut rect).is_err()
        {
            return BOOL(1);
        }
        if let Some(rect) = physical(rect).intersection(state.bounds) {
            if state.candidates.try_reserve(1).is_err() {
                state.failed = true;
                return BOOL(0);
            }
            state.candidates.push(WindowCandidate {
                id: hwnd.0 as isize,
                rect,
                z_order: state.candidates.len(),
            });
        }
        BOOL(1)
    }
    let mut state = Enumeration {
        excluded,
        bounds,
        candidates: vec![],
        failed: false,
    };
    unsafe {
        EnumWindows(Some(visit), LPARAM(&mut state as *mut _ as isize))
            .map_err(|_| NativeError::CaptureFailed)?;
    }
    if state.failed {
        return Err(ScreenshotError::AllocationFailed.into());
    }
    Ok(state.candidates)
}

pub(super) fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}
pub(super) fn shade(dc: HDC, rect: RECT, rgb: u32, opacity: u8) -> Result<(), NativeError> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    let surface = Surface::create(&WindowsGdi, 1, 1)?;
    unsafe {
        std::ptr::copy_nonoverlapping(
            [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8, 255].as_ptr(),
            surface.bits,
            4,
        );
        if !AlphaBlend(
            dc,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            HDC(surface.memory.unwrap() as *mut _),
            0,
            0,
            1,
            1,
            BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: opacity,
                AlphaFormat: 0,
            },
        )
        .as_bool()
        {
            return Err(NativeError::OverlayFailed);
        }
    }
    Ok(())
}
struct WindowsGdi;
impl GdiApi for WindowsGdi {
    fn screen(&self) -> Result<usize, NativeError> {
        unsafe {
            let dc = GetDC(None);
            if dc.0.is_null() {
                Err(NativeError::CaptureFailed)
            } else {
                Ok(dc.0 as usize)
            }
        }
    }
    fn memory(&self, screen: usize) -> Result<usize, NativeError> {
        unsafe {
            let dc = CreateCompatibleDC(Some(HDC(screen as *mut _)));
            if dc.0.is_null() {
                Err(NativeError::CaptureFailed)
            } else {
                Ok(dc.0 as usize)
            }
        }
    }
    fn bitmap(
        &self,
        screen: usize,
        width: i32,
        height: i32,
    ) -> Result<(usize, *mut u8), NativeError> {
        unsafe {
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                Some(HDC(screen as *mut _)),
                &bitmap_info(width, height),
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )
            .map_err(|_| NativeError::CaptureFailed)?;
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                return Err(NativeError::CaptureFailed);
            }
            Ok((bitmap.0 as usize, bits.cast()))
        }
    }
    fn select(&self, memory: usize, object: usize) -> Result<usize, NativeError> {
        unsafe {
            let old = SelectObject(HDC(memory as *mut _), HGDIOBJ(object as *mut _));
            if old.0.is_null() || old.0 as isize == -1 {
                Err(NativeError::CaptureFailed)
            } else {
                Ok(old.0 as usize)
            }
        }
    }
    fn release_screen(&self, value: usize) {
        unsafe {
            ReleaseDC(None, HDC(value as *mut _));
        }
    }
    fn delete_dc(&self, value: usize) {
        unsafe {
            let _ = DeleteDC(HDC(value as *mut _));
        }
    }
    fn delete_bitmap(&self, value: usize) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(value as *mut _));
        }
    }
}
struct WindowsSource<'a>(&'a Cancellation);
impl FrameSource for WindowsSource<'_> {
    fn capture(
        &mut self,
        rect: PhysicalRect,
        budget: &PixelBudget,
    ) -> Result<BudgetedImage, NativeError> {
        if self.0.reason().is_some() {
            return Err(NativeError::CaptureFailed);
        }
        let (width, height) = rect.dimensions()?;
        let mut image = BudgetedImage::allocate(width, height, budget)?;
        let surface = Surface::create(&WindowsGdi, width as i32, height as i32)?;
        unsafe {
            // CAPTUREBLT keeps visible layered application windows. Our overlays
            // do not exist yet; no PrintWindow/variance fallback changes semantics.
            BitBlt(
                HDC(surface.memory.unwrap() as *mut _),
                0,
                0,
                width as i32,
                height as i32,
                Some(HDC(surface.screen as *mut _)),
                rect.left,
                rect.top,
                SRCCOPY | CAPTUREBLT,
            )
            .map_err(|_| NativeError::CaptureFailed)?;
            if !GdiFlush().as_bool() {
                return Err(NativeError::CaptureFailed);
            }
            let bgra = std::slice::from_raw_parts(surface.bits, image.image().pixels.len());
            for (src, dest) in bgra
                .chunks_exact(4)
                .zip(image.pixels_mut().chunks_exact_mut(4))
            {
                dest.copy_from_slice(&[src[2], src[1], src[0], 255]);
            }
        }
        if self.0.reason().is_some() {
            return Err(NativeError::CaptureFailed);
        }
        Ok(image)
    }
}

// Opaque native resources permit the production acquisition/rollback algorithm
// itself to run against injected resources, without touching the user's desktop.
trait GdiApi {
    fn screen(&self) -> Result<usize, NativeError>;
    fn memory(&self, screen: usize) -> Result<usize, NativeError>;
    fn bitmap(
        &self,
        screen: usize,
        width: i32,
        height: i32,
    ) -> Result<(usize, *mut u8), NativeError>;
    fn select(&self, memory: usize, object: usize) -> Result<usize, NativeError>;
    fn release_screen(&self, value: usize);
    fn delete_dc(&self, value: usize);
    fn delete_bitmap(&self, value: usize);
}
struct Surface<'a, A: GdiApi> {
    api: &'a A,
    screen: usize,
    memory: Option<usize>,
    bitmap: Option<usize>,
    previous: Option<usize>,
    bits: *mut u8,
}
impl<'a, A: GdiApi> Surface<'a, A> {
    fn create(api: &'a A, width: i32, height: i32) -> Result<Self, NativeError> {
        let mut owned = Self {
            api,
            screen: api.screen()?,
            memory: None,
            bitmap: None,
            previous: None,
            bits: std::ptr::null_mut(),
        };
        owned.memory = Some(api.memory(owned.screen)?);
        let (bitmap, bits) = api.bitmap(owned.screen, width, height)?;
        owned.bitmap = Some(bitmap);
        owned.bits = bits;
        owned.previous = Some(api.select(owned.memory.unwrap(), bitmap)?);
        Ok(owned)
    }
}
impl<A: GdiApi> Drop for Surface<'_, A> {
    fn drop(&mut self) {
        if let (Some(memory), Some(previous)) = (self.memory, self.previous) {
            if self.api.select(memory, previous).is_err() {
                // A bitmap still selected into a DC cannot be deleted. Destroy
                // that DC first if restoration fails, then release the bitmap.
                self.api.delete_dc(memory);
                self.memory = None;
            }
        }
        if let Some(bitmap) = self.bitmap {
            self.api.delete_bitmap(bitmap);
        }
        if let Some(memory) = self.memory {
            self.api.delete_dc(memory);
        }
        self.api.release_screen(self.screen);
    }
}

trait FrameSource {
    fn capture(
        &mut self,
        rect: PhysicalRect,
        budget: &PixelBudget,
    ) -> Result<BudgetedImage, NativeError>;
}
fn collect_frames(
    source: &mut impl FrameSource,
    rects: &[PhysicalRect],
    budget: &PixelBudget,
) -> Result<FrameSet, NativeError> {
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(rects.len())
        .map_err(|_| ScreenshotError::AllocationFailed)?;
    for rect in rects {
        frames.push((*rect, source.capture(*rect, budget)?));
    }
    Ok(FrameSet::new(frames)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_gdi_restore_deletes_dc_before_bitmap_to_avoid_selected_object_leak() {
        use std::cell::Cell;
        struct Api {
            selected: Cell<bool>,
            live_bitmap: Cell<bool>,
        }
        impl GdiApi for Api {
            fn screen(&self) -> Result<usize, NativeError> {
                Ok(1)
            }
            fn memory(&self, _: usize) -> Result<usize, NativeError> {
                Ok(2)
            }
            fn bitmap(&self, _: usize, _: i32, _: i32) -> Result<(usize, *mut u8), NativeError> {
                self.live_bitmap.set(true);
                Ok((3, std::ptr::null_mut()))
            }
            fn select(&self, _: usize, object: usize) -> Result<usize, NativeError> {
                if object == 99 {
                    return Err(NativeError::CaptureFailed);
                }
                self.selected.set(true);
                Ok(99)
            }
            fn release_screen(&self, _: usize) {}
            fn delete_dc(&self, _: usize) {
                self.selected.set(false);
            }
            fn delete_bitmap(&self, _: usize) {
                if !self.selected.get() {
                    self.live_bitmap.set(false);
                }
            }
        }
        let api = Api {
            selected: Cell::new(false),
            live_bitmap: Cell::new(false),
        };
        drop(Surface::create(&api, 2, 2).unwrap());
        assert!(
            !api.live_bitmap.get(),
            "selected bitmap was leaked after restore failure"
        );
    }
    struct Source {
        calls: usize,
        fail_at: usize,
        color: u8,
    }
    impl FrameSource for Source {
        fn capture(
            &mut self,
            rect: PhysicalRect,
            budget: &PixelBudget,
        ) -> Result<BudgetedImage, NativeError> {
            self.calls += 1;
            if self.calls == self.fail_at {
                return Err(NativeError::CaptureFailed);
            }
            let (w, h) = rect.dimensions()?;
            let mut image = BudgetedImage::allocate(w, h, budget)?;
            image.pixels_mut().fill(self.color);
            Ok(image)
        }
    }
    fn rect() -> PhysicalRect {
        PhysicalRect {
            left: -2,
            top: 0,
            right: 0,
            bottom: 2,
        }
    }
    #[test]
    fn second_capture_failure_releases_first_frame_lease() {
        let budget = PixelBudget::new(64);
        let mut source = Source {
            calls: 0,
            fail_at: 2,
            color: 0,
        };
        assert!(collect_frames(&mut source, &[rect(), rect()], &budget).is_err());
        assert_eq!(source.calls, 2);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn black_and_white_frames_are_valid_and_keep_budget_until_drop() {
        for color in [0, 255] {
            let budget = PixelBudget::new(64);
            let mut source = Source {
                calls: 0,
                fail_at: usize::MAX,
                color,
            };
            let frames = collect_frames(&mut source, &[rect()], &budget).unwrap();
            assert!(frames.frames()[0].image.pixels.iter().all(|v| *v == color));
            assert_eq!(budget.used(), 16);
            drop(frames);
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    fn native_resources_restore_selection_and_pair_every_acquisition_on_failure() {
        use std::cell::{Cell, RefCell};
        struct Api {
            step: Cell<usize>,
            fail: usize,
            events: RefCell<Vec<&'static str>>,
        }
        impl Api {
            fn acquire(&self, name: &'static str) -> Result<usize, NativeError> {
                self.step.set(self.step.get() + 1);
                if self.step.get() == self.fail {
                    return Err(NativeError::CaptureFailed);
                }
                self.events.borrow_mut().push(name);
                Ok(self.step.get())
            }
        }
        impl GdiApi for Api {
            fn screen(&self) -> Result<usize, NativeError> {
                self.acquire("screen")
            }
            fn memory(&self, _: usize) -> Result<usize, NativeError> {
                self.acquire("memory")
            }
            fn bitmap(&self, _: usize, _: i32, _: i32) -> Result<(usize, *mut u8), NativeError> {
                Ok((self.acquire("bitmap")?, std::ptr::null_mut()))
            }
            fn select(&self, _: usize, object: usize) -> Result<usize, NativeError> {
                if object == 99 {
                    self.events.borrow_mut().push("restore");
                    Ok(3)
                } else {
                    self.acquire("select")?;
                    Ok(99)
                }
            }
            fn release_screen(&self, _: usize) {
                self.events.borrow_mut().push("release-screen")
            }
            fn delete_dc(&self, _: usize) {
                self.events.borrow_mut().push("delete-memory")
            }
            fn delete_bitmap(&self, _: usize) {
                self.events.borrow_mut().push("delete-bitmap")
            }
        }
        for fail in 1..=5 {
            let api = Api {
                step: Cell::new(0),
                fail,
                events: RefCell::new(vec![]),
            };
            drop(Surface::create(&api, 2, 2));
            let events = api.events.borrow();
            for (created, destroyed) in [
                ("screen", "release-screen"),
                ("memory", "delete-memory"),
                ("bitmap", "delete-bitmap"),
                ("select", "restore"),
            ] {
                assert_eq!(
                    events.iter().filter(|v| **v == created).count(),
                    events.iter().filter(|v| **v == destroyed).count(),
                    "failure step {fail}: {events:?}"
                );
            }
            if fail == 5 {
                assert_eq!(
                    &events[4..],
                    &[
                        "restore",
                        "delete-bitmap",
                        "delete-memory",
                        "release-screen"
                    ]
                );
            }
        }
    }
}
