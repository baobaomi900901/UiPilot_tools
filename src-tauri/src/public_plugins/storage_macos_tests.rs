use super::super::{
    private_file_macos::{Failure, MAX_DOCUMENT},
    storage_document::STORAGE_QUOTA_BYTES,
};
use super::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{symlink, DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const A: &str = "com.example.alpha";
const B: &str = "com.example.beta";
struct Temp {
    container: PathBuf,
    root: PathBuf,
}
impl Temp {
    fn new() -> Self {
        let container = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "uipilot-plugin-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        mkdir(&container);
        let root = container.join("root");
        mkdir(&root);
        Self { container, root }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.container).unwrap();
    }
}
fn mkdir(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}
fn file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn authority(id: &str) -> (PluginDataScope, PluginDataCallIdentity) {
    (
        PluginDataScope::new(id).unwrap(),
        PluginDataCallIdentity::new(id, 1, 1, 1).unwrap(),
    )
}
fn fixture() -> (Temp, Arc<PluginDataCallGate>, PluginStorageStore) {
    let temp = Temp::new();
    let gate = Arc::new(PluginDataCallGate::default());
    let store = PluginStorageStore::load(&temp.root, gate.clone()).unwrap();
    (temp, gate, store)
}
#[test]
fn real_disk_roundtrip_isolation_remove_and_reopen() {
    let (temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    let (b, ib) = authority(B);
    gate.activate(ia.clone()).unwrap();
    gate.activate(ib.clone()).unwrap();
    store
        .set(
            &a,
            &ia,
            A,
            "value",
            json!({"中文":"😀  保留", "empty":"","zero":0,"false":false}),
        )
        .unwrap();
    store
        .set(&b, &ib, B, "value", json!([1, null, "B"]))
        .unwrap();
    assert_eq!(
        store.get(&a, &ib, A, "value"),
        Err(PluginStorageError::InvalidScope)
    );
    assert_eq!(
        store.get(&a, &ia, B, "value"),
        Err(PluginStorageError::InvalidScope)
    );
    assert_eq!(
        store.get(&b, &ib, B, "value").unwrap(),
        Some(json!([1, null, "B"]))
    );
    drop(store);
    let store = PluginStorageStore::load(&temp.root, gate).unwrap();
    assert_eq!(
        store.get(&a, &ia, A, "value").unwrap().unwrap()["中文"],
        json!("😀  保留")
    );
    store.remove(&a, &ia, A, "value").unwrap();
    store.remove(&a, &ia, A, "absent").unwrap();
    assert_eq!(store.get(&a, &ia, A, "value").unwrap(), None);
    assert_eq!(
        store.get(&b, &ib, B, "value").unwrap(),
        Some(json!([1, null, "B"]))
    );
}
#[test]
fn revoked_mixed_and_wrong_owner_do_zero_owner_io() {
    let (temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    let (_, ib) = authority(B);
    gate.activate(ib.clone()).unwrap();
    assert_eq!(
        store.set(&a, &ib, A, "v", json!(1)),
        Err(PluginStorageError::InvalidScope)
    );
    assert_eq!(
        store.set(&a, &ia, A, "v", json!(1)),
        Err(PluginStorageError::InvalidScope)
    );
    gate.activate(ia.clone()).unwrap();
    gate.close(&ia).unwrap().wait().unwrap();
    assert_eq!(
        store.get(&a, &ia, A, "v"),
        Err(PluginStorageError::InvalidScope)
    );
    assert_eq!(
        store.remove(&a, &ia, A, "v"),
        Err(PluginStorageError::InvalidScope)
    );
    assert!(!temp.root.join(A).exists());
    assert!(!temp.root.join(B).exists());
}
#[test]
fn exact_quota_and_shared_invalid_rules_preserve_memory_and_disk() {
    let (temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    gate.activate(ia.clone()).unwrap();
    let max = "x".repeat(STORAGE_QUOTA_BYTES - 8); // {"v":""} is eight compact bytes.
    store.set(&a, &ia, A, "v", json!(max)).unwrap();
    let path = temp.root.join(A).join("storage.json");
    let before = fs::read(&path).unwrap();
    assert_eq!(
        store.set(&a, &ia, A, "v", json!("x".repeat(STORAGE_QUOTA_BYTES - 7))),
        Err(PluginStorageError::QuotaExceeded)
    );
    assert_eq!(
        store.set(&a, &ia, A, "constructor", json!(true)),
        Err(PluginStorageError::InvalidKey)
    );
    assert_eq!(
        store.set(&a, &ia, A, "v", json!({"nested":{"__proto__":1}})),
        Err(PluginStorageError::InvalidValue)
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(store.get(&a, &ia, A, "v").unwrap(), Some(json!(max)));
    drop(store);
    let reopened = PluginStorageStore::load(&temp.root, gate).unwrap();
    assert!(reopened.get(&a, &ia, A, "v").unwrap().is_some());
}
#[test]
fn corrupted_and_oversized_current_is_never_empty_or_overwritten() {
    for bytes in [
        b"not json".to_vec(),
        br#"{"schema":2,"pluginId":"com.example.alpha","values":{}}"#.to_vec(),
        vec![b' '; MAX_DOCUMENT + 1],
    ] {
        let (temp, gate, store) = fixture();
        let (a, ia) = authority(A);
        gate.activate(ia.clone()).unwrap();
        mkdir(&temp.root.join(A));
        let path = temp.root.join(A).join("storage.json");
        file(&path, &bytes);
        assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
        assert_eq!(
            store.set(&a, &ia, A, "v", json!(1)),
            Err(PluginStorageError::Storage)
        );
        assert_eq!(
            store.remove(&a, &ia, A, "v"),
            Err(PluginStorageError::Storage)
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn pre_rename_failure_preserves_old_value_post_rename_poison_requires_reopen() {
    for failure in [Failure::BeforeRename, Failure::AfterRename] {
        let (temp, gate, store) = fixture();
        let (a, ia) = authority(A);
        gate.activate(ia.clone()).unwrap();
        store.set(&a, &ia, A, "v", json!("old")).unwrap();
        store
            .owners
            .lock()
            .unwrap()
            .get_mut(A)
            .unwrap()
            .file
            .failure = Some(failure);
        assert_eq!(
            store.set(&a, &ia, A, "v", json!("new")),
            Err(PluginStorageError::Storage)
        );
        if failure == Failure::BeforeRename {
            assert_eq!(store.get(&a, &ia, A, "v").unwrap(), Some(json!("old")));
        } else {
            assert_eq!(
                store.set(&a, &ia, A, "v", json!("retry")),
                Err(PluginStorageError::Storage)
            );
            assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
            assert_eq!(
                store.remove(&a, &ia, A, "v"),
                Err(PluginStorageError::Storage)
            );
        }
        drop(store);
        let reopened = PluginStorageStore::load(&temp.root, gate).unwrap();
        assert_eq!(
            reopened.get(&a, &ia, A, "v").unwrap(),
            Some(json!(if failure == Failure::BeforeRename {
                "old"
            } else {
                "new"
            }))
        );
    }
}
#[test]
fn root_owner_and_same_content_inode_replacement_fail_closed() {
    for level in ["root", "owner", "current", "lock"] {
        let (temp, gate, store) = fixture();
        let (a, ia) = authority(A);
        gate.activate(ia.clone()).unwrap();
        store.set(&a, &ia, A, "v", json!(1)).unwrap();
        let path = match level {
            "root" => temp.root.clone(),
            "owner" => temp.root.join(A),
            "lock" => temp.root.join(".storage.lock"),
            _ => temp.root.join(A).join("storage.json"),
        };
        let old = temp.container.join("old");
        let bytes = if path.is_file() {
            Some(fs::read(&path).unwrap())
        } else {
            None
        };
        fs::rename(&path, &old).unwrap();
        if let Some(bytes) = bytes {
            file(&path, &bytes);
        } else {
            mkdir(&path);
        }
        assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
        assert_eq!(
            store.set(&a, &ia, A, "v", json!(2)),
            Err(PluginStorageError::Storage)
        );
        if level == "root" {
            assert!(!path.join(A).exists());
        }
        if level == "owner" {
            assert!(!path.join("storage.json").exists());
        }
    }
}
#[test]
fn stable_lock_excludes_second_store_and_releases_on_drop() {
    let (temp, gate, store) = fixture();
    assert!(PluginStorageStore::load(&temp.root, gate.clone()).is_err());
    drop(store);
    assert!(PluginStorageStore::load(&temp.root, gate).is_ok());
}
#[test]
fn links_modes_and_safe_orphan_namespace_are_checked() {
    for kind in ["valid", "symlink", "hardlink", "mode"] {
        let (temp, gate, store) = fixture();
        let (a, ia) = authority(A);
        gate.activate(ia.clone()).unwrap();
        store.set(&a, &ia, A, "v", json!(1)).unwrap();
        drop(store);
        let stage = temp.root.join(A).join("storage.pending");
        let outside = temp.container.join("outside");
        file(&outside, b"private");
        match kind {
            "symlink" => symlink(&outside, &stage).unwrap(),
            "hardlink" => fs::hard_link(&outside, &stage).unwrap(),
            _ => file(&stage, b"orphan"),
        };
        if kind == "mode" {
            fs::set_permissions(&stage, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let reopened = PluginStorageStore::load(&temp.root, gate).unwrap();
        if kind == "valid" {
            assert_eq!(reopened.get(&a, &ia, A, "v").unwrap(), Some(json!(1)));
            assert!(!stage.exists());
        } else {
            assert_eq!(
                reopened.get(&a, &ia, A, "v"),
                Err(PluginStorageError::Storage)
            );
            assert!(fs::symlink_metadata(stage).is_ok());
        }
        assert_eq!(fs::read(outside).unwrap(), b"private");
    }
    let temp = Temp::new();
    fs::set_permissions(&temp.root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(PluginStorageStore::load(&temp.root, Arc::new(PluginDataCallGate::default())).is_err());
}
#[test]
fn revoke_blocks_new_io_and_waits_for_the_admitted_write() {
    let (_temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    gate.activate(ia.clone()).unwrap();
    store.set(&a, &ia, A, "v", json!("old")).unwrap();
    let store = Arc::new(store);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    store
        .owners
        .lock()
        .unwrap()
        .get_mut(A)
        .unwrap()
        .file
        .before_publish = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let writer = {
        let store = store.clone();
        let a = a.clone();
        let ia = ia.clone();
        thread::spawn(move || store.set(&a, &ia, A, "v", json!("admitted")))
    };
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let drain = gate.close(&ia).unwrap();
    assert!(!drain.is_drained().unwrap());
    assert_eq!(
        store.set(&a, &ia, A, "v", json!("late")),
        Err(PluginStorageError::InvalidScope)
    );
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        drain.wait().unwrap();
        done_tx.send(()).unwrap();
    });
    // The blocked before-publish hook proves an actual lease is still held.
    assert!(done_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    waiter.join().unwrap();
    let fresh = PluginDataCallIdentity::new(A, 2, 2, 2).unwrap();
    gate.activate(fresh.clone()).unwrap();
    assert_eq!(
        store.get(&a, &ia, A, "v"),
        Err(PluginStorageError::InvalidScope)
    );
    assert_eq!(
        store.get(&a, &fresh, A, "v").unwrap(),
        Some(json!("admitted"))
    );
}
#[test]
fn root_replaced_before_first_owner_and_missing_current_created_later_are_rejected() {
    let (temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    gate.activate(ia.clone()).unwrap();
    fs::rename(&temp.root, temp.container.join("old-root")).unwrap();
    mkdir(&temp.root);
    assert_eq!(
        store.set(&a, &ia, A, "v", json!(1)),
        Err(PluginStorageError::Storage)
    );
    assert!(!temp.root.join(A).exists());
    drop(store);
    let store = PluginStorageStore::load(&temp.root, gate).unwrap();
    mkdir(&temp.root.join(A));
    assert_eq!(store.get(&a, &ia, A, "v").unwrap(), None);
    file(
        &temp.root.join(A).join("storage.json"),
        br#"{"schema":1,"pluginId":"com.example.alpha","values":{"v":1}}"#,
    );
    assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
}
#[test]
fn external_in_place_change_cannot_be_masked_by_cache() {
    let (temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    gate.activate(ia.clone()).unwrap();
    store.set(&a, &ia, A, "v", json!(1)).unwrap();
    let path = temp.root.join(A).join("storage.json");
    file(&path, b"damaged");
    assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
    assert_eq!(
        store.remove(&a, &ia, A, "v"),
        Err(PluginStorageError::Storage)
    );
    assert_eq!(fs::read(&path).unwrap(), b"damaged");
}
#[test]
fn duplicate_raw_keys_are_corrupt_in_mac_private_files() {
    for bytes in [
        br#"{"schema":1,"pluginId":"com.example.alpha","values":{"v":1,"v":2}}"#.as_slice(),
        br#"{"schema":1,"pluginId":"com.example.alpha","values":{"v":{"nested":1,"nested":2}}}"#
            .as_slice(),
    ] {
        let (temp, gate, store) = fixture();
        let (a, ia) = authority(A);
        gate.activate(ia.clone()).unwrap();
        mkdir(&temp.root.join(A));
        let path = temp.root.join(A).join("storage.json");
        file(&path, bytes);
        assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn current_unsafe_types_and_owner_links_reject_without_touching_targets() {
    for kind in ["symlink", "hardlink", "mode", "fifo"] {
        let (temp, gate, store) = fixture();
        let (a, ia) = authority(A);
        gate.activate(ia.clone()).unwrap();
        mkdir(&temp.root.join(A));
        let outside = temp.container.join("outside");
        file(&outside, b"must remain");
        let current = temp.root.join(A).join("storage.json");
        match kind {
            "symlink" => symlink(&outside, &current).unwrap(),
            "hardlink" => fs::hard_link(&outside, &current).unwrap(),
            "mode" => {
                file(&current, b"private");
                fs::set_permissions(&current, fs::Permissions::from_mode(0o644)).unwrap();
            }
            _ => {
                let path = std::ffi::CString::new(current.to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
        }
        assert_eq!(store.get(&a, &ia, A, "v"), Err(PluginStorageError::Storage));
        assert_eq!(
            store.set(&a, &ia, A, "v", json!(1)),
            Err(PluginStorageError::Storage)
        );
        assert_eq!(fs::read(outside).unwrap(), b"must remain");
        assert!(!temp.root.join(A).join("storage.pending").exists());
    }
    let (temp, gate, store) = fixture();
    let (a, ia) = authority(A);
    gate.activate(ia.clone()).unwrap();
    let outside = temp.container.join("outside-owner");
    mkdir(&outside);
    symlink(&outside, temp.root.join(A)).unwrap();
    assert_eq!(
        store.set(&a, &ia, A, "v", json!(1)),
        Err(PluginStorageError::Storage)
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
#[test]
fn stable_lock_rejects_hardlinked_or_nonprivate_files() {
    for mode in [0o600, 0o644] {
        let temp = Temp::new();
        let outside = temp.container.join("outside-lock");
        file(&outside, b"lock");
        let lock = temp.root.join(".storage.lock");
        if mode == 0o600 {
            fs::hard_link(&outside, &lock).unwrap();
        } else {
            file(&lock, b"lock");
            fs::set_permissions(&lock, fs::Permissions::from_mode(mode)).unwrap();
        }
        assert!(
            PluginStorageStore::load(&temp.root, Arc::new(PluginDataCallGate::default())).is_err()
        );
        assert_eq!(fs::read(outside).unwrap(), b"lock");
    }
}
