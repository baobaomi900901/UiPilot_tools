//! Host-owned pin workers. Hidden preparation owns all pixels before the gate
//! authorizes publication. Each worker pumps independently of screenshot sessions.
use super::*;
use std::sync::{mpsc, Mutex};
use windows::Win32::Foundation::SIZE;

#[derive(Default)]
pub(crate) struct PinManager {
    state: Mutex<ManagerState>,
}
#[derive(Default)]
struct ManagerState {
    stopped: bool,
    pins: Vec<PinHandle>,
}
struct PinHandle {
    stop: Arc<WakeEvent>,
    thread: std::thread::JoinHandle<()>,
}
impl PinManager {
    pub(crate) fn publish(
        &self,
        image: BudgetedImage,
        rect: PhysicalRect,
        budget: &PixelBudget,
        gate: Arc<OutputGate>,
    ) -> Result<(), String> {
        self.publish_prepared(image, rect, budget, gate, |_, _| {})
    }
    fn publish_prepared(
        &self,
        image: BudgetedImage,
        rect: PhysicalRect,
        budget: &PixelBudget,
        gate: Arc<OutputGate>,
        before_commit: impl FnOnce(isize, isize),
    ) -> Result<(), String> {
        gate.begin()?;
        let _finish = desktop_output::FinishJob(&gate);
        if gate.is_cancelled() {
            return Err("Cancelled".into());
        }
        let budget = budget.clone();
        let stop = Arc::new(WakeEvent(
            unsafe { CreateEventW(None, true, false, None) }.map_err(|_| "PinFailed".to_owned())?,
        ));
        let worker_stop = stop.clone();
        let (ready, prepared) = mpsc::sync_channel(1);
        let (commit, decision) = mpsc::sync_channel(1);
        let (ack, published) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("screenshot-pin".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    let _dpi = DpiContext::enter().map_err(|_| "PinFailed".to_owned())?;
                    let mut message = MSG::default();
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                    let pin = PreparedPin::new(rect, image, &budget)?;
                    ready
                        .send(Ok((pin.image.0 as isize, pin.shadow.0 as isize)))
                        .map_err(|_| "Cancelled".to_owned())?;
                    if decision.recv() != Ok(true) {
                        return Err("Cancelled".into());
                    }
                    pin.show()?;
                    ack.send(Ok(())).map_err(|_| "Cancelled".to_owned())?;
                    loop {
                        let wait = MsgWaitForMultipleObjectsEx(
                            Some(std::slice::from_ref(&worker_stop.0)),
                            INFINITE,
                            QS_ALLINPUT,
                            MWMO_INPUTAVAILABLE,
                        );
                        if wait == WAIT_OBJECT_0 || wait == WAIT_FAILED {
                            break;
                        }
                        if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                            if message.message == WM_QUIT {
                                break;
                            }
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    Ok::<_, String>(())
                }))
                .unwrap_or_else(|_| Err("PinFailed".into()));
                if let Err(error) = result {
                    let _ = ready.send(Err(error.clone()));
                    let _ = ack.send(Err(error));
                }
            })
            .map_err(|_| "PinFailed".to_owned())?;
        let (image, shadow) = match prepared.recv().unwrap_or_else(|_| Err("PinFailed".into())) {
            Ok(id) => id,
            Err(error) => {
                let _ = thread.join();
                return Err(error);
            }
        };
        before_commit(image, shadow);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        // Reaping completed workers also bounds manager records after repeated
        // individual closes. Joining finished workers never waits for native UI.
        let mut index = 0;
        while index < state.pins.len() {
            if state.pins[index].thread.is_finished() {
                let _ = state.pins.swap_remove(index).thread.join();
            } else {
                index += 1;
            }
        }
        // Shutdown and publication serialize here. Cancellation uses the same
        // output gate as save/copy and can still win during all preparation.
        let authorization = if state.stopped {
            Err("Cancelled".into())
        } else if state.pins.try_reserve(1).is_err() {
            Err("ResourceLimit".into())
        } else {
            gate.authorize_commit()
        };
        if let Err(error) = authorization {
            let _ = commit.send(false);
            drop(state);
            let _ = thread.join();
            return Err(error);
        }
        if commit.send(true).is_err() {
            drop(state);
            let _ = thread.join();
            return Err("PinFailed".into());
        }
        let result = published.recv().unwrap_or_else(|_| Err("PinFailed".into()));
        if result.is_ok() {
            state.pins.push(PinHandle { stop, thread });
        } else {
            drop(state);
            let _ = thread.join();
        }
        result
    }
    /// Called by host cleanup, including when publication is still preparing.
    /// The stopped flag prevents a staged worker from publishing afterwards.
    pub(crate) fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.stopped = true;
        for pin in &state.pins {
            unsafe {
                let _ = SetEvent(pin.stop.0);
            }
        }
    }
    pub(crate) fn shutdown(&self) {
        self.stop();
        let pins = std::mem::take(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()).pins);
        for pin in pins {
            let _ = pin.thread.join();
        }
    }
}
impl Drop for PinManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

