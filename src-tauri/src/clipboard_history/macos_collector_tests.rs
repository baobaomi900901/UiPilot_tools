use super::*;
use super::super::{macos_history::{CaptureBatch, CaptureReadPermit}, macos_observer::{ChangeCount,
    Content, Source, CapturedEvent, ReadBinding}, macos_store::tests::Temp};
use std::{sync::{mpsc, atomic::{AtomicIsize, AtomicUsize, Ordering}}, thread, time::Duration};
const WAIT: Duration = Duration::from_secs(5);
const A: &str = "com.example.a";
const B: &str = "com.example.b";
const T1: &str = "2026-11-07T01:02:03Z";
const T2: &str = "2026-11-07T04:05:06Z";
struct Fake {
    count: Arc<AtomicIsize>, reads: Arc<AtomicUsize>,
    read: Box<dyn FnMut() -> Content>,
}
impl Source for Fake {
    fn change_count(&self) -> Result<ChangeCount, ()> { Ok(ChangeCount(self.count.load(Ordering::SeqCst))) }
    fn read_content(&mut self) -> Content { self.reads.fetch_add(1, Ordering::SeqCst); (self.read)() }
}
fn factory(
    read: impl FnMut() -> Content + Send + 'static,
) -> (impl FnOnce() -> Result<Fake, StartError> + Send, Arc<AtomicIsize>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicIsize::new(0)); let reads = Arc::new(AtomicUsize::new(0));
    let source_count = Arc::clone(&count); let source_reads = Arc::clone(&reads);
    (move || Ok(Fake { count: source_count, reads: source_reads, read: Box::new(read) }), count, reads)
}

// A deterministic boundary BEFORE history.prepare_read, with no native access.
fn stepped_binding(history: Arc<MacHistory>) -> (
    impl FnMut() -> Result<ReadBinding<CaptureBatch, CaptureReadPermit>, ()> + Send,
    mpsc::Sender<()>, mpsc::Receiver<()>,
) {
    let (step_tx, step_rx) = mpsc::channel(); let (waiting_tx, waiting_rx) = mpsc::channel();
    let mut baseline = true;
    (move || {
        if !baseline { waiting_tx.send(()).map_err(|_| ())?; step_rx.recv_timeout(WAIT).map_err(|_| ())?; }
        baseline = false;
        history.prepare_read().map_err(|_| ())
    }, step_tx, waiting_rx)
}

#[test]
fn collector_writes_private_history_and_stop_leaves_shared_history_available() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let token = history.authorize(A).unwrap();
    let (make_source, count, reads) = factory(|| Content::Text("synthetic pipeline".into()));
    let (tx, rx) = mpsc::channel();
    let collector = MacHistoryCollector::start_with(Arc::clone(&history), make_source,
        || Ok(T1.into()), move |report| { tx.send(report).unwrap(); }).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    count.store(1, Ordering::SeqCst);
    assert_eq!(rx.recv_timeout(WAIT).unwrap().unwrap().accepted, 1);
    assert_eq!(collector.stop(), StopOutcome::Stopped);
    let snapshot = history.snapshot(&token, None).unwrap();
    assert_eq!(snapshot.entries.len(), 1); assert_eq!(snapshot.entries[0].captured_at(), T1);
    history.shutdown();
    let reopened = MacHistory::open(&temp.0).unwrap(); let token = reopened.authorize(A).unwrap();
    assert_eq!(reopened.snapshot(&token, None).unwrap(), snapshot);
}

