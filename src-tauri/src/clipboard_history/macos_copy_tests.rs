use super::*;
use super::super::{macos_history::{AccessToken, HistoryError}, macos_observer::ClipboardEvent, macos_store::tests::Temp};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};
const A: &str = "com.example.copy";
struct Fixture { history: Arc<MacHistory>, read: AccessToken, copy: CopyToken, id: String, _temp: Temp }
impl Fixture {
    fn new(text: &str) -> Self {
        let temp = Temp::new();
        let history = Arc::new(MacHistory::open(&temp.0).unwrap());
        let read = history.authorize(A).unwrap();
        history.ingest_unbound_for_test(&ClipboardEvent::synthetic_text(text), history.bind_capture().unwrap()).unwrap();
        let id = history.snapshot(&read, None).unwrap().entries[0].id();
        let copy = history.authorize_copy(&read).unwrap();
        Self { history, read, copy, id, _temp: temp }
    }
    fn request(&self) -> CopyRequest { prepare_request(&self.history, &self.copy, &self.id, CopyPurpose::ExplicitCopy).unwrap() }
}
#[derive(Default)]
struct Board { count: isize, text: String, trace: Vec<&'static str> }
type SharedBoard = Arc<Mutex<Board>>;
fn external(board: &SharedBoard, text: &str) { let mut b = board.lock().unwrap(); b.count += 1; b.text = text.into(); }
// A fake declaration is deliberately NOT a CAS. It always takes ownership and
// replaces content, even if an external write happened after the final count.
struct Fake {
    board: SharedBoard,
    counts: usize,
    after_prepare: Option<Box<dyn FnOnce() + Send>>,
    after_check: Option<Box<dyn FnOnce() + Send>>,
    after_claim: Option<Box<dyn FnOnce() + Send>>,
    after_write: Option<Box<dyn FnOnce() + Send>>,
    error_after_claim: bool,
}
impl Fake {
    fn new(board: &SharedBoard) -> Self { Self { board: Arc::clone(board), counts: 0, after_prepare: None, after_check: None, after_claim: None, after_write: None, error_after_claim: false } }
}
impl CopyDriver for Fake {
    type Prepared = String;
    fn change_count(&mut self) -> Result<ChangeCount, CopyError> {
        self.counts += 1;
        let count = { let mut b = self.board.lock().unwrap(); b.trace.push("count"); b.count };
        if self.counts == 2 { if let Some(hook) = self.after_check.take() { hook(); } }
        Ok(ChangeCount(count))
    }
    fn prepare(&mut self, text: &str) -> Result<String, CopyError> {
        self.board.lock().unwrap().trace.push("prepare");
        if let Some(hook) = self.after_prepare.take() { hook(); }
        Ok(text.into())
    }
    fn publish(&mut self, text: String) -> Result<(), CopyError> {
        let claimed = { let mut b = self.board.lock().unwrap(); b.trace.push("claim"); b.count += 1; b.text.clear(); b.count };
        if let Some(hook) = self.after_claim.take() { hook(); }
        if self.error_after_claim { return Err(CopyError::WriteUnconfirmed); }
        { let mut b = self.board.lock().unwrap(); b.trace.push("set");
            if b.count != claimed { return Err(CopyError::OwnershipLost); }
            b.text = text;
        }
        if let Some(hook) = self.after_write.take() { hook(); }
        Ok(())
    }
}
fn board() -> SharedBoard { Arc::new(Mutex::new(Board { text: "initial".into(), ..Board::default() })) }
fn run(f: &Fixture, driver: Fake) -> Result<CopyOutcome, CopyError> { execute_with(&f.history, f.request(), || Ok(driver)) }

#[test]
fn restore_rejected_before_record_lookup_even_with_expired_copy_authority() {
    let f = Fixture::new("text"); f.history.revoke_copy(&f.copy).unwrap();
    assert!(matches!(prepare_request(&f.history, &f.copy, "missing", CopyPurpose::TemporaryRestore), Err(CopyError::Unsupported)));
}
#[test]
fn independent_copy_revocation_keeps_read_but_blocks_before_factory() {
    let f = Fixture::new("text"); let request = f.request();
    f.history.revoke_copy(&f.copy).unwrap();
    assert_eq!(f.history.snapshot(&f.read, None).unwrap().entries.len(), 1);
    assert_eq!(execute_with::<Fake>(&f.history, request, || panic!("no native factory for read-only authority")), Err(CopyError::PermissionDenied));
    let fresh = f.history.authorize_copy(&f.read).unwrap();
    assert!(prepare_request(&f.history, &fresh, &f.id, CopyPurpose::ExplicitCopy).is_ok());
    assert!(matches!(prepare_request(&f.history, &f.copy, &f.id, CopyPurpose::ExplicitCopy), Err(CopyError::PermissionDenied)));
}
#[test]
fn copy_preserves_empty_unicode_nul_and_full_width_count_without_recency_mutation() {
    for text in ["", "中文🦀\0tail", "plain"] {
        let f = Fixture::new(text); let b = board(); b.lock().unwrap().count = isize::MAX - 2;
        let before = f.history.snapshot(&f.read, None).unwrap();
        assert_eq!(run(&f, Fake::new(&b)), Ok(CopyOutcome::Accepted));
        let b = b.lock().unwrap(); assert_eq!(b.text, text);
        assert_eq!(b.trace, ["count", "prepare", "count", "claim", "set"]);
        assert_eq!(f.history.snapshot(&f.read, None).unwrap(), before);
    }
}
#[test]
fn removed_record_and_foreign_history_refuse_before_native_factory() {
    let f = Fixture::new("text"); let request = f.request();
    f.history.remove(&f.read, &f.id).unwrap();
    assert_eq!(execute_with::<Fake>(&f.history, request, || panic!("removed record")), Err(CopyError::RecordNotFound));
    let a = Fixture::new("a"); let b = Fixture::new("b");
    assert_eq!(execute_with::<Fake>(&b.history, a.request(), || panic!("foreign history")), Err(CopyError::PermissionDenied));
    assert!(matches!(b.history.authorize_copy(&a.read), Err(CopyError::PermissionDenied)));
}
#[test]
fn payload_preparation_does_not_survive_delete_clear_or_regrant() {
    for action in 0..4 {
        let f = Fixture::new("text"); let b = board(); let mut d = Fake::new(&b);
        let history = Arc::clone(&f.history); let read = f.read.clone(); let copy = f.copy.clone(); let id = f.id.clone();
        d.after_prepare = Some(Box::new(move || { match action {
            0 => { history.remove(&read, &id).unwrap(); }
            1 => { history.clear(&read).unwrap(); }
            2 => { history.revoke_copy(&copy).unwrap(); history.authorize_copy(&read).unwrap(); }
            _ => { history.revoke(A).unwrap(); let fresh = history.authorize(A).unwrap(); history.authorize_copy(&fresh).unwrap(); }
        } }));
        assert_eq!(run(&f, d), Err(if action < 2 { CopyError::RecordNotFound } else { CopyError::PermissionDenied }));
        assert_eq!(b.lock().unwrap().trace, ["count", "prepare"]);
        assert_eq!(b.lock().unwrap().text, "initial");
    }
}
#[test]
fn observed_external_change_aborts_before_claim() {
    let f = Fixture::new("text"); let b = board(); let mut d = Fake::new(&b); let external_board = Arc::clone(&b);
    d.after_prepare = Some(Box::new(move || external(&external_board, "B")));
    assert_eq!(run(&f, d), Err(CopyError::ClipboardChanged));
    let b = b.lock().unwrap(); assert_eq!(b.text, "B"); assert_eq!(b.trace, ["count", "prepare", "count"]);
}
#[test]
fn check_then_claim_is_not_cas_and_explicit_copy_can_replace_intervening_b() {
    let f = Fixture::new("explicit A"); let b = board(); let mut d = Fake::new(&b); let external_board = Arc::clone(&b);
    d.after_check = Some(Box::new(move || external(&external_board, "B in unavoidable gap")));
    assert_eq!(run(&f, d), Ok(CopyOutcome::Accepted));
    assert_eq!(b.lock().unwrap().text, "explicit A");
}
#[test]
fn lost_ownership_after_claim_does_not_retry_or_restore() {
    let f = Fixture::new("A"); let b = board(); let mut d = Fake::new(&b); let external_board = Arc::clone(&b);
    d.after_claim = Some(Box::new(move || external(&external_board, "B")));
    assert_eq!(run(&f, d), Err(CopyError::OwnershipLost));
    let b = b.lock().unwrap(); assert_eq!(b.text, "B"); assert_eq!(b.trace, ["count", "prepare", "count", "claim", "set"]);
}
#[test]
fn mutation_failure_is_unconfirmed_and_does_not_restore_cleared_content() {
    let f = Fixture::new("A"); let b = board(); let mut d = Fake::new(&b); d.error_after_claim = true;
    assert_eq!(run(&f, d), Err(CopyError::WriteUnconfirmed));
    let b = b.lock().unwrap(); assert_eq!(b.text, ""); assert_eq!(b.trace, ["count", "prepare", "count", "claim"]);
}
#[test]
fn accepted_does_not_imply_still_owned_and_late_b_survives_completion() {
    let f = Fixture::new("A"); let b = board(); let mut d = Fake::new(&b); let external_board = Arc::clone(&b);
    d.after_write = Some(Box::new(move || external(&external_board, "B")));
    assert_eq!(run(&f, d), Ok(CopyOutcome::Accepted));
    assert_eq!(b.lock().unwrap().text, "B");
    drop(f);
    assert_eq!(b.lock().unwrap().text, "B");
    assert_eq!(b.lock().unwrap().trace, ["count", "prepare", "count", "claim", "set"]);
}

fn drain_case(action: usize) {
    let f = Fixture::new("A"); let b = board(); let mut d = Fake::new(&b);
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    d.after_claim = Some(Box::new(move || { entered_tx.send(()).unwrap(); release_rx.recv_timeout(Duration::from_secs(5)).unwrap(); }));
    let history = Arc::clone(&f.history); let request = f.request();
    let writer = thread::spawn(move || execute_with(&history, request, || Ok(d)));
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // An unrelated authorized store remains usable while native publication is blocked.
    let other = f.history.authorize("com.example.other").unwrap();
    assert!(f.history.snapshot(&other, None).unwrap().entries.is_empty());
    let (done_tx, done_rx) = mpsc::channel(); let history = Arc::clone(&f.history); let token = f.copy.clone();
    let revoker = thread::spawn(move || {
        match action { 0 => history.revoke_copy(&token).unwrap(), 1 => history.revoke(A).unwrap(), _ => history.shutdown() }
        done_tx.send(()).unwrap();
    });
    // Observe actual closed admission before asserting the operation is draining;
    // merely timing out after spawning could pass if the thread never ran.
    let deadline = Instant::now() + Duration::from_secs(2);
    while f.copy.active_for_test() { assert!(Instant::now() < deadline); thread::yield_now(); }
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(matches!(prepare_request(&f.history, &f.copy, &f.id, CopyPurpose::ExplicitCopy), Err(CopyError::PermissionDenied)));
    release_tx.send(()).unwrap();
    assert_eq!(writer.join().unwrap(), Ok(CopyOutcome::Accepted));
    done_rx.recv_timeout(Duration::from_secs(2)).unwrap(); revoker.join().unwrap();
    assert_eq!(b.lock().unwrap().trace, ["count", "prepare", "count", "claim", "set"]);
    if action == 0 { assert_eq!(f.history.snapshot(&f.read, None).unwrap().entries.len(), 1); }
    if action == 2 { assert!(matches!(f.history.authorize(A), Err(HistoryError::Stopped))); }
}
#[test] fn copy_revoke_drains_admitted_native_write() { drain_case(0); }
#[test] fn read_revoke_drains_admitted_native_write() { drain_case(1); }
#[test] fn shutdown_drains_admitted_native_write() { drain_case(2); }

#[test]
fn copy_approval_is_idempotent_but_not_restored_by_read_reauthorization() {
    let f = Fixture::new("A"); let old_request = f.request();
    let same = f.history.authorize_copy(&f.read).unwrap();
    assert!(prepare_request(&f.history, &same, &f.id, CopyPurpose::ExplicitCopy).is_ok());
    let b = board();
    assert_eq!(execute_with(&f.history, old_request, || Ok(Fake::new(&b))), Ok(CopyOutcome::Accepted));
    let old_request = f.request();
    f.history.revoke(A).unwrap(); let fresh_read = f.history.authorize(A).unwrap();
    assert!(matches!(f.history.authorize_copy(&f.read), Err(CopyError::PermissionDenied)));
    f.history.authorize_copy(&fresh_read).unwrap();
    assert_eq!(execute_with::<Fake>(&f.history, old_request, || panic!("old approval revived")), Err(CopyError::PermissionDenied));
}

#[test]
#[ignore = "opt-in unique named pasteboard only; no general clipboard; requires desktop pasteboard service"]
fn native_unique_board_explicit_copy_round_trip() {
    let f = Fixture::new("合成 fixture 🦀\0tail"); let history = Arc::clone(&f.history); let request = f.request();
    thread::spawn(move || autoreleasepool(|_| {
        let board = NSPasteboard::pasteboardWithUniqueName();
        struct Release(Retained<NSPasteboard>);
        impl Drop for Release { fn drop(&mut self) { // SAFETY: private named board owned only by this fixture; SDK declares releaseGlobally.
            unsafe { let _: () = objc2::msg_send![&*self.0, releaseGlobally]; } } }
        let owned = Release(board);
        assert_eq!(execute_with(&history, request, || Ok(NativeDriver(owned.0.clone()))), Ok(CopyOutcome::Accepted));
        let bytes = owned.0.dataForType(&NSString::from_str("public.utf8-plain-text")).unwrap().to_vec();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "合成 fixture 🦀\0tail");
    })).join().unwrap();
}

