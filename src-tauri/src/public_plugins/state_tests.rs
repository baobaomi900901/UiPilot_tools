use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{json, Value};

use super::*;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-public-state-{label}-{}-{id}",
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

fn manifest(plugin_id: &str, default_name: &str, settings: Value) -> PublicManifestV1 {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "pluginId": plugin_id,
        "version": "1.0.0",
        "apiVersion": 1,
        "minimumHostVersion": "0.2.0",
        "name": plugin_id,
        "supportedPlatforms": ["windows"],
        "command": {
            "defaultName": default_name,
            "activationMode": "live",
            "outputMode": "mainResult",
            "inputRequired": false
        },
        "runtime": { "entry": "runtime.js" },
        "permissions": [],
        "settings": settings
    }))
    .unwrap()
}

fn install(store: &PluginStateStore, manifest: &PublicManifestV1) -> EffectivePluginConfig {
    store.install_or_upgrade(manifest, BTreeSet::new()).unwrap()
}

fn timer_manifest(version: &str) -> PublicManifestV1 {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "pluginId": "com.example.timer",
        "version": version,
        "apiVersion": 1,
        "minimumHostVersion": "0.2.0",
        "name": "Timer",
        "supportedPlatforms": ["windows"],
        "command": {
            "defaultName": "timer",
            "activationMode": "submit",
            "outputMode": "window",
            "inputRequired": false
        },
        "runtime": { "entry": "runtime.js" },
        "window": { "entry": "window.html" },
        "permissions": ["ui.window", "notifications.publish", "timer.control"],
        "settings": []
    }))
    .unwrap()
}

fn definitions() -> Value {
    json!([
        {
            "type": "number",
            "key": "limit",
            "label": "Limit",
            "default": 5.0,
            "min": 1.0,
            "max": 10.0,
            "step": 1.0
        },
        {
            "type": "select",
            "key": "mode",
            "label": "Mode",
            "options": [
                { "value": "fast", "label": "Fast" },
                { "value": "safe", "label": "Safe" }
            ],
            "default": "fast"
        },
        {
            "type": "text",
            "key": "prefix",
            "label": "Prefix",
            "default": "ready"
        },
        {
            "type": "boolean",
            "key": "compact",
            "label": "Compact",
            "default": false
        },
        {
            "type": "secret",
            "key": "api-key",
            "label": "API key"
        }
    ])
}

#[test]
fn effective_names_are_global_and_failed_rename_is_atomic() {
    let dir = TestDir::new("names");
    let store = PluginStateStore::load(dir.path(), ["find".into(), "math".into()]).unwrap();
    let alpha = manifest("com.example.alpha", "alpha", json!([]));
    let beta = manifest("com.example.beta", "beta", json!([]));
    install(&store, &alpha);
    let before = install(&store, &beta);

    assert_eq!(
        store.rename("com.example.beta", Some("alpha")),
        Err(PluginStateError::NameConflict {
            owner: Some("com.example.alpha".into())
        })
    );
    assert_eq!(store.config("com.example.beta").unwrap(), Some(before));

    store.set_enabled("com.example.alpha", false).unwrap();
    assert!(matches!(
        store.rename("com.example.beta", Some("alpha")),
        Err(PluginStateError::NameConflict { .. })
    ));
    assert_eq!(
        store.rename("com.example.beta", Some("find")),
        Err(PluginStateError::NameConflict { owner: None })
    );

    drop(store);
    let reloaded = PluginStateStore::load(dir.path(), ["find".into(), "math".into()]).unwrap();
    assert_eq!(
        reloaded
            .config("com.example.beta")
            .unwrap()
            .unwrap()
            .effective_name,
        "beta"
    );
}

#[test]
fn timer_upgrade_requires_every_declared_grant_and_preserves_current_generation() {
    let dir = TestDir::new("timer-grants");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let installed_manifest = timer_manifest("1.0.0");
    let all_grants = installed_manifest.permissions.iter().copied().collect();
    let installed = store
        .install_or_upgrade(&installed_manifest, all_grants)
        .unwrap();

    let upgrade = timer_manifest("1.1.0");
    let missing_timer_grant = upgrade.permissions.iter().copied().take(2).collect();
    assert_eq!(
        store.install_or_upgrade(&upgrade, missing_timer_grant),
        Err(PluginStateError::InvalidPermissions)
    );
    assert_eq!(store.config(&upgrade.plugin_id).unwrap(), Some(installed));
}