#[test]
fn no_grants_and_late_first_grant_never_backfill_disabled_content() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let (make_source, count, reads) = factory(|| Content::Text("after permission".into()));
    let (binding, steps, waiting) = stepped_binding(Arc::clone(&history));
    let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1),
        binding, || Ok(T1.into()), move |event| { tx.send(event).unwrap(); }).unwrap();
    waiting.recv_timeout(WAIT).unwrap(); count.store(1, Ordering::SeqCst); steps.send(()).unwrap();
    waiting.recv_timeout(WAIT).unwrap(); assert_eq!(reads.load(Ordering::SeqCst), 0);
    // The disabled clipboard changes, then grant arrives before the next poll.
    count.store(2, Ordering::SeqCst); let token = history.authorize(A).unwrap(); steps.send(()).unwrap();
    waiting.recv_timeout(WAIT).unwrap(); assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
    count.store(3, Ordering::SeqCst); steps.send(()).unwrap();
    let event = rx.recv_timeout(WAIT).unwrap().unwrap();
    assert_eq!(history.ingest(event).unwrap().accepted, 1);
    assert_eq!(history.snapshot(&token, None).unwrap().entries.len(), 1);
    observer.request_stop(); let _ = steps.send(()); assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn grant_during_read_cannot_join_old_batch_and_delay_preserves_observation_time() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let a = history.authorize(A).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    let (make_source, count, _) = factory(move || {
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(WAIT).unwrap(); Content::Text("captured once".into())
    });
    let binding_history = Arc::clone(&history); let clock = Arc::new(AtomicUsize::new(0)); let source_clock = Arc::clone(&clock);
    let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(2),
        move || binding_history.prepare_read().map_err(|_| ()),
        move || Ok(if source_clock.load(Ordering::SeqCst) == 0 { T1 } else { T2 }.into()),
        move |event| { tx.send(event).unwrap(); }).unwrap();
    count.store(1, Ordering::SeqCst); entered_rx.recv_timeout(WAIT).unwrap();
    let b = history.authorize(B).unwrap(); clock.store(1, Ordering::SeqCst); release_tx.send(()).unwrap();
    let event = rx.recv_timeout(WAIT).unwrap().unwrap();
    let report = history.ingest(event).unwrap(); assert_eq!(report.accepted, 1);
    assert_eq!(history.snapshot(&a, None).unwrap().entries[0].captured_at(), T1);
    assert!(history.snapshot(&b, None).unwrap().entries.is_empty());
    assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn revoke_drains_read_and_regrant_never_revives_delayed_envelope() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let old = history.authorize(A).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    let (make_source, count, _) = factory(move || {
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(WAIT).unwrap(); Content::Text("old generation".into())
    });
    let binding_history = Arc::clone(&history); let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(2),
        move || binding_history.prepare_read().map_err(|_| ()), || Ok(T1.into()), move |event| { tx.send(event).unwrap(); }).unwrap();
    count.store(1, Ordering::SeqCst); entered_rx.recv_timeout(WAIT).unwrap();
    let revoker_history = Arc::clone(&history); let (done_tx, done_rx) = mpsc::channel();
    let revoker = thread::spawn(move || { revoker_history.revoke(A).unwrap(); done_tx.send(()).unwrap(); });
    let deadline = std::time::Instant::now() + WAIT;
    while history.snapshot(&old, None) != Err(HistoryError::PermissionDenied) {
        assert!(std::time::Instant::now() < deadline); thread::yield_now();
    }
    assert!(done_rx.try_recv().is_err());
    release_tx.send(()).unwrap(); let event = rx.recv_timeout(WAIT).unwrap().unwrap();
    done_rx.recv_timeout(WAIT).unwrap(); revoker.join().unwrap();
    let fresh = history.authorize(A).unwrap();
    assert_eq!(history.ingest(event).unwrap().denied, 1);
    assert!(history.snapshot(&fresh, None).unwrap().entries.is_empty());
    assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn callback_can_revoke_because_read_permit_is_already_released() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap()); history.authorize(A).unwrap();
    let (make_source, count, _) = factory(|| Content::Unsupported);
    let callback_history = Arc::clone(&history); let binding_history = Arc::clone(&history); let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1),
        move || binding_history.prepare_read().map_err(|_| ()), || Ok(T1.into()), move |event| {
            callback_history.revoke(A).unwrap();
            tx.send(event.is_ok()).unwrap();
        }).unwrap();
    count.store(1, Ordering::SeqCst); assert!(rx.recv_timeout(WAIT).unwrap()); assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn clock_failure_does_not_read_and_releases_permission_before_error_callback() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap()); history.authorize(A).unwrap();
    let (make_source, count, reads) = factory(|| panic!("clock error must not read"));
    let binding_history = Arc::clone(&history); let callback_history = Arc::clone(&history); let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1),
        move || binding_history.prepare_read().map_err(|_| ()), || Err(()), move |event| {
            callback_history.revoke(A).unwrap();
            tx.send(matches!(event, Err(CaptureFailure::ClockUnavailable))).unwrap();
        }).unwrap();
    count.store(1, Ordering::SeqCst); assert!(rx.recv_timeout(WAIT).unwrap());
    assert_eq!(reads.load(Ordering::SeqCst), 0); assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn stopped_observer_and_foreign_history_reject_sealed_events() {
    let temp = Temp::new(); let other_temp = Temp::new();
    let history = Arc::new(MacHistory::open(&temp.0).unwrap()); history.authorize(A).unwrap();
    let other = MacHistory::open(&other_temp.0).unwrap(); let other_token = other.authorize(A).unwrap();
    let (make_source, count, _) = factory(|| Content::Text("private".into()));
    let binding_history = Arc::clone(&history); let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1),
        move || binding_history.prepare_read().map_err(|_| ()), || Ok(T1.into()), move |event| { tx.send(event).unwrap(); }).unwrap();
    count.store(1, Ordering::SeqCst); let event = rx.recv_timeout(WAIT).unwrap().unwrap();
    assert_eq!(other.ingest(event), Err(HistoryError::PermissionDenied));
    assert!(other.snapshot(&other_token, None).unwrap().entries.is_empty());
    count.store(2, Ordering::SeqCst); let event = rx.recv_timeout(WAIT).unwrap().unwrap();
    assert_eq!(observer.stop(), StopOutcome::Stopped);
    assert_eq!(history.ingest(event), Err(HistoryError::PermissionDenied));
}

