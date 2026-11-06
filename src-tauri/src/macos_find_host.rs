//! Preview-only local Find window. All native transfers and hides are serialized on
//! AppKit's main thread. The shared controller owns readiness, pin and invocation order.
use std::{
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex},
    time::{Duration, Instant},
};

use objc2_app_kit::NSRunningApplication;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent};

use crate::{
    find_window::{
        FindReadyStatus, FindWindowController, FocusEffect, ForegroundWindow, NativeFocusSnapshot,
        OpenFindCompletion, TransferFocusResult, WindowLabel, TRANSFER_TIMEOUT,
    },
    host_capabilities::{require_find, require_main},
    result_registry::ResultRegistries,
    settings::SettingsStore,
};

#[path = "macos_find_files.rs"]
mod files;
pub(crate) use files::*;

#[cfg(not(feature = "macos-find-system-qa"))]
#[path = "macos_find_access.rs"]
mod documents;
#[cfg(not(feature = "macos-find-system-qa"))]
pub(crate) use documents::*;

type Response<T> = Result<T, Value>;
fn error(code: &str) -> Value { json!({ "code": code }) }
fn stale() -> Value { error("staleRequest") }
fn failed() -> Value { error("windowFailed") }

struct Transfer { id: u64, was_visible: bool, focus_before: ForegroundWindow, deadline: Instant }

#[derive(Default)]
struct Host {
    controller: FindWindowController,
    files: files::Files,
    #[cfg(not(feature = "macos-find-system-qa"))]
    documents: Mutex<documents::Access>,
    #[cfg(not(feature = "macos-find-system-qa"))]
    document_changes: Mutex<()>,
    #[cfg(not(feature = "macos-find-system-qa"))]
    exit_drain: Mutex<ExitDrain>,
    transfer: Mutex<Option<Transfer>>,
    driving: AtomicBool,
    stopping: AtomicBool,
    // An already queued launcher blur check must not hide the source after Find
    // closes. Only a new main focus edge or invocation ends this exemption.
    transferred_main: AtomicU64,
}

pub(crate) fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    app.add_capability(include_str!("../macos-find-preview-acl.json"))?;
    #[cfg(not(feature = "macos-find-system-qa"))]
    app.add_capability(include_str!("../macos-find-documents-acl.json"))?;
    app.manage(Arc::new(Host::default()));
    let scope_marker = if cfg!(feature = "macos-find-system-qa") {
        "Object.defineProperty(window,'__UIPILOT_MACOS_FIND__',{value:'indexed-local-qa'});"
    } else {
        "Object.defineProperty(window,'__UIPILOT_MACOS_FIND__',{value:'public-preview'});"
    };
    WebviewWindowBuilder::new(app, "find", WebviewUrl::App("index.html".into()))
        .title("UiPilot_tools Find").inner_size(900., 600.).min_inner_size(720., 420.)
        .visible(false).focused(false).decorations(false).resizable(true)
        .initialization_script(scope_marker)
        .build()?;
    Ok(())
}

fn host(app: &AppHandle) -> Arc<Host> { Arc::clone(app.state::<Arc<Host>>().inner()) }
fn registries(app: &AppHandle) -> Arc<ResultRegistries> { Arc::clone(app.state::<Arc<ResultRegistries>>().inner()) }
fn check_find(window: &WebviewWindow) -> Response<()> {
    require_find(window).map_err(|_| error("invalidCaller"))
}

fn native_snapshot(app: &AppHandle) -> NativeFocusSnapshot {
    let focused = |label| app.get_webview_window(label)
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    let main_focused = focused("main");
    let find_focused = focused("find");
    // On AppKit, a background app can retain a key window.
    let active = NSRunningApplication::currentApplication().isActive();
    let foreground = match (active, main_focused, find_focused) {
        (true, true, false) => ForegroundWindow::Main,
        (true, false, true) => ForegroundWindow::Find,
        _ => ForegroundWindow::Other,
    };
    NativeFocusSnapshot { main_focused, find_focused, foreground }
}