#[test]
fn read_only_native_failures_never_claim_or_modify_content() {
    struct Failing { inner: Fake, stage: usize, counts: usize }
    impl CopyDriver for Failing {
        type Prepared = String;
        fn change_count(&mut self) -> Result<ChangeCount, CopyError> {
            self.counts += 1;
            if self.stage == self.counts { return Err(CopyError::Unavailable); }
            self.inner.change_count()
        }
        fn prepare(&mut self, text: &str) -> Result<String, CopyError> {
            if self.stage == 3 { return Err(CopyError::Unavailable); }
            self.inner.prepare(text)
        }
        fn publish(&mut self, _: String) -> Result<(), CopyError> { panic!("no mutation after preparation failure") }
    }
    let f = Fixture::new("A");
    assert_eq!(execute_with::<Fake>(&f.history, f.request(), || Err(CopyError::Unavailable)), Err(CopyError::Unavailable));
    for stage in 1..=3 {
        let b = board();
        assert_eq!(execute_with(&f.history, f.request(), || Ok(Failing { inner: Fake::new(&b), stage, counts: 0 })), Err(CopyError::Unavailable));
        assert_eq!(b.lock().unwrap().text, "initial");
        assert!(!b.lock().unwrap().trace.contains(&"claim"));
    }
}

