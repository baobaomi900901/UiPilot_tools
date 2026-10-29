//! Host-owned screenshot window and asynchronous native selection orchestration.
use super::{
    geometry::BudgetReservation,
    model::ScreenshotMode,
    service::{
        Begin, CaptureFinish, CaptureTicket, CapturedImage, Origin, OutputFinish, Owner,
        ScreenshotService, Snapshot,
    },
    windows::{self, CancelReason, OverlayPalette, SelectionHandle, SelectionResult},
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[derive(Serialize, Clone)]
pub(crate) struct WireSnapshot {
    #[serde(flatten)]
    snapshot: Snapshot,
    theme: String,
}
struct Preview {
    instance: u64,
    hwnd: isize,
    ready: bool,
    focus_foreground: Option<isize>,
    transfers: Vec<Transfer>,
    last_transfer_id: u64,
}
struct Transfer {
    owner: Owner,
    id: u64,
    _reservation: Arc<BudgetReservation>,
}
pub(crate) struct ImageTransfer {
    bytes: Vec<u8>,
    // Retain both the source and transfer lease through response construction,
    // even if a close or early acknowledgement removes the stored transfer.
    _image: Arc<CapturedImage>,
    _reservation: Arc<BudgetReservation>,
}
impl ImageTransfer {
    pub fn into_response(self) -> tauri::ipc::Response {
        tauri::ipc::Response::new(self.bytes)
    }
}
struct LauncherOrigin {
    session_id: u64,
    invocation_id: String,
    query_sequence: u64,
}
impl Preview {
    fn accepts(&self, label: &str, hwnd: isize, instance: u64) -> bool {
        label == "screenshot" && hwnd != 0 && self.hwnd == hwnd && self.instance == instance
    }
}
#[derive(Default)]
pub(crate) struct ScreenshotHost {
    pub service: ScreenshotService,
    next_instance: AtomicU64,
    preview: Mutex<Option<Preview>>,
    selection: Mutex<Vec<SelectionHandle>>,
    pins: Arc<windows::PinManager>,
    launcher_origin: Mutex<Option<LauncherOrigin>>,
}
impl ScreenshotHost {
    pub fn authorize(&self, window: &WebviewWindow, instance: u64) -> Result<(), String> {
        // WebviewWindow is injected by Tauri; never trust a caller-supplied label.
        if window.label() != "screenshot" {
            return Err("unauthorized".into());
        }
        let hwnd = window.hwnd().map_err(|_| "unauthorized")?.0 as isize;
        let state = self.preview.lock().map_err(|_| "unavailable")?;
        if state
            .as_ref()
            .is_some_and(|p| p.accepts(window.label(), hwnd, instance))
        {
            Ok(())
        } else {
            Err("expired".into())
        }
    }
    pub fn ready(&self, app: &AppHandle, instance: u64) -> Result<WireSnapshot, String> {
        let snapshot = self.snapshot(app, instance)?;
        {
            let mut state = self.preview.lock().map_err(|_| "unavailable")?;
            let preview = state
                .as_mut()
                .filter(|p| p.instance == instance)
                .ok_or("expired")?;
            preview.ready = true;
        }
        // Marking ready precedes the snapshot read so completion cannot be lost
        // between initial fetch and subscribing. Client subscribes before calling.
        let current = self.snapshot(app, instance).unwrap_or(snapshot);
        self.show(app, instance);
        Ok(current)
    }
    fn snapshot(&self, app: &AppHandle, instance: u64) -> Result<WireSnapshot, String> {
        let snapshot = self.service.snapshot(instance).map_err(|_| "expired")?;
        Ok(WireSnapshot {
            snapshot,
            theme: theme(app),
        })
    }
    pub fn emit(&self, app: &AppHandle) {
        let instance = self
            .preview
            .lock()
            .ok()
            .and_then(|p| p.as_ref().filter(|p| p.ready).map(|p| p.instance));
        if let Some(instance) = instance {
            if let (Ok(snapshot), Some(window)) = (
                self.snapshot(app, instance),
                app.get_webview_window("screenshot"),
            ) {
                let _ = window.emit("screenshot://state-changed", snapshot);
            }
        }
    }
    pub fn emit_theme(&self, app: &AppHandle) {
        if self.service.theme_changed().is_ok() {
            self.emit(app);
        }
    }
    fn show(&self, app: &AppHandle, instance: u64) {
        if !self.service.snapshot(instance).is_ok_and(|s| {
            matches!(
                s.phase,
                super::service::Phase::Preview
                    | super::service::Phase::Copying
                    | super::service::Phase::Saving
            )
        }) {
            return;
        }
        let focus = self.preview.lock().ok().and_then(|p| {
            p.as_ref()
                .filter(|p| p.instance == instance && p.ready)
                .map(|p| p.focus_foreground)
        });
        if let Some(expected) = focus {
            if let Some(window) = app.get_webview_window("screenshot") {
                if let Ok(hwnd) = window.hwnd() {
                    unsafe {
                        let _ = ::windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                            ::windows::Win32::Foundation::HWND(hwnd.0),
                            ::windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
                        );
                    }
                }
                let current =
                    unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0
                        as isize;
                if expected == Some(current) {
                    let _ = window.set_focus();
                }
            }
        }
    }
    pub fn start(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: ScreenshotMode,
        origin: Origin,
    ) -> Result<(), String> {
        self.start_with_origin(app, mode, origin, None)
    }
    pub fn start_from_launcher(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: ScreenshotMode,
        invocation_id: String,
        query_sequence: u64,
    ) -> Result<(), String> {
        self.start_with_origin(
            app,
            mode,
            Origin::Launcher,
            Some((invocation_id, query_sequence)),
        )
    }
    fn start_with_origin(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: ScreenshotMode,
        origin: Origin,
        source: Option<(String, u64)>,
    ) -> Result<(), String> {
        let mut launcher_origin = self.launcher_origin.lock().map_err(|_| "unavailable")?;
        // Admission and the pure in-memory session transition share the settings
        // commit lock. Capture/window work below runs after releasing that lock.
        let admission = app
            .state::<crate::settings::SettingsStore>()
            .with_enabled_builtin(crate::settings::ManagedBuiltinFeature::Screenshot, || {
                self.service.begin(mode, origin)
            })
            .map_err(|_| "disabled".to_string())?;
        match admission.map_err(|e| format!("{e:?}"))? {
            Begin::Busy => Err("busy".into()),
            Begin::FocusPreview => {
                let instance = self.preview.lock().ok().and_then(|mut p| {
                    p.as_mut().map(|p| {
                        p.focus_foreground = Some(
                            unsafe {
                                ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()
                            }
                            .0 as isize,
                        );
                        p.instance
                    })
                });
                if let Some(instance) = instance {
                    self.show(app, instance);
                }
                Ok(())
            }
            Begin::Capture(ticket) => {
                *launcher_origin = source.map(|(invocation_id, query_sequence)| LauncherOrigin {
                    session_id: ticket.session_id,
                    invocation_id,
                    query_sequence,
                });
                drop(launcher_origin);
                self.capture(app, ticket);
                Ok(())
            }
        }
    }
    fn error_payload(&self, ticket: CaptureTicket, code: &str) -> Option<serde_json::Value> {
        let origin = self.launcher_origin.lock().ok()?;
        let origin = origin
            .as_ref()
            .filter(|origin| origin.session_id == ticket.session_id)?;
        Some(
            serde_json::json!({"code":code,"invocationId":origin.invocation_id,"querySequence":origin.query_sequence}),
        )
    }
    fn report_error(&self, app: &AppHandle, ticket: CaptureTicket, code: &str) {
        if let Some(payload) = self.error_payload(ticket, code) {
            let _ = app.emit_to("main", "screenshot://error", payload);
        }
    }
    pub fn capture(self: &Arc<Self>, app: &AppHandle, ticket: CaptureTicket) {
        // Freeze the committed preference before dispatching work to Tauri main.
        // Output validates this exact path; no later preference read or fallback.
        let save_directory = app
            .state::<crate::settings::SettingsStore>()
            .screenshot_save_directory_with(|| app.path().desktop_dir().ok());
        let host = self.clone();
        let app = app.clone();
        let dispatch = app.clone();
        // All Tauri mutations execute on main; the native callback never waits
        // for main while a Tauri close/shutdown might wait for its native thread.
        if dispatch
            .run_on_main_thread(move || host.prepare(&app, ticket, save_directory))
            .is_err()
        {
            let _ = self
                .service
                .finish_capture(ticket, Err("captureFailed".into()));
        }
    }
    fn prepare(
        self: &Arc<Self>,
        app: &AppHandle,
        ticket: CaptureTicket,
        save_directory: Option<std::path::PathBuf>,
    ) {
        if self.service.current_ticket() != Some(ticket) {
            return;
        }
        let coordinator = app.state::<Arc<crate::lifecycle::LifecycleCoordinator>>();
        let Some(display) = coordinator.suppress_screenshot_display() else {
            self.complete(app, ticket, true, Err("busy".into()));
            return;
        };
        let Ok(focus) = coordinator.suppress_transient_focus_loss() else {
            self.complete(app, ticket, true, Err("unavailable".into()));
            return;
        };

        let mut excluded = Vec::new();
        for label in ["main", "screenshot"] {
            if let Some(window) = app.get_webview_window(label) {
                let Ok(hwnd) = window.hwnd() else {
                    self.complete(app, ticket, true, Err("captureFailed".into()));
                    return;
                };
                excluded.push(hwnd.0 as isize);
                if windows::hide_for_capture(hwnd.0 as isize, || {
                    window
                        .hide()
                        .map_err(|_| windows::NativeError::CaptureFailed)
                })
                .is_err()
                {
                    self.complete(app, ticket, true, Err("captureFailed".into()));
                    return;
                }
            }
        }
        if !self.service.selecting(ticket) {
            return;
        }
        self.emit(app);
        let mode = match self.service.mode(ticket) {
            Ok(mode) => mode,
            Err(_) => return,
        };
        let app = app.clone();
        let desktop_foreground =
            unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
        let callback_app = app.clone();
        let host = self.clone();
        let result = windows::start_selection(
            mode,
            self.service.budget.clone(),
            palette(&theme(&app)),
            excluded,
            super::probe::executable(&app),
            save_directory,
            self.pins.clone(),
            move |result| {
                let app = callback_app;
                let _display = display;
                let _focus = focus;
                let handoff_foreground =
                    unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0
                        as isize;
                let focus_allowed =
                    handoff_allows_focus(&result, desktop_foreground, handoff_foreground);
                let finish_hidden = selection_finishes_hidden(&result);
                let result = match result {
                    Ok(SelectionResult::Completed { .. }) => Ok(None),
                    Ok(SelectionResult::Selected { image, .. }) => (|| {
                        let _workspace = host
                            .service
                            .budget
                            .reserve(
                                super::output::encoding_workspace_bytes(
                                    image.width(),
                                    image.height(),
                                )
                                .map_err(|e| e.to_string())?,
                            )
                            .map_err(|e| e.to_string())?;
                        let png = super::output::encode_png(
                            image.width(),
                            image.height(),
                            &image.image().pixels,
                        )
                        .map_err(|e| e.to_string())?;
                        CapturedImage::from_encoded(
                            image.width(),
                            image.height(),
                            png,
                            &host.service.budget,
                        )
                        .map(Arc::new)
                        .map(Some)
                        .map_err(|e| e.to_string())
                    })(),
                    Ok(SelectionResult::Cancelled(CancelReason::DisplayChanged)) => {
                        Err("layoutChanged".into())
                    }
                    Ok(SelectionResult::Cancelled(_)) => Ok(None),
                    Err(windows::NativeError::Pixels(
                        super::model::ScreenshotError::ResourceLimit
                        | super::model::ScreenshotError::AllocationFailed,
                    )) => Err("resourceLimit".into()),
                    Err(_) => Err("captureFailed".into()),
                };
                // Release show suppression only after all native HWNDs are destroyed.
                let dispatch = app.clone();
                let _ = dispatch.run_on_main_thread(move || {
                    let foreground =
                        unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
                            .0 as isize;
                    drop(_display);
                    if finish_hidden {
                        host.complete_hidden(&app, ticket);
                    } else {
                        host.complete(
                            &app,
                            ticket,
                            focus_allowed && foreground == handoff_foreground,
                            result,
                        );
                    }
                    drop(_focus);
                });
            },
        );
        match result {
            Ok(handle) => {
                if let Ok(mut handles) = self.selection.lock() {
                    handles.retain(|h| !h.is_finished());
                    handles.push(handle);
                }
            }
            Err(error) => {
                self.complete(&app, ticket, true, Err(format!("captureFailed:{error:?}")))
            }
        }
    }
    fn complete_hidden(&self, app: &AppHandle, ticket: CaptureTicket) {
        let previous_preview = self
            .preview
            .lock()
            .ok()
            .and_then(|preview| preview.as_ref().map(|preview| preview.instance));
        if !self.service.finish_selection_hidden(ticket) {
            return;
        }
        if let Some(instance) = previous_preview {
            self.destroy(app, instance);
        }
        if let Ok(mut origin) = self.launcher_origin.lock() {
            if origin
                .as_ref()
                .is_some_and(|origin| origin.session_id == ticket.session_id)
            {
                origin.take();
            }
        }
        // Preparation already hid the host windows. Success or explicit cancel
        // leaves them hidden in the tray; never reopen an old retake preview.
    }
    fn complete(
        self: &Arc<Self>,
        app: &AppHandle,
        ticket: CaptureTicket,
        focus_allowed: bool,
        result: Result<Option<Arc<CapturedImage>>, String>,
    ) {
        let error = result.as_ref().err().cloned();
        match self.service.finish_capture(ticket, result) {
            CaptureFinish::Preview => {
                let existing = self
                    .preview
                    .lock()
                    .ok()
                    .and_then(|p| p.as_ref().map(|p| p.instance));
                let foreground=focus_allowed.then(||unsafe{::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()}.0 as isize);
                if let Some(instance) = existing {
                    if let Ok(mut state) = self.preview.lock() {
                        if let Some(preview) = state.as_mut() {
                            preview.focus_foreground = foreground;
                        }
                    }
                    self.emit(app);
                    self.show(app, instance);
                } else if self.open(app, ticket, foreground).is_err() {
                    let origin = self.service.abort(ticket);
                    self.report_error(app, ticket, "previewFailed");
                    if origin == Some(Origin::Launcher) && focus_allowed {
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                }
            }
            CaptureFinish::Cancelled(origin) => {
                if let Some(code) = error {
                    self.report_error(app, ticket, &code);
                }
                if origin == Origin::Launcher && focus_allowed {
                    if let Some(main) = app.get_webview_window("main") {
                        let _ = main.show();
                        let _ = main.set_focus();
                    }
                }
            }
            CaptureFinish::Stale => {}
        }
    }
    fn open(
        self: &Arc<Self>,
        app: &AppHandle,
        ticket: CaptureTicket,
        focus_foreground: Option<isize>,
    ) -> Result<(), String> {
        let instance = self
            .next_instance
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "exhausted")?
            + 1;
        let script=format!("Object.defineProperty(window,'__UIPILOT_SCREENSHOT_INSTANCE__',{{value:'{instance}',writable:false,configurable:false}});");
        let window = WebviewWindowBuilder::new(
            app,
            "screenshot",
            WebviewUrl::App("index.html?window=screenshot".into()),
        )
        .title("截图")
        .inner_size(900.0, 640.0)
        .min_inner_size(560.0, 360.0)
        .decorations(false)
        .visible(false)
        .focused(false)
        .initialization_script(script)
        .build()
        .map_err(|_| "previewFailed")?;
        let hwnd = match window.hwnd() {
            Ok(hwnd) => hwnd.0 as isize,
            Err(_) => {
                let _ = window.destroy();
                return Err("previewFailed".into());
            }
        };
        if self.service.bind_window(ticket, instance).is_err() {
            let _ = window.destroy();
            return Err("expired".into());
        }
        match self.preview.lock() {
            Ok(mut p) => {
                *p = Some(Preview {
                    instance,
                    hwnd,
                    ready: false,
                    focus_foreground,
                    transfers: Vec::new(),
                    last_transfer_id: 0,
                })
            }
            Err(_) => {
                let _ = window.destroy();
                return Err("unavailable".into());
            }
        }
        let host = self.clone();
        let app = app.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::ThemeChanged(_) = event {
                // Publish a resolved host theme; never set the native theme here,
                // so this event cannot trigger another ThemeChanged event.
                host.emit_theme(&app);
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Ok(owner) = host.service.owner(instance) {
                    if host.service.close(owner).unwrap_or(false) {
                        host.destroy(&app, instance);
                    }
                }
            }
            if let tauri::WindowEvent::Destroyed = event {
                if let Ok(owner) = host.service.owner(instance) {
                    let _ = host.service.close(owner);
                }
                if let Ok(mut preview) = host.preview.lock() {
                    if preview.as_ref().is_some_and(|p| p.instance == instance) {
                        preview.take();
                    }
                }
            }
        });
        let app = window.app_handle().clone();
        let host = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let dispatch = app.clone();
            let _ = dispatch.run_on_main_thread(move || {
                let expired = host.preview.lock().ok().and_then(|p| {
                    p.as_ref()
                        .filter(|p| p.instance == instance && !p.ready)
                        .map(|p| p.focus_foreground)
                });
                if let Some(foreground) = expired {
                    let origin = host.service.abort(ticket);
                    host.destroy(&app, instance);
                    host.report_error(&app, ticket, "previewFailed");
                    let current =
                        unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
                            .0 as isize;
                    if origin == Some(Origin::Launcher) && foreground == Some(current) {
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                }
            });
        });
        Ok(())
    }
    pub fn read_image(&self, owner: Owner, transfer_id: u64) -> Result<ImageTransfer, String> {
        let image = self.service.image(owner).map_err(|_| "expired")?;
        let reservation = {
            let mut state = self.preview.lock().map_err(|_| "unavailable")?;
            let preview = state
                .as_mut()
                .filter(|p| p.instance == owner.window_instance)
                .ok_or("expired")?;
            if transfer_id <= preview.last_transfer_id {
                return Err("expired".into());
            }
            if preview
                .transfers
                .iter()
                .any(|transfer| transfer.owner == owner)
            {
                return Err("busy".into());
            }
            let reservation = Arc::new(
                self.service
                    .budget
                    .reserve(image.png().len())
                    .map_err(|_| "resourceLimit")?,
            );
            preview.transfers.push(Transfer {
                owner,
                id: transfer_id,
                _reservation: reservation.clone(),
            });
            preview.last_transfer_id = transfer_id;
            reservation
        };
        let prepared = (|| {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(image.png().len())
                .map_err(|_| "resourceLimit")?;
            bytes.extend_from_slice(image.png());
            // Retake/close may revoke ownership while the buffer is prepared.
            self.service.image(owner).map_err(|_| "expired")?;
            Ok(ImageTransfer {
                bytes,
                _image: image,
                _reservation: reservation,
            })
        })();
        if prepared.is_err() {
            let _ = self.release_image(owner, transfer_id);
        }
        prepared
    }
    pub fn release_image(&self, owner: Owner, transfer_id: u64) -> Result<(), String> {
        let mut state = self.preview.lock().map_err(|_| "unavailable")?;
        let preview = state
            .as_mut()
            .filter(|p| p.instance == owner.window_instance)
            .ok_or("expired")?;
        // A completed old-generation read is allowed to release only its own
        // stored token. Unknown/already released tokens are harmless.
        preview
            .transfers
            .retain(|transfer| transfer.owner != owner || transfer.id != transfer_id);
        Ok(())
    }
    pub fn after_output(&self, app: &AppHandle, finish: OutputFinish, instance: u64) {
        match finish {
            OutputFinish::Closed => self.destroy(app, instance),
            OutputFinish::Preview => self.emit(app),
            OutputFinish::Stale => {}
        }
    }
    pub fn destroy(&self, app: &AppHandle, instance: u64) {
        let current = self
            .preview
            .lock()
            .ok()
            .is_some_and(|p| p.as_ref().is_some_and(|p| p.instance == instance));
        if current {
            if let Some(window) = app.get_webview_window("screenshot") {
                let _ = window.destroy();
            } else if let Ok(mut preview) = self.preview.lock() {
                if preview.as_ref().is_some_and(|p| p.instance == instance) {
                    preview.take();
                }
            }
        }
    }
    pub fn cancel_selection(&self, reason: CancelReason) {
        if let Ok(handles) = self.selection.lock() {
            for handle in handles.iter() {
                handle.cancel(reason);
            }
        }
    }
    pub fn shutdown(&self, app: &AppHandle) {
        self.service.shutdown();
        self.cancel_selection(CancelReason::Shutdown);
        self.pins.stop();
        if let Some(instance) = self
            .preview
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.instance))
        {
            self.destroy(app, instance);
        }
        let handles = self
            .selection
            .lock()
            .map(|mut h| std::mem::take(&mut *h))
            .unwrap_or_default();
        let pins = self.pins.clone();
        // Native loop cancellation is cooperative and wakes its message queue.
        // Avoid blocking Tauri main; handles remain rooted until native cleanup.
        let _ = std::thread::Builder::new()
            .name("screenshot-cleanup".into())
            .spawn(move || {
                pins.shutdown();
                for handle in handles {
                    let _ = handle.join();
                }
            });
    }
}
fn selection_finishes_hidden(result: &Result<SelectionResult, windows::NativeError>) -> bool {
    matches!(
        result,
        Ok(SelectionResult::Completed { .. } | SelectionResult::Cancelled(CancelReason::User))
    )
}
fn handoff_allows_focus(
    result: &Result<SelectionResult, windows::NativeError>,
    desktop: isize,
    handoff: isize,
) -> bool {
    desktop != 0
        && desktop == handoff
        && matches!(
            result,
            Ok(SelectionResult::Selected { .. })
                | Ok(SelectionResult::Cancelled(CancelReason::DisplayChanged))
                | Err(_)
        )
}
fn theme(app: &AppHandle) -> String {
    let preference = app
        .state::<crate::settings::SettingsStore>()
        .snapshot()
        .theme;
    match crate::commands::invocation_theme(app, preference) {
        crate::public_plugins::PluginInvocationTheme::Dark => "dark",
        _ => "light",
    }
    .into()
}
fn palette(theme: &str) -> OverlayPalette {
    if theme == "dark" {
        OverlayPalette {
            foreground: 0xf4f4f6,
            surface: 0x0d0d0d,
            selection_border: 0xffffff,
            mask: 0x07080a,
            mask_opacity: 110,
            primary: 0xffffff,
            primary_foreground: 0x000000,
            destructive: 0xff6161,
        }
    } else {
        OverlayPalette {
            foreground: 0x171719,
            surface: 0xffffff,
            selection_border: 0x171719,
            mask: 0xf7f7f8,
            mask_opacity: 110,
            primary: 0x000000,
            primary_foreground: 0xffffff,
            destructive: 0xc62828,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_output_success_never_restores_launcher_focus() {
        use super::super::desktop_output::OutputSuccess;
        for output in [
            OutputSuccess::Saved(std::path::PathBuf::from("D:/Redirected Desktop/截图.png")),
            OutputSuccess::Copied,
            OutputSuccess::Pinned,
        ] {
            let result = Ok(SelectionResult::Completed { output });
            assert!(selection_finishes_hidden(&result));
            assert!(!handoff_allows_focus(&result, 10, 10));
        }
    }
    #[test]
    fn explicit_cancel_does_not_restore_launcher_focus() {
        assert!(selection_finishes_hidden(&Ok(SelectionResult::Cancelled(
            CancelReason::User
        ))));
        assert!(!handoff_allows_focus(
            &Ok(SelectionResult::Cancelled(CancelReason::User)),
            10,
            10
        ));
        for reason in [
            CancelReason::DisplayChanged,
            CancelReason::LostCapture,
            CancelReason::Shutdown,
        ] {
            assert!(
                !selection_finishes_hidden(&Ok(SelectionResult::Cancelled(reason))),
                "non-user interruptions retain their existing recovery path"
            );
        }
    }
    #[test]
    fn launcher_errors_keep_original_query_identity_and_ignore_unrelated_or_hotkey_sessions() {
        let host = ScreenshotHost::default();
        let ticket = CaptureTicket {
            session_id: 1,
            generation: 1,
        };
        assert!(host.error_payload(ticket, "previewFailed").is_none());
        *host.launcher_origin.lock().unwrap() = Some(LauncherOrigin {
            session_id: 1,
            invocation_id: "invocation-A".into(),
            query_sequence: 12,
        });
        assert_eq!(
            host.error_payload(ticket, "previewFailed"),
            Some(
                serde_json::json!({"code":"previewFailed","invocationId":"invocation-A","querySequence":12})
            )
        );
        assert!(host
            .error_payload(
                CaptureTicket {
                    session_id: 2,
                    ..ticket
                },
                "captureFailed"
            )
            .is_none());
        host.launcher_origin.lock().unwrap().take();
        assert!(host.error_payload(ticket, "captureFailed").is_none());
    }
    fn ready_host() -> (ScreenshotHost, Owner) {
        let mut host = ScreenshotHost::default();
        host.service.budget = super::super::geometry::PixelBudget::new(32);
        let Begin::Capture(ticket) = host
            .service
            .begin(ScreenshotMode::Region, Origin::Launcher)
            .unwrap()
        else {
            panic!()
        };
        host.service.finish_capture(
            ticket,
            Ok(Some(Arc::new(
                CapturedImage::from_encoded(1, 1, vec![1, 2, 3], &host.service.budget).unwrap(),
            ))),
        );
        let owner = host.service.bind_window(ticket, 8).unwrap();
        *host.preview.lock().unwrap() = Some(Preview {
            instance: 8,
            hwnd: 80,
            ready: true,
            focus_foreground: None,
            transfers: Vec::new(),
            last_transfer_id: 0,
        });
        (host, owner)
    }
    #[test]
    fn acknowledged_transfers_do_not_accumulate_across_retakes() {
        let (host, mut owner) = ready_host();
        for id in 1..=50 {
            drop(host.read_image(owner, id).unwrap());
            let old = owner;
            let ticket = host.service.retake(owner).unwrap();
            host.service.finish_capture(ticket, Ok(None));
            owner = host.service.owner(8).unwrap();
            host.release_image(old, id).unwrap();
            assert_eq!(
                host.service.budget.used(),
                3,
                "completed read must release its own lease even after retake"
            );
        }
    }
    #[test]
    fn transfer_ack_is_bound_to_owner_token_and_cannot_release_a_new_window_or_read() {
        let (host, owner) = ready_host();
        drop(host.read_image(owner, 1).unwrap());
        assert!(host.read_image(owner, 1).is_err());
        assert!(host.read_image(owner, 2).is_err());
        assert_eq!(host.service.budget.used(), 6);
        host.release_image(
            Owner {
                generation: 99,
                ..owner
            },
            1,
        )
        .unwrap();
        host.release_image(owner, 99).unwrap();
        assert!(host
            .release_image(
                Owner {
                    window_instance: 7,
                    ..owner
                },
                1
            )
            .is_err());
        assert_eq!(host.service.budget.used(), 6);
        host.release_image(owner, 1).unwrap();
        assert_eq!(host.service.budget.used(), 3);
        assert!(host.read_image(owner, 1).is_err());
        drop(host.read_image(owner, 2).unwrap());
        host.release_image(owner, 1).unwrap();
        assert_eq!(host.service.budget.used(), 6);
        host.release_image(owner, 2).unwrap();
        host.release_image(owner, 2).unwrap();
        assert_eq!(host.service.budget.used(), 3);
    }
    #[test]
    fn early_ack_and_window_destruction_keep_inflight_source_and_transfer_budget_pinned() {
        let (host, owner) = ready_host();
        let transfer = host.read_image(owner, 1).unwrap();
        host.release_image(owner, 1).unwrap();
        host.service.shutdown();
        host.preview.lock().unwrap().take();
        assert_eq!(host.service.budget.used(), 6);
        assert_eq!(transfer.bytes, vec![1, 2, 3]);
        drop(transfer);
        assert_eq!(host.service.budget.used(), 0);
    }
    #[test]
    fn overlay_teardown_never_authorizes_an_unrelated_foreground() {
        let result = Ok(SelectionResult::Selected {
            rect: super::super::model::PhysicalRect {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            },
            image: super::super::geometry::BudgetedImage::allocate(
                1,
                1,
                &super::super::geometry::PixelBudget::new(4),
            )
            .unwrap(),
        });
        assert!(handoff_allows_focus(&result, 10, 10));
        assert!(!handoff_allows_focus(&result, 10, 20));
        assert!(!handoff_allows_focus(&result, 0, 0));
        assert!(!handoff_allows_focus(
            &Ok(SelectionResult::Cancelled(CancelReason::LostCapture)),
            10,
            10
        ));
    }
    #[test]
    fn window_recreation_rejects_old_ready_and_close_identity() {
        let preview = Preview {
            instance: 8,
            hwnd: 80,
            ready: false,
            focus_foreground: None,
            transfers: Vec::new(),
            last_transfer_id: 0,
        };
        assert!(preview.accepts("screenshot", 80, 8));
        assert!(!preview.accepts("screenshot", 70, 7));
        assert!(!preview.accepts("screenshot", 80, 7));
        assert!(!preview.accepts("screenshot", 70, 8));
        assert!(!preview.accepts("main", 80, 8));
    }
    #[test]
    fn late_save_dialog_cannot_access_terminated_session() {
        let service = ScreenshotService::default();
        let Begin::Capture(ticket) = service
            .begin(ScreenshotMode::Region, Origin::Launcher)
            .unwrap()
        else {
            panic!()
        };
        let image = Arc::new(CapturedImage::from_encoded(1, 1, vec![1], &service.budget).unwrap());
        service.finish_capture(ticket, Ok(Some(image)));
        let owner = service.bind_window(ticket, 1).unwrap();
        let lease = service
            .begin_output(owner, super::super::service::OutputKind::Save)
            .unwrap();
        service.shutdown();
        assert!(service.image(lease.owner).is_err());
        assert_eq!(service.finish_output(&lease, None), OutputFinish::Stale);
    }
}