async fn on_main<T: Send + 'static>(
    app: &AppHandle, f: impl FnOnce(&AppHandle) -> T + Send + 'static,
) -> Response<T> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || { let _ = send.send(f(&handle)); }).map_err(|_| failed())?;
    receive.await.map_err(|_| failed())
}

pub(crate) fn owns_main_blur(app: &AppHandle) -> bool {
    let Some(service) = app.try_state::<Arc<Host>>() else { return false; };
    if service.transfer.lock().expect("Find transfer").is_some() { return true; }
    let transferred = service.transferred_main.load(Ordering::Acquire);
    transferred != 0 && crate::macos_host::main_invocation_current(app, transferred)
}

fn schedule(app: &AppHandle) {
    let service = host(app);
    if service.stopping.load(Ordering::Acquire) || service.driving.swap(true, Ordering::AcqRel) { return; }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if on_main(&handle, drive).await.is_err() || service.stopping.load(Ordering::Acquire) { break; }
            if service.transfer.lock().expect("Find transfer").is_none() && service.controller.queued_query().is_none() { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        service.driving.store(false, Ordering::Release);
        if !service.stopping.load(Ordering::Acquire) && service.controller.queued_query().is_some() {
            schedule(&handle);
        }
    });
}

fn rollback(app: &AppHandle, transfer: Transfer) {
    let service = host(app);
    service.controller.fail_transfer_before_ownership(transfer.id);
    let foreground = native_snapshot(app).foreground;
    if !transfer.was_visible {
        if let Some(find) = app.get_webview_window("find") { let _ = find.hide(); }
    }
    // Never take focus back from an unrelated application.
    if foreground != ForegroundWindow::Other && transfer.focus_before == ForegroundWindow::Main {
        if let Some(main) = app.get_webview_window("main") { let _ = main.set_focus(); }
    }
}

