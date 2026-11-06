//! Session-only Documents access. A native sheet can only commit to the Find
//! invocation that opened it. Clients supply neither a directory nor a URL.
use super::{check_find, error, failed, host, on_main, registries, stale, Host, Response};
use crate::{
    macos_find_results::{documents_directory_url, DocumentsGrant},
    result_registry::{QueryDomain, ResultRegistry},
};
use serde_json::{json, Value};
use std::{cell::RefCell, path::{Path, PathBuf}, sync::Arc};

use block2::RcBlock;
use objc2::{define_class, msg_send, rc::Retained, runtime::{AnyObject, NSObject, ProtocolObject},
    DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSOpenSavePanelDelegate, NSWindow};
use objc2_foundation::{NSDictionary, NSError, NSLocalizedDescriptionKey, NSObjectProtocol, NSString, NSURL};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;

#[derive(Clone, Eq, PartialEq)]
struct Owner { invocation: String, serial: u64 }

#[derive(Default)]
pub(super) struct Access {
    revision: u64,
    next_panel: u64,
    panel: Option<Owner>,
    grant: Option<Arc<DocumentsGrant>>,
    lost: bool,
}

// Serialize scope changes through completion of an old lease. A second revoke
// cannot acknowledge while a previous change is still draining a native submit.
struct Retiring(Option<Arc<DocumentsGrant>>);
impl Retiring {
    fn mark(&mut self, grant: Option<Arc<DocumentsGrant>>) {
        if let Some(grant) = &grant { grant.mark_revoked(); }
        self.0 = grant;
    }
}
impl Drop for Retiring {
    fn drop(&mut self) {
        if let Some(old) = self.0.take() { old.revoke(); }
    }
}
fn change<T>(host: &Host, operation: impl FnOnce(&mut Retiring) -> T) -> T {
    let _turn = host.document_changes.lock().expect("Documents transition");
    let mut retiring = Retiring(None);
    // operation releases controller/access/registry guards before Retiring drops.
    operation(&mut retiring)
}

impl Access {
    fn view(&self) -> Value {
        json!({"revision":self.revision.to_string(), "status": if self.grant.is_some() { "granted" }
            else if self.lost { "needsReauthorization" } else { "notGranted" }})
    }

    fn reserve(&mut self, invocation: &str) -> Response<Owner> {
        if self.panel.is_some() { return Err(error("documentsAccessBusy")); }
        self.next_panel = self.next_panel.checked_add(1).ok_or_else(failed)?;
        let owner = Owner { invocation: invocation.into(), serial: self.next_panel };
        self.panel = Some(owner.clone());
        Ok(owner)
    }

    fn replace(&mut self, grant: Option<Arc<DocumentsGrant>>, lost: bool, host: &Host, registry: &ResultRegistry,
        retiring: &mut Retiring) -> Response<()> {
        let next = self.revision.checked_add(1);
        retiring.mark(self.grant.take());
        self.lost = lost;
        host.files.clear();
        let invalidated = registry.invalidate_domain(QueryDomain::File);
        self.revision = next.ok_or_else(failed)?;
        invalidated.map_err(|_| failed())?;
        self.grant = grant;
        Ok(())
    }
}

pub(super) fn sheet_owns_focus(host: &Host, id: &str) -> bool {
    host.documents.lock().expect("Find documents").panel.as_ref().is_some_and(|owner| owner.invocation == id)
}

pub(super) fn retire_panel(host: &Host) {
    let owner = host.documents.lock().expect("Find documents").panel.take();
    if let Some(owner) = owner { cancel_sheet(&owner); }
}

pub(super) fn detach_on_exit(host: &Host) -> Option<Arc<DocumentsGrant>> {
    retire_panel(host);
    let grant = host.documents.lock().expect("Find documents").grant.take();
    if let Some(old) = &grant { old.mark_revoked(); }
    grant
}

