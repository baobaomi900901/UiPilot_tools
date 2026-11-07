//! Native text-only observer, intentionally not installed by the production host.
//! No NSPasteboard object crosses threads. Consumers must act inside with_current.
use std::{
    cell::RefCell,
    sync::{mpsc, Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::Duration,
};

use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSString, NSUTF8StringEncoding};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_TYPES: usize = 32;
const MAX_TYPE_BYTES: usize = 256;
const TEXT_TYPE: &str = "public.utf8-plain-text";

/// Native NSInteger, opaque and compared only for equality. Zero is valid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ChangeCount(isize);

#[derive(Eq, PartialEq)]
pub(super) enum ReadOutcome {
    Text { change_count: ChangeCount, text: String },
    Unsupported,
    Unavailable,
    TooLarge,
    ChangedDuringRead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StartError { Unavailable, WorkerFailed }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StopOutcome {
    /// The worker has joined and all previously admitted consumers have returned.
    Stopped,
    /// Admission is retired, but this thread cannot wait on its own callback.
    RequestedFromCallback,
    /// Admission is retired and drained; the worker panicked.
    WorkerFailed,
}

/// Holds the payload privately so a queued message must re-enter admission.
pub(super) struct ClipboardEvent {
    session: Arc<Session>,
    outcome: ReadOutcome,
}

impl ClipboardEvent {
    #[cfg(test)]
    pub(super) fn synthetic_text(text: &str) -> Self {
        Self { session: Arc::new(Session::new()), outcome: ReadOutcome::Text {
            change_count: ChangeCount(0), text: text.into() } }
    }

    /// The closure runs synchronously. Do not queue side effects outside it.
    pub(super) fn with_current<T>(&self, action: impl FnOnce(&ReadOutcome) -> T) -> Option<T> {
        let _admission = self.session.admit()?;
        Some(action(&self.outcome))
    }

    pub(super) fn request_stop(&self) { self.session.request_stop(); }
}

#[derive(Clone)]
pub(super) struct ObservationLease(Arc<Session>);
impl ObservationLease {
    /// Retires admission immediately; does not wait for native reads or consumers.
    pub(super) fn request_stop(&self) { self.0.request_stop(); }
    pub(super) fn is_active(&self) -> bool { lock(&self.0.state).active }
}

struct SessionState { active: bool, in_flight: usize }
struct Session { state: Mutex<SessionState>, wake: Condvar }
thread_local! { static ADMITTED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) }; }

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Session {
    fn new() -> Self {
        Self { state: Mutex::new(SessionState { active: true, in_flight: 0 }), wake: Condvar::new() }
    }
    fn key(&self) -> usize { self as *const Self as usize }
    fn on_admitted_thread(&self) -> bool {
        ADMITTED.with(|sessions| sessions.borrow().contains(&self.key()))
    }
    fn admit(&self) -> Option<Admission<'_>> {
        let mut state = lock(&self.state);
        if !state.active { return None; }
        state.in_flight += 1;
        drop(state);
        ADMITTED.with(|sessions| sessions.borrow_mut().push(self.key()));
        Some(Admission(self))
    }
    fn request_stop(&self) {
        lock(&self.state).active = false;
        self.wake.notify_all();
    }
    fn wait_tick(&self, interval: Duration) -> bool {
        let state = lock(&self.state);
        if !state.active { return false; }
        let (state, _) = self.wake.wait_timeout(state, interval)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active
    }
    fn wait_drained(&self) {
        let mut state = lock(&self.state);
        while state.in_flight != 0 {
            state = self.wake.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

struct Admission<'a>(&'a Session);
impl Drop for Admission<'_> {
    fn drop(&mut self) {
        ADMITTED.with(|sessions| {
            let key = sessions.borrow_mut().pop();
            debug_assert_eq!(key, Some(self.0.key()));
        });
        lock(&self.0.state).in_flight -= 1;
        self.0.wake.notify_all();
    }
}

struct WorkerLifetime(Arc<Session>);
impl Drop for WorkerLifetime {
    fn drop(&mut self) { self.0.request_stop(); }
}
struct JoinState { handle: Option<JoinHandle<()>>, outcome: Option<StopOutcome> }

pub(super) struct MacClipboardObserver {
    session: Arc<Session>,
    worker_id: thread::ThreadId,
    join: Mutex<JoinState>,
    joined: Condvar,
}

impl MacClipboardObserver {
    /// This is deliberately unassembled. Starting it is a future explicit host action.
    pub(super) fn start(callback: impl FnMut(ClipboardEvent) + Send + 'static) -> Result<Self, StartError> {
        Self::start_with(|| Ok(NativeSource(NSPasteboard::generalPasteboard())), POLL_INTERVAL, callback)
    }

