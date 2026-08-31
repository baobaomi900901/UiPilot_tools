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

fn network_manifest(version: &str, hosts: &[&str]) -> PublicManifestV1 {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "pluginId": "com.example.network",
        "version": version,
        "apiVersion": 1,
        "minimumHostVersion": "0.3.2",
        "name": "Network",
        "supportedPlatforms": ["windows"],
        "command": {
            "defaultName": "network",
            "activationMode": "live",
            "outputMode": "mainResult",
            "inputRequired": false
        },
        "runtime": { "entry": "runtime.js" },
        "network": { "httpsHosts": hosts },
        "permissions": ["network.https"],
        "settings": []
    }))
    .unwrap()
}

fn network_permission_grants() -> BTreeSet<PublicPermission> {
    BTreeSet::from([PublicPermission::NetworkHttps])
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
fn plugin_network_grant_is_exact_durable_and_can_be_revoked_and_regranted() {
    let dir = TestDir::new("network-grant");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let plugin = network_manifest("1.0.0", &["api.example.com", "auth.example.com"]);
    let installed = store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    assert_eq!(
        installed.network_https_hosts_grant,
        BTreeSet::from(["api.example.com".into(), "auth.example.com".into()])
    );
    assert_eq!(
        installed.network_grant_snapshot(&plugin),
        Some(PluginNetworkGrantSnapshot {
            plugin_id: plugin.plugin_id.clone(),
            generation: installed.active_generation,
            https_hosts: installed.network_https_hosts_grant.clone(),
        })
    );

    let revoked = store
        .set_network_access(
            &plugin.plugin_id,
            BTreeSet::from(["api.example.com".into(), "auth.example.com".into()]),
            false,
        )
        .unwrap();
    assert!(!revoked
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert!(revoked.network_https_hosts_grant.is_empty());
    assert_eq!(revoked.network_grant_snapshot(&plugin), None);
    assert!(revoked.inventory_revision > installed.inventory_revision);

    let regranted = store
        .set_network_access(
            &plugin.plugin_id,
            BTreeSet::from(["api.example.com".into(), "auth.example.com".into()]),
            true,
        )
        .unwrap();
    assert!(regranted
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert_eq!(
        regranted.network_https_hosts_grant,
        installed.network_https_hosts_grant
    );
    assert!(regranted.inventory_revision > revoked.inventory_revision);

    drop(store);
    assert_eq!(
        PluginStateStore::load(dir.path(), Vec::<String>::new())
            .unwrap()
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap(),
        regranted
    );
}

#[test]
fn plugin_network_grant_rejects_noncanonical_management_hosts_and_permission_mismatch() {
    let dir = TestDir::new("network-invalid");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let plugin = network_manifest("1.0.0", &["api.example.com"]);
    let installed = store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();

    assert_eq!(
        store.set_network_access(
            &plugin.plugin_id,
            BTreeSet::from(["API.example.com".into()]),
            true,
        ),
        Err(PluginStateError::InvalidPermissions)
    );
    assert_eq!(store.config(&plugin.plugin_id).unwrap(), Some(installed));

    let plain = manifest("com.example.plain", "plain", json!([]));
    assert!(matches!(
        store.prepare_activation(&plain, network_permission_grants(), 1, None),
        Err(PluginStateError::InvalidPermissions)
    ));

    let mut invalid_grants = BTreeSet::new();
    let prepared = store.prepare_activation(&plugin, invalid_grants.clone(), 2, None);
    assert!(prepared.is_ok());
    invalid_grants.insert(PublicPermission::NetworkHttps);
    assert!(store
        .prepare_activation(
            &network_manifest("1.1.0", &["api.example.com"]),
            invalid_grants,
            2,
            None,
        )
        .is_ok());
}

#[test]
fn plugin_network_grant_legacy_permission_without_hosts_loads_fail_closed() {
    let dir = TestDir::new("network-legacy");
    let plugin_id = "com.example.network";
    let owner = dir.path().join(plugin_id);
    fs::create_dir_all(&owner).unwrap();
    fs::write(
        owner.join("state.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "pluginId": plugin_id,
            "version": "1.0.0",
            "defaultName": "network",
            "nameOverride": null,
            "installed": true,
            "enabled": true,
            "fault": null,
            "permissionGrants": ["network.https"],
            "inventoryRevision": 1,
            "activeGeneration": 1,
            "packageDigest": null,
            "settings": {}
        }))
        .unwrap(),
    )
    .unwrap();

    let config = PluginStateStore::load(dir.path(), Vec::<String>::new())
        .unwrap()
        .config(plugin_id)
        .unwrap()
        .unwrap();
    assert!(!config
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert!(config.network_https_hosts_grant.is_empty());
}

#[test]
fn plugin_network_grant_invalid_hosts_without_permission_loads_fail_closed() {
    let dir = TestDir::new("network-invalid-host-grant");
    let plugin_id = "com.example.network";
    let owner = dir.path().join(plugin_id);
    fs::create_dir_all(&owner).unwrap();
    fs::write(
        owner.join("state.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "pluginId": plugin_id,
            "version": "1.0.0",
            "defaultName": "network",
            "nameOverride": null,
            "installed": true,
            "enabled": true,
            "fault": null,
            "permissionGrants": [],
            "networkHttpsHostsGrant": ["LOCALHOST"],
            "inventoryRevision": 1,
            "activeGeneration": 1,
            "packageDigest": null,
            "settings": {}
        }))
        .unwrap(),
    )
    .unwrap();

    let config = PluginStateStore::load(dir.path(), Vec::<String>::new())
        .unwrap()
        .config(plugin_id)
        .unwrap()
        .unwrap();
    assert!(!config
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert!(config.network_https_hosts_grant.is_empty());
}

#[test]
fn effective_names_are_global_and_failed_rename_is_atomic() {
    let dir = TestDir::new("names");
    let store = PluginStateStore::load(
        dir.path(),
        ["find".into(), "math".into(), "web-search".into()],
    )
    .unwrap();
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
    assert_eq!(
        store.rename("com.example.beta", Some("web-search")),
        Err(PluginStateError::NameConflict { owner: None })
    );
    assert!(matches!(
        store.install_or_upgrade(
            &manifest("com.example.web", "web-search", json!([])),
            BTreeSet::new(),
        ),
        Err(PluginStateError::NameConflict { owner: None })
    ));

    drop(store);
    let reloaded = PluginStateStore::load(
        dir.path(),
        ["find".into(), "math".into(), "web-search".into()],
    )
    .unwrap();
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
fn dynamic_reserved_names_block_public_plugin_activation_and_rename() {
    let dir = TestDir::new("dynamic-names");
    let store = PluginStateStore::load(dir.path(), ["find".into()]).unwrap();
    store
        .replace_external_reserved_names(["jd".into()])
        .unwrap();

    assert_eq!(
        store.install_or_upgrade(
            &manifest("com.example.quicklink", "jd", json!([])),
            BTreeSet::new(),
        ),
        Err(PluginStateError::NameConflict { owner: None })
    );

    install(&store, &manifest("com.example.alpha", "alpha", json!([])));
    assert_eq!(
        store.rename("com.example.alpha", Some("jd")),
        Err(PluginStateError::NameConflict { owner: None })
    );

    store
        .replace_external_reserved_names(Vec::<String>::new())
        .unwrap();
    assert_eq!(
        store
            .rename("com.example.alpha", Some("jd"))
            .unwrap()
            .effective_name,
        "jd"
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

#[test]
fn prepared_activation_is_invisible_until_durable_state_is_published() {
    let dir = TestDir::new("prepared-activation");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let plugin = manifest("com.example.prepared", "prepared", json!([]));
    let prepared = store
        .prepare_activation(&plugin, BTreeSet::new(), 1, Some("a".repeat(64)))
        .unwrap();

    assert_eq!(store.config(&plugin.plugin_id).unwrap(), None);
    assert_eq!(
        store.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    assert_eq!(store.config(&plugin.plugin_id).unwrap(), None);

    let published = store.publish_prepared(prepared).unwrap();
    assert_eq!(published.active_generation, 1);
    assert_eq!(published.package_digest, Some("a".repeat(64)));
    assert_eq!(store.config(&plugin.plugin_id).unwrap(), Some(published));
}

#[test]
fn not_committed_rollback_requires_the_exact_previous_owner_bytes() {
    let dir = TestDir::new("prepared-revalidation");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let mut plugin = manifest("com.example.prepared", "prepared", json!([]));
    store
        .activate(&plugin, BTreeSet::new(), 1, Some("a".repeat(64)))
        .unwrap();
    plugin.version = "1.1.0".into();
    let prepared = store
        .prepare_activation(&plugin, BTreeSet::new(), 2, Some("b".repeat(64)))
        .unwrap();

    assert!(store.revalidate_previous(&prepared));
    fs::write(
        dir.path().join(&plugin.plugin_id).join("state.json"),
        b"ambiguous-owner",
    )
    .unwrap();
    assert!(!store.revalidate_previous(&prepared));
}

#[test]
fn favorite_state_defaults_persists_and_survives_non_uninstall_mutations() {
    let dir = TestDir::new("favorite-state");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let mut plugin = manifest("com.example.favorite", "favorite", definitions());
    let installed = install(&store, &plugin);
    assert!(!installed.favorite);

    let prepared = store.prepare_set_favorite(&plugin.plugin_id, true).unwrap();
    assert_eq!(
        store.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    let favorited = store.publish_prepared(prepared).unwrap();
    assert!(favorited.favorite);

    let renamed = store
        .rename(&plugin.plugin_id, Some("favorite-renamed"))
        .unwrap();
    assert!(renamed.favorite);
    let disabled = store.set_enabled(&plugin.plugin_id, false).unwrap();
    assert!(disabled.favorite);
    plugin.version = "1.1.0".into();
    let upgraded = install(&store, &plugin);
    assert!(upgraded.favorite);
    drop(store);

    let reloaded = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    assert!(
        reloaded
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap()
            .favorite
    );

    let prepared = reloaded
        .prepare_uninstall(&plugin.plugin_id, true)
        .unwrap()
        .unwrap();
    assert_eq!(
        reloaded.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    let removed = reloaded.publish_prepared(prepared).unwrap();
    assert!(!removed.installed);
    assert!(!removed.favorite);
    assert!(!install(&reloaded, &plugin).favorite);
}

#[test]
fn legacy_state_without_favorite_loads_false_and_next_write_is_canonical() {
    let dir = TestDir::new("favorite-legacy");
    let store = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    let plugin = manifest("com.example.legacy-favorite", "legacy-favorite", json!([]));
    install(&store, &plugin);
    drop(store);

    let state_path = dir.path().join(&plugin.plugin_id).join("state.json");
    let mut document = serde_json::from_slice::<Value>(&fs::read(&state_path).unwrap()).unwrap();
    document.as_object_mut().unwrap().remove("favorite");
    fs::write(&state_path, serde_json::to_vec(&document).unwrap()).unwrap();

    let reloaded = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    assert!(
        !reloaded
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap()
            .favorite
    );
    reloaded
        .rename(&plugin.plugin_id, Some("legacy-favorite-renamed"))
        .unwrap();
    let canonical = serde_json::from_slice::<Value>(&fs::read(state_path).unwrap()).unwrap();
    assert_eq!(canonical["favorite"], false);
}
