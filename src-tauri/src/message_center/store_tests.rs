use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use serde_json::json;

use super::store::{
    AtomicMessageCommitter, MessageClock, MessageCommitter, MessageStore, MessageStoreError,
    PublishInput,
};
use crate::atomic_file::{commit_with_backup, AtomicFileError, AtomicPaths};

const FIRST_TIME: &str = "2026-08-19T01:02:03Z";
const SECOND_TIME: &str = "2026-08-19T01:03:04Z";

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-message-center-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn current(&self) -> PathBuf {
        self.0.join("messages.json")
    }

    fn backup(&self) -> PathBuf {
        self.0.join("messages.json.backup")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[derive(Clone)]
struct FixedClock(&'static str);

impl MessageClock for FixedClock {
    fn now_utc_rfc3339(&self) -> Result<String, MessageStoreError> {
        Ok(self.0.into())
    }
}

struct FailNextCommitter {
    fail_next: AtomicBool,
}

impl FailNextCommitter {
    fn new() -> Self {
        Self {
            fail_next: AtomicBool::new(false),
        }
    }

    fn fail_next(&self) {
        self.fail_next.store(true, Ordering::SeqCst);
    }
}

impl MessageCommitter for FailNextCommitter {
    fn commit(
        &self,
        paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(AtomicFileError::CandidateWrite);
        }
        commit_with_backup(paths, previous, candidate)
    }
}

fn input(content: impl Into<String>) -> PublishInput {
    PublishInput {
        plugin_id: "com.example.messages".into(),
        plugin_name_snapshot: "Messages Example".into(),
        content: content.into(),
    }
}

fn load_store(dir: &TestDir) -> MessageStore {
    MessageStore::load_with(
        dir.path(),
        Arc::new(AtomicMessageCommitter),
        Arc::new(FixedClock(FIRST_TIME)),
    )
}

#[test]
fn first_publish_persists_and_restart_recovers() {
    let dir = TestDir::new("first-publish");
    let store = load_store(&dir);

    let published = store.publish(input("first")).unwrap();

    assert_eq!(published.record.id, "1");
    assert_eq!(published.record.created_at, FIRST_TIME);
    assert_eq!(published.summary.revision, "1");
    assert_eq!(published.summary.unread_count, 1);

    let restarted = load_store(&dir).read_snapshot().unwrap();
    assert_eq!(restarted.revision, "1");
    assert_eq!(restarted.unread_count, 1);
    assert_eq!(restarted.messages, vec![published.record]);
}

#[test]
fn publishing_101_messages_evicts_only_the_lowest_id() {
    let dir = TestDir::new("eviction");
    let store = load_store(&dir);

    for number in 1..=101 {
        store.publish(input(format!("message {number}"))).unwrap();
    }

    let snapshot = store.read_snapshot().unwrap();
    assert_eq!(snapshot.revision, "101");
    assert_eq!(snapshot.unread_count, 100);
    assert_eq!(snapshot.messages.len(), 100);
    assert_eq!(snapshot.messages.first().unwrap().id, "2");
    assert_eq!(snapshot.messages.last().unwrap().id, "101");
}

#[test]
fn open_cutoff_marks_only_messages_that_already_exist() {
    let dir = TestDir::new("open-cutoff");
    let store = load_store(&dir);
    store.publish(input("before open")).unwrap();

    let opened = store.open_and_mark_read().unwrap();
    assert_eq!(opened.revision, "2");
    assert_eq!(opened.unread_count, 0);
    assert_eq!(opened.messages[0].read_at.as_deref(), Some(FIRST_TIME));

    let later = MessageStore::load_with(
        dir.path(),
        Arc::new(AtomicMessageCommitter),
        Arc::new(FixedClock(SECOND_TIME)),
    );
    later.publish(input("after open")).unwrap();

    let snapshot = later.read_snapshot().unwrap();
    assert_eq!(snapshot.revision, "3");
    assert_eq!(snapshot.unread_count, 1);
    assert_eq!(snapshot.messages[0].read_at.as_deref(), Some(FIRST_TIME));
    assert_eq!(snapshot.messages[1].read_at, None);
}

#[test]
fn clear_then_later_publish_preserves_the_new_unread_message() {
    let dir = TestDir::new("clear-race");
    let store = load_store(&dir);
    store.publish(input("old")).unwrap();

    let cleared = store.clear().unwrap();
    assert_eq!(cleared.revision, "2");
    assert!(cleared.messages.is_empty());

    store.publish(input("new")).unwrap();
    let snapshot = store.read_snapshot().unwrap();
    assert_eq!(snapshot.revision, "3");
    assert_eq!(snapshot.unread_count, 1);
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].id, "2");
    assert_eq!(snapshot.messages[0].content, "new");
}