fn drive(app: &AppHandle) {
    let service = host(app);
    if service.stopping.load(Ordering::Acquire) { return; }
    let now = Instant::now();
    let controller = &service.controller;
    let Some(find) = app.get_webview_window("find") else { controller.shutdown(); return; };
    let registries = registries(app);
    let transfer = service.transfer.lock().expect("Find transfer").take();
    if let Some(transfer) = transfer {
        let source = controller.transfer_source_invocation(transfer.id, &registries);
        if now >= transfer.deadline || source.is_none() {
            rollback(app, transfer);
            return;
        }
        match controller.confirm_transfer_focus(transfer.id, native_snapshot(app)) {
            TransferFocusResult::AwaitingEvidence => {
                *service.transfer.lock().expect("Find transfer") = Some(transfer);
            }
            TransferFocusResult::CommitFindScope => {
                let Ok(payload) = controller.commit_find_scope(transfer.id, &registries) else {
                    rollback(app, transfer);
                    return;
                };
                service.files.clear();
                let mut forwarded = json!(payload);
                forwarded["pinned"] = json!(controller.pinned());
                let emitted = find.emit_to("find", "find://forwarded", forwarded).is_ok();
                controller.finish_forward_emit(transfer.id, emitted, &registries);
                if emitted {
                    service.transferred_main.store(source.expect("verified source"), Ordering::Release);
                } else {
                    let _ = find.hide();
                    if transfer.focus_before == ForegroundWindow::Main && NSRunningApplication::currentApplication().isActive() {
                        if let Some(main) = app.get_webview_window("main") { let _ = main.set_focus(); }
                    }
                }
            }
            TransferFocusResult::Stale => rollback(app, transfer),
        }
        return;
    }
    controller.expire(now);
    let snapshot = native_snapshot(app);
    let Some(plan) = controller.admit_queued_transfer(snapshot, now) else { return; };
    let transfer = Transfer { id: plan.transfer_id, was_visible: find.is_visible().unwrap_or(false),
        focus_before: snapshot.foreground, deadline: now + TRANSFER_TIMEOUT };
    if controller.transfer_source_invocation(transfer.id, &registries).is_none() {
        rollback(app, transfer);
        return;
    }
    #[cfg(not(feature = "macos-find-system-qa"))]
    documents::retire_panel(&service);
    *service.transfer.lock().expect("Find transfer") = Some(transfer);
    if find.unminimize().and_then(|_| find.show()).and_then(|_| find.set_focus()).is_err() {
        let failed = service.transfer.lock().expect("Find transfer").take();
        if let Some(failed) = failed { rollback(app, failed); }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenInput { query: String, invocation_id: String, query_sequence: u64 }
#[tauri::command]
pub(crate) async fn open_find_window(window: WebviewWindow, input: OpenInput) -> Response<Value> {
    require_main(&window).map_err(|_| error("invalidCaller"))?;
    if !crate::host_capabilities::mac_find_enabled() { return Err(error("windowFailed")); }
    if input.invocation_id.is_empty() || input.query_sequence == 0 || input.query.len() > 1024
        || input.query.contains('\0') { return Err(error("invalidFileQuery")); }
    let app = window.app_handle();
    let service = host(app);
    let id = input.invocation_id.parse().map_err(|_| stale())?;
    if !crate::macos_host::main_invocation_current(app, id) { return Err(stale()); }
    let lease = registries(app).main().prepare_application_query_retirement(&input.invocation_id, input.query_sequence)
        .ok().flatten().ok_or_else(stale)?;
    let submission = service.controller.submit_open(input.query, lease, Instant::now()).map_err(|_| failed())?;
    #[cfg(not(feature = "macos-find-system-qa"))]
    on_main(app, |app| documents::retire_panel(&host(app))).await?;
    schedule(app);
    let completion = tauri::async_runtime::spawn_blocking(move || submission.completion.recv_timeout(Duration::from_secs(8)))
        .await.map_err(|_| failed())?.map_err(|_| failed())?;
    match completion {
        OpenFindCompletion::Forwarded => Ok(json!({"status":"forwarded"})),
        OpenFindCompletion::Superseded => Ok(json!({"status":"superseded"})),
        OpenFindCompletion::Unavailable => Err(failed()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Initialization { initialization_token: String }
fn readiness(app: &AppHandle, token: String, status: FindReadyStatus) -> Value {
    match status {
        FindReadyStatus::Prepared => {
            let p = app.state::<SettingsStore>().find_preference_snapshot();
            json!({"status":"prepared","initialization":{
                "initializationToken":token,"themeRevision":p.theme_revision.to_string(),"theme":p.theme,
                "filePreviewRevision":p.file_preview_revision.to_string(),"filePreviewEnabled":p.file_preview_enabled,
                "pinned":host(app).controller.pinned()
            }})
        }
        FindReadyStatus::Ready => json!({"status":"ready","initializationToken":token}),
        FindReadyStatus::Superseded => json!({"status":"superseded"}),
    }
}
#[tauri::command]
pub(crate) fn prepare_find_initialization(window: WebviewWindow) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle();
    let token = host(app).controller.prepare_initialization(Instant::now()).map_err(|_| failed())?.token;
    Ok(readiness(app, token, FindReadyStatus::Prepared))
}
#[tauri::command]
pub(crate) fn commit_find_ready(window: WebviewWindow, input: Initialization) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle();
    let commit = host(app).controller.commit_ready(&input.initialization_token, Instant::now());
    if commit.snapshot_required { schedule(app); }
    Ok(readiness(app, input.initialization_token, commit.outcome))
}
#[tauri::command]
pub(crate) fn get_find_ready_status(window: WebviewWindow, input: Initialization) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle();
    let status = host(app).controller.ready_status(&input.initialization_token, Instant::now());
    Ok(readiness(app, input.initialization_token, status))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Pin { invocation_id: String, pinned: bool }
#[tauri::command]
pub(crate) async fn set_find_pinned(window: WebviewWindow, input: Pin) -> Response<Value> {
    check_find(&window)?;
    on_main(window.app_handle(), move |app| {
        let service = host(app);
        let find = app.get_webview_window("find").ok_or_else(failed)?;
        if !find.is_visible().unwrap_or(false) || !service.controller.set_pin(&input.invocation_id, input.pinned) {
            return Err(stale());
        }
        Ok(json!({"pinned":input.pinned}))
    }).await?
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preference { enabled: bool }
#[tauri::command]
pub(crate) async fn set_find_preview_preference(window: WebviewWindow, preference: Preference) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle().clone();
    if !window.is_visible().unwrap_or(false) || host(&app).controller.current_invocation().is_none() { return Err(stale()); }
    tauri::async_runtime::spawn_blocking(move || {
        let revision = app.state::<SettingsStore>().set_file_preview_enabled_with_revision(preference.enabled)
            .map_err(|_| error("settingsFailed"))?;
        Ok(json!({"filePreviewRevision":revision.to_string(),"filePreviewEnabled":preference.enabled}))
    }).await.map_err(|_| failed())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Hide { invocation_id: String, force: bool }
fn hide(app: &AppHandle, id: &str, force: bool) -> Response<()> {
    let service = host(app);
    if !service.controller.request_explicit_hide(id, force) {
        return if !force && service.controller.current_invocation().as_deref() == Some(id) && service.controller.pinned() {
            Ok(())
        } else { Err(stale()) };
    }
    #[cfg(not(feature = "macos-find-system-qa"))]
    documents::retire_panel(&service);
    let hidden = app.get_webview_window("find").is_some_and(|find| find.hide().is_ok());
    if service.controller.finish_explicit_hide(id, hidden, &registries(app)) && hidden {
        service.files.clear();
    }
    if hidden { Ok(()) } else { Err(failed()) }
}
#[tauri::command]
pub(crate) async fn hide_find_window(window: WebviewWindow, input: Hide) -> Response<()> {
    check_find(&window)?;
    on_main(window.app_handle(), move |app| hide(app, &input.invocation_id, input.force)).await?
}

fn blur_owned(host: &Host, id: &str) -> bool {
    #[cfg(not(feature = "macos-find-system-qa"))]
    if documents::sheet_owns_focus(host, id) { return true; }
    host.files.dispatching(id)
}

fn should_hide_after_blur(host: &Host, id: &str, foreground: ForegroundWindow) -> bool {
    foreground != ForegroundWindow::Find && !blur_owned(host, id) && host.controller.request_explicit_hide(id, false)
}

fn recheck_blur(app: &AppHandle, id: String) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(75)).await;
        let _ = on_main(&handle, move |app| {
            if should_hide_after_blur(&host(app), &id, native_snapshot(app).foreground) {
                let _ = hide(app, &id, false);
            }
        }).await;
    });
}

