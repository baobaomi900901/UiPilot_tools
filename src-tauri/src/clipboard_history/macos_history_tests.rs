use super::*;
use super::super::macos_store::tests::Temp;
const A: &str = "com.example.a";
const B: &str = "com.example.b";

#[test]
fn batch_does_not_upgrade_new_grants_or_revoked_generations() {
    let temp = Temp::new(); let history = MacHistory::open(&temp.0).unwrap();
    let old_a = history.authorize(A).unwrap();
    let batch = history.bind_capture().unwrap();
    let b = history.authorize(B).unwrap();
    let event = ClipboardEvent::synthetic_text("before b");
    assert_eq!(history.ingest_at(&event, batch, "t1").unwrap().accepted, 1);
    assert!(history.snapshot(&b, None).unwrap().entries.is_empty());
    let batch = history.bind_capture().unwrap();
    history.revoke(A).unwrap();
    let fresh_a = history.authorize(A).unwrap();
    assert!(!Arc::ptr_eq(&old_a.epoch, &fresh_a.epoch));
    assert!(matches!(history.snapshot(&old_a, None), Err(HistoryError::PermissionDenied)));
    let report = history.ingest_at(&ClipboardEvent::synthetic_text("new b only"), batch, "t2").unwrap();
    assert_eq!(report, IngestReport { accepted: 1, denied: 1, failed: 0 });
    assert_eq!(history.snapshot(&fresh_a, None).unwrap().entries.len(), 1);
}

#[test]
fn shutdown_releases_locks_even_while_old_tokens_and_instance_survive() {
    let temp = Temp::new(); let old = MacHistory::open(&temp.0).unwrap();
    let token = old.authorize(A).unwrap(); let batch = old.bind_capture().unwrap();
    old.shutdown();
    assert!(matches!(old.authorize(A), Err(HistoryError::Stopped)));
    let fresh = MacHistory::open(&temp.0).unwrap();
    let fresh_token = fresh.authorize(A).unwrap();
    assert!(matches!(fresh.snapshot(&token, None), Err(HistoryError::PermissionDenied)));
    assert!(matches!(fresh.ingest_unbound_for_test(&ClipboardEvent::synthetic_text("old"), batch), Err(HistoryError::PermissionDenied)));
    assert!(fresh.snapshot(&fresh_token, None).unwrap().entries.is_empty());
}

#[test]
fn revoked_tokens_hold_no_files_and_reauthorization_reloads() {
    let temp = Temp::new(); let history = MacHistory::open(&temp.0).unwrap();
    assert!(matches!(history.bind_capture(), Err(HistoryError::PermissionDenied)));
    let token = history.authorize(A).unwrap();
    let same = history.authorize(A).unwrap(); assert!(Arc::ptr_eq(&token.epoch, &same.epoch));
    let batch = history.bind_capture().unwrap();
    history.revoke(A).unwrap();
    let temporary = MacTextStore::open(&temp.0, A).unwrap(); drop(temporary);
    let fresh = history.authorize(A).unwrap();
    assert!(matches!(history.clear(&token), Err(HistoryError::PermissionDenied)));
    assert!(matches!(history.remove(&token, "1"), Err(HistoryError::PermissionDenied)));
    assert_eq!(history.ingest_unbound_for_test(&ClipboardEvent::synthetic_text("old"), batch).unwrap().denied, 1);
    assert!(history.snapshot(&fresh, None).unwrap().entries.is_empty());
}

#[test]
fn stopped_events_and_cross_instance_requests_do_no_history_io() {
    let first_temp = Temp::new(); let second_temp = Temp::new();
    let first = MacHistory::open(&first_temp.0).unwrap(); let second = MacHistory::open(&second_temp.0).unwrap();
    let token = first.authorize(A).unwrap(); let batch = first.bind_capture().unwrap();
    assert!(matches!(second.snapshot(&token, None), Err(HistoryError::PermissionDenied)));
    assert!(matches!(second.clear(&token), Err(HistoryError::PermissionDenied)));
    assert!(matches!(second.remove(&token, "1"), Err(HistoryError::PermissionDenied)));
    assert!(matches!(second.ingest_unbound_for_test(&ClipboardEvent::synthetic_text("foreign"), batch), Err(HistoryError::PermissionDenied)));
    assert!(!second_temp.0.join("clipboard-history").join(A).exists());
    let batch = first.bind_capture().unwrap(); let event = ClipboardEvent::synthetic_text("late"); event.request_stop();
    assert!(matches!(first.ingest_unbound_for_test(&event, batch), Err(HistoryError::PermissionDenied)));
    assert!(first.snapshot(&token, None).unwrap().entries.is_empty());
}