pub(super) fn drain_on_worker(host: &Host, detached: Option<Arc<DocumentsGrant>>) {
    // Also join an explicit revocation/replacement that already detached its grant.
    let _turn = host.document_changes.lock().expect("Documents transition");
    if let Some(old) = detached { old.revoke(); }
}

/// Check the native grant outside host locks. A change in either invocation or
/// revision during validation cannot revoke a replacement grant or publish old rows.
pub(super) fn snapshot(app: &AppHandle, id: &str) -> Response<(Value, Option<Arc<DocumentsGrant>>)> {
    let host = host(app);
    let (revision, grant) = host.controller.with_visible_admission(id, || {
        let access = host.documents.lock().expect("Find documents");
        (access.revision, access.grant.clone())
    }).ok_or_else(stale)?;
    let lost = grant.as_ref().is_some_and(|grant| !grant.is_active());
    change(&host, |retiring| {
        host.controller.with_visible_admission(id, || {
            let mut access = host.documents.lock().expect("Find documents");
            if access.revision != revision { return Err(stale()); }
            if lost { access.replace(None, true, &host, registries(app).find(), retiring)?; }
            Ok((access.view(), access.grant.clone()))
        }).unwrap_or_else(|| Err(stale()))
    })
}

#[tauri::command]
pub(crate) async fn get_find_documents_access(window: WebviewWindow, invocation_id: String) -> Response<Value> {
    check_find(&window)?;
    if !window.is_visible().unwrap_or(false) { return Err(stale()); }
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || snapshot(&app, &invocation_id).map(|(view, _)| view))
        .await.map_err(|_| failed())?
}

#[tauri::command]
pub(crate) async fn revoke_find_documents_access(window: WebviewWindow, invocation_id: String) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle().clone();
    let worker_app = app.clone();
    let (owner, view) = tauri::async_runtime::spawn_blocking(move || {
        if !window.is_visible().unwrap_or(false) { return Err(stale()); }
        let service = host(&worker_app);
        change(&service, |retiring| service.controller.with_visible_admission(&invocation_id, || {
            let mut access = service.documents.lock().expect("Find documents");
            let owner = access.panel.take();
            if access.grant.is_some() || access.lost {
                access.replace(None, false, &service, registries(&worker_app).find(), retiring)?;
            }
            Ok((owner, access.view()))
        }).unwrap_or_else(|| Err(stale())))
    }).await.map_err(|_| failed())??;
    if let Some(owner) = owner {
        on_main(&app, move |_| cancel_sheet(&owner)).await?;
    }
    Ok(view)
}

fn still_selecting(service: &Host, owner: &Owner) -> bool {
    service.controller.with_visible_admission(&owner.invocation, || {
        service.documents.lock().expect("Find documents").panel.as_ref() == Some(owner)
    }) == Some(true)
}

fn acquire_for_owner<T>(service: &Host, owner: &Owner, visible: bool, acquire: impl FnOnce() -> Response<T>) -> Response<T> {
    if !visible || !still_selecting(service, owner) { return Err(stale()); }
    acquire()
}

fn finish_selection(service: &Host, registry: &ResultRegistry, owner: &Owner, visible: bool,
    picked: Response<Option<Arc<DocumentsGrant>>>) -> Response<Value> {
    let mut candidate = picked;
    let response = change(service, |retiring| service.controller.with_visible_admission(&owner.invocation, || {
        let mut access = service.documents.lock().expect("Find documents");
        if !visible || access.panel.as_ref() != Some(owner) { return Err(stale()); }
        access.panel = None;
        match &mut candidate {
            Ok(picked) => {
                if let Some(grant) = picked.take() { access.replace(Some(grant), false, service, registry, retiring)?; }
                Ok(access.view())
            }
            Err(err) => Err(err.clone()),
        }
    }).unwrap_or_else(|| Err(stale())));
    if response.is_err() {
        let mut access = service.documents.lock().expect("Find documents");
        if access.panel.as_ref() == Some(owner) { access.panel = None; }
        drop(access);
        if let Ok(Some(discarded)) = candidate { discarded.revoke(); }
    }
    response
}

