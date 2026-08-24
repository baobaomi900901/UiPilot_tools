use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-public-secrets-{label}-{}-{id}",
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
fn dpapi_record_round_trips_without_plaintext_and_is_bound_to_identity() {
    let dir = TestDir::new("round-trip");
    let store = PluginSecretStore::load(dir.path()).unwrap();
    let plugin_id = "com.example.secret";
    let key = "api-key";
    let plaintext = "not-written-as-plaintext-5f8c3d";
    let scope = PluginDataScope::new(plugin_id).unwrap();
    store.write(plugin_id, key, plaintext).unwrap();

    assert!(store.is_configured(&scope, plugin_id, key).unwrap());
    assert_eq!(
        store.plaintext_for_test(plugin_id, key).unwrap(),
        plaintext.as_bytes()
    );
    let path = store.record_path_for_test(plugin_id, key).unwrap();
    let record = fs::read(&path).unwrap();
    assert!(!record
        .windows(plaintext.len())
        .any(|window| window == plaintext.as_bytes()));
    assert!(!path.file_name().unwrap().to_string_lossy().contains(key));

    let foreign_scope = PluginDataScope::new("com.example.foreign").unwrap();
    assert_eq!(
        store.is_configured(&foreign_scope, plugin_id, key),
        Err(PluginSecretError::InvalidScope)
    );

    let copied_path = store.record_path_for_test(plugin_id, "other-key").unwrap();
    fs::copy(path, copied_path).unwrap();
    assert_eq!(
        store.plaintext_for_test(plugin_id, "other-key"),
        Err(PluginSecretError::ProtectFailed)
    );
}

#[test]
fn uninstall_can_retain_or_delete_secrets() {
    let dir = TestDir::new("uninstall");
    let store = PluginSecretStore::load(dir.path()).unwrap();
    let plugin_id = "com.example.secret-retain";
    let key = "token";
    let scope = PluginDataScope::new(plugin_id).unwrap();
    store.write(plugin_id, key, "secret").unwrap();

    store.uninstall(plugin_id, true).unwrap();
    assert!(store.is_configured(&scope, plugin_id, key).unwrap());

    store.uninstall(plugin_id, false).unwrap();
    assert!(!store.is_configured(&scope, plugin_id, key).unwrap());
    assert!(!dir.path().join(plugin_id).exists());
}