const SHADOW_PAD: i32 = 32;
fn shadow_alpha(x: i32, y: i32, width: i32, height: i32) -> u8 {
    if (0..width).contains(&x) && (0..height).contains(&y) {
        return 0;
    }
    let dx = (x - x.clamp(0, width - 1)) as f64;
    let dy = (y - y.clamp(8, height + 7)) as f64;
    (68.0 * (-(dx * dx + dy * dy) / 98.0).exp()).round() as u8
}
struct PinClass {
    name: Vec<u16>,
    instance: HINSTANCE,
}
impl PinClass {
    unsafe fn new() -> Result<Self, String> {
        let instance = HINSTANCE(
            GetModuleHandleW(None)
                .map_err(|_| "PinFailed".to_owned())?
                .0,
        );
        let name: Vec<u16> = format!("UiPilotScreenshotPin-{}", GetCurrentThreadId())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        if RegisterClassW(&WNDCLASSW {
            hInstance: instance,
            lpszClassName: PCWSTR(name.as_ptr()),
            lpfnWndProc: Some(pin_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|_| "PinFailed".to_owned())?,
            ..Default::default()
        }) == 0
        {
            return Err("PinFailed".into());
        }
        Ok(Self { name, instance })
    }
}
impl Drop for PinClass {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterClassW(PCWSTR(self.name.as_ptr()), Some(self.instance));
        }
    }
}
struct PinContext {
    content: BackBuffer,
    _pixels: BudgetedImage,
    shadow: Cell<HWND>,
}
struct PreparedPin {
    image: HWND,
    shadow: HWND,
    context: Box<PinContext>,
    _shadow_buffer: BackBuffer,
    _class: PinClass,
}
impl PreparedPin {
    unsafe fn new(
        rect: PhysicalRect,
        image: BudgetedImage,
        budget: &PixelBudget,
    ) -> Result<Self, String> {
        let (width, height) = rect.dimensions().map_err(|e| e.to_string())?;
        if (width, height) != (image.width(), image.height()) {
            return Err("PinFailed".into());
        }
        let shadow_width = width
            .checked_add((SHADOW_PAD * 2) as u32)
            .ok_or("ResourceLimit")?;
        let shadow_height = height
            .checked_add((SHADOW_PAD * 2) as u32)
            .ok_or("ResourceLimit")?;
        let mut content =
            BackBuffer::new(HDC::default(), width, height, budget).map_err(pin_error)?;
        let mut shadow_buffer =
            BackBuffer::new(HDC::default(), shadow_width, shadow_height, budget)
                .map_err(pin_error)?;
        // Both native DIBs have separate leases in addition to frozen pixels.
        let content_bits = bitmap_bytes(&mut content, width, height)?;
        for (dst, src) in content_bits
            .chunks_exact_mut(4)
            .zip(image.image().pixels.chunks_exact(4))
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
        }
        let shadow_bits = bitmap_bytes(&mut shadow_buffer, shadow_width, shadow_height)?;
        for (index, pixel) in shadow_bits.chunks_exact_mut(4).enumerate() {
            let x = (index % shadow_width as usize) as i32 - SHADOW_PAD;
            let y = (index / shadow_width as usize) as i32 - SHADOW_PAD;
            pixel.copy_from_slice(&[0, 0, 0, shadow_alpha(x, y, width as i32, height as i32)]);
        }
        let class = PinClass::new()?;
        let context = Box::new(PinContext {
            content,
            _pixels: image,
            shadow: Cell::new(HWND::default()),
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            PCWSTR(class.name.as_ptr()),
            w!("UiPilot 贴图"),
            WS_POPUP,
            rect.left,
            rect.top,
            width as i32,
            height as i32,
            None,
            None,
            Some(class.instance),
            Some(&*context as *const _ as *const _),
        )
        .map_err(|_| "PinFailed".to_owned())?;
        let mut pin = Self {
            image: hwnd,
            shadow: HWND::default(),
            context,
            _shadow_buffer: shadow_buffer,
            _class: class,
        };
        // A layered WS_EX_TRANSPARENT window passes hit testing through even
        // to unrelated threads/processes; HTTRANSPARENT alone does not.
        pin.shadow = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            PCWSTR(pin._class.name.as_ptr()),
            w!("UiPilot 贴图阴影"),
            WS_POPUP,
            rect.left.saturating_sub(SHADOW_PAD),
            rect.top.saturating_sub(SHADOW_PAD),
            shadow_width as i32,
            shadow_height as i32,
            Some(pin.image),
            None,
            Some(pin._class.instance),
            None,
        )
        .map_err(|_| "PinFailed".to_owned())?;
        pin.context.shadow.set(pin.shadow);
        UpdateLayeredWindow(
            pin.shadow,
            None,
            Some(&POINT {
                x: rect.left.saturating_sub(SHADOW_PAD),
                y: rect.top.saturating_sub(SHADOW_PAD),
            }),
            Some(&SIZE {
                cx: shadow_width as i32,
                cy: shadow_height as i32,
            }),
            Some(pin._shadow_buffer.dc),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            }),
            ULW_ALPHA,
        )
        .map_err(|_| "PinFailed".to_owned())?;
        Ok(pin)
    }
    unsafe fn show(&self) -> Result<(), String> {
        // Publication never activates the pin or takes overlay capture. Its
        // image becomes focusable through an ordinary later user click.
        SetWindowPos(
            self.shadow,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|_| "PinFailed".to_owned())?;
        SetWindowPos(
            self.image,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|_| "PinFailed".to_owned())?;
        if !IsWindowVisible(self.image).as_bool() {
            return Err("PinFailed".into());
        }
        let _ = UpdateWindow(self.image);
        Ok(())
    }
}
impl Drop for PreparedPin {
    fn drop(&mut self) {
        unsafe {
            if !self.shadow.0.is_null() {
                let _ = DestroyWindow(self.shadow);
            }
            let _ = DestroyWindow(self.image);
        }
    }
}
fn pin_error(error: NativeError) -> String {
    match error {
        NativeError::Pixels(error) => error.to_string(),
        _ => "PinFailed".into(),
    }
}
unsafe fn bitmap_bytes<'a>(
    buffer: &'a mut BackBuffer,
    width: u32,
    height: u32,
) -> Result<&'a mut [u8], String> {
    let mut bitmap = BITMAP::default();
    if GetObjectW(
        HGDIOBJ(buffer.bitmap.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some((&mut bitmap as *mut BITMAP).cast()),
    ) == 0
        || bitmap.bmBits.is_null()
    {
        return Err("PinFailed".into());
    }
    Ok(std::slice::from_raw_parts_mut(
        bitmap.bmBits.cast(),
        crate::screenshot::geometry::rgba_len(width, height).map_err(|e| e.to_string())?,
    ))
}
unsafe extern "system" fn pin_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PinContext;
    if context.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let context = &*context;
    match message {
        WM_PAINT => {
            if let Ok(paint) = Paint::begin(hwnd) {
                let _ = BitBlt(
                    paint.dc,
                    0,
                    0,
                    context._pixels.width() as i32,
                    context._pixels.height() as i32,
                    Some(context.content.dc),
                    0,
                    0,
                    SRCCOPY,
                );
            }
            LRESULT(0)
        }
        WM_ERASEBKGND | WM_DPICHANGED => LRESULT(1),
        WM_NCHITTEST => LRESULT(HTCAPTION as isize),
        WM_WINDOWPOSCHANGED => {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() && !context.shadow.get().0.is_null() {
                let _ = SetWindowPos(
                    context.shadow.get(),
                    Some(hwnd),
                    rect.left.saturating_sub(SHADOW_PAD),
                    rect.top.saturating_sub(SHADOW_PAD),
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_CONTEXTMENU => {
            if let Ok(menu) = CreatePopupMenu() {
                if AppendMenuW(menu, MF_STRING, 1, w!("关闭贴图")).is_ok() {
                    let point = if lparam.0 == -1 {
                        let mut r = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut r);
                        POINT {
                            x: r.left,
                            y: r.top,
                        }
                    } else {
                        POINT {
                            x: (lparam.0 as u16 as i16) as i32,
                            y: ((lparam.0 >> 16) as u16 as i16) as i32,
                        }
                    };
                    if TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_RIGHTBUTTON,
                        point.x,
                        point.y,
                        None,
                        hwnd,
                        None,
                    )
                    .0 == 1
                    {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                let _ = DestroyMenu(menu);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    fn rect() -> PhysicalRect {
        PhysicalRect {
            left: -30000,
            top: -30000,
            right: -29920,
            bottom: -29940,
        }
    }
    #[test]
    fn native_pin_preserves_exact_pixels_position_and_physical_size_on_dpi_change() {
        let _dpi = DpiContext::enter().unwrap();
        let budget = PixelBudget::new(1024 * 1024);
        let mut image = BudgetedImage::allocate(80, 60, &budget).unwrap();
        for pixel in image.pixels_mut().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[12, 34, 56, 255]);
        }
        let pin = unsafe { PreparedPin::new(rect(), image, &budget).unwrap() };
        unsafe {
            let mut actual = RECT::default();
            GetWindowRect(pin.image, &mut actual).unwrap();
            assert_eq!(
                (actual.left, actual.top, actual.right, actual.bottom),
                (-30000, -30000, -29920, -29940)
            );
            assert_eq!(GetPixel(pin.context.content.dc, 0, 0), color(0x0c2238));
            let suggested = RECT {
                left: 100,
                top: 100,
                right: 260,
                bottom: 220,
            };
            SendMessageW(
                pin.image,
                WM_DPICHANGED,
                Some(WPARAM(192 | (192 << 16))),
                Some(LPARAM(&suggested as *const _ as isize)),
            );
            GetWindowRect(pin.image, &mut actual).unwrap();
            assert_eq!(
                (actual.right - actual.left, actual.bottom - actual.top),
                (80, 60)
            );
            assert_ne!(
                GetWindowLongPtrW(pin.shadow, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0,
                0
            );
            assert_ne!(
                GetWindowLongPtrW(pin.shadow, GWL_EXSTYLE) as u32 & WS_EX_LAYERED.0,
                0
            );
        }
        assert!(budget.used() > 80 * 60 * 4);
        drop(pin);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn shadow_is_external_with_top_light_bottom_deeper_and_farther() {
        assert_eq!(shadow_alpha(10, 10, 80, 60), 0);
        assert!(shadow_alpha(40, -3, 80, 60) < shadow_alpha(40, 63, 80, 60));
        assert!(shadow_alpha(40, -12, 80, 60) < shadow_alpha(40, 72, 80, 60));
        assert_eq!(shadow_alpha(-5, 30, 80, 60), shadow_alpha(84, 30, 80, 60));
    }
    #[test]
    fn cancellation_or_shutdown_after_hidden_preparation_never_shows_the_pin() {
        for shutdown in [false, true] {
            let manager = PinManager::default();
            let budget = PixelBudget::new(1024 * 1024);
            let gate = Arc::new(OutputGate::new());
            let handles = Cell::new((0isize, 0isize));
            let result = manager.publish_prepared(
                BudgetedImage::allocate(80, 60, &budget).unwrap(),
                rect(),
                &budget,
                gate.clone(),
                |image, shadow| unsafe {
                    handles.set((image, shadow));
                    assert!(!IsWindowVisible(HWND(image as *mut _)).as_bool());
                    assert!(!IsWindowVisible(HWND(shadow as *mut _)).as_bool());
                    if shutdown {
                        manager.stop();
                    } else {
                        assert!(gate.cancel());
                    }
                },
            );
            assert_eq!(result, Err("Cancelled".into()));
            unsafe {
                assert!(!IsWindow(Some(HWND(handles.get().0 as *mut _))).as_bool());
                assert!(!IsWindow(Some(HWND(handles.get().1 as *mut _))).as_bool());
            }
            assert!(manager.state.lock().unwrap().pins.is_empty());
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    fn native_shadow_and_exact_content_render_light_and_dark_evidence() {
        let _dpi = DpiContext::enter().unwrap();
        for (name, background) in [("light", 0xf7f7f8), ("dark", 0x0d0d0d)] {
            let budget = PixelBudget::new(4 * 1024 * 1024);
            let mut image = BudgetedImage::allocate(200, 120, &budget).unwrap();
            for (index, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
                let (x, y) = (index % 200, index / 200);
                let grid = x % 20 == 0 || y % 20 == 0;
                pixel.copy_from_slice(if grid {
                    &[116, 142, 170, 255]
                } else {
                    &[210, 225, 242, 255]
                });
            }
            let mut pin = unsafe {
                PreparedPin::new(
                    PhysicalRect {
                        left: 80,
                        top: 60,
                        right: 280,
                        bottom: 180,
                    },
                    image,
                    &budget,
                )
                .unwrap()
            };
            unsafe {
                let mut canvas = BackBuffer::new(HDC::default(), 360, 260, &budget).unwrap();
                fill(
                    canvas.dc,
                    RECT {
                        left: 0,
                        top: 0,
                        right: 360,
                        bottom: 260,
                    },
                    background,
                )
                .unwrap();
                assert!(AlphaBlend(
                    canvas.dc,
                    80 - SHADOW_PAD,
                    60 - SHADOW_PAD,
                    200 + SHADOW_PAD * 2,
                    120 + SHADOW_PAD * 2,
                    pin._shadow_buffer.dc,
                    0,
                    0,
                    200 + SHADOW_PAD * 2,
                    120 + SHADOW_PAD * 2,
                    BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8
                    }
                )
                .as_bool());
                BitBlt(
                    canvas.dc,
                    80,
                    60,
                    200,
                    120,
                    Some(pin.context.content.dc),
                    0,
                    0,
                    SRCCOPY,
                )
                .unwrap();
                assert_eq!(GetPixel(canvas.dc, 80, 60), color(0x748eaa));
                assert_eq!(GetPixel(canvas.dc, 279, 179), color(0xd2e1f2));
                assert_ne!(GetPixel(canvas.dc, 180, 183), color(background));
                let _ = GdiFlush();
                let mut rgba = bitmap_bytes(&mut canvas, 360, 260).unwrap().to_vec();
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                    pixel[3] = 255;
                }
                let png = crate::screenshot::output::encode_png(360, 260, &rgba).unwrap();
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "../.superpowers/sdd/toolbar-pin/native-pin-{name}.png"
                ));
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, png).unwrap();
                // The native window's source is exactly the crop, without its
                // external shadow or any added content inset.
                let bytes = bitmap_bytes(&mut pin.context.content, 200, 120).unwrap();
                assert_eq!(&bytes[..4], &[170, 142, 116, 255]);
            }
            drop(pin);
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    #[ignore = "controlled child fixture used by the shadow pass-through test"]
    fn shadow_underlay_process() {
        if std::env::var_os("UIPILOT_PIN_UNDERLAY_FIXTURE").is_none() {
            return;
        }
        unsafe {
            let _dpi = DpiContext::enter().unwrap();
            let class = PinClass::new().unwrap();
            let window = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(class.name.as_ptr()),
                w!("UiPilot controlled pin underlay"),
                WS_POPUP,
                40,
                40,
                400,
                300,
                None,
                None,
                Some(class.instance),
                None,
            )
            .unwrap();
            SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
            .unwrap();
            println!("UIPILOT_PIN_UNDERLAY={}", window.0 as isize);
            use std::io::Write;
            std::io::stdout().flush().unwrap();
            let mut message = MSG::default();
            while IsWindow(Some(window)).as_bool() && GetMessageW(&mut message, None, 0, 0).0 > 0 {
                DispatchMessageW(&message);
            }
        }
    }
    #[test]
    fn layered_shadow_passes_hit_testing_to_an_unrelated_process() {
        use std::io::{BufRead, BufReader};
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "screenshot::windows::overlay::pin::tests::shadow_underlay_process",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("UIPILOT_PIN_UNDERLAY_FIXTURE", "1")
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut underlay = None;
        let mut child_output = BufReader::new(child.0.stdout.take().unwrap()).lines();
        for line in child_output.by_ref() {
            let line = line.unwrap();
            if let Some((_, value)) = line.split_once("UIPILOT_PIN_UNDERLAY=") {
                underlay = Some(HWND(value.trim().parse::<isize>().unwrap() as *mut _));
                break;
            }
        }
        let underlay = underlay.expect("controlled child must report its native window");
        let _dpi = DpiContext::enter().unwrap();
        let budget = PixelBudget::new(1024 * 1024);
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(underlay, Some(&mut pid));
            assert_ne!(pid, std::process::id());
            let pin = PreparedPin::new(
                PhysicalRect {
                    left: 100,
                    top: 100,
                    right: 180,
                    bottom: 160,
                },
                BudgetedImage::allocate(80, 60, &budget).unwrap(),
                &budget,
            )
            .unwrap();
            pin.show().unwrap();
            windows::Win32::Graphics::Dwm::DwmFlush().unwrap();
            assert_eq!(WindowFromPoint(POINT { x: 110, y: 110 }), pin.image);
            for point in [
                POINT { x: 95, y: 125 },
                POINT { x: 130, y: 165 },
                POINT { x: 185, y: 125 },
            ] {
                assert_eq!(
                    WindowFromPoint(point),
                    underlay,
                    "shadow must not intercept an unrelated process at {point:?}"
                );
            }
            SetWindowPos(
                pin.image,
                None,
                220,
                140,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
            let mut image_rect = RECT::default();
            let mut shadow_rect = RECT::default();
            GetWindowRect(pin.image, &mut image_rect).unwrap();
            GetWindowRect(pin.shadow, &mut shadow_rect).unwrap();
            assert_eq!(
                (
                    image_rect.right - image_rect.left,
                    image_rect.bottom - image_rect.top
                ),
                (80, 60)
            );
            assert_eq!(
                (shadow_rect.left, shadow_rect.top),
                (220 - SHADOW_PAD, 140 - SHADOW_PAD)
            );
            SendMessageW(
                pin.image,
                WM_KEYDOWN,
                Some(WPARAM(VK_ESCAPE.0 as usize)),
                Some(LPARAM(0)),
            );
            assert!(
                !IsWindow(Some(pin.image)).as_bool(),
                "Escape closes only this pin"
            );
            assert!(IsWindow(Some(underlay)).as_bool());
            PostMessageW(Some(underlay), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
        }
        assert!(child.0.wait().unwrap().success());
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn cancelled_or_over_budget_pin_never_publishes_and_releases_every_lease() {
        let manager = PinManager::default();
        let budget = PixelBudget::new(80 * 60 * 4);
        let gate = Arc::new(OutputGate::new());
        let image = BudgetedImage::allocate(80, 60, &budget).unwrap();
        assert_eq!(
            manager.publish(image, rect(), &budget, gate),
            Err("ResourceLimit".into())
        );
        assert_eq!(budget.used(), 0);
        let budget = PixelBudget::new(1024 * 1024);
        let gate = Arc::new(OutputGate::new());
        assert!(gate.cancel());
        assert_eq!(
            manager.publish(
                BudgetedImage::allocate(80, 60, &budget).unwrap(),
                rect(),
                &budget,
                gate
            ),
            Err("Cancelled".into())
        );
        assert!(manager.state.lock().unwrap().pins.is_empty());
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn published_pins_are_independent_and_host_shutdown_releases_them() {
        let manager = PinManager::default();
        let budget = PixelBudget::new(1024 * 1024);
        for _ in 0..2 {
            let gate = Arc::new(OutputGate::new());
            manager
                .publish(
                    BudgetedImage::allocate(80, 60, &budget).unwrap(),
                    rect(),
                    &budget,
                    gate.clone(),
                )
                .unwrap();
            assert!(!gate.cancel(), "publication wins and reports real success");
        }
        let used = budget.used();
        let first = manager.state.lock().unwrap().pins.remove(0);
        unsafe {
            SetEvent(first.stop.0).unwrap();
        }
        first.thread.join().unwrap();
        assert_eq!(budget.used(), used / 2);
        assert_eq!(manager.state.lock().unwrap().pins.len(), 1);
        manager.shutdown();
        assert_eq!(budget.used(), 0);
        let image = BudgetedImage::allocate(80, 60, &budget).unwrap();
        assert_eq!(
            manager.publish(image, rect(), &budget, Arc::new(OutputGate::new())),
            Err("Cancelled".into())
        );
        assert_eq!(budget.used(), 0);
    }
}
