use super::*;

#[test]
fn native_count_keeps_zero_and_full_signed_width() {
    assert_ne!(ChangeCount(0), ChangeCount(1_isize << 32));
    assert_ne!(ChangeCount(-1), ChangeCount(u32::MAX as isize));
}

#[test]
fn retired_event_cannot_admit_a_queued_consumer() {
    let session = Arc::new(Session::new());
    let event = ClipboardEvent {
        session: Arc::clone(&session),
        outcome: ReadOutcome::Text { change_count: ChangeCount(0), text: "fixture".into() },
    };
    assert_eq!(event.with_current(|_| 1), Some(1));
    session.request_stop();
    assert_eq!(event.with_current(|_| panic!("retired event admitted")), None::<()>);
}

use std::sync::{atomic::{AtomicIsize, AtomicUsize, Ordering}, Barrier};

const DEADLINE: Duration = Duration::from_secs(3);
struct FakeSource {
    count: Arc<AtomicIsize>,
    reads: Arc<AtomicUsize>,
    read: Box<dyn FnMut() -> Content>,
}
impl Source for FakeSource {
    fn change_count(&self) -> Result<ChangeCount, ()> {
        Ok(ChangeCount(self.count.load(Ordering::SeqCst)))
    }
    fn read_content(&mut self) -> Content {
        self.reads.fetch_add(1, Ordering::SeqCst);
        (self.read)()
    }
}
fn fake(
    read: impl FnMut() -> Content + Send + 'static,
    callback: impl FnMut(ClipboardEvent) + Send + 'static,
) -> (MacClipboardObserver, Arc<AtomicIsize>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicIsize::new(0));
    let reads = Arc::new(AtomicUsize::new(0));
    let worker_count = Arc::clone(&count);
    let worker_reads = Arc::clone(&reads);
    let observer = MacClipboardObserver::start_with(move || Ok(FakeSource {
        count: worker_count, reads: worker_reads, read: Box::new(read),
    }), Duration::from_millis(2), callback).unwrap();
    (observer, count, reads)
}

#[test]
fn start_has_baseline_without_read_and_delivers_full_width_changes_once() {
    let (tx, rx) = mpsc::channel();
    let (observer, count, reads) = fake(|| Content::Text("fixture".into()), move |event| { tx.send(event).unwrap(); });
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    count.store(1_isize << 32, Ordering::SeqCst);
    let event = rx.recv_timeout(DEADLINE).unwrap();
    assert_eq!(event.with_current(|result| matches!(result,
        ReadOutcome::Text { change_count: ChangeCount(value), text } if *value == 1_isize << 32 && text == "fixture")), Some(true));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    assert_eq!(observer.stop(), StopOutcome::Stopped);
    assert_eq!(event.with_current(|_| true), None);
}

#[test]
fn stop_retires_before_a_blocked_native_read_returns() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::channel();
    let (observer, count, _) = fake(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(DEADLINE).unwrap();
        Content::Text("late".into())
    }, move |event| { events_tx.send(event).unwrap(); });
    count.store(1, Ordering::SeqCst);
    entered_rx.recv_timeout(DEADLINE).unwrap();
    observer.request_stop();
    assert!(!observer.lease().is_active());
    release_tx.send(()).unwrap();
    assert_eq!(observer.stop(), StopOutcome::Stopped);
    assert!(events_rx.try_recv().is_err());
}