#[test]
fn native_read_panic_releases_permissions_and_marks_worker_failed() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap()); history.authorize(A).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (make_source, count, _) = factory(move || { entered_tx.send(()).unwrap(); panic!("synthetic provider panic") });
    let binding_history = Arc::clone(&history);
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1),
        move || binding_history.prepare_read().map_err(|_| ()), || Ok(T1.into()), |_| {}).unwrap();
    count.store(1, Ordering::SeqCst); entered_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(observer.stop(), StopOutcome::WorkerFailed); history.revoke(A).unwrap();
}

#[test]
fn startup_binding_precedes_baseline_even_if_grant_arrives_inside_count_call() {
    struct StartupSource {
        history: Arc<MacHistory>, count: Arc<AtomicIsize>, reads: Arc<AtomicUsize>, calls: AtomicUsize,
    }
    impl Source for StartupSource {
        fn change_count(&self) -> Result<ChangeCount, ()> {
            let count = self.count.load(Ordering::SeqCst);
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                // Count was sampled BEFORE the grant and disabled-period change.
                self.count.store(1, Ordering::SeqCst);
                self.history.authorize(A).unwrap();
            }
            Ok(ChangeCount(count))
        }
        fn read_content(&mut self) -> Content {
            self.reads.fetch_add(1, Ordering::SeqCst); Content::Text("new change".into())
        }
    }
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let count = Arc::new(AtomicIsize::new(0)); let reads = Arc::new(AtomicUsize::new(0));
    let source_history = Arc::clone(&history); let source_count = Arc::clone(&count); let source_reads = Arc::clone(&reads);
    let (binding, steps, waiting) = stepped_binding(Arc::clone(&history)); let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(move || Ok(StartupSource {
        history: source_history, count: source_count, reads: source_reads, calls: AtomicUsize::new(0),
    }), Duration::from_millis(1), binding, || Ok(T1.into()), move |event| { tx.send(event).unwrap(); }).unwrap();
    waiting.recv_timeout(WAIT).unwrap(); steps.send(()).unwrap();
    waiting.recv_timeout(WAIT).unwrap(); assert_eq!(reads.load(Ordering::SeqCst), 0); assert!(rx.try_recv().is_err());
    history.authorize(A).unwrap(); // Idempotent authorization must not reset baseline.
    count.store(2, Ordering::SeqCst); steps.send(()).unwrap();
    assert_eq!(history.ingest(rx.recv_timeout(WAIT).unwrap().unwrap()).unwrap().accepted, 1);
    observer.request_stop(); let _ = steps.send(()); assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn rejected_before_read_hook_never_reads_and_retires_worker() {
    let (make_source, count, reads) = factory(|| panic!("rejected hook must not read"));
    let revision = super::super::macos_observer::AuthorizationRevision::new();
    let (entered_tx, entered_rx) = mpsc::channel(); let (step_tx, step_rx) = mpsc::channel();
    let mut first = true; let (tx, rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1), move || {
        if first { first = false; return Ok(ReadBinding::<(), ()>::new(revision.clone(), Some(((), ())))); }
        entered_tx.send(()).unwrap(); step_rx.recv_timeout(WAIT).unwrap(); Err(())
    }, || Ok(T1.into()), move |event| { tx.send(matches!(event, Err(CaptureFailure::BindingUnavailable))).unwrap(); }).unwrap();
    entered_rx.recv_timeout(WAIT).unwrap(); count.store(1, Ordering::SeqCst); step_tx.send(()).unwrap();
    assert!(rx.recv_timeout(WAIT).unwrap()); assert_eq!(observer.stop(), StopOutcome::Stopped);
    assert_eq!(reads.load(Ordering::SeqCst), 0); assert!(!observer.lease().is_active());
}