#[test]
fn trusted_host_copy_admits_after_preparation_and_never_claims_stale_ticket() {
    use crate::result_registry::{QueryDomain, ResultAction, ResultRegistries};
    for invalidate in [false, true] {
        let registry = ResultRegistries::default().main().clone(); registry.on_show("1".into());
        let token = registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let (request, result) = registry.publish_if_latest(token, vec![((), Some(ResultAction::CopyBuiltInText { text: "4".into() }))], || true,
            |request, rows| (request, rows[0].0.clone())).unwrap();
        let (_, ticket) = registry.resolve_with_ticket(&request, &result).unwrap();
        let b = board(); let mut driver = Fake::new(&b);
        if invalidate { let registry = registry.clone(); driver.after_prepare = Some(Box::new(move || { registry.begin_query(QueryDomain::Application, "1", 2).unwrap(); })); }
        let mut owner = None;
        let result = copy_host_with("4", || { owner = registry.consume_main_copy(&ticket).ok(); owner.is_some() }, || Ok(driver));
        if invalidate {
            assert_eq!(result, Err(CopyError::PermissionDenied)); assert_eq!(b.lock().unwrap().text, "initial");
            assert_eq!(b.lock().unwrap().trace, ["count", "prepare", "count"]);
        } else {
            assert_eq!(result, Ok(CopyOutcome::Accepted)); assert_eq!(b.lock().unwrap().text, "4");
            assert!(registry.finish_main_copy(&owner.unwrap()));
        }
    }
}
#[test]
fn trusted_host_changed_count_does_not_consume_action_or_claim_clipboard() {
    let b = board(); let mut driver = Fake::new(&b); let external_board = Arc::clone(&b);
    driver.after_prepare = Some(Box::new(move || external(&external_board, "B")));
    assert_eq!(copy_host_with("4", || panic!("nonmutating failure must leave ticket live"), || Ok(driver)), Err(CopyError::ClipboardChanged));
    assert_eq!(b.lock().unwrap().text, "B");
}