#[test]
fn consumer_is_drained_before_stop_reports_completed() {
    let (tx, rx) = mpsc::channel();
    let (observer, count, _) = fake(|| Content::Text("fixture".into()), move |event| { tx.send(event).unwrap(); });
    count.store(1, Ordering::SeqCst);
    let event = rx.recv_timeout(DEADLINE).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let consumer = thread::spawn(move || event.with_current(|_| {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(DEADLINE).unwrap();
    }));
    entered_rx.recv_timeout(DEADLINE).unwrap();
    observer.request_stop();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let stopper = thread::spawn(move || { stopped_tx.send(observer.stop()).unwrap(); });
    assert!(stopped_rx.recv_timeout(Duration::from_millis(20)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(consumer.join().unwrap(), Some(()));
    assert_eq!(stopped_rx.recv_timeout(DEADLINE).unwrap(), StopOutcome::Stopped);
    stopper.join().unwrap();
}

#[test]
fn consumer_reentrant_stop_requests_without_waiting_on_itself() {
    let (tx, rx) = mpsc::channel();
    let (observer, count, _) = fake(|| Content::Text("fixture".into()), move |event| { tx.send(event).unwrap(); });
    count.store(1, Ordering::SeqCst);
    let event = rx.recv_timeout(DEADLINE).unwrap();
    assert_eq!(event.with_current(|_| observer.stop()), Some(StopOutcome::RequestedFromCallback));
    assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn callback_reentrant_stop_requests_without_joining_itself() {
    let owner: Arc<Mutex<Option<Arc<MacClipboardObserver>>>> = Arc::new(Mutex::new(None));
    let callback_owner = Arc::clone(&owner);
    let (tx, rx) = mpsc::channel();
    let (observer, count, _) = fake(|| Content::Text("fixture".into()), move |_| {
        let observer = lock(&callback_owner).as_ref().unwrap().clone();
        tx.send(observer.stop()).unwrap();
    });
    let observer = Arc::new(observer);
    *lock(&owner) = Some(Arc::clone(&observer));
    count.store(1, Ordering::SeqCst);
    assert_eq!(rx.recv_timeout(DEADLINE).unwrap(), StopOutcome::RequestedFromCallback);
    assert_eq!(observer.stop(), StopOutcome::Stopped);
    lock(&owner).take();
}

#[test]
fn concurrent_stop_waiters_receive_same_joined_terminal_state() {
    let (observer, _, _) = fake(|| Content::Unavailable, |_| {});
    let observer = Arc::new(observer);
    let barrier = Arc::new(Barrier::new(5));
    let threads: Vec<_> = (0..4).map(|_| {
        let observer = Arc::clone(&observer);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || { barrier.wait(); observer.stop() })
    }).collect();
    barrier.wait();
    for worker in threads { assert_eq!(worker.join().unwrap(), StopOutcome::Stopped); }
    assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn callback_panic_retires_lease_and_reports_worker_failed() {
    let (tx, rx) = mpsc::channel();
    let (observer, count, _) = fake(|| Content::Text("fixture".into()), move |_| {
        tx.send(()).unwrap();
        panic!("synthetic callback failure");
    });
    count.store(1, Ordering::SeqCst);
    rx.recv_timeout(DEADLINE).unwrap();
    assert_eq!(observer.stop(), StopOutcome::WorkerFailed);
    assert!(!observer.lease().is_active());
    assert_eq!(observer.stop(), StopOutcome::WorkerFailed);
}

#[test]
fn consumer_panic_releases_admission_guard() {
    let session = Arc::new(Session::new());
    let event = ClipboardEvent { session, outcome: ReadOutcome::Unsupported };
    let _ = std::panic::catch_unwind(|| event.with_current(|_| panic!("synthetic consumer failure")));
    event.request_stop();
    event.session.wait_drained();
    assert_eq!(event.with_current(|_| ()), None);
}

#[test]
fn restart_new_baseline_cannot_revive_old_queued_events() {
    let (tx, rx) = mpsc::channel();
    let (old, count, _) = fake(|| Content::Text("old".into()), move |event| { tx.send(event).unwrap(); });
    count.store(1, Ordering::SeqCst);
    let old_event = rx.recv_timeout(DEADLINE).unwrap();
    assert_eq!(old.stop(), StopOutcome::Stopped);
    let (fresh, _, reads) = fake(|| Content::Text("new".into()), |_| {});
    assert!(fresh.lease().is_active());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(old_event.with_current(|_| true), None);
    assert_eq!(fresh.stop(), StopOutcome::Stopped);
}

#[test]
fn count_change_during_read_discards_text_and_unavailable_is_not_retried() {
    let count = Arc::new(AtomicIsize::new(1));
    let moved_count = Arc::clone(&count);
    let mut source = FakeSource { count, reads: Arc::new(AtomicUsize::new(0)), read: Box::new(move || {
        moved_count.store(2, Ordering::SeqCst);
        Content::Text("stale".into())
    }) };
    assert!(matches!(sample(&mut source, ChangeCount(1)), ReadOutcome::ChangedDuringRead));
    let (tx, rx) = mpsc::channel();
    let (observer, count, reads) = fake(|| Content::Unavailable, move |event| { tx.send(event).unwrap(); });
    count.store(1, Ordering::SeqCst);
    assert_eq!(rx.recv_timeout(DEADLINE).unwrap().with_current(|outcome| matches!(outcome, ReadOutcome::Unavailable)), Some(true));
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(observer.stop(), StopOutcome::Stopped);
}

#[test]
fn failed_start_does_not_leave_a_live_worker() {
    let result = MacClipboardObserver::start_with::<FakeSource>(|| Err(StartError::Unavailable), Duration::from_millis(1), |_| {});
    assert!(matches!(result, Err(StartError::Unavailable)));
}

#[test]
fn strict_text_decode_preserves_empty_unicode_nul_and_rejects_invalid_or_oversized() {
    assert!(matches!(decode_text(None), Content::Unavailable));
    for text in ["", "中文🙂", "a\0b"] {
        assert!(matches!(decode_text(Some(text.as_bytes())), Content::Text(value) if value == text));
    }
    assert!(matches!(decode_text(Some(&[0xff])), Content::Unavailable));
    assert!(matches!(decode_text(Some(&vec![b'a'; MAX_TEXT_BYTES])), Content::Text(_)));
    assert!(matches!(decode_text(Some(&vec![b'a'; MAX_TEXT_BYTES + 1])), Content::TooLarge));
}

/// This is the only native fixture. No call to generalPasteboard occurs here.
#[test]
#[ignore = "explicit opt-in: isolated unique named pasteboard, never general"]
fn native_unique_board_text_and_mixed_format_classification() {
    use objc2_foundation::NSData;
    struct UniqueBoard(Retained<NSPasteboard>);
    impl Drop for UniqueBoard {
        fn drop(&mut self) {
            // SAFETY: only this fixture's unique board; never a standard board.
            unsafe { let _: () = objc2::msg_send![&*self.0, releaseGlobally]; }
        }
    }
    autoreleasepool(|_| {
        let owner = UniqueBoard(NSPasteboard::pasteboardWithUniqueName());
        let mut source = NativeSource(NSPasteboard::pasteboardWithName(&owner.0.name()));
        let text_type = NSString::from_str(TEXT_TYPE);
        for text in ["", "中文🙂", "a\0b"] {
            owner.0.clearContents();
            assert!(owner.0.setData_forType(Some(&NSData::with_bytes(text.as_bytes())), &text_type));
            let count = source.change_count().unwrap();
            assert!(matches!(sample(&mut source, count), ReadOutcome::Text { text: captured, .. } if captured == text));
        }
        owner.0.setData_forType(Some(&NSData::with_bytes(b"{rtf}")), &NSString::from_str("public.rtf"));
        let count = source.change_count().unwrap();
        assert!(matches!(sample(&mut source, count), ReadOutcome::Unsupported));

        // Each worker opens its own object for this unique name, never general.
        let name = owner.0.name().to_string();
        let (tx, rx) = mpsc::channel();
        let observer = MacClipboardObserver::start_with(move || {
            Ok(NativeSource(NSPasteboard::pasteboardWithName(&NSString::from_str(&name))))
        }, POLL_INTERVAL, move |event| { tx.send(event).unwrap(); }).unwrap();
        owner.0.clearContents();
        assert!(owner.0.setData_forType(Some(&NSData::with_bytes(b"worker fixture")), &text_type));
        let event = rx.recv_timeout(DEADLINE).unwrap();
        assert_eq!(event.with_current(|outcome| matches!(outcome,
            ReadOutcome::Text { text, .. } if text == "worker fixture")), Some(true));
        assert_eq!(observer.stop(), StopOutcome::Stopped);
        assert_eq!(event.with_current(|_| true), None);
    });
}

#[test]
fn conservative_format_policy_rejects_multi_item_mixed_unknown_and_unbounded_types() {
    for count in [0, 2, usize::MAX] {
        assert!(matches!(classify_offer(count, || panic!("must not read types for multiple items")), Err(Content::Unsupported)));
    }
    assert!(classify_offer(1, || Ok(vec![TEXT_TYPE.into()])).is_ok());
    for other in ["public.rtf", "public.html", "public.png", "public.file-url", "private.unknown"] {
        assert!(matches!(classify_offer(1, || Ok(vec![TEXT_TYPE.into(), other.into()])), Err(Content::Unsupported)));
    }
    assert!(matches!(classify_offer(1, || Ok(vec![])), Err(Content::Unsupported)));
    assert!(matches!(classify_offer(1, || Err(Content::Unavailable)), Err(Content::Unavailable)));
    assert!(classify_offer(1, || Ok(vec![TEXT_TYPE.into(); MAX_TYPES])).is_ok());
    assert!(matches!(classify_offer(1, || Ok(vec![TEXT_TYPE.into(); MAX_TYPES + 1])), Err(Content::TooLarge)));
    assert!(matches!(classify_offer(1, || Ok(vec!["x".repeat(MAX_TYPE_BYTES)])), Err(Content::Unsupported)));
    assert!(matches!(classify_offer(1, || Ok(vec!["x".repeat(MAX_TYPE_BYTES + 1)])), Err(Content::TooLarge)));
}

#[test]
fn callback_can_reenter_stop_after_external_stop_claims_join() {
    let owner: Arc<Mutex<Option<Arc<MacClipboardObserver>>>> = Arc::new(Mutex::new(None));
    let callback_owner = Arc::clone(&owner);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let (observer, count, _) = fake(|| Content::Text("fixture".into()), move |_| {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(DEADLINE).unwrap();
        let observer = lock(&callback_owner).as_ref().unwrap().clone();
        result_tx.send(observer.stop()).unwrap();
    });
    let observer = Arc::new(observer);
    *lock(&owner) = Some(Arc::clone(&observer));
    count.store(1, Ordering::SeqCst);
    entered_rx.recv_timeout(DEADLINE).unwrap();
    let external_observer = Arc::clone(&observer);
    let stopper = thread::spawn(move || external_observer.stop());
    // Observe the actual join claim, not just the stopper's thread startup.
    let deadline = std::time::Instant::now() + DEADLINE;
    while lock(&observer.join).handle.is_some() {
        assert!(std::time::Instant::now() < deadline);
        thread::yield_now();
    }
    release_tx.send(()).unwrap();
    assert_eq!(result_rx.recv_timeout(DEADLINE).unwrap(), StopOutcome::RequestedFromCallback);
    assert_eq!(stopper.join().unwrap(), StopOutcome::Stopped);
    lock(&owner).take();
}

#[test]
fn both_concurrent_stoppers_wait_for_external_consumer() {
    let (tx, rx) = mpsc::channel();
    let (observer, count, _) = fake(|| Content::Text("fixture".into()), move |event| { tx.send(event).unwrap(); });
    let observer = Arc::new(observer);
    count.store(1, Ordering::SeqCst);
    let event = rx.recv_timeout(DEADLINE).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let consumer = thread::spawn(move || event.with_current(|_| {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(DEADLINE).unwrap();
    }));
    entered_rx.recv_timeout(DEADLINE).unwrap();
    observer.request_stop();
    let (done_tx, done_rx) = mpsc::channel();
    let stoppers: Vec<_> = (0..2).map(|_| {
        let observer = Arc::clone(&observer);
        let done_tx = done_tx.clone();
        thread::spawn(move || { done_tx.send(observer.stop()).unwrap(); })
    }).collect();
    assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(consumer.join().unwrap(), Some(()));
    for stopper in stoppers {
        assert_eq!(done_rx.recv_timeout(DEADLINE).unwrap(), StopOutcome::Stopped);
        stopper.join().unwrap();
    }
}

#[test]
fn drop_retires_session_and_joins_idle_worker() {
    let (observer, _, _) = fake(|| Content::Unavailable, |_| {});
    let lease = observer.lease();
    drop(observer);
    assert!(!lease.is_active());
}

#[test]
fn baseline_error_drops_source_and_factory_panic_reports_failure() {
    struct FailedBaseline(Arc<AtomicUsize>);
    impl Source for FailedBaseline {
        fn change_count(&self) -> Result<ChangeCount, ()> { Err(()) }
        fn read_content(&mut self) -> Content { panic!("baseline must not read body") }
    }
    impl Drop for FailedBaseline {
        fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let worker_drops = Arc::clone(&drops);
    let result = MacClipboardObserver::start_with(move || Ok(FailedBaseline(worker_drops)), POLL_INTERVAL, |_| {});
    assert!(matches!(result, Err(StartError::Unavailable)));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let result = MacClipboardObserver::start_with::<FailedBaseline>(|| panic!("synthetic factory failure"), POLL_INTERVAL, |_| {});
    assert!(matches!(result, Err(StartError::WorkerFailed)));
}

#[test]
fn callback_capture_destructor_cannot_join_its_own_worker() {
    struct StopOnDrop {
        owner: Arc<Mutex<Option<Arc<MacClipboardObserver>>>>,
        done: mpsc::Sender<StopOutcome>,
    }
    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            let observer = lock(&self.owner).take().unwrap();
            // The callback's admission guard has already left by this point.
            self.done.send(observer.stop()).unwrap();
        }
    }
    let owner = Arc::new(Mutex::new(None));
    let (done_tx, done_rx) = mpsc::channel();
    let captured = StopOnDrop { owner: Arc::clone(&owner), done: done_tx };
    let (observer, count, _) = fake(|| Content::Unsupported, move |event| {
        let _keep_entire_capture = &captured;
        event.request_stop();
    });
    let observer = Arc::new(observer);
    *lock(&owner) = Some(Arc::clone(&observer));
    count.store(1, Ordering::SeqCst);
    assert_eq!(done_rx.recv_timeout(DEADLINE).unwrap(), StopOutcome::RequestedFromCallback);
    assert_eq!(observer.stop(), StopOutcome::Stopped);
}