#[test]
fn stop_during_native_read_drops_permission_and_suppresses_late_store_write() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap()); let token = history.authorize(A).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    let (make_source, count, _) = factory(move || {
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(WAIT).unwrap(); Content::Text("late".into())
    });
    let (tx, rx) = mpsc::channel();
    let collector = MacHistoryCollector::start_with(Arc::clone(&history), make_source, || Ok(T1.into()), move |report| { tx.send(report).unwrap(); }).unwrap();
    count.store(1, Ordering::SeqCst); entered_rx.recv_timeout(WAIT).unwrap();
    collector.request_stop(); release_tx.send(()).unwrap(); assert_eq!(collector.stop(), StopOutcome::Stopped);
    assert!(rx.try_recv().is_err()); assert!(history.snapshot(&token, None).unwrap().entries.is_empty()); history.revoke(A).unwrap();
}

#[test]
fn callback_panic_and_changed_read_release_native_permissions() {
    for panic_callback in [false, true] {
        let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap()); history.authorize(A).unwrap();
        let count = Arc::new(AtomicIsize::new(0)); let source_count = Arc::clone(&count); let moving_count = Arc::clone(&count);
        let (entered_tx, entered_rx) = mpsc::channel(); let binding_history = Arc::clone(&history);
        let callback_history = Arc::clone(&history);
        let observer = MacClipboardObserver::start_with_binding(move || Ok(Fake { count: source_count,
            reads: Arc::new(AtomicUsize::new(0)), read: Box::new(move || {
                if !panic_callback { moving_count.store(2, Ordering::SeqCst); }
                Content::Text("fixture".into())
            }) }), Duration::from_millis(1), move || binding_history.prepare_read().map_err(|_| ()), || Ok(T1.into()), move |event| {
                if !panic_callback {
                    let changed = event.unwrap().with_current(|outcome, _, _| matches!(outcome, super::super::macos_observer::ReadOutcome::ChangedDuringRead));
                    assert_eq!(changed, Some(true));
                    callback_history.revoke(A).unwrap();
                }
                entered_tx.send(()).unwrap();
                if panic_callback { panic!("synthetic report panic"); }
            }).unwrap();
        count.store(1, Ordering::SeqCst); entered_rx.recv_timeout(WAIT).unwrap();
        let outcome = observer.stop();
        assert_eq!(outcome, if panic_callback { StopOutcome::WorkerFailed } else { StopOutcome::Stopped });
        history.revoke(A).unwrap();
    }
}

#[test]
fn shutdown_closes_authority_then_drains_blocked_native_read() {
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let token = history.authorize(A).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    let (make_source, count, _) = factory(move || {
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(WAIT).unwrap(); Content::Text("must not persist".into())
    });
    let binding_history = Arc::clone(&history); let (events_tx, events_rx) = mpsc::channel();
    let observer = MacClipboardObserver::start_with_binding(make_source, Duration::from_millis(1),
        move || binding_history.prepare_read().map_err(|_| ()), || Ok(T1.into()),
        move |event| { events_tx.send(event).unwrap(); }).unwrap();
    count.store(1, Ordering::SeqCst); entered_rx.recv_timeout(WAIT).unwrap();
    let shutdown_history = Arc::clone(&history); let (done_tx, done_rx) = mpsc::channel();
    let shutdown = thread::spawn(move || { shutdown_history.shutdown(); done_tx.send(()).unwrap(); });
    let deadline = std::time::Instant::now() + WAIT;
    while history.snapshot(&token, None) != Err(HistoryError::PermissionDenied) {
        assert!(std::time::Instant::now() < deadline); thread::yield_now();
    }
    assert!(done_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    let event = events_rx.recv_timeout(WAIT).unwrap().unwrap();
    done_rx.recv_timeout(WAIT).unwrap(); shutdown.join().unwrap();
    assert!(matches!(history.ingest(event), Err(HistoryError::Stopped | HistoryError::PermissionDenied)));
    assert!(matches!(history.authorize(A), Err(HistoryError::Stopped)));
    assert_eq!(observer.stop(), StopOutcome::Stopped);
    let reopened = MacHistory::open(&temp.0).unwrap(); let fresh = reopened.authorize(A).unwrap();
    assert!(reopened.snapshot(&fresh, None).unwrap().entries.is_empty());
}