#[test]
fn corrupt_current_recovers_the_valid_backup() {
    let dir = TestDir::new("backup-recovery");
    let store = load_store(&dir);
    store.publish(input("backup value")).unwrap();
    store.publish(input("current value")).unwrap();
    fs::write(dir.current(), b"not-json").unwrap();

    let recovered = load_store(&dir).read_snapshot().unwrap();

    assert_eq!(recovered.revision, "1");
    assert_eq!(recovered.messages.len(), 1);
    assert_eq!(recovered.messages[0].content, "backup value");
}

#[test]
fn corrupt_current_and_backup_are_preserved_and_make_the_store_unavailable() {
    let dir = TestDir::new("both-corrupt");
    let current = b"bad-current";
    let backup = b"bad-backup";
    fs::write(dir.current(), current).unwrap();
    fs::write(dir.backup(), backup).unwrap();
    let store = load_store(&dir);

    assert_eq!(store.summary(), Err(MessageStoreError::Unavailable));
    assert_eq!(store.read_snapshot(), Err(MessageStoreError::Unavailable));
    assert_eq!(
        store.open_and_mark_read(),
        Err(MessageStoreError::Unavailable)
    );
    assert_eq!(store.clear(), Err(MessageStoreError::Unavailable));
    assert_eq!(
        store.publish(input("ignored")),
        Err(MessageStoreError::Unavailable)
    );
    assert_eq!(fs::read(dir.current()).unwrap(), current);
    assert_eq!(fs::read(dir.backup()).unwrap(), backup);
}

#[test]
fn transient_write_failure_preserves_the_ready_snapshot() {
    let dir = TestDir::new("write-failure");
    let committer = Arc::new(FailNextCommitter::new());
    let store = MessageStore::load_with(
        dir.path(),
        committer.clone(),
        Arc::new(FixedClock(FIRST_TIME)),
    );
    store.publish(input("kept")).unwrap();
    let before = fs::read(dir.current()).unwrap();
    committer.fail_next();

    assert_eq!(
        store.publish(input("not committed")),
        Err(MessageStoreError::OperationFailed)
    );
    let snapshot = store.read_snapshot().unwrap();
    assert_eq!(snapshot.revision, "1");
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].content, "kept");
    assert_eq!(fs::read(dir.current()).unwrap(), before);

    let recovered = store.publish(input("later succeeds")).unwrap();
    assert_eq!(recovered.summary.revision, "2");
}

#[test]
fn id_or_revision_exhaustion_transitions_once_without_wrapping() {
    for (label, revision, next_message_id) in [
        ("id-exhaustion", "0".to_string(), u64::MAX.to_string()),
        ("revision-exhaustion", u64::MAX.to_string(), "1".to_string()),
    ] {
        let dir = TestDir::new(label);
        let document = json!({
            "schema": 1,
            "revision": revision,
            "nextMessageId": next_message_id,
            "messages": []
        });
        let original = serde_json::to_vec(&document).unwrap();
        fs::write(dir.current(), &original).unwrap();
        let store = load_store(&dir);

        assert_eq!(
            store.publish(input("cannot commit")),
            Err(MessageStoreError::BecameUnavailable)
        );
        assert_eq!(
            store.publish(input("still unavailable")),
            Err(MessageStoreError::Unavailable)
        );
        assert_eq!(store.summary(), Err(MessageStoreError::Unavailable));
        assert_eq!(fs::read(dir.current()).unwrap(), original);
    }
}
