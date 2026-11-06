use super::{check_find, error, failed, host, on_main, registries, stale, Response};
use crate::{
    find_window::{ExecutionHideAdmission, FindWindowController},
    macos_find_query::{gather_on_worker, ApprovedScope, GatherError, QueryCancellation},
    macos_find_results::{authorize_batch, submit_action, AuthorizedBatch, AuthorizedFile, FindAction, FindError, IndexedLocalScope, DocumentsGrant, Submission},
    result_registry::{ExecutionTicket, QueryDomain, QueryToken, RegistryError, ResultAction, ResultRegistry},
};
use serde_json::{json, Value};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use tauri::{Manager, WebviewWindow};

struct Search { id: String, sequence: u64, cancel: QueryCancellation }

struct Published {
    request: String,
    invocation: String,
    scope: Arc<IndexedLocalScope>,
    targets: Vec<AuthorizedFile>,
    dispatch: Dispatch,
}

#[derive(Default, Debug, Eq, PartialEq)]
enum Dispatch {
    #[default]
    Available,
    InFlight,
    Consumed,
}

impl Dispatch {
    fn claim(&mut self) -> bool {
        if *self != Self::Available { return false; }
        *self = Self::InFlight;
        true
    }
    fn finish(&mut self) {
        if *self == Self::InFlight { *self = Self::Consumed; }
    }
}

#[derive(Default)]
struct Rows {
    search: Option<Search>,
    current: Option<Published>,
    revision: u64,
}

impl Rows {
    fn clear(&mut self) {
        if let Some(old) = self.search.take() { old.cancel.cancel(); }
        self.current = None;
    }
    fn owns(&self, id: &str, seq: u64) -> bool {
        self.search.as_ref().is_some_and(|old| old.id == id && old.sequence == seq)
    }
}

#[derive(Default)]
pub(super) struct Files {
    rows: Mutex<Rows>,
    serial_search: Arc<tokio::sync::Mutex<()>>,
}

impl Files {
    pub(super) fn clear(&self) { self.rows.lock().expect("Find rows").clear(); }
    pub(super) fn dispatching(&self, id: &str) -> bool {
        self.rows.lock().expect("Find rows").current.as_ref()
            .is_some_and(|p| p.invocation == id && p.dispatch == Dispatch::InFlight)
    }

    pub(super) fn begin(&self, controller: &FindWindowController, registry: &ResultRegistry, id: &str, seq: u64,
        cancel_only: bool) -> Option<(QueryToken, QueryCancellation)> {
        controller.with_visible_admission(id, || {
            let mut rows = self.rows.lock().expect("Find rows");
            let token = registry.begin_query(QueryDomain::File, id, seq)?;
            rows.clear();
            let cancel = QueryCancellation::default();
            if !cancel_only {
                rows.search = Some(Search { id: id.into(), sequence: seq, cancel: cancel.clone() });
            }
            Some((token, cancel))
        }).flatten()
    }
}

fn query_error(error_code: FindError) -> Value {
    error(match error_code {
        FindError::InvalidInput => "invalidFileQuery",
        FindError::Stale | FindError::Cancelled => "staleRequest",
        FindError::Denied => "fileOpenFailed",
        FindError::Unavailable | FindError::TimedOut => "searchUnavailable",
    })
}

fn collect(query: &str, category: &str, cancel: &QueryCancellation, grant: Option<Arc<DocumentsGrant>>) -> Result<(Arc<IndexedLocalScope>, AuthorizedBatch), FindError> {
    let scope = Arc::new(IndexedLocalScope::with_documents(grant)?);
    let candidates = gather_on_worker(ApprovedScope::IndexedLocal(&scope), query, cancel)
        .map_err(|e| match e { GatherError::Cancelled => FindError::Cancelled,
            GatherError::InvalidInput => FindError::InvalidInput, GatherError::TimedOut => FindError::TimedOut,
            GatherError::ServiceUnavailable => FindError::Unavailable })?;
    let rows = authorize_batch(&scope, candidates, category, cancel)?;
    Ok((scope, rows))
}