pub(crate) fn on_window_event(window: &Window, event: &WindowEvent) -> bool {
    let app = window.app_handle();
    let Some(service) = app.try_state::<Arc<Host>>() else { return false; };
    if service.stopping.load(Ordering::Acquire) { return false; }
    if window.label() == "main" {
        if let WindowEvent::Focused(true) = event {
            if native_snapshot(app).foreground == ForegroundWindow::Main {
                service.transferred_main.store(0, Ordering::Release);
            }
        }
        return false;
    }
    if window.label() != "find" { return false; }
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            // A native close targets the window itself, including a show/focus
            // transfer that has not yet assigned a renderer invocation.
            #[cfg(not(feature = "macos-find-system-qa"))]
            documents::retire_panel(&service);
            if window.hide().is_ok() {
                service.transfer.lock().expect("Find transfer").take();
                service.controller.finish_native_close(&registries(app));
                service.files.clear();
            }
        }
        WindowEvent::Focused(focused) => {
            if service.controller.observe_focus(WindowLabel::Find, *focused) == FocusEffect::HideFind {
                if let Some(id) = service.controller.current_invocation().filter(|id| !blur_owned(&service, id)) {
                    recheck_blur(app, id);
                }
            }
        }
        _ => {}
    }
    true
}

#[cfg(not(feature = "macos-find-system-qa"))]
#[derive(Default)]
struct ExitDrain { deferred: bool, complete: bool }

#[cfg(not(feature = "macos-find-system-qa"))]
impl ExitDrain {
    fn request(&mut self, first: bool, pending: bool, code: Option<i32>) -> bool {
        if first {
            self.complete = !pending;
            self.deferred = pending && code == Some(0);
        } else if code != Some(0) {
            // The OS, a restart, or an abnormal exit supersedes our own Quit.
            self.deferred = false;
        }
        self.deferred && !self.complete
    }
    fn finish(&mut self) -> bool {
        self.complete = true;
        std::mem::take(&mut self.deferred)
    }
}

