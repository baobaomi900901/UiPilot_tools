use super::*;
use std::{fs, os::unix::fs::{PermissionsExt, symlink}, sync::atomic::{AtomicU64, Ordering}};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(in crate::clipboard_history) struct Temp(pub(in crate::clipboard_history) PathBuf);
impl Temp {
    pub(in crate::clipboard_history) fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!("uipilot-history-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

#[test]
fn private_restart_and_failure_preserve_confirmed_history() {
    let temp = Temp::new();
    let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    store.capture("fixture".into(), "t1".into()).unwrap();
    assert_eq!(store.snapshot(None).unwrap().revision, "1");
    drop(store);
    let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    assert_eq!(store.snapshot(None).unwrap().entries.len(), 1);
    store.fail_next = Some(FailurePoint::BeforeRename);
    assert!(store.capture("failed".into(), "t2".into()).is_err());
    assert_eq!(store.snapshot(None).unwrap().revision, "1");
    drop(store);
    let store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    assert_eq!(store.snapshot(None).unwrap().revision, "1");
}

fn index(temp: &Temp) -> PathBuf { temp.0.join(ROOT).join("com.example.a").join(CURRENT) }
fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap(); fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
#[test]
fn post_rename_failure_poison_requires_reopen_and_does_not_claim_rollback() {
    let temp = Temp::new(); let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    store.capture("old".into(), "t".into()).unwrap();
    store.fail_next = Some(FailurePoint::AfterRename);
    assert_eq!(store.capture("new".into(), "t".into()), Err(StoreError::CommitUncertain));
    assert_eq!(store.snapshot(None), Err(StoreError::CommitUncertain));
    assert_eq!(store.clear(), Err(StoreError::CommitUncertain));
    drop(store);
    let store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    assert_eq!(store.snapshot(None).unwrap().revision, "2");
}
#[test]
fn unsafe_ids_links_hardlinks_permissions_and_directory_replacement_fail_closed() {
    let temp = Temp::new();
    for id in ["", "../other", "/tmp/x", "a/b", "..", "Upper"] { assert!(MacTextStore::open(&temp.0, id).is_err()); }
    assert!(!temp.0.join(ROOT).exists());
    let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    assert!(MacTextStore::open(&temp.0, "com.example.a").is_err());
    store.capture("old".into(), "t".into()).unwrap();
    let path = index(&temp); let outside = temp.0.join("outside"); write_private(&outside, b"sentinel");
    fs::remove_file(&path).unwrap(); symlink(&outside, &path).unwrap();
    assert!(store.clear().is_err()); assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
    fs::remove_file(&path).unwrap(); fs::hard_link(&outside, &path).unwrap();
    assert!(store.clear().is_err()); assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
    fs::remove_file(&path).unwrap(); write_private(&path, store.raw.as_ref().unwrap());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(); assert!(store.snapshot(None).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let root = temp.0.join(ROOT); fs::rename(&root, temp.0.join("original-root")).unwrap();
    fs::create_dir(&root).unwrap(); fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(store.clear().is_err()); assert!(!root.join("com.example.a").exists());
}
#[test]
fn replacing_app_data_path_or_adding_acl_blocks_further_access() {
    let temp = Temp::new(); let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    store.capture("old".into(), "t".into()).unwrap();
    let status = std::process::Command::new("/bin/chmod").args(["+a", "everyone allow read"]).arg(index(&temp)).status().unwrap();
    assert!(status.success()); assert!(store.snapshot(None).is_err());
    let original = temp.0.with_extension("original"); fs::rename(&temp.0, &original).unwrap();
    fs::create_dir(&temp.0).unwrap(); fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(store.clear().is_err()); assert!(!temp.0.join(ROOT).exists());
    fs::remove_dir_all(original).unwrap();
}
#[test]
fn only_safe_reserved_orphan_is_discarded_without_recovery() {
    let temp = Temp::new(); let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    store.capture("confirmed".into(), "t".into()).unwrap(); drop(store);
    let stage = index(&temp).with_file_name(STAGE); write_private(&stage, b"unfinished private bytes");
    let store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    assert_eq!(store.snapshot(None).unwrap().revision, "1"); assert!(!stage.exists()); drop(store);
    let outside = temp.0.join("outside"); write_private(&outside, b"untouched"); symlink(&outside, &stage).unwrap();
    assert!(MacTextStore::open(&temp.0, "com.example.a").is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"untouched"); assert!(fs::symlink_metadata(stage).unwrap().file_type().is_symlink());
}
#[test]
fn corrupt_or_oversized_current_is_preserved_not_reset() {
    let temp = Temp::new(); drop(MacTextStore::open(&temp.0, "com.example.a").unwrap());
    let path = index(&temp); write_private(&path, b"{bad-json");
    assert!(matches!(MacTextStore::open(&temp.0, "com.example.a"), Err(StoreError::Corrupt)));
    assert_eq!(fs::read(&path).unwrap(), b"{bad-json");
    let file = fs::OpenOptions::new().write(true).open(path).unwrap(); file.set_len((MAX_INDEX + 1) as u64).unwrap();
    assert!(matches!(MacTextStore::open(&temp.0, "com.example.a"), Err(StoreError::Corrupt)));
}
#[test]
fn document_rejects_noncanonical_counts_duplicates_payload_and_forged_preview() {
    let mut good = HistoryDocument::default(); document::capture_text(&mut good, "text".into(), "t".into()).unwrap();
    let mut cases = Vec::new();
    let mut value = good.clone(); value.revision = "+1".into(); cases.push(value);
    let mut value = good.clone(); value.next_id = "1".into(); cases.push(value);
    let mut value = good.clone(); value.entries.push(value.entries[0].clone()); cases.push(value);
    let mut value = good.clone(); value.entries[0].fingerprint = "0".repeat(64); cases.push(value);
    let mut value = good.clone(); value.entries[0].payload = HistoryEntryPayload::Text { text: "text".into(), text_preview: "forged".into() }; cases.push(value);
    let mut value = good.clone(); value.entries[0].payload = HistoryEntryPayload::Files { paths: vec![] }; cases.push(value);
    let mut value = good.clone(); value.next_recency_rank = "01".into(); cases.push(value);
    for value in cases { assert_eq!(validate_document(&value), Err(StoreError::Corrupt)); }
}
#[test]
fn text_and_serialized_budgets_evict_oldest_without_reusing_ids() {
    let temp = Temp::new(); let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    store.capture("\0".repeat(MAX_TEXT), "t1".into()).unwrap();
    store.capture(format!("x{}", "\0".repeat(MAX_TEXT - 1)), "t2".into()).unwrap();
    let snapshot = store.snapshot(None).unwrap(); assert_eq!(snapshot.entries.len(), 1); assert_eq!(snapshot.entries[0].id(), "2");
    assert!(fs::metadata(index(&temp)).unwrap().len() <= MAX_INDEX as u64);
    assert!(store.capture("a".repeat(MAX_TEXT + 1), "t".into()).is_err());
    assert_eq!(store.snapshot(None).unwrap(), snapshot);
    store.clear().unwrap(); store.capture("later".into(), "t3".into()).unwrap();
    assert_eq!(store.snapshot(None).unwrap().entries[0].id(), "3");
}

#[test]
fn store_drop_releases_flock_even_if_descriptor_was_inherited() {
    let temp = Temp::new(); let store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
    let inherited = rustix::io::dup(&store.directory).unwrap();
    drop(store);
    let reopened = MacTextStore::open(&temp.0, "com.example.a");
    assert!(reopened.is_ok(), "closed authority must release flock despite inherited fd");
    drop(inherited);
}

#[test]
fn unsafe_orphan_hardlink_and_public_mode_are_not_deleted() {
    let temp = Temp::new(); drop(MacTextStore::open(&temp.0, "com.example.a").unwrap());
    let stage = index(&temp).with_file_name(STAGE); let outside = temp.0.join("other");
    write_private(&outside, b"untouched"); fs::hard_link(&outside, &stage).unwrap();
    assert!(MacTextStore::open(&temp.0, "com.example.a").is_err()); assert!(stage.exists());
    fs::remove_file(&stage).unwrap(); write_private(&stage, b"unsafe stage");
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(MacTextStore::open(&temp.0, "com.example.a").is_err()); assert!(stage.exists());
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
}

#[test]
fn counter_overflow_does_not_publish_partial_candidate_or_reuse_ids() {
    for counter in ["revision", "id", "rank"] {
        let temp = Temp::new(); drop(MacTextStore::open(&temp.0, "com.example.a").unwrap());
        let mut document = HistoryDocument::default();
        document::capture_text(&mut document, "confirmed".into(), "t".into()).unwrap();
        match counter { "revision" => document.revision = u64::MAX.to_string(),
            "id" => document.next_id = u64::MAX.to_string(), _ => document.next_recency_rank = u64::MAX.to_string() }
        let raw = serde_json::to_vec(&document).unwrap(); write_private(&index(&temp), &raw);
        let mut store = MacTextStore::open(&temp.0, "com.example.a").unwrap();
        let snapshot = store.snapshot(None).unwrap();
        assert!(store.capture("must not persist".into(), "new time".into()).is_err());
        assert_eq!(store.snapshot(None).unwrap(), snapshot);
        assert_eq!(fs::read(index(&temp)).unwrap(), raw);
    }
}
