//! Explicit session or remembered Documents access. A native sheet can only commit to the Find
//! invocation that opened it. Clients supply neither a directory nor a URL.
use super::{check_find, error, failed, host, on_main, registries, stale, Host, Response};
use crate::{
    macos_find_results::{documents_directory_url, DocumentsGrant, FindError},
    result_registry::{QueryDomain, ResultRegistry},
};
use serde_json::{json, Value};
use std::{cell::RefCell, path::{Path, PathBuf}, sync::{Arc, OnceLock}};

#[path = "macos_find_bookmark_store.rs"]
mod bookmark_store;
use bookmark_store::{Record, Store};

#[derive(Default)]
pub(super) struct Storage { data_dir: Option<PathBuf>, store: OnceLock<Store> }

impl Storage {
    pub(super) fn new(data_dir: PathBuf) -> Self { Self { data_dir: Some(data_dir), ..Self::default() } }
    // Every caller holds the host's document_changes gate. Do not cache a
    // temporary directory-open failure; the next visible request may retry.
    fn store(&self) -> Response<&Store> {
        if let Some(store) = self.store.get() { return Ok(store); }
        let store = Store::open(self.data_dir.as_deref().ok_or_else(storage_failed)?).map_err(|_| storage_failed())?;
        self.store.set(store).map_err(|_| storage_failed())?;
        self.store.get().ok_or_else(storage_failed)
    }
}
fn storage_failed() -> Value { error("documentsAccessStorageFailed") }


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
    loaded: bool,
    remembered: bool,
    block_restore: bool,
}

// Serialize scope changes through completion of an old lease. A second revoke
// cannot acknowledge while a previous change is still draining a native submit.
struct Retiring(Vec<Arc<DocumentsGrant>>);
impl Retiring {
    fn mark(&mut self, grant: Option<Arc<DocumentsGrant>>) {
        if let Some(grant) = grant {
            grant.mark_revoked();
            self.0.push(grant);
        }
    }
}
impl Drop for Retiring {
    fn drop(&mut self) {
        for old in self.0.drain(..) { old.revoke(); }
    }
}
fn change<T>(host: &Host, operation: impl FnOnce(&mut Retiring) -> T) -> T {
    let _turn = host.document_changes.lock().expect("Documents transition");
    let mut retiring = Retiring(Vec::new());
    // operation releases controller/access/registry guards before Retiring drops.
    operation(&mut retiring)
}