#[test]
fn settings_enforce_schema_scope_and_survive_upgrade() {
    let dir = TestDir::new("settings");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let mut plugin = manifest("com.example.settings", "settings", definitions());
    let installed = install(&store, &plugin);
    assert_eq!(installed.settings["limit"], json!(5.0));
    assert_eq!(installed.settings["mode"], json!("fast"));
    assert!(!installed.settings.contains_key("api-key"));

    for invalid in [
        BTreeMap::from([("limit".into(), json!(0))]),
        BTreeMap::from([("mode".into(), json!("unknown"))]),
        BTreeMap::from([("prefix".into(), json!(false))]),
        BTreeMap::from([("compact".into(), json!("false"))]),
    ] {
        assert_eq!(
            store.save_settings(&plugin.plugin_id, &plugin.settings, &invalid),
            Err(PluginStateError::InvalidSetting)
        );
    }
    let saved = store
        .save_settings(
            &plugin.plugin_id,
            &plugin.settings,
            &BTreeMap::from([
                ("limit".into(), json!(7)),
                ("mode".into(), json!("safe")),
                ("prefix".into(), json!("done")),
                ("compact".into(), json!(true)),
            ]),
        )
        .unwrap();
    assert_eq!(saved.settings["limit"], json!(7));
    assert_eq!(saved.settings["prefix"], json!("done"));
    assert_eq!(saved.settings["compact"], json!(true));

    let own_scope = PluginDataScope::new(&plugin.plugin_id).unwrap();
    let foreign_scope = PluginDataScope::new("com.example.foreign").unwrap();
    assert_eq!(
        store
            .setting(&own_scope, &plugin.plugin_id, &plugin.settings, "mode")
            .unwrap(),
        Some(json!("safe"))
    );
    assert_eq!(
        store.setting(&foreign_scope, &plugin.plugin_id, &plugin.settings, "mode"),
        Err(PluginStateError::InvalidScope)
    );
    assert_eq!(
        store.setting(&own_scope, &plugin.plugin_id, &plugin.settings, "api-key"),
        Err(PluginStateError::InvalidSetting)
    );

    store
        .disable_for_fault(&plugin.plugin_id, PublicPluginFault::ConsecutiveFailures)
        .unwrap();
    plugin.version = "1.1.0".into();
    let upgraded = install(&store, &plugin);
    assert!(!upgraded.enabled);
    assert_eq!(upgraded.fault, Some(PublicPluginFault::ConsecutiveFailures));
    assert_eq!(upgraded.settings["limit"], json!(7));
    assert_eq!(upgraded.settings["mode"], json!("safe"));
    assert!(!upgraded.settings.contains_key("api-key"));
}

#[test]
fn uninstall_retention_and_corrupt_owner_isolation_are_durable() {
    let dir = TestDir::new("lifecycle");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let retained = manifest("com.example.retained", "retained", definitions());
    let broken = manifest("com.example.broken", "broken", json!([]));
    let healthy = manifest("com.example.healthy", "healthy", json!([]));
    install(&store, &retained);
    install(&store, &broken);
    install(&store, &healthy);
    store
        .rename(&retained.plugin_id, Some("custom-name"))
        .unwrap();
    store
        .save_settings(
            &retained.plugin_id,
            &retained.settings,
            &BTreeMap::from([("limit".into(), json!(8))]),
        )
        .unwrap();
    store.uninstall(&retained.plugin_id, true).unwrap();
    let kept = store.config(&retained.plugin_id).unwrap().unwrap();
    assert!(!kept.installed);
    assert_eq!(kept.effective_name, "custom-name");
    assert_eq!(kept.settings["limit"], json!(8));
    assert_eq!(install(&store, &retained).settings["limit"], json!(8));
    store.uninstall(&retained.plugin_id, false).unwrap();
    assert_eq!(store.config(&retained.plugin_id).unwrap(), None);
    let revision_before_restart =
        serde_json::from_slice::<Value>(&fs::read(dir.path().join("inventory.json")).unwrap())
            .unwrap()["revision"]
            .as_u64()
            .unwrap();
    drop(store);

    let broken_owner = dir.path().join(&broken.plugin_id);
    fs::write(broken_owner.join("state.json"), b"not-json").unwrap();
    let reloaded = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    assert_eq!(reloaded.config(&broken.plugin_id).unwrap(), None);
    assert_eq!(
        reloaded
            .config(&healthy.plugin_id)
            .unwrap()
            .unwrap()
            .effective_name,
        "healthy"
    );
    let replacement = manifest("com.example.replacement", "replacement", json!([]));
    assert!(install(&reloaded, &replacement).inventory_revision > revision_before_restart);
    assert!(fs::read_dir(broken_owner).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("state.json.invalid-")));
}