#[test]
fn revoke_drains_write_before_regrant_and_other_plugin_stays_available() {
    use std::{sync::mpsc, time::{Duration, Instant}, thread};
    let timeout = Duration::from_secs(5);
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let a = history.authorize(A).unwrap(); let b = history.authorize(B).unwrap();
    let batch = history.bind_capture().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    lock(&lock(&history.inner.state).stores[A]).before_publish = Some(Box::new(move || {
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(timeout).unwrap();
    }));
    let writer_history = Arc::clone(&history);
    let writer = thread::spawn(move || writer_history.ingest_unbound_for_test(&ClipboardEvent::synthetic_text("admitted"), batch));
    entered_rx.recv_timeout(timeout).unwrap();
    let revoker_history = Arc::clone(&history); let (revoked_tx, revoked_rx) = mpsc::channel();
    let revoker = thread::spawn(move || { revoker_history.revoke(A).unwrap(); revoked_tx.send(()).unwrap(); });
    let deadline = Instant::now() + timeout;
    while a.epoch.active.load(Ordering::Acquire) { assert!(Instant::now() < deadline); thread::yield_now(); }
    assert!(revoked_rx.recv_timeout(Duration::from_millis(20)).is_err());
    assert!(history.snapshot(&b, None).is_ok());
    let fresh_history = Arc::clone(&history); let (fresh_tx, fresh_rx) = mpsc::channel();
    let regrant = thread::spawn(move || { fresh_tx.send(fresh_history.authorize(A).unwrap()).unwrap(); });
    assert!(fresh_rx.recv_timeout(Duration::from_millis(20)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(writer.join().unwrap().unwrap().accepted, 2);
    revoked_rx.recv_timeout(timeout).unwrap(); revoker.join().unwrap();
    let fresh = fresh_rx.recv_timeout(timeout).unwrap(); regrant.join().unwrap();
    assert!(matches!(history.snapshot(&a, None), Err(HistoryError::PermissionDenied)));
    assert_eq!(history.snapshot(&fresh, None).unwrap().entries.len(), 1);
}

#[test]
fn shutdown_waits_for_write_and_reopen_starts_without_authority() {
    use std::{sync::mpsc, time::Duration, thread};
    let timeout = Duration::from_secs(5);
    let temp = Temp::new(); let history = Arc::new(MacHistory::open(&temp.0).unwrap());
    let token = history.authorize(A).unwrap(); let batch = history.bind_capture().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    lock(&lock(&history.inner.state).stores[A]).before_publish = Some(Box::new(move || {
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(timeout).unwrap();
    }));
    let writer_history = Arc::clone(&history);
    let writer = thread::spawn(move || writer_history.ingest_unbound_for_test(&ClipboardEvent::synthetic_text("admitted"), batch));
    entered_rx.recv_timeout(timeout).unwrap();
    let shutting_history = Arc::clone(&history); let (done_tx, done_rx) = mpsc::channel();
    let shutting = thread::spawn(move || { shutting_history.shutdown(); done_tx.send(()).unwrap(); });
    assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
    release_tx.send(()).unwrap(); writer.join().unwrap().unwrap(); done_rx.recv_timeout(timeout).unwrap(); shutting.join().unwrap();
    let fresh = MacHistory::open(&temp.0).unwrap();
    assert!(matches!(fresh.bind_capture(), Err(HistoryError::PermissionDenied)));
    assert!(matches!(fresh.snapshot(&token, None), Err(HistoryError::PermissionDenied)));
    let fresh_token = fresh.authorize(A).unwrap(); assert_eq!(fresh.snapshot(&fresh_token, None).unwrap().entries.len(), 1);
}

#[test]
fn coordinator_rejects_replaced_root_before_first_grant_and_after_revoke() {
    use std::{fs, os::unix::fs::PermissionsExt};
    for grant_first in [false, true] {
        let temp = Temp::new(); let history = MacHistory::open(&temp.0).unwrap();
        if grant_first { let _ = history.authorize(A).unwrap(); history.revoke(A).unwrap(); }
        let root = temp.0.join("clipboard-history");
        fs::rename(&root, temp.0.join("original")).unwrap();
        fs::create_dir(&root).unwrap(); fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(history.authorize(A), Err(HistoryError::Storage)));
        assert!(!root.join(A).exists());
    }
    let temp = Temp::new(); let history = MacHistory::open(&temp.0).unwrap();
    let original = temp.0.with_extension("original"); fs::rename(&temp.0, &original).unwrap();
    fs::create_dir(&temp.0).unwrap(); fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(temp.0.join("clipboard-history")).unwrap();
    fs::set_permissions(temp.0.join("clipboard-history"), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(history.authorize(A), Err(HistoryError::Storage)));
    assert!(!temp.0.join("clipboard-history").join(A).exists());
    fs::remove_dir_all(original).unwrap();
}

#[test]
fn query_error_is_preserved_only_after_authorization() {
    let temp = Temp::new(); let history = MacHistory::open(&temp.0).unwrap();
    let token = history.authorize(A).unwrap(); let invalid = "界".repeat(513);
    assert_eq!(history.snapshot(&token, Some(&invalid)), Err(HistoryError::InvalidQuery));
    assert!(history.snapshot(&token, Some(&"界".repeat(512))).is_ok());
    history.revoke(A).unwrap();
    assert_eq!(history.snapshot(&token, Some(&invalid)), Err(HistoryError::PermissionDenied));
}