    fn start_with<S: Source + 'static>(
        factory: impl FnOnce() -> Result<S, StartError> + Send + 'static,
        interval: Duration,
        mut callback: impl FnMut(ClipboardEvent) + Send + 'static,
    ) -> Result<Self, StartError> {
        let session = Arc::new(Session::new());
        let worker_session = Arc::clone(&session);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let handle = thread::Builder::new().name("uipilot-macos-clipboard".into()).spawn(move || autoreleasepool(|_| {
            let _lifetime = WorkerLifetime(Arc::clone(&worker_session));
            // Factory and baseline run on the owning worker, never on the caller.
            let initialized = autoreleasepool(|_| {
                let source = factory()?;
                let baseline = source.change_count().map_err(|_| StartError::Unavailable)?;
                Ok::<_, StartError>((source, baseline))
            });
            let (mut source, mut last_count) = match initialized {
                Ok(value) => value,
                Err(error) => { let _ = ready_tx.send(Err(error)); return; }
            };
            if ready_tx.send(Ok(())).is_err() { return; }
            while worker_session.wait_tick(interval) {
                autoreleasepool(|_| {
                    let Ok(count) = source.change_count() else { return; };
                    if count == last_count { return; }
                    // At most one attempt per observed count, including transient failure.
                    last_count = count;
                    let outcome = {
                        let Some(_reading) = worker_session.admit() else { return; };
                        sample(&mut source, count)
                    };
                    if let Some(_admission) = worker_session.admit() {
                        callback(ClipboardEvent { session: Arc::clone(&worker_session), outcome });
                    }
                });
            }
        })).map_err(|_| StartError::Unavailable)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                session,
                worker_id: handle.thread().id(),
                join: Mutex::new(JoinState { handle: Some(handle), outcome: None }),
                joined: Condvar::new(),
            }),
            result => {
                session.request_stop();
                let joined = handle.join();
                Err(if joined.is_err() { StartError::WorkerFailed }
                    else { result.ok().and_then(Result::err).unwrap_or(StartError::WorkerFailed) })
            }
        }
    }

    pub(super) fn lease(&self) -> ObservationLease { ObservationLease(Arc::clone(&self.session)) }
    pub(super) fn request_stop(&self) { self.session.request_stop(); }

    /// Call without host/state locks. Native providers may block; no time bound is promised.
    pub(super) fn stop(&self) -> StopOutcome {
        self.request_stop();
        if thread::current().id() == self.worker_id || self.session.on_admitted_thread() {
            return StopOutcome::RequestedFromCallback;
        }
        let mut state = lock(&self.join);
        if let Some(handle) = state.handle.take() {
            drop(state);
            let result = handle.join();
            self.session.wait_drained();
            let outcome = if result.is_ok() { StopOutcome::Stopped } else { StopOutcome::WorkerFailed };
            lock(&self.join).outcome = Some(outcome);
            self.joined.notify_all();
            outcome
        } else {
            loop {
                if let Some(outcome) = state.outcome { return outcome; }
                state = self.joined.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
    }
}
impl Drop for MacClipboardObserver {
    fn drop(&mut self) { let _ = self.stop(); }
}

trait Source {
    fn change_count(&self) -> Result<ChangeCount, ()>;
    fn read_content(&mut self) -> Content;
}
#[derive(Eq, PartialEq)]
enum Content { Text(String), Unsupported, Unavailable, TooLarge }

fn sample(source: &mut impl Source, expected: ChangeCount) -> ReadOutcome {
    match source.change_count() {
        Ok(count) if count == expected => {},
        Ok(_) => return ReadOutcome::ChangedDuringRead,
        Err(()) => return ReadOutcome::Unavailable,
    }
    let content = source.read_content();
    match source.change_count() {
        Ok(count) if count == expected => {},
        Ok(_) => return ReadOutcome::ChangedDuringRead,
        Err(()) => return ReadOutcome::Unavailable,
    }
    match content {
        Content::Text(text) => ReadOutcome::Text { change_count: expected, text },
        Content::Unsupported => ReadOutcome::Unsupported,
        Content::Unavailable => ReadOutcome::Unavailable,
        Content::TooLarge => ReadOutcome::TooLarge,
    }
}

// Intentionally neither Send nor Sync. Created and released on its worker.
struct NativeSource(Retained<NSPasteboard>);
impl Source for NativeSource {
    fn change_count(&self) -> Result<ChangeCount, ()> { Ok(ChangeCount(self.0.changeCount())) }
    fn read_content(&mut self) -> Content {
        let Some(items) = self.0.pasteboardItems() else { return Content::Unavailable; };
        let classification = classify_offer(items.len(), || {
            let types = items.objectAtIndex(0).types();
            if types.len() > MAX_TYPES { return Err(Content::TooLarge); }
            let mut names = Vec::with_capacity(types.len());
            for kind in types.iter() {
                if kind.length() > MAX_TYPE_BYTES
                    || kind.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) > MAX_TYPE_BYTES {
                    return Err(Content::TooLarge);
                }
                names.push(kind.to_string());
            }
            Ok(names)
        });
        if let Err(outcome) = classification { return outcome; }
        let item = items.objectAtIndex(0);
        let text_type = NSString::from_str(TEXT_TYPE);
        let data = item.dataForType(&text_type);
        if data.as_ref().is_some_and(|data| data.len() > MAX_TEXT_BYTES) { return Content::TooLarge; }
        // Native IPC may allocate NSData before this check; only Rust copies are bounded.
        let bytes = data.map(|data| data.to_vec());
        decode_text(bytes.as_deref())
    }
}

// Type metadata is bounded before conversion by NativeSource and checked again
// here so fake sources exercise the same conservative format policy.
fn classify_offer(
    item_count: usize,
    types: impl FnOnce() -> Result<Vec<String>, Content>,
) -> Result<(), Content> {
    if item_count != 1 { return Err(Content::Unsupported); }
    let types = types()?;
    if types.len() > MAX_TYPES || types.iter().any(|kind| kind.len() > MAX_TYPE_BYTES) {
        return Err(Content::TooLarge);
    }
    if types.is_empty() || types.iter().any(|kind| kind != TEXT_TYPE) {
        return Err(Content::Unsupported);
    }
    Ok(())
}

fn decode_text(bytes: Option<&[u8]>) -> Content {
    let Some(bytes) = bytes else { return Content::Unavailable; };
    if bytes.len() > MAX_TEXT_BYTES { return Content::TooLarge; }
    match std::str::from_utf8(bytes) {
        Ok(text) => Content::Text(text.to_owned()),
        Err(_) => Content::Unavailable,
    }
}

#[cfg(test)]
#[path = "macos_observer_tests.rs"]
mod tests;