impl Access {
    fn view(&self) -> Value {
        json!({"revision":self.revision.to_string(),
            "persistence": if self.remembered { "remembered" } else if self.grant.is_some() { "session" } else { "none" },
            "status": if self.grant.is_some() { "granted" }
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
        let (Some(next), Ok(())) = (next, invalidated) else {
            // Do not let a newly selected URL's native lease drop under host locks.
            retiring.mark(grant);
            return Err(failed());
        };
        self.revision = next;
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

// A stored grant is resolved on a worker only for the current visible invocation.
// Reading a missing record or tombstone never looks up the system Documents URL.
fn restore(service: &Host, registry: &ResultRegistry, id: &str, retiring: &mut Retiring,
    visible: impl Fn() -> bool, resolve: impl FnOnce(&[u8]) -> Result<Arc<DocumentsGrant>, crate::macos_find_results::FindError>) -> Response<()> {
    if !visible() { return Err(stale()); }
    let loaded = service.controller.with_visible_admission(id, || service.documents.lock().expect("Find documents").loaded)
        .ok_or_else(stale)?;
    if loaded { return Ok(()); }
    let store = service.bookmark_store.store()?;
    let record = store.read().map_err(|_| storage_failed())?;
    let remembered = record != Record::Forgotten;
    let mut permanent = false;
    let restored = match record {
        Record::Bookmark(bytes) => {
            if !visible() || !service.controller.admit_search(id) { return Err(stale()); }
            match resolve(&bytes) {
                Ok(grant) => Some(grant),
                Err(err) => {
                    permanent = matches!(err, FindError::Stale | FindError::InvalidInput);
                    None
                }
            }
        }
        Record::Forgotten | Record::NeedsAuthorization => None,
    };
    let lost = remembered && restored.is_none();
    if !visible() { return Err(stale()); }
    service.controller.with_visible_admission(id, || {
        let mut access = service.documents.lock().expect("Find documents");
        if remembered { access.replace(restored, lost, service, registry, retiring)?; }
        access.loaded = true;
        access.remembered = remembered;
        access.block_restore = permanent;
        Ok(())
    }).unwrap_or_else(|| Err(stale()))
}

// Persist loss as a non-restorable marker. Without this marker, a replaced
// object or a later OS permission change could re-enable a previously lost grant.
fn block_if_needed(service: &Host) -> Response<()> {
    if service.documents.lock().expect("Find documents").block_restore {
        service.bookmark_store.store()?.write(&Record::NeedsAuthorization).map_err(|_| storage_failed())?;
        service.documents.lock().expect("Find documents").block_restore = false;
    }
    Ok(())
}

fn access_snapshot(app: &AppHandle, id: &str, strict: bool) -> Response<(Value, Option<Arc<DocumentsGrant>>)> {
    let service = host(app);
    change(&service, |retiring| {
        let registry = registries(app);
        let restored = restore(&service, registry.find(), id, retiring,
            || app.get_webview_window("find").is_some_and(|window| window.is_visible().unwrap_or(false)),
            DocumentsGrant::from_persistent_record);
        if let Err(err) = restored {
            if strict || err != storage_failed() { return Err(err); }
            // Storage outages cannot expose Documents or prevent public file search.
            return Ok((service.documents.lock().expect("Find documents").view(), None));
        }
        let grant = service.documents.lock().expect("Find documents").grant.clone();
        let lost = grant.as_ref().is_some_and(|grant| !grant.is_active());
        let view = service.controller.with_visible_admission(id, || {
            let mut access = service.documents.lock().expect("Find documents");
            if lost {
                access.replace(None, true, &service, registry.find(), retiring)?;
                // A boolean health check cannot distinguish lost OS permission
                // from a temporary volume/service failure. Keep any original
                // record: a future owner still must pass native identity checks.
            }
            Ok((access.view(), access.grant.clone()))
        }).unwrap_or_else(|| Err(stale()))?;
        let blocked = block_if_needed(&service);
        if strict { blocked?; }
        Ok(view)
    })
}

pub(super) fn snapshot(app: &AppHandle, id: &str) -> Response<(Value, Option<Arc<DocumentsGrant>>)> {
    access_snapshot(app, id, false)
}

// Focus can reach WebKit before the host finishes committing the handoff.
// Retry only a still-owned session's lifecycle rejection, outside all host locks.
fn settled_status_read(
    mut read: impl FnMut() -> Response<Value>,
    mut retained: impl FnMut() -> bool,
    deadline: std::time::Instant,
) -> Response<Value> {
    loop {
        let result = read();
        if result.as_ref().err() != Some(&stale()) || !retained() { return result; }
        if std::time::Instant::now() >= deadline { return Err(error("documentsAccessBusy")); }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[tauri::command]
pub(crate) async fn get_find_documents_access(window: WebviewWindow, invocation_id: String) -> Response<Value> {
    check_find(&window)?;
    if !window.is_visible().unwrap_or(false) { return Err(stale()); }
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let service = host(&app);
        settled_status_read(
            || access_snapshot(&app, &invocation_id, true).map(|(view, _)| view),
            || service.controller.retains_document_status_owner(&invocation_id),
            std::time::Instant::now() + crate::find_window::TRANSFER_TIMEOUT,
        )
    }).await.map_err(|_| failed())?
}

#[tauri::command]
pub(crate) async fn revoke_find_documents_access(window: WebviewWindow, invocation_id: String) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle().clone();
    // Invalidate the picker owner before waiting for a binding operation already
    // on a worker. Its late URL must never be published to this invocation.
    let id = invocation_id.clone();
    on_main(&app, move |app| {
        let service = host(app);
        if !app.get_webview_window("find").is_some_and(|w| w.is_visible().unwrap_or(false)) { return Err(stale()); }
        let panel = service.controller.with_visible_admission(&id, || {
            service.documents.lock().expect("Find documents").panel.take()
        }).ok_or_else(stale)?;
        if let Some(panel) = panel { cancel_sheet(&panel); }
        Ok(())
    }).await??;
    tauri::async_runtime::spawn_blocking(move || {
        let service = host(&app);
        change(&service, |retiring| forget(&service, registries(&app).find(), &invocation_id, retiring))
    }).await.map_err(|_| failed())?
}

fn forget(service: &Host, registry: &ResultRegistry, id: &str, retiring: &mut Retiring) -> Response<Value> {
    // Stop access even on a read-only or offline volume. Never report that a
    // bookmark was forgotten until the replacement tombstone is durable.
    service.controller.with_visible_admission(id, || {
        let mut access = service.documents.lock().expect("Find documents");
        access.replace(None, true, service, registry, retiring)?;
        access.remembered = true;
        access.loaded = true;
        // If forgetting fails, return an error and keep a retryable local state.
        // A status read must never substitute a different disk operation for forget.
        access.block_restore = false;
        Ok(())
    }).unwrap_or_else(|| Err(stale()))?;
    let store = service.bookmark_store.store()?;
    store.write(&Record::Forgotten).map_err(|_| storage_failed())?;
    let mut access = service.documents.lock().expect("Find documents");
    access.lost = false;
    access.remembered = false;
    access.block_restore = false;
    Ok(access.view())
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

// Prepare sensitive bytes and fsync them on a worker. A failed attempt to
// remember can become session-only only after excluding any prior restorable
// record. Never return a session-only grant over an old persistent authority.
fn plan_record(store: &Store, bookmark: Option<Vec<u8>>) -> Response<Option<Record>> {
    let previous = store.read().map_err(|_| storage_failed())?;
    if let Some(bytes) = bookmark {
        let next = Record::Bookmark(bytes);
        if store.stage(&next).is_ok() { return Ok(Some(next)); }
        if previous != Record::Forgotten { return Err(storage_failed()); }
    }
    if previous != Record::Forgotten {
        // Only an explicit forget may discard an existing record when no new
        // persistent grant is available (including unsigned/session-only builds).
        return Err(error("documentsRememberedAccessExists"));
    }
    store.discard_stage().map_err(|_| storage_failed())?;
    store.confirm(&Record::Forgotten).map_err(|_| storage_failed())?;
    Ok(None)
}

fn finish_selection(service: &Host, registry: &ResultRegistry, owner: &Owner,
    make_record: impl FnOnce(&DocumentsGrant) -> Option<Vec<u8>>, visible: impl Fn() -> bool,
    acquire: impl FnOnce() -> Response<Option<Arc<DocumentsGrant>>>) -> Response<Value> {
    // Keep native acquisition, commit, and discard in the same worker-side
    // transition. A revoke acknowledgment cannot overtake an in-flight pickup.
    change(service, |retiring| {
        let mut candidate = acquire_for_owner(service, owner, visible(), acquire);
        let mut planned = None;
        if matches!(&candidate, Ok(Some(_))) && (!visible() || !still_selecting(service, owner)) {
            if let Ok(Some(discarded)) = candidate { discarded.revoke(); }
            candidate = Err(stale());
        }
        if let Ok(Some(grant)) = &candidate {
            let bookmark = make_record(grant);
            match service.bookmark_store.store().and_then(|store| plan_record(store, bookmark)) {
                Ok(plan) => planned = plan,
                Err(err) => {
                    if let Ok(Some(discarded)) = candidate { discarded.revoke(); }
                    candidate = Err(err);
                }
            }
        }
        let is_visible = visible();
        let mut published = false;
        let response = service.controller.with_visible_admission(&owner.invocation, || {
            let mut access = service.documents.lock().expect("Find documents");
            if !is_visible || access.panel.as_ref() != Some(owner) { return Err(stale()); }
            access.panel = None;
            match &mut candidate {
                Ok(picked) => {
                    if picked.is_some() {
                        if planned.is_some() {
                            service.bookmark_store.store()?.publish().map_err(|_| storage_failed())?;
                            published = true;
                        }
                        let grant = picked.take();
                        access.replace(grant, false, service, registry, retiring)?;
                        access.loaded = true;
                        access.block_restore = false;
                        access.remembered = matches!(&planned, Some(Record::Bookmark(_)));
                    }
                    Ok(access.view())
                }
                Err(err) => Err(err.clone()),
            }
        }).unwrap_or_else(|| Err(stale()));
        let mut response = response;
        if let Some(record) = &planned {
            let store = service.bookmark_store.store()?;
            if published {
                if store.confirm(record).is_err() || response.is_err() {
                    // The rename may have succeeded. Stop the in-memory grant
                    // and avoid restoring uncertain bytes. Forget remains retryable.
                    let mut access = service.documents.lock().expect("Find documents");
                    let _ = access.replace(None, true, service, registry, retiring);
                    access.loaded = true;
                    access.remembered = true;
                    access.block_restore = true;
                    drop(access);
                    let _ = block_if_needed(service);
                    response = Err(storage_failed());
                }
            } else if store.discard_stage().is_err() {
                response = Err(storage_failed());
            }
        }
        if response.is_err() {
            let mut access = service.documents.lock().expect("Find documents");
            if access.panel.as_ref() == Some(owner) { access.panel = None; }
            drop(access);
            if let Ok(Some(discarded)) = candidate { discarded.revoke(); }
        }
        response
    })
}

type Selection = Response<Option<Retained<NSURL>>>;

#[tauri::command]
pub(crate) async fn grant_find_documents_access(window: WebviewWindow, invocation_id: String, remember: Option<bool>) -> Response<Value> {
    check_find(&window)?;
    let app = window.app_handle().clone();
    let remember = remember.unwrap_or(false);
    let (owner, selected) = on_main(&app, move |app| {
        let service = host(app);
        let find = app.get_webview_window("find").ok_or_else(failed)?;
        if !find.is_visible().unwrap_or(false) || !find.is_focused().unwrap_or(false) { return Err(stale()); }
        let owner = service.controller.with_visible_admission(&invocation_id, || {
            service.documents.lock().expect("Find documents").reserve(&invocation_id)
        }).unwrap_or_else(|| Err(stale()))?;
        match begin_sheet(&find, owner.clone(), remember) {
            Ok(rx) => Ok((owner, rx)),
            Err(err) => { retire_panel(&service); Err(err) }
        }
    }).await??;
    let selection = selected.await.unwrap_or_else(|_| Err(failed()));
    let finished = owner.invocation.clone();
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        finish_selection(&host(&worker), registries(&worker).find(), &owner,
            |grant| if remember { grant.persistent_record().ok() } else { None },
            || window.is_visible().unwrap_or(false), || match selection? {
                Some(url) => objc2::rc::autoreleasepool(|_| DocumentsGrant::from_selection(&url))
                    .map(Some).map_err(|_| error("documentsAccessDenied")),
                None => Ok(None),
            })
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

fn begin_sheet(window: &WebviewWindow, owner: Owner, remember: bool) -> Response<oneshot::Receiver<Selection>> {
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
    let (prompt, message) = if remember {
        ("授权并记住文稿", "选择系统文稿文件夹。UiPilot 将尝试保存只读授权，下次打开文件搜索时恢复；如果 macOS 仅允许本次访问，界面会显示“仅本次”。")
    } else {
        ("仅授权本次", "选择系统文稿文件夹，允许 UiPilot 在本次运行期间搜索其中的文件。")
    };
    panel.setTitle(Some(&NSString::from_str(prompt)));
    panel.setPrompt(Some(&NSString::from_str(prompt)));
    panel.setMessage(Some(&NSString::from_str(message)));
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

    #[test]
    fn status_read_retries_only_retained_handoff_rejections() {
        let mut calls = 0;
        let allowed = json!({"status":"granted","persistence":"remembered","revision":"1"});
        let value = settled_status_read(|| {
            calls += 1;
            if calls < 3 { Err(stale()) } else { Ok(allowed.clone()) }
        }, || true, std::time::Instant::now() + std::time::Duration::from_secs(1)).unwrap();
        assert_eq!(value, allowed);
        assert_eq!(calls, 3);
        assert_eq!(settled_status_read(|| Err(stale()), || true, std::time::Instant::now()), Err(error("documentsAccessBusy")));
        assert_eq!(settled_status_read(|| Err(stale()), || false, std::time::Instant::now()), Err(stale()));
        assert_eq!(settled_status_read(|| Err(storage_failed()), || panic!("storage error must not retry"), std::time::Instant::now()), Err(storage_failed()));
        let revoked = json!({"status":"needsReauthorization","persistence":"remembered","revision":"2"});
        assert_eq!(settled_status_read(|| Ok(revoked.clone()), || panic!("actual grant loss must be returned"), std::time::Instant::now()), Ok(revoked));
    }

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
        let service = Host { bookmark_store: Storage::new(fixture.base.clone()), ..Host::default() };
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
        assert_eq!(finish_selection(&service, registry, &owner, |_| None, || true, || Ok(Some(grant.clone()))).unwrap(),
            json!({"status":"granted","revision":"1","persistence":"session"}));
        assert!(!registry.is_execution_ticket_current(&old));
        let owner = reserve();
        assert_eq!(finish_selection(&service, registry, &owner, |_| None, || true, || Ok(None)).unwrap()["revision"], "1");
        assert!(grant.is_active());
        let owner = reserve();
        assert!(finish_selection(&service, registry, &owner, |_| None, || true, || Err(error("documentsAccessDenied"))).is_err());
        assert!(grant.is_active());
        let (token, cancel) = service.files.begin(&service.controller, registry, "A", 2, false).unwrap();
        change(&service, |retiring| service.documents.lock().unwrap()
            .replace(None, true, &service, registry, retiring)).unwrap();
        assert_eq!(service.documents.lock().unwrap().view(), json!({"status":"needsReauthorization","revision":"2","persistence":"none"}));
        assert!(!grant.is_active());
        assert!(cancel.check().is_err());
        assert!(registry.publish_if_latest(token, Vec::<((), ResultAction)>::new(), || true, |_, _| ()).is_none());
        change(&service, |retiring| service.documents.lock().unwrap()
            .replace(None, false, &service, registry, retiring)).unwrap();
        assert_eq!(service.documents.lock().unwrap().view(), json!({"status":"notGranted","revision":"3","persistence":"none"}));
    }

    #[test]
    fn late_choices_after_hide_reopen_or_shutdown_cannot_replace_current_owner() {
        let fixture = Fixture::new();
        let service = Host { bookmark_store: Storage::new(fixture.base.clone()), ..Host::default() };
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
        assert!(finish_selection(&service, registries.find(), &a, |_| None, || true,
            || panic!("old panel acquired a grant")).is_err());
        assert!(still_selecting(&service, &b));
        assert_eq!(service.documents.lock().unwrap().view()["revision"], "0");
        let discarded = fixture.grant();
        assert!(finish_selection(&service, registries.find(), &b, |_| None, || true, || {
            service.controller.shutdown();
            Ok(Some(discarded.clone()))
        }).is_err());
        assert!(!discarded.is_active());
        let grant = detach_on_exit(&service);
        drain_on_worker(&service, grant);
        assert!(!still_selecting(&service, &b));
        assert!(finish_selection(&service, registries.find(), &b, |_| None, || false, || Ok(None)).is_err());
    }

    fn fresh_host(fixture: &Fixture) -> (Host, ResultRegistries) {
        let service = Host { bookmark_store: Storage::new(fixture.base.clone()), ..Host::default() };
        let regs = ResultRegistries::default();
        service.controller.set_visible_for_test("current");
        regs.find().on_show("current".into());
        (service, regs)
    }

    #[test]
    fn no_restore_for_hidden_stale_or_empty_record() {
        let fixture = Fixture::new();
        let (service, regs) = fresh_host(&fixture);
        let no_native_call = |_: &[u8]| -> Result<Arc<DocumentsGrant>, FindError> {
            panic!("absent grant must not probe Documents")
        };
        change(&service, |r| {
            assert!(restore(&service, regs.find(), "old", r, || true, no_native_call).is_err());
            assert!(restore(&service, regs.find(), "current", r, || false, no_native_call).is_err());
            assert!(!fixture.base.join("find-documents-access").exists());
            restore(&service, regs.find(), "current", r, || true, no_native_call).unwrap();
        });
        assert_eq!(service.documents.lock().unwrap().view(),
            json!({"status":"notGranted","persistence":"none","revision":"0"}));
    }

    #[test]
    fn unavailable_signer_preserves_record_and_next_eligible_process_restores_it_once() {
        let fixture = Fixture::new();
        let bytes = vec![91; 32];
        {
            let (service, regs) = fresh_host(&fixture);
            let store = service.bookmark_store.store().unwrap();
            store.write(&Record::Bookmark(bytes.clone())).unwrap();
            change(&service, |r| restore(&service, regs.find(), "current", r, || true,
                |_| Err(FindError::Denied))).unwrap();
            block_if_needed(&service).unwrap();
            assert_eq!(service.documents.lock().unwrap().view(),
                json!({"status":"needsReauthorization","persistence":"remembered","revision":"1"}));
            assert_eq!(store.read().unwrap(), Record::Bookmark(bytes.clone()));
            assert_eq!(plan_record(store, None), Err(error("documentsRememberedAccessExists")));
            assert_eq!(store.read().unwrap(), Record::Bookmark(bytes.clone()));
        }
        let (service, regs) = fresh_host(&fixture);
        change(&service, |r| restore(&service, regs.find(), "current", r, || true,
            |record| { assert_eq!(record, &bytes); Ok(fixture.grant()) })).unwrap();
        assert_eq!(service.documents.lock().unwrap().view(),
            json!({"status":"granted","persistence":"remembered","revision":"1"}));
        let (token, cancel) = service.files.begin(&service.controller, regs.find(), "current", 1, false).unwrap();
        change(&service, |r| restore(&service, regs.find(), "current", r, || true,
            |_| panic!("second get cannot retire the first query"))).unwrap();
        assert!(cancel.check().is_ok());
        assert!(regs.find().publish_if_latest(token, Vec::<((), ResultAction)>::new(), || true, |_, _| ()).is_some());
        change(&service, |r| forget(&service, regs.find(), "current", r)).unwrap();
        drop(service);
        let (service, regs) = fresh_host(&fixture);
        change(&service, |r| restore(&service, regs.find(), "current", r, || true,
            |_| panic!("forgotten grant resurfaced after restart"))).unwrap();
        assert_eq!(service.documents.lock().unwrap().view()["status"], "notGranted");
    }

    #[test]
    fn stale_record_is_quarantined_but_failed_forget_is_not_acknowledged() {
        let fixture = Fixture::new();
        let (service, regs) = fresh_host(&fixture);
        let store = service.bookmark_store.store().unwrap();
        store.write(&Record::Bookmark(vec![1,2])).unwrap();
        change(&service, |r| restore(&service, regs.find(), "current", r, || true,
            |_| Err(FindError::Stale))).unwrap();
        block_if_needed(&service).unwrap();
        assert_eq!(store.read().unwrap(), Record::NeedsAuthorization);
        let obstacle = fixture.base.join("find-documents-access/grant.pending");
        fs::create_dir(&obstacle).unwrap();
        assert_eq!(change(&service, |r| forget(&service, regs.find(), "current", r)), Err(storage_failed()));
        assert_eq!(service.documents.lock().unwrap().view()["status"], "needsReauthorization");
        assert_eq!(service.documents.lock().unwrap().view()["persistence"], "remembered");
        assert_eq!(store.read().unwrap(), Record::NeedsAuthorization);
        fs::remove_dir(obstacle).unwrap();
        let forgotten = change(&service, |r| forget(&service, regs.find(), "current", r)).unwrap();
        assert_eq!(forgotten["persistence"], "none");
        assert_eq!(store.read().unwrap(), Record::Forgotten);
    }

    #[test]
    fn explicit_remember_publishes_only_while_the_sheet_owns_the_invocation() {
        let fixture = Fixture::new();
        let (service, regs) = fresh_host(&fixture);
        let first = service.documents.lock().unwrap().reserve("current").unwrap();
        let bytes = vec![4; 64];
        let view = finish_selection(&service, regs.find(), &first, |_| Some(bytes.clone()), || true,
            || Ok(Some(fixture.grant()))).unwrap();
        assert_eq!(view, json!({"status":"granted","revision":"1","persistence":"remembered"}));
        let store = service.bookmark_store.store().unwrap();
        assert_eq!(store.read().unwrap(), Record::Bookmark(bytes.clone()));

        let replacement = service.documents.lock().unwrap().reserve("current").unwrap();
        let candidate = fixture.grant();
        let outcome = finish_selection(&service, regs.find(), &replacement, |_| {
            service.controller.finish_native_close(&regs);
            Some(vec![5; 64])
        }, || true, || Ok(Some(candidate.clone())));
        assert_eq!(outcome, Err(stale()));
        assert!(!candidate.is_active());
        assert_eq!(store.read().unwrap(), Record::Bookmark(bytes));
        assert!(!fixture.base.join("find-documents-access/grant.pending").exists());
    }

    #[test]
    fn unusable_new_bookmark_falls_back_only_with_durable_absence_of_an_old_one() {
        let fixture = Fixture::new();
        let (service, regs) = fresh_host(&fixture);
        let owner = service.documents.lock().unwrap().reserve("current").unwrap();
        let view = finish_selection(&service, regs.find(), &owner, |_| Some(Vec::new()), || true,
            || Ok(Some(fixture.grant()))).unwrap();
        assert_eq!(view["persistence"], "session");
        let store = service.bookmark_store.store().unwrap();
        assert_eq!(store.read().unwrap(), Record::Forgotten);
        store.write(&Record::Bookmark(vec![7])).unwrap();
        assert_eq!(plan_record(store, None), Err(error("documentsRememberedAccessExists")));
        assert_eq!(store.read().unwrap(), Record::Bookmark(vec![7]));
    }

}