type Selection = Response<Option<Retained<NSURL>>>;

#[tauri::command]
pub(crate) async fn grant_find_documents_access(window: WebviewWindow, invocation_id: String) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle().clone();
    let (owner, selected) = on_main(&app, move |app| {
        let service = host(app);
        let find = app.get_webview_window("find").ok_or_else(failed)?;
        if !find.is_visible().unwrap_or(false) || !find.is_focused().unwrap_or(false) { return Err(stale()); }
        let owner = service.controller.with_visible_admission(&invocation_id, || {
            service.documents.lock().expect("Find documents").reserve(&invocation_id)
        }).unwrap_or_else(|| Err(stale()))?;
        match begin_sheet(&find, owner.clone()) {
            Ok(rx) => Ok((owner, rx)),
            Err(err) => { retire_panel(&service); Err(err) }
        }
    }).await??;
    let selection = selected.await.unwrap_or_else(|_| Err(failed()));
    let service = host(&app);
    let picked = match selection {
        Ok(Some(url)) => {
            let owner_copy = owner.clone();
            tauri::async_runtime::spawn_blocking(move || {
                // The blocking worker may have queued behind newer input. Check
                // again immediately before acquiring any native authorization.
                acquire_for_owner(&service, &owner_copy, window.is_visible().unwrap_or(false), || {
                    objc2::rc::autoreleasepool(|_| DocumentsGrant::from_selection(&url))
                        .map(Some).map_err(|_| error("documentsAccessDenied"))
                })
            }).await.unwrap_or_else(|_| Err(failed()))
        }
        Ok(None) => Ok(None),
        Err(err) => Err(err),
    };
    let finished = owner.invocation.clone();
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let visible = worker.get_webview_window("find").is_some_and(|w| w.is_visible().unwrap_or(false));
        finish_selection(&host(&worker), registries(&worker).find(), &owner, visible, picked)
    }).await.map_err(|_| failed())?;
    // AppKit may restore the parent key window after the sheet callback. If the
    // user switched apps in the meantime, this invocation now owns ordinary blur.
    super::recheck_blur(&app, finished);
    result
}

fn matches_documents(expected: &Path, url: &NSURL) -> bool {
    url.isFileURL() && url.to_file_path().is_some_and(|path| path.as_os_str() == expected.as_os_str())
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PathBuf]
    struct DocumentsOnly;

    unsafe impl NSObjectProtocol for DocumentsOnly {}
    unsafe impl NSOpenSavePanelDelegate for DocumentsOnly {
        #[allow(non_snake_case)]
        #[unsafe(method(panel:shouldEnableURL:))]
        fn panel_shouldEnableURL(&self, _: &AnyObject, url: &NSURL) -> bool {
            matches_documents(self.ivars(), url)
        }
    }
    // Implement Cocoa's NSError** ABI directly for this optional protocol method.
    impl DocumentsOnly {
        #[unsafe(method(panel:validateURL:error:))]
        fn validate(&self, _: &AnyObject, url: &NSURL, out: *mut *mut NSError) -> bool {
            let matches = matches_documents(self.ivars(), url);
            if !matches && !out.is_null() {
                let message = NSString::from_str("请选择当前账户的系统文稿文件夹。");
                // SAFETY: Cocoa owns a writable autoreleasing NSError slot for this callback.
                unsafe {
                    let info = NSDictionary::from_slices(&[NSLocalizedDescriptionKey], &[&*message as &AnyObject]);
                    let err = NSError::errorWithDomain_code_userInfo(&NSString::from_str("UiPilot.Find"), 1, Some(&info));
                    *out = Retained::autorelease_ptr(err);
                }
            }
            matches
        }
    }
);