#[tauri::command]
pub(crate) async fn search_files(window: WebviewWindow, query: String, category: String, sort: String,
    invocation_id: String, query_sequence: u64) -> Response<Option<Value>> {
    check_find(&window)?;
    if !window.is_visible().unwrap_or(false) { return Ok(None); }
    let service = host(window.app_handle());
    let registries = registries(window.app_handle());
    if invocation_id.is_empty() || query_sequence == 0 || query.trim().is_empty() || query.len() > 1024 || query.chars().count() > 255 || query.contains('\0')
        || sort != "modifiedDesc" || !matches!(category.as_str(), "all"|"folder"|"excel"|"word"|"ppt"|"pdf"|"image"|"video"|"audio"|"archive") {
        return Err(error("invalidFileQuery"));
    }
    // A lazy grant restoration changes the file domain epoch. Resolve it before
    // reserving the first query so a concurrently mounting Find keeps its search.
    #[cfg(not(feature = "macos-find-system-qa"))]
    {
        let app = window.app_handle().clone();
        let id = invocation_id.clone();
        tauri::async_runtime::spawn_blocking(move || super::documents::snapshot(&app, &id))
            .await.map_err(|_| error("fileSearchWorkerFailed"))??;
    }
    let Some((token, cancel)) = service.files.begin(&service.controller, registries.find(), &invocation_id, query_sequence, false)
        else { return Ok(None); };
    let serial = service.files.serial_search.clone().lock_owned().await;
    if !service.files.rows.lock().expect("Find rows").owns(&invocation_id, query_sequence) { return Ok(None); }
    #[cfg(not(feature = "macos-find-system-qa"))]
    let scope_owner = (window.app_handle().clone(), invocation_id.clone());
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let _serial = serial;
        objc2::rc::autoreleasepool(|_| {
            #[cfg(not(feature = "macos-find-system-qa"))]
            let grant = super::documents::snapshot(&scope_owner.0, &scope_owner.1)?.1;
            #[cfg(feature = "macos-find-system-qa")]
            let grant = None;
            let outcome = collect(&query, &category, &cancel, grant).map_err(query_error);
            #[cfg(not(feature = "macos-find-system-qa"))]
            super::documents::snapshot(&scope_owner.0, &scope_owner.1)?;
            outcome
        })
    }).await.map_err(|_| error("fileSearchWorkerFailed"))?;
    if !window.is_visible().unwrap_or(false) { return Ok(None); }
    service.controller.with_visible_admission(&invocation_id, || {
        let mut rows = service.files.rows.lock().expect("Find rows");
        if !rows.owns(&invocation_id, query_sequence) { return Ok(None); }
        rows.search = None;
        let (scope, batch) = outcome?;
        let revision = rows.revision.checked_add(1).ok_or_else(failed)?;
        let mut targets = Vec::with_capacity(batch.rows.len());
        let entries = batch.rows.into_iter().map(|row| {
            let index = targets.len();
            targets.push(row.target);
            let item = json!({"name":row.name,"fullPath":row.full_path,"modifiedUtc":row.modified_utc,
                "kind":if row.is_directory {"folder"} else {"file"},"sizeBytes":row.size_bytes.map(|n|n.to_string()),
                "canOpenDocument":row.can_open_document});
            (item, ResultAction::MacFindRow(index))
        }).collect();
        Ok(registries.find().publish_if_latest(token, entries, || true, |request, items| {
            let items: Vec<_> = items.into_iter().map(|(id, mut item)| {
                item["resultId"] = json!(id);
                item
            }).collect();
            rows.revision = revision;
            rows.current = Some(Published { request: request.clone(), invocation: invocation_id.clone(),
                scope, targets, dispatch: Dispatch::Available });
            json!({"requestId":request,"indexRevision":revision.to_string(),"total":items.len().to_string(),
                "status":if batch.partial {"partial"} else {"ready"},"items":items})
        }))
    }).unwrap_or(Ok(None))
}

#[tauri::command]
pub(crate) fn cancel_find_query(window: WebviewWindow, invocation_id: String, query_sequence: u64) -> Response<()> {
    check_find(&window)?;
    let app = window.app_handle();
    let service = host(app);
    service.files.begin(&service.controller, registries(app).find(), &invocation_id, query_sequence, true);
    Ok(())
}

fn resolve(service: &super::Host, registry: &ResultRegistry, request: &str, result: &str)
    -> Response<(Arc<IndexedLocalScope>, AuthorizedFile, ExecutionTicket)> {
    let id = service.controller.current_invocation().ok_or_else(stale)?;
    service.controller.with_visible_admission(&id, || {
        let rows = service.files.rows.lock().expect("Find rows");
        let (action, ticket) = registry.resolve_with_ticket(request, result)
            .map_err(|e| if e == RegistryError::UnknownResult { error("unknownResult") } else { stale() })?;
        let ResultAction::MacFindRow(index) = action else { return Err(stale()); };
        let published = rows.current.as_ref().filter(|p| p.request == request && p.dispatch == Dispatch::Available).ok_or_else(stale)?;
        let target = published.targets.get(index).ok_or_else(stale)?.clone();
        Ok((published.scope.clone(), target, ticket))
    }).unwrap_or_else(|| Err(stale()))
}