pub(crate) fn shutdown(app: &AppHandle, code: Option<i32>, api: &tauri::ExitRequestApi) {
    let Some(service) = app.try_state::<Arc<Host>>() else { return; };
    let first = !service.stopping.swap(true, Ordering::AcqRel);
    if first {
        service.controller.shutdown();
        service.files.clear();
        registries(app).find().hide_and_clear();
    }
    #[cfg(not(feature = "macos-find-system-qa"))]
    {
        // Stop accepting actions synchronously. Never wait on AppKit's event loop
        // for an NSWorkspace submission that may itself need that event loop.
        let detached = if first { documents::detach_on_exit(&service) } else { None };
        let pending = first && (detached.is_some() || service.document_changes.try_lock().is_err());
        if service.exit_drain.lock().expect("Find exit drain").request(first, pending, code) {
            api.prevent_exit();
        }
        if pending {
            let service = Arc::clone(service.inner());
            let handle = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                documents::drain_on_worker(&service, detached);
                if service.exit_drain.lock().expect("Find exit drain").finish() { handle.exit(0); }
            });
        }
    }
    #[cfg(feature = "macos-find-system-qa")]
    let _ = (code, api);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_registry::QueryDomain;

    #[cfg(not(feature = "macos-find-system-qa"))]
    #[test]
    fn quit_waits_for_drain_only_for_an_explicit_normal_exit() {
        let mut drain = ExitDrain::default();
        assert!(drain.request(true, true, Some(0)));
        assert!(drain.request(false, false, Some(0)));
        assert!(drain.finish());
        assert!(!drain.request(false, false, Some(0)));
        for code in [None, Some(1), Some(tauri::RESTART_EXIT_CODE)] {
            let mut drain = ExitDrain::default();
            assert!(drain.request(true, true, Some(0)));
            assert!(!drain.request(false, false, code));
            assert!(!drain.finish());
        }
        let mut drain = ExitDrain::default();
        assert!(!drain.request(true, false, Some(0)));
        assert!(!drain.request(false, false, Some(0)));
    }

    #[test]
    fn delayed_blur_respects_current_invocation_actual_focus_and_pin() {
        let host = Host::default();
        host.controller.set_visible_for_test("second");
        assert!(!should_hide_after_blur(&host, "first", ForegroundWindow::Other));
        assert!(!should_hide_after_blur(&host, "second", ForegroundWindow::Find));
        assert!(should_hide_after_blur(&host, "second", ForegroundWindow::Other));
        host.controller.set_pin("second", true);
        assert!(!should_hide_after_blur(&host, "second", ForegroundWindow::Other));
    }

    #[test]
    fn native_close_cancels_the_active_transfer_and_its_latest_waiter() {
        let registries = ResultRegistries::default();
        let ctl = FindWindowController::default();
        let now = Instant::now();
        let init = ctl.prepare_initialization(now).unwrap();
        ctl.commit_ready(&init.token, now);
        let main = registries.main();
        main.on_show("1".into());
        let open = |seq| {
            main.begin_query(QueryDomain::Application, "1", seq).unwrap();
            let lease = main.prepare_application_query_retirement("1", seq).unwrap().unwrap();
            ctl.submit_open("font".into(), lease, now).unwrap()
        };
        let first = open(1);
        let plan = ctl.admit_queued_transfer(NativeFocusSnapshot {
            main_focused: true, find_focused: false, foreground: ForegroundWindow::Main,
        }, now).unwrap();
        assert_eq!(ctl.transfer_source_invocation(plan.transfer_id, &registries), Some(1));
        let queued = open(2);
        assert_eq!(ctl.transfer_source_invocation(plan.transfer_id, &registries), None);
        ctl.finish_native_close(&registries);
        assert_eq!(first.completion.recv().unwrap(), OpenFindCompletion::Superseded);
        assert_eq!(queued.completion.recv().unwrap(), OpenFindCompletion::Superseded);
        assert!(ctl.current_invocation().is_none());
        assert!(ctl.queued_query().is_none());
        assert_eq!(ctl.ready_status(&init.token, now), FindReadyStatus::Ready);
    }
}
