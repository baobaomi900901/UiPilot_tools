//! Unassembled authorization coordinator. Bind a batch BEFORE native reading;
//! ingest never upgrades an old event to newly granted authority.
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, Ordering}, Arc, Condvar, Mutex, MutexGuard, Weak}};
use super::{macos_observer::{AuthorizationRevision, CapturedEvent, ClipboardEvent, ReadBinding, ReadOutcome}, macos_store::{valid_id, MacTextStore, RootIdentity, StoreError}, ClipboardHistorySnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HistoryError { PermissionDenied, Stopped, Storage, InvalidQuery }
impl From<StoreError> for HistoryError { fn from(error: StoreError) -> Self { match error { StoreError::InvalidQuery => Self::InvalidQuery, _ => Self::Storage } } }
type Result<T> = std::result::Result<T, HistoryError>;
fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> { value.lock().unwrap_or_else(|error| error.into_inner()) }
struct Epoch { active: AtomicBool, gate: Mutex<()>, read: Arc<ReadGate> }
struct ReadState { active: bool, in_flight: usize }
struct ReadGate { state: Mutex<ReadState>, drained: Condvar }
impl ReadGate {
    fn new() -> Self { Self { state: Mutex::new(ReadState { active: true, in_flight: 0 }), drained: Condvar::new() } }
    fn admit(self: &Arc<Self>) -> Option<ReadPermit> {
        let mut state = lock(&self.state);
        if !state.active { return None; }
        state.in_flight += 1;
        Some(ReadPermit(Arc::clone(self)))
    }
    fn close(&self) { lock(&self.state).active = false; }
    fn drain(&self) {
        let mut state = lock(&self.state);
        while state.in_flight != 0 { state = self.drained.wait(state).unwrap_or_else(|e| e.into_inner()); }
    }
}
struct ReadPermit(Arc<ReadGate>);
impl Drop for ReadPermit {
    fn drop(&mut self) {
        lock(&self.0.state).in_flight -= 1;
        self.0.drained.notify_all();
    }
}
/// Owns only read gates; never a history store, directory or observer.
pub(super) struct CaptureReadPermit(Vec<ReadPermit>);

/// Tokens contain no store or native descriptor and cannot keep flock alive.
#[derive(Clone)]
pub(super) struct AccessToken { owner: Weak<Inner>, id: String, epoch: Arc<Epoch> }
/// Single-use by ownership. Obtain before reading, never in the post-read callback.
pub(super) struct CaptureBatch { owner: Weak<Inner>, targets: Vec<AccessToken> }
#[derive(Default, Debug, Eq, PartialEq)]
pub(super) struct IngestReport { pub(super) accepted: usize, pub(super) denied: usize, pub(super) failed: usize }
struct State {
    stopped: bool,
    revision: AuthorizationRevision,
    active: BTreeMap<String, Arc<Epoch>>,
    stores: BTreeMap<String, Arc<Mutex<MacTextStore>>>,
}
struct Inner { location: PathBuf, root_identity: RootIdentity, lifecycle: Mutex<()>, state: Mutex<State> }
pub(super) struct MacHistory { inner: Arc<Inner> }

impl MacHistory {
    pub(super) fn open(location: &Path) -> Result<Self> {
        let root_identity = MacTextStore::prepare_root(location)?;
        Ok(Self { inner: Arc::new(Inner { location: location.to_owned(), root_identity, lifecycle: Mutex::new(()),
            state: Mutex::new(State { stopped: false, revision: AuthorizationRevision::new(), active: BTreeMap::new(), stores: BTreeMap::new() }) }) })
    }
    pub(super) fn authorize(&self, id: &str) -> Result<AccessToken> {
        if !valid_id(id) { return Err(HistoryError::PermissionDenied); }
        let _lifecycle = lock(&self.inner.lifecycle);
        {
            let state = lock(&self.inner.state);
            if state.stopped { return Err(HistoryError::Stopped); }
            if let Some(epoch) = state.active.get(id) {
                return Ok(AccessToken { owner: Arc::downgrade(&self.inner), id: id.into(), epoch: Arc::clone(epoch) });
            }
        }
        let needs_store = !lock(&self.inner.state).stores.contains_key(id);
        if needs_store {
            let store = MacTextStore::open_bound(&self.inner.location, id, self.inner.root_identity)?;
            lock(&self.inner.state).stores.insert(id.into(), Arc::new(Mutex::new(store)));
        }
        let epoch = Arc::new(Epoch { active: AtomicBool::new(true), gate: Mutex::new(()), read: Arc::new(ReadGate::new()) });
        {
            let mut state = lock(&self.inner.state);
            state.active.insert(id.into(), Arc::clone(&epoch));
            state.revision = AuthorizationRevision::new();
        }
        Ok(AccessToken { owner: Arc::downgrade(&self.inner), id: id.into(), epoch })
    }
    pub(super) fn prepare_read(&self) -> Result<ReadBinding<CaptureBatch, CaptureReadPermit>> {
        let state = lock(&self.inner.state);
        if state.stopped { return Err(HistoryError::Stopped); }
        let revision = state.revision.clone();
        if state.active.is_empty() { return Ok(ReadBinding::new(revision, None)); }
        let mut targets = Vec::with_capacity(state.active.len());
        let mut permits = Vec::with_capacity(state.active.len());
        for (id, epoch) in &state.active {
            permits.push(epoch.read.admit().ok_or(HistoryError::PermissionDenied)?);
            targets.push(AccessToken { owner: Arc::downgrade(&self.inner), id: id.clone(), epoch: Arc::clone(epoch) });
        }
        Ok(ReadBinding::new(revision, Some((CaptureBatch { owner: Arc::downgrade(&self.inner), targets }, CaptureReadPermit(permits)))))
    }
    #[cfg(test)]
    pub(super) fn bind_capture(&self) -> Result<CaptureBatch> {
        let state = lock(&self.inner.state);
        if state.stopped { return Err(HistoryError::Stopped); }
        if state.active.is_empty() { return Err(HistoryError::PermissionDenied); }
        Ok(CaptureBatch { owner: Arc::downgrade(&self.inner), targets: state.active.iter().map(|(id, epoch)|
            AccessToken { owner: Arc::downgrade(&self.inner), id: id.clone(), epoch: Arc::clone(epoch) }).collect() })
    }
    fn with_token<T>(&self, token: &AccessToken, action: impl FnOnce(&mut MacTextStore) -> std::result::Result<T, StoreError>) -> Result<T> {
        if !Weak::ptr_eq(&token.owner, &Arc::downgrade(&self.inner)) || !token.epoch.active.load(Ordering::Acquire) {
            return Err(HistoryError::PermissionDenied);
        }
        let _admission = lock(&token.epoch.gate);
        if !token.epoch.active.load(Ordering::Acquire) { return Err(HistoryError::PermissionDenied); }
        let store = {
            let state = lock(&self.inner.state);
            if state.stopped { return Err(HistoryError::Stopped); }
            if !state.active.get(&token.id).is_some_and(|epoch| Arc::ptr_eq(epoch, &token.epoch)) {
                return Err(HistoryError::PermissionDenied);
            }
            state.stores.get(&token.id).cloned().ok_or(HistoryError::Storage)?
        };
        // Drop the store reference before releasing admission so drain also bounds FD lifetime.
        let result = {
            let mut store = store.lock().map_err(|_| HistoryError::Storage)?;
            action(&mut store).map_err(HistoryError::from)
        };
        drop(store);
        result
    }
    pub(super) fn snapshot(&self, token: &AccessToken, query: Option<&str>) -> Result<ClipboardHistorySnapshot> {
        self.with_token(token, |store| store.snapshot(query))
    }
    pub(super) fn remove(&self, token: &AccessToken, id: &str) -> Result<bool> {
        self.with_token(token, |store| store.remove(id))
    }
    pub(super) fn clear(&self, token: &AccessToken) -> Result<()> {
        self.with_token(token, MacTextStore::clear)
    }
    pub(super) fn ingest(&self, event: CapturedEvent<CaptureBatch>) -> Result<IngestReport> {
        event.with_current(|outcome, batch, captured_at| self.ingest_contents(outcome, batch, captured_at))
            .ok_or(HistoryError::PermissionDenied)?
    }
    #[cfg(test)]
    fn ingest_unbound_for_test(&self, event: &ClipboardEvent, batch: CaptureBatch) -> Result<IngestReport> {
        self.ingest_at(event, batch, "2026-11-07T00:00:00Z")
    }
    #[cfg(test)]
    fn ingest_at(&self, event: &ClipboardEvent, batch: CaptureBatch, captured_at: &str) -> Result<IngestReport> {
        event.with_current(|outcome| self.ingest_contents(outcome, batch, captured_at))
            .ok_or(HistoryError::PermissionDenied)?
    }
    fn ingest_contents(&self, outcome: &ReadOutcome, batch: CaptureBatch, captured_at: &str) -> Result<IngestReport> {
        if !Weak::ptr_eq(&batch.owner, &Arc::downgrade(&self.inner)) { return Err(HistoryError::PermissionDenied); }
        if lock(&self.inner.state).stopped { return Err(HistoryError::Stopped); }
        let ReadOutcome::Text { text, .. } = outcome else { return Ok(IngestReport::default()); };
        if text.len() > super::macos_store::MAX_TEXT { return Err(HistoryError::Storage); }
        let mut report = IngestReport::default();
        for token in batch.targets {
            match self.with_token(&token, |store| store.capture(text.clone(), captured_at.into())) {
                Ok(_) => report.accepted += 1,
                Err(HistoryError::PermissionDenied | HistoryError::Stopped) => report.denied += 1,
                Err(HistoryError::Storage | HistoryError::InvalidQuery) => report.failed += 1,
            }
        }
        Ok(report)
    }
    pub(super) fn revoke(&self, id: &str) -> Result<()> {
        if !valid_id(id) { return Err(HistoryError::PermissionDenied); }
        let _lifecycle = lock(&self.inner.lifecycle);
        let retired = {
            let mut state = lock(&self.inner.state);
            if state.stopped { return Err(HistoryError::Stopped); }
            let retired = state.active.remove(id);
            if let Some(epoch) = &retired {
                epoch.active.store(false, Ordering::Release);
                epoch.read.close();
                state.revision = AuthorizationRevision::new();
            }
            retired
        };
        if let Some(epoch) = retired { epoch.read.drain(); drop(lock(&epoch.gate)); }
        let store = lock(&self.inner.state).stores.remove(id);
        drop(store);
        Ok(())
    }
    pub(super) fn shutdown(&self) {
        let _lifecycle = lock(&self.inner.lifecycle);
        let epochs = {
            let mut state = lock(&self.inner.state);
            state.stopped = true;
            state.revision = AuthorizationRevision::new();
            let active = std::mem::take(&mut state.active);
            for epoch in active.values() { epoch.active.store(false, Ordering::Release); epoch.read.close(); }
            active.into_values().collect::<Vec<_>>()
        };
        for epoch in epochs { epoch.read.drain(); drop(lock(&epoch.gate)); }
        lock(&self.inner.state).stores.clear();
    }
}
impl Drop for MacHistory { fn drop(&mut self) { self.shutdown(); } }

#[cfg(test)]
#[path = "macos_history_tests.rs"]
mod tests;