pub(crate) async fn execute_file(window: WebviewWindow, request_id: String, result_id: String, document: bool) -> Response<Value> {
    check_find(&window)?;
    if !window.is_visible().unwrap_or(false) { return Err(stale()); }
    let app = window.app_handle().clone();
    let service = host(&app);
    let registry = registries(&app).find().clone();
    let (scope, target, ticket) = resolve(&service, &registry, &request_id, &result_id)?;
    let owner = ticket.clone();
    let final_request = request_id.clone();
    let claimed = Arc::new(AtomicBool::new(false));
    let worker_claim = claimed.clone();
    let action = if document { FindAction::OpenDocument } else { FindAction::Default };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(not(feature = "macos-find-system-qa"))]
        super::documents::snapshot(window.app_handle(), owner.invocation_id()).map_err(|_| FindError::Stale)?;
        submit_action(&scope, &target, action, || {
            if !window.is_visible().unwrap_or(false) { return false; }
            service.controller.with_visible_admission(owner.invocation_id(), || {
                let mut rows = service.files.rows.lock().expect("Find rows");
                let Some(p) = rows.current.as_mut().filter(|p| p.request == request_id && p.dispatch == Dispatch::Available) else { return false; };
                if !registry.is_execution_ticket_current(&owner) { return false; }
                let accepted = p.dispatch.claim();
                worker_claim.store(accepted, Ordering::Release);
                accepted
            }).unwrap_or(false)
        })
    }).await.map_err(|_| error("fileOpenFailed"));
    on_main(&app, move |app| {
        let service = host(app);
        let registries = registries(app);
        let registry = registries.find();
        // Clear only this dispatch. A newer published set may be in flight
        // already. A consumed result can never submit a second OS operation.
        if claimed.load(Ordering::Acquire) {
            if let Some(published) = service.files.rows.lock().expect("Find rows").current.as_mut()
                .filter(|p| p.request == final_request) {
                published.dispatch.finish();
            }
        }
        let outcome = outcome?.map_err(query_error)?;
        if outcome == Submission::Unconfirmed {
            registry.retire_result_set_if_current(&ticket);
            return Err(error("fileActionUnconfirmed"));
        }
        match service.controller.begin_execution_hide(&ticket, registry) {
            ExecutionHideAdmission::Started => {
                #[cfg(not(feature = "macos-find-system-qa"))]
                super::documents::retire_panel(&service);
                let hidden = app.get_webview_window("find").is_some_and(|w| w.hide().is_ok());
                service.controller.finish_execution_hide(hidden, &registries);
                if hidden { service.files.clear(); }
                super::schedule(app);
            }
            ExecutionHideAdmission::Pinned => { registry.retire_result_set_if_current(&ticket); }
            ExecutionHideAdmission::Stale => {}
        }
        Ok(json!({"status":match outcome { Submission::FileRevealRequested => "fileRevealRequested",
            Submission::FolderOpenRequested => "folderOpenRequested", Submission::FileOpenRequested => "fileOpenRequested",
            Submission::Unconfirmed => unreachable!() }}))
    }).await?
}

#[tauri::command]
pub(crate) async fn open_find_document(window: WebviewWindow, request_id: String, result_id: String) -> Response<Value> {
    execute_file(window, request_id, result_id, true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_registry::ResultRegistries;

    #[test]
    fn consumed_dispatch_releases_focus_but_cannot_submit_again() {
        let mut state = Dispatch::default();
        assert!(state.claim());
        assert_eq!(state, Dispatch::InFlight);
        assert!(!state.claim());
        state.finish();
        assert_eq!(state, Dispatch::Consumed);
        assert!(!state.claim());
        state.finish();
        assert_eq!(state, Dispatch::Consumed);
    }

    #[test]
    fn clearing_or_hiding_retires_tickets_and_only_cancels_its_current_search() {
        let files = Files::default();
        let ctl = FindWindowController::default();
        let regs = ResultRegistries::default();
        let find = regs.find();
        ctl.set_visible_for_test("find-test");
        find.on_show("find-test".into());
        let (token, first) = files.begin(&ctl, find, "find-test", 1, false).unwrap();
        let (request, id) = find.publish_if_latest(token, vec![((), ResultAction::MacFindRow(0))],
            || true, |r, items| (r, items[0].0.clone())).unwrap();
        let (_, ticket) = find.resolve_with_ticket(&request, &id).unwrap();
        let (_, second) = files.begin(&ctl, find, "find-test", 2, false).unwrap();
        assert!(first.check().is_err());
        assert!(!find.is_execution_ticket_current(&ticket));
        assert!(files.begin(&ctl, find, "find-test", 1, true).is_none());
        assert!(second.check().is_ok());
        assert!(files.begin(&ctl, find, "find-test", 3, true).is_some());
        assert!(second.check().is_err());
        let (late_token, third) = files.begin(&ctl, find, "find-test", 4, false).unwrap();
        assert!(ctl.finish_explicit_hide("find-test", true, &regs));
        files.clear();
        assert!(third.check().is_err());
        assert!(find.publish_if_latest(late_token, vec![((), ResultAction::MacFindRow(0))],
            || true, |_, _| ()).is_none());
        assert!(files.begin(&ctl, find, "find-test", 5, false).is_none());
    }

    #[test]
    fn shutdown_prevents_pending_search_publication_and_new_admission() {
        let files = Files::default();
        let ctl = FindWindowController::default();
        let regs = ResultRegistries::default();
        ctl.set_visible_for_test("closing");
        regs.find().on_show("closing".into());
        let (token, cancel) = files.begin(&ctl, regs.find(), "closing", 1, false).unwrap();
        ctl.shutdown();
        files.clear();
        regs.find().hide_and_clear();
        assert!(cancel.check().is_err());
        assert!(files.begin(&ctl, regs.find(), "closing", 2, false).is_none());
        assert!(regs.find().publish_if_latest(token, vec![((), ResultAction::MacFindRow(0))],
            || true, |_, _| ()).is_none());
    }
}