// NSOpenPanel and its delegate never leave AppKit's main thread. A stale
// completion cannot remove a newer sheet or wake a different invocation.
thread_local! { static SHEET: RefCell<Option<(Owner, Retained<NSOpenPanel>)>> = const { RefCell::new(None) }; }

fn cancel_sheet(owner: &Owner) {
    let panel = SHEET.with_borrow_mut(|sheet| {
        if sheet.as_ref().is_some_and(|(current, _)| current == owner) {
            sheet.take().map(|(_, panel)| panel)
        } else { None }
    });
    if let Some(panel) = panel { unsafe { panel.cancel(None) }; }
}

fn begin_sheet(window: &WebviewWindow, owner: Owner) -> Response<oneshot::Receiver<Selection>> {
    let mtm = MainThreadMarker::new().ok_or_else(failed)?;
    let expected = documents_directory_url().map_err(|_| error("documentsAccessDenied"))?;
    let path = expected.to_file_path().ok_or_else(failed)?;
    let delegate = mtm.alloc::<DocumentsOnly>().set_ivars(path);
    let delegate: Retained<DocumentsOnly> = unsafe { msg_send![super(delegate), init] };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(false);
    panel.setResolvesAliases(false);
    panel.setTitle(Some(&NSString::from_str("授权搜索文稿")));
    panel.setPrompt(Some(&NSString::from_str("授权文稿")));
    panel.setMessage(Some(&NSString::from_str("选择当前账户的系统文稿文件夹，允许 UiPilot 在本次运行期间搜索其中的文件。")));
    // Show the parent so Documents itself is a selectable row. Subdirectories
    // and neighboring folders cannot pass either of the two delegate checks.
    let parent_url = expected.URLByDeletingLastPathComponent().ok_or_else(failed)?;
    panel.setDirectoryURL(Some(&parent_url));
    unsafe { panel.setDelegate(Some(ProtocolObject::from_ref(&*delegate))); }
    let native = window.ns_window().map_err(|_| failed())?.cast::<NSWindow>();
    // SAFETY: Tauri owns this NSWindow for the duration of this main-thread call.
    let parent = unsafe { native.as_ref() }.ok_or_else(failed)?;
    let (tx, rx) = oneshot::channel();
    let tx = RefCell::new(Some(tx));
    let current = owner.clone();
    SHEET.with_borrow_mut(|sheet| *sheet = Some((owner, panel.clone())));
    let done = RcBlock::new(move |response| {
        let value = SHEET.with_borrow_mut(|sheet| {
            if !sheet.as_ref().is_some_and(|(id, _)| id == &current) { return Err(stale()); }
            let (_, panel) = sheet.take().expect("verified sheet");
            if response == NSModalResponseCancel { return Ok(None); }
            if response != NSModalResponseOK { return Err(failed()); }
            let url = panel.URL().filter(|url| matches_documents(delegate.ivars(), url))
                .ok_or_else(|| error("documentsAccessDenied"))?;
            Ok(Some(url))
        });
        if let Some(tx) = tx.borrow_mut().take() { let _ = tx.send(value); }
    });
    panel.beginSheetModalForWindow_completionHandler(parent, &done);
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_registry::{QueryDomain, ResultAction, ResultRegistries};
    use std::{fs, sync::atomic::{AtomicU64, Ordering}};

    struct Fixture { base: PathBuf, expected: PathBuf }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "uipilot-find-access-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let expected = base.join("account/Documents");
            fs::create_dir_all(&expected).unwrap();
            Self { base, expected }
        }
        fn grant(&self) -> Arc<DocumentsGrant> {
            DocumentsGrant::from_fixture(&self.expected, &NSURL::from_directory_path(&self.expected).unwrap()).unwrap()
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.base); } }

    #[test]
    fn panel_limits_selection_to_exact_directory_without_reading_its_contents() {
        let fixture = Fixture::new();
        for path in [&fixture.expected, &fixture.base, &fixture.expected.join("child")] {
            assert_eq!(matches_documents(&fixture.expected, &NSURL::from_directory_path(path).unwrap()), path == &fixture.expected);
        }
    }

    #[test]
    fn grant_cancel_revoke_and_loss_retire_only_their_own_scope() {
        let fixture = Fixture::new();
        let service = Host::default();
        let registries = ResultRegistries::default();
        let registry = registries.find();
        service.controller.set_visible_for_test("A");
        registry.on_show("A".into());
        let token = registry.begin_query(QueryDomain::File, "A", 1).unwrap();
        let (r, id) = registry.publish_if_latest(token, vec![((), ResultAction::MacFindRow(0))],
            || true, |r, items| (r, items[0].0.clone())).unwrap();
        let (_, old) = registry.resolve_with_ticket(&r, &id).unwrap();
        let grant = fixture.grant();
        let reserve = || service.documents.lock().unwrap().reserve("A").unwrap();
        let owner = reserve();
        assert!(sheet_owns_focus(&service, "A"));
        assert_eq!(finish_selection(&service, registry, &owner, true, Ok(Some(grant.clone()))).unwrap(),
            json!({"status":"granted","revision":"1"}));
        assert!(!registry.is_execution_ticket_current(&old));
        let owner = reserve();
        assert_eq!(finish_selection(&service, registry, &owner, true, Ok(None)).unwrap()["revision"], "1");
        assert!(grant.is_active());
        let owner = reserve();
        assert!(finish_selection(&service, registry, &owner, true, Err(error("documentsAccessDenied"))).is_err());
        assert!(grant.is_active());
        let (token, cancel) = service.files.begin(&service.controller, registry, "A", 2, false).unwrap();
        change(&service, |retiring| service.documents.lock().unwrap()
            .replace(None, true, &service, registry, retiring)).unwrap();
        assert_eq!(service.documents.lock().unwrap().view(), json!({"status":"needsReauthorization","revision":"2"}));
        assert!(!grant.is_active());
        assert!(cancel.check().is_err());
        assert!(registry.publish_if_latest(token, Vec::<((), ResultAction)>::new(), || true, |_, _| ()).is_none());
        change(&service, |retiring| service.documents.lock().unwrap()
            .replace(None, false, &service, registry, retiring)).unwrap();
        assert_eq!(service.documents.lock().unwrap().view(), json!({"status":"notGranted","revision":"3"}));
    }

    #[test]
    fn late_choices_after_hide_reopen_or_shutdown_cannot_replace_current_owner() {
        let fixture = Fixture::new();
        let service = Host::default();
        let registries = ResultRegistries::default();
        service.controller.set_visible_for_test("A");
        let a = service.documents.lock().unwrap().reserve("A").unwrap();
        assert!(still_selecting(&service, &a));
        service.controller.finish_native_close(&registries);
        retire_panel(&service);
        assert!(!still_selecting(&service, &a));
        service.controller.set_visible_for_test("B");
        let b = service.documents.lock().unwrap().reserve("B").unwrap();
        let should_not_acquire = || -> Response<()> { panic!("stale chooser reached native authorization") };
        assert!(acquire_for_owner(&service, &a, true, should_not_acquire).is_err());
        assert!(acquire_for_owner(&service, &b, false, should_not_acquire).is_err());
        let discarded = fixture.grant();
        assert!(finish_selection(&service, registries.find(), &a, true, Ok(Some(discarded.clone()))).is_err());
        assert!(!discarded.is_active());
        assert!(still_selecting(&service, &b));
        assert_eq!(service.documents.lock().unwrap().view()["revision"], "0");
        service.controller.shutdown();
        let grant = detach_on_exit(&service);
        drain_on_worker(&service, grant);
        assert!(!still_selecting(&service, &b));
        assert!(finish_selection(&service, registries.find(), &b, false, Ok(None)).is_err());
    }
}
