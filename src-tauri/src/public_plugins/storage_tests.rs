use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::json;

use super::*;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-public-storage-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn stores_are_independent_and_share_one_scope_guard() {
    let dir = TestDir::new("isolation");
    let store = PluginStorageStore::load(dir.path()).unwrap();
    let alpha = "com.example.alpha";
    let beta = "com.example.beta";
    let alpha_scope = PluginDataScope::new(alpha).unwrap();
    let beta_scope = PluginDataScope::new(beta).unwrap();

    store
        .set(&alpha_scope, alpha, "shared", json!({ "owner": "alpha" }))
        .unwrap();
    store
        .set(&beta_scope, beta, "shared", json!({ "owner": "beta" }))
        .unwrap();
    assert_eq!(
        store.get(&alpha_scope, alpha, "shared").unwrap(),
        Some(json!({ "owner": "alpha" }))
    );
    assert_eq!(
        store.get(&beta_scope, beta, "shared").unwrap(),
        Some(json!({ "owner": "beta" }))
    );
    assert_eq!(
        store.get(&alpha_scope, beta, "shared"),
        Err(PluginStorageError::InvalidScope)
    );
    assert_eq!(
        store.set(&alpha_scope, alpha, "__proto__", json!(true)),
        Err(PluginStorageError::InvalidKey)
    );
    assert_eq!(
        store.set(
            &alpha_scope,
            alpha,
            "object",
            json!({ "nested": { "constructor": true } })
        ),
        Err(PluginStorageError::InvalidValue)
    );
}

#[test]
fn quota_failure_keeps_the_previous_value_in_memory_and_on_disk() {
    let dir = TestDir::new("quota");
    let plugin_id = "com.example.quota";
    let scope = PluginDataScope::new(plugin_id).unwrap();
    let store = PluginStorageStore::load(dir.path()).unwrap();
    store.set(&scope, plugin_id, "value", json!("old")).unwrap();

    let oversized = "x".repeat(STORAGE_QUOTA_BYTES);
    assert_eq!(
        store.set(&scope, plugin_id, "value", json!(oversized)),
        Err(PluginStorageError::QuotaExceeded)
    );
    assert_eq!(
        store.get(&scope, plugin_id, "value").unwrap(),
        Some(json!("old"))
    );

    drop(store);
    let reloaded = PluginStorageStore::load(dir.path()).unwrap();
    assert_eq!(
        reloaded.get(&scope, plugin_id, "value").unwrap(),
        Some(json!("old"))
    );
}

#[test]
fn storage_keys_follow_the_single_runtime_and_window_contract() {
    let dir = TestDir::new("keys");
    let plugin_id = "com.example.keys";
    let scope = PluginDataScope::new(plugin_id).unwrap();
    let store = PluginStorageStore::load(dir.path()).unwrap();
    let max_key = format!("a{}", "9.-".repeat(21));
    let too_long_key = format!("a{}", "b".repeat(64));
    assert_eq!(max_key.len(), 64);

    for key in ["a", "pomodoro.duration-minutes", max_key.as_str()] {
        store.set(&scope, plugin_id, key, json!(key)).unwrap();
        assert_eq!(store.get(&scope, plugin_id, key).unwrap(), Some(json!(key)));
    }

    store
        .set(&scope, plugin_id, "stable", json!("before"))
        .unwrap();
    for key in [
        "",
        "1starts-with-digit",
        "Uppercase",
        "has_underscore",
        "has/slash",
        "__proto__",
        "prototype",
        "constructor",
        too_long_key.as_str(),
    ] {
        assert_eq!(
            store.set(&scope, plugin_id, key, json!("rejected")),
            Err(PluginStorageError::InvalidKey),
            "key={key}"
        );
    }
    assert_eq!(
        store.get(&scope, plugin_id, "stable").unwrap(),
        Some(json!("before"))
    );
}

#[test]
fn uninstall_can_retain_or_delete_private_storage() {
    let dir = TestDir::new("uninstall");
    let plugin_id = "com.example.storage";
    let scope = PluginDataScope::new(plugin_id).unwrap();
    let store = PluginStorageStore::load(dir.path()).unwrap();
    store.set(&scope, plugin_id, "value", json!(42)).unwrap();

    store.uninstall(plugin_id, true).unwrap();
    assert_eq!(
        store.get(&scope, plugin_id, "value").unwrap(),
        Some(json!(42))
    );

    store.uninstall(plugin_id, false).unwrap();
    assert_eq!(store.get(&scope, plugin_id, "value").unwrap(), None);
    assert!(!dir.path().join(plugin_id).exists());
}

#[test]
fn loaded_document_with_legacy_invalid_key_is_quarantined() {
    let dir = TestDir::new("legacy-invalid-key");
    let plugin_id = "com.example.legacy";
    let owner = dir.path().join(plugin_id);
    fs::create_dir_all(&owner).unwrap();
    fs::write(
        owner.join("storage.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "pluginId": plugin_id,
            "values": { "Uppercase": true }
        }))
        .unwrap(),
    )
    .unwrap();

    let store = PluginStorageStore::load(dir.path()).unwrap();
    let scope = PluginDataScope::new(plugin_id).unwrap();
    assert_eq!(store.get(&scope, plugin_id, "valid-key").unwrap(), None);
    assert!(!owner.join("storage.json").exists());
}
