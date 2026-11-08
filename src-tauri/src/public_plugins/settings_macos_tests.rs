use super::super::{private_file_macos::Failure, storage_document::STORAGE_QUOTA_BYTES};
use super::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
const ID: &str = "com.example.settings";
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "uipilot-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn defs() -> Vec<PublicSettingV1> {
    serde_json::from_value(json!([
     {"type":"text","key":"text","label":"Text","default":"default"},
     {"type":"number","key":"count","label":"Count","min":0,"max":10,"step":2,"default":4},
     {"type":"boolean","key":"enabled","label":"Enabled","default":true},
     {"type":"text","key":"optional","label":"Optional"},
     {"type":"text","key":"constructor","label":"Legal setting key"},
     {"type":"secret","key":"secret","label":"Secret"}
    ]))
    .unwrap()
}
fn updates(v: Value) -> BTreeMap<String, Value> {
    serde_json::from_value(v).unwrap()
}
fn fixture() -> (
    Temp,
    Arc<PluginDataCallGate>,
    MacSettingsStore,
    PluginDataScope,
    PluginDataCallIdentity,
) {
    let temp = Temp::new();
    let gate = Arc::new(PluginDataCallGate::default());
    let scope = PluginDataScope::new(ID).unwrap();
    let id = PluginDataCallIdentity::new(ID, 1, 1, 1).unwrap();
    gate.activate(id.clone()).unwrap();
    let store = MacSettingsStore::load(&temp.0, gate.clone()).unwrap();
    (temp, gate, store, scope, id)
}
#[test]
fn real_defaults_save_false_zero_empty_and_reopen() {
    let (temp, gate, store, scope, id) = fixture();
    let first = store.configure(&scope, &id, &defs()).unwrap();
    assert_eq!(
        store.setting(&scope, &id, &first, "text").unwrap(),
        Some(json!("default"))
    );
    assert_eq!(
        store.setting(&scope, &id, &first, "optional").unwrap(),
        None
    );
    let next = store
        .save(
            &scope,
            &id,
            &first,
            &updates(json!({"text":"","count":0,"enabled":false,"constructor":"中文 😀"})),
        )
        .unwrap();
    assert_eq!(
        store.setting(&scope, &id, &first, "text"),
        Err(SettingsError::InvalidScope)
    );
    for (key, value) in [
        ("text", json!("")),
        ("count", json!(0)),
        ("enabled", json!(false)),
        ("constructor", json!("中文 😀")),
    ] {
        assert_eq!(store.setting(&scope, &id, &next, key).unwrap(), Some(value));
    }
    assert!(!temp.0.join(ID).join("storage.json").exists());
    let disk = fs::read(temp.0.join(ID).join("settings.json")).unwrap();
    assert!(!String::from_utf8(disk).unwrap().contains("secret"));
    drop(store);
    let store = MacSettingsStore::load(&temp.0, gate).unwrap();
    let reopened = store.configure(&scope, &id, &defs()).unwrap();
    assert_eq!(
        store
            .setting(&scope, &id, &reopened, "constructor")
            .unwrap(),
        Some(json!("中文 😀"))
    );
    assert_eq!(
        store.setting(&scope, &id, &next, "text"),
        Err(SettingsError::InvalidScope)
    );
}
#[test]
fn invalid_definitions_reject_before_owner_io() {
    let (temp, _gate, store, scope, id) = fixture();
    let mut cases = Vec::new();
    let mut duplicate = defs();
    duplicate.push(duplicate[0].clone());
    cases.push(duplicate);
    for value in [
        json!({"type":"text","key":"Upper","label":"x"}),
        json!({"type":"number","key":"n","label":"n","min":10,"max":1}),
        json!({"type":"select","key":"s","label":"s","options":[]}),
        json!({"type":"boolean","key":"b","label":""}),
    ] {
        cases.push(vec![serde_json::from_value(value).unwrap()]);
    }
    for invalid in cases {
        assert!(matches!(
            store.configure(&scope, &id, &invalid),
            Err(SettingsError::InvalidDefinition)
        ));
        assert!(!temp.0.join(ID).exists());
    }
}
#[test]
fn invalid_batch_secret_unknown_and_quota_preserve_all_old_values() {
    let (temp, _gate, store, scope, id) = fixture();
    let handle = store.configure(&scope, &id, &defs()).unwrap();
    let path = temp.0.join(ID).join("settings.json");
    let before = fs::read(&path).unwrap();
    for bad in [
        json!({"text":"should not save","count":11}),
        json!({"secret":"never persist"}),
        json!({"unknown":true}),
        json!({"enabled":1}),
        json!({"text":"bad\u{0000}"}),
    ] {
        assert!(matches!(
            store.save(&scope, &id, &handle, &updates(bad)),
            Err(SettingsError::InvalidSetting)
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    assert!(matches!(
        store.save(
            &scope,
            &id,
            &handle,
            &updates(json!({"text":"x".repeat(STORAGE_QUOTA_BYTES)}))
        ),
        Err(SettingsError::QuotaExceeded)
    ));
    assert_eq!(
        store.setting(&scope, &id, &handle, "secret"),
        Err(SettingsError::InvalidSetting)
    );
    assert_eq!(
        store.setting(&scope, &id, &handle, "unknown"),
        Err(SettingsError::InvalidSetting)
    );
    assert_eq!(
        store.setting(&scope, &id, &handle, "text").unwrap(),
        Some(json!("default"))
    );
}
#[test]
fn upgrade_reconciles_defaults_removes_obsolete_and_rotates_handle() {
    let (_temp, _gate, store, scope, id) = fixture();
    let h = store.configure(&scope, &id, &defs()).unwrap();
    let h = store
        .save(
            &scope,
            &id,
            &h,
            &updates(json!({"text":"kept","count":9,"optional":"remove"})),
        )
        .unwrap();
    let mut next = defs();
    next.retain(|d| d.key() != "optional");
    next[1] = serde_json::from_value(
        json!({"type":"number","key":"count","label":"N","max":5,"default":2}),
    )
    .unwrap();
    let new = store.configure(&scope, &id, &next).unwrap();
    assert_eq!(
        store.setting(&scope, &id, &h, "text"),
        Err(SettingsError::InvalidScope)
    );
    assert_eq!(
        store.setting(&scope, &id, &new, "text").unwrap(),
        Some(json!("kept"))
    );
    assert_eq!(
        store.setting(&scope, &id, &new, "count").unwrap(),
        Some(json!(2.0))
    );
    assert_eq!(
        store.setting(&scope, &id, &new, "optional"),
        Err(SettingsError::InvalidSetting)
    );
    let same = store
        .save(&scope, &id, &new, &updates(json!({"text":"kept"})))
        .unwrap();
    assert!(Arc::ptr_eq(&new.generation, &same.generation));
    assert!(store.setting(&scope, &id, &new, "text").is_ok());
}
#[test]
fn foreign_handles_wrong_scope_and_closed_identity_do_zero_owner_io() {
    let (_temp, gate, store, scope, id) = fixture();
    let handle = store.configure(&scope, &id, &defs()).unwrap();
    let other = Temp::new();
    let foreign = MacSettingsStore::load(&other.0, gate.clone()).unwrap();
    assert_eq!(
        foreign.setting(&scope, &id, &handle, "text"),
        Err(SettingsError::InvalidScope)
    );
    assert!(matches!(
        foreign.save(&scope, &id, &handle, &BTreeMap::new()),
        Err(SettingsError::InvalidScope)
    ));
    assert!(!other.0.join(ID).exists());
    let wrong = PluginDataScope::new("com.example.other").unwrap();
    assert_eq!(
        store.setting(&wrong, &id, &handle, "text"),
        Err(SettingsError::InvalidScope)
    );
    gate.close(&id).unwrap().wait().unwrap();
    assert_eq!(
        store.setting(&scope, &id, &handle, "text"),
        Err(SettingsError::InvalidScope)
    );
    let fresh = PluginDataCallIdentity::new(ID, 2, 2, 2).unwrap();
    gate.activate(fresh.clone()).unwrap();
    assert_eq!(
        store.setting(&scope, &fresh, &handle, "text"),
        Err(SettingsError::InvalidScope)
    );
    let next = store.configure(&scope, &fresh, &defs()).unwrap();
    assert_eq!(
        store.setting(&scope, &id, &next, "text"),
        Err(SettingsError::InvalidScope)
    );
}
#[test]
fn commit_failures_do_not_publish_new_handle_and_uncertainty_poison_persists() {
    for failure in [Failure::BeforeRename, Failure::AfterRename] {
        let (temp, gate, store, scope, id) = fixture();
        let old = store.configure(&scope, &id, &defs()).unwrap();
        store
            .owners
            .lock()
            .unwrap()
            .get_mut(ID)
            .unwrap()
            .file
            .failure = Some(failure);
        assert!(matches!(
            store.save(&scope, &id, &old, &updates(json!({"text":"changed"}))),
            Err(SettingsError::Storage)
        ));
        if failure == Failure::BeforeRename {
            assert_eq!(
                store.setting(&scope, &id, &old, "text").unwrap(),
                Some(json!("default"))
            );
        } else {
            assert_eq!(
                store.setting(&scope, &id, &old, "text"),
                Err(SettingsError::Storage)
            );
            assert!(matches!(
                store.save(&scope, &id, &old, &BTreeMap::new()),
                Err(SettingsError::Storage)
            ));
            assert!(matches!(
                store.configure(&scope, &id, &defs()),
                Err(SettingsError::Storage)
            ));
        }
        drop(store);
        let store = MacSettingsStore::load(&temp.0, gate).unwrap();
        let handle = store.configure(&scope, &id, &defs()).unwrap();
        assert_eq!(
            store.setting(&scope, &id, &handle, "text").unwrap(),
            Some(json!(if failure == Failure::BeforeRename {
                "default"
            } else {
                "changed"
            }))
        );
    }
}
#[test]
fn configure_failure_keeps_old_definition_and_handle() {
    let (_temp, _gate, store, scope, id) = fixture();
    let old = store.configure(&scope, &id, &defs()).unwrap();
    let mut next = defs();
    next[0] =
        serde_json::from_value(json!({"type":"boolean","key":"text","label":"T","default":false}))
            .unwrap();
    store
        .owners
        .lock()
        .unwrap()
        .get_mut(ID)
        .unwrap()
        .file
        .failure = Some(Failure::BeforeRename);
    assert!(matches!(
        store.configure(&scope, &id, &next),
        Err(SettingsError::Storage)
    ));
    assert_eq!(
        store.setting(&scope, &id, &old, "text").unwrap(),
        Some(json!("default"))
    );
}
#[test]
fn damaged_duplicate_and_oversized_documents_are_not_reset() {
    for bytes in [
        b"bad json".to_vec(),
        br#"{"schema":1,"pluginId":"com.example.settings","revision":1,"values":{"text":"a","text":"b"}}"#
            .to_vec(),
        vec![b' '; STORAGE_QUOTA_BYTES + 257],
    ] {
        let (temp, _gate, store, scope, id) = fixture();
        let owner = temp.0.join(ID);
        fs::DirBuilder::new().mode(0o700).create(&owner).unwrap();
        let path = owner.join("settings.json");
        fs::write(&path, &bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            store.configure(&scope, &id, &defs()),
            Err(SettingsError::Storage)
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
#[test]
fn revoke_waits_for_admitted_save_and_old_handles_do_not_rebind() {
    let (_temp, gate, store, scope, id) = fixture();
    let handle = store.configure(&scope, &id, &defs()).unwrap();
    let store = Arc::new(store);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    store
        .owners
        .lock()
        .unwrap()
        .get_mut(ID)
        .unwrap()
        .file
        .before_publish = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let writer = {
        let store = store.clone();
        let scope = scope.clone();
        let id = id.clone();
        let handle = handle.clone();
        thread::spawn(move || {
            store.save(&scope, &id, &handle, &updates(json!({"text":"admitted"})))
        })
    };
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let drain = gate.close(&id).unwrap();
    assert!(!drain.is_drained().unwrap());
    assert_eq!(
        store.setting(&scope, &id, &handle, "text"),
        Err(SettingsError::InvalidScope)
    );
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        drain.wait().unwrap();
        done_tx.send(()).unwrap();
    });
    assert!(done_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    let completed = writer.join().unwrap().unwrap();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    waiter.join().unwrap();
    let fresh = PluginDataCallIdentity::new(ID, 2, 2, 2).unwrap();
    gate.activate(fresh.clone()).unwrap();
    assert_eq!(
        store.setting(&scope, &fresh, &completed, "text"),
        Err(SettingsError::InvalidScope)
    );
    let current = store.configure(&scope, &fresh, &defs()).unwrap();
    assert_eq!(
        store.setting(&scope, &fresh, &current, "text").unwrap(),
        Some(json!("admitted"))
    );
}
#[test]
fn configuration_and_changes_persist_revision_but_noop_does_not() {
    let (temp, _gate, store, scope, id) = fixture();
    let h = store.configure(&scope, &id, &defs()).unwrap();
    let path = temp.0.join(ID).join("settings.json");
    let read =
        || serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["revision"].clone();
    assert_eq!(read(), json!(1));
    let h = store
        .save(&scope, &id, &h, &updates(json!({"text":"changed"})))
        .unwrap();
    assert_eq!(read(), json!(2));
    let _same = store
        .save(&scope, &id, &h, &updates(json!({"text":"changed"})))
        .unwrap();
    assert_eq!(read(), json!(2));
    let _new = store.configure(&scope, &id, &defs()).unwrap();
    assert_eq!(read(), json!(3));
}
#[test]
fn maximum_revision_rejects_changes_before_commit_and_keeps_current_handle() {
    let (temp, _gate, store, scope, id) = fixture();
    let owner = temp.0.join(ID);
    fs::DirBuilder::new().mode(0o700).create(&owner).unwrap();
    let path = owner.join("settings.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"schema":1,"pluginId":ID,"revision":u64::MAX-1,"values":{}}))
            .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let handle = store.configure(&scope, &id, &defs()).unwrap();
    let before = fs::read(&path).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&before).unwrap()["revision"],
        json!(u64::MAX)
    );
    assert!(matches!(
        store.save(&scope, &id, &handle, &updates(json!({"text":"overflow"}))),
        Err(SettingsError::Storage)
    ));
    assert!(matches!(
        store.configure(&scope, &id, &defs()),
        Err(SettingsError::Storage)
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(store.save(&scope, &id, &handle, &BTreeMap::new()).is_ok());
    assert_eq!(
        store.setting(&scope, &id, &handle, "text").unwrap(),
        Some(json!("default"))
    );
}
#[test]
fn secret_and_obsolete_loaded_values_are_removed_before_publishing() {
    let (temp, _gate, store, scope, id) = fixture();
    let owner = temp.0.join(ID);
    fs::DirBuilder::new().mode(0o700).create(&owner).unwrap();
    let path = owner.join("settings.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema":1,"pluginId":ID,"revision":1,"values":{"secret":"synthetic","obsolete":"old","text":"retained"}})).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let handle = store.configure(&scope, &id, &defs()).unwrap();
    let document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(document["values"].get("secret").is_none());
    assert!(document["values"].get("obsolete").is_none());
    assert_eq!(
        store.setting(&scope, &id, &handle, "text").unwrap(),
        Some(json!("retained"))
    );
}
#[test]
fn upgrade_and_updates_are_one_transaction_not_partial_configuration() {
    let (temp, _gate, store, scope, id) = fixture();
    let old = store.configure(&scope, &id, &defs()).unwrap();
    let path = temp.0.join(ID).join("settings.json");
    let before = fs::read(&path).unwrap();
    let mut next = defs();
    next[0] =
        serde_json::from_value(json!({"type":"boolean","key":"text","label":"T","default":false}))
            .unwrap();
    assert!(matches!(
        store.configure_with_updates(&scope, &id, &next, &updates(json!({"count":999}))),
        Err(SettingsError::InvalidSetting)
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        store.setting(&scope, &id, &old, "text").unwrap(),
        Some(json!("default"))
    );
    let mut quota = defs();
    quota.retain(|d| d.key() != "count");
    assert!(matches!(
        store.configure_with_updates(
            &scope,
            &id,
            &quota,
            &updates(json!({"text":"x".repeat(STORAGE_QUOTA_BYTES)}))
        ),
        Err(SettingsError::QuotaExceeded)
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(store.setting(&scope, &id, &old, "count").is_ok());
    let new = store
        .configure_with_updates(&scope, &id, &next, &updates(json!({"text":true,"count":3})))
        .unwrap();
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["revision"], json!(2));
    assert_eq!(
        store.setting(&scope, &id, &new, "text").unwrap(),
        Some(json!(true))
    );
}
#[test]
fn settings_layout_does_not_touch_storage_names_and_revision_fits_budget() {
    let (temp, _gate, store, scope, id) = fixture();
    let owner = temp.0.join(ID);
    fs::DirBuilder::new().mode(0o700).create(&owner).unwrap();
    for name in ["storage.json", "storage.pending"] {
        let path = owner.join(name);
        fs::write(&path, b"unrelated storage namespace").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    store.configure(&scope, &id, &defs()).unwrap();
    for name in ["storage.json", "storage.pending"] {
        assert_eq!(
            fs::read(owner.join(name)).unwrap(),
            b"unrelated storage namespace"
        );
    }
    let candidate = SettingsDocument {
        schema: 1,
        plugin_id: "a".repeat(64),
        revision: u64::MAX,
        values: BTreeMap::from([("v".into(), json!("x".repeat(STORAGE_QUOTA_BYTES - 8)))]),
    };
    assert_eq!(
        serde_json::to_vec(&candidate.values).unwrap().len(),
        STORAGE_QUOTA_BYTES
    );
    assert!(encode(&candidate).unwrap().len() <= super::super::private_file_macos::MAX_DOCUMENT);
}
