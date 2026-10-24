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
            credentials: vec![],
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
            .command("default")
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
            .command("default")
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
    assert_eq!(
        kept.command("default").unwrap().effective_name,
        "custom-name"
    );
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
            .command("default")
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
    assert!(!installed.command("default").unwrap().favorite);

    let prepared = store
        .prepare_set_favorite(&plugin.plugin_id, "default", true)
        .unwrap()
        .unwrap();
    assert_eq!(
        store.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    let favorited = store.publish_prepared(prepared).unwrap();
    assert!(favorited.command("default").unwrap().favorite);

    let renamed = store
        .rename(&plugin.plugin_id, Some("favorite-renamed"))
        .unwrap();
    assert!(renamed.command("default").unwrap().favorite);
    let disabled = store.set_enabled(&plugin.plugin_id, false).unwrap();
    assert!(disabled.command("default").unwrap().favorite);
    plugin.version = "1.1.0".into();
    let upgraded = install(&store, &plugin);
    assert!(upgraded.command("default").unwrap().favorite);
    drop(store);

    let reloaded = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    assert!(
        reloaded
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap()
            .command("default")
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
    assert!(!removed.command("default").unwrap().favorite);
    assert!(
        !install(&reloaded, &plugin)
            .command("default")
            .unwrap()
            .favorite
    );
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
    let command = document
        .as_object_mut()
        .unwrap()
        .remove("commands")
        .unwrap()["default"]
        .clone();
    document["schema"] = json!(1);
    document["defaultName"] = command["defaultName"].clone();
    document["nameOverride"] = command["nameOverride"].clone();
    fs::write(&state_path, serde_json::to_vec(&document).unwrap()).unwrap();

    let reloaded = PluginStateStore::load(dir.path(), Vec::<String>::new()).unwrap();
    assert!(
        !reloaded
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap()
            .command("default")
            .unwrap()
            .favorite
    );
    reloaded
        .rename(&plugin.plugin_id, Some("legacy-favorite-renamed"))
        .unwrap();
    let canonical = serde_json::from_slice::<Value>(&fs::read(state_path).unwrap()).unwrap();
    assert_eq!(canonical["commands"]["default"]["favorite"], false);
}

#[test]
fn credential_grant_round_trip_and_revocation() {
    let dir = TestDir::new("credential-grant");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let mut plugin = network_manifest("1.0.0", &["api.example.com"]);
    let credential: super::super::manifest::PluginNetworkCredential = serde_json::from_value(
        json!({"id":"token", "settingKey":"token", "host":"api.example.com", "type":"bearer"}),
    )
    .unwrap();
    plugin.network.as_mut().unwrap().credentials = Some(vec![credential.clone()]);
    let installed = store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    assert_eq!(installed.network_credentials_grant, vec![credential]);
    assert!(installed.network_grant_snapshot(&plugin).is_some());
    let mut changed = plugin.clone();
    changed
        .network
        .as_mut()
        .unwrap()
        .credentials
        .as_mut()
        .unwrap()[0]
        .setting_key = "other".into();
    assert!(installed.network_grant_snapshot(&changed).is_none());

    let restored = PluginStateStore::load(dir.path(), []).unwrap();
    assert_eq!(
        restored.config(&plugin.plugin_id).unwrap().unwrap(),
        installed
    );
    let prepared = restored
        .prepare_set_network_access(
            &plugin.plugin_id,
            installed.network_https_hosts_grant.clone(),
            false,
        )
        .unwrap();
    assert_eq!(
        restored.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    assert!(restored
        .publish_prepared(prepared)
        .unwrap()
        .network_credentials_grant
        .is_empty());
}

#[test]
fn credential_uninstall_retention_clears_both_grants_and_keeps_empty_field_omitted() {
    let dir = TestDir::new("credential-uninstall");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let mut plugin = network_manifest("1.0.0", &["api.example.com"]);
    plugin.network.as_mut().unwrap().credentials = Some(
        serde_json::from_value(json!([
            {"id":"token","settingKey":"token","host":"api.example.com","type":"bearer"}
        ]))
        .unwrap(),
    );
    store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    let prepared = store
        .prepare_uninstall(&plugin.plugin_id, true)
        .unwrap()
        .unwrap();
    assert_eq!(
        store.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    let config = store.publish_prepared(prepared).unwrap();
    assert!(!config.installed);
    assert!(config.network_credentials_grant.is_empty());
    assert!(config.network_https_hosts_grant.is_empty());
    let raw: Value = serde_json::from_slice(
        &fs::read(dir.path().join(&plugin.plugin_id).join("state.json")).unwrap(),
    )
    .unwrap();
    assert!(raw.get("networkCredentialsGrant").is_none());
}

#[test]
fn strong_credential_structure_corruption_falls_back_and_revokes_backup() {
    for bad in [
        json!("bad"),
        json!([{"id":"token"}]),
        json!([
            {"id":"token","settingKey":"token","host":"api.example.com","type":"oauth"}
        ]),
    ] {
        let dir = TestDir::new("credential-structure");
        let store = PluginStateStore::load(dir.path(), []).unwrap();
        let plugin = network_manifest("1.0.0", &["api.example.com"]);
        store
            .install_or_upgrade(&plugin, network_permission_grants())
            .unwrap();
        let owner = dir.path().join(&plugin.plugin_id);
        let paths = AtomicPaths::new(&owner, "state.json");
        let original = fs::read(paths.current()).unwrap();
        fs::write(paths.backup(), &original).unwrap();
        let mut doc: Value = serde_json::from_slice(&original).unwrap();
        doc["networkCredentialsGrant"] = bad;
        doc["settings"] = json!({"latest":"not-in-backup"});
        fs::write(paths.current(), serde_json::to_vec(&doc).unwrap()).unwrap();
        let restored = PluginStateStore::load(dir.path(), []).unwrap();
        let config = restored.config(&plugin.plugin_id).unwrap().unwrap();
        assert!(!config.settings.contains_key("latest"));
        assert!(config.network_https_hosts_grant.is_empty());
        assert!(config.network_credentials_grant.is_empty());
        assert_eq!(
            restored.lock().unwrap().by_plugin[&plugin.plugin_id].raw,
            fs::read(paths.current()).unwrap()
        );
    }
}

#[test]
fn backup_recovery_revokes_network_durably_before_publish() {
    let dir = TestDir::new("backup-revoke");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let plugin = network_manifest("1.0.0", &["api.example.com"]);
    store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    let path = dir.path().join(&plugin.plugin_id).join("state.json");
    let backup = path.with_file_name("state.json.backup");
    fs::copy(&path, &backup).unwrap();
    fs::remove_file(&path).unwrap();
    let restored = PluginStateStore::load(dir.path(), []).unwrap();
    let config = restored.config(&plugin.plugin_id).unwrap().unwrap();
    assert!(config.network_https_hosts_grant.is_empty());
    assert!(path.exists());
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["permissionGrants"], json!([]));
    assert_eq!(
        PluginStateStore::load(dir.path(), [])
            .unwrap()
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap(),
        config
    );
}

#[test]
fn recovery_of_missing_truncated_and_bad_structure_current_never_restores_backup_authority() {
    for current in [
        None,
        Some(b"{".as_slice()),
        Some(b"{\"networkCredentialsGrant\":\"bad\"}".as_slice()),
    ] {
        let dir = TestDir::new("recovery-matrix");
        let store = PluginStateStore::load(dir.path(), []).unwrap();
        let plugin = network_manifest("1.0.0", &["api.example.com"]);
        store
            .install_or_upgrade(&plugin, network_permission_grants())
            .unwrap();
        let owner = dir.path().join(&plugin.plugin_id);
        let paths = AtomicPaths::new(&owner, "state.json");
        fs::copy(paths.current(), paths.backup()).unwrap();
        match current {
            Some(raw) => fs::write(paths.current(), raw).unwrap(),
            None => fs::remove_file(paths.current()).unwrap(),
        }
        let restored = PluginStateStore::load(dir.path(), []).unwrap();
        let config = restored.config(&plugin.plugin_id).unwrap().unwrap();
        assert!(!config
            .permission_grants
            .contains(&PublicPermission::NetworkHttps));
        let raw = fs::read(paths.current()).unwrap();
        assert_eq!(
            restored.lock().unwrap().by_plugin[&plugin.plugin_id].raw,
            raw
        );
        assert!(config.network_credentials_grant.is_empty());
    }
}

#[test]
fn semantic_credential_corruption_preserves_current_settings_and_cannot_fall_back_to_grants() {
    for credentials in [
        json!([{"id":"token","settingKey":"token","host":"other.example.com","type":"bearer"}]),
        json!([
            {"id":"token","settingKey":"token","host":"api.example.com","type":"bearer"},
            {"id":"token","settingKey":"token","host":"api.example.com","type":"bearer"}
        ]),
    ] {
        let dir = TestDir::new("semantic-grants");
        let plugin = network_manifest("1.0.0", &["api.example.com"]);
        let store = PluginStateStore::load(dir.path(), []).unwrap();
        store
            .install_or_upgrade(&plugin, network_permission_grants())
            .unwrap();
        let owner = dir.path().join(&plugin.plugin_id);
        let paths = AtomicPaths::new(&owner, "state.json");
        let original = fs::read(paths.current()).unwrap();
        fs::write(paths.backup(), &original).unwrap();
        let mut doc: Value = serde_json::from_slice(&original).unwrap();
        doc["settings"] = json!({"preserve":"latest"});
        doc["networkCredentialsGrant"] = credentials;
        fs::write(paths.current(), serde_json::to_vec(&doc).unwrap()).unwrap();
        let restored = PluginStateStore::load(dir.path(), []).unwrap();
        let config = restored.config(&plugin.plugin_id).unwrap().unwrap();
        assert_eq!(config.settings["preserve"], json!("latest"));
        assert!(config.network_https_hosts_grant.is_empty());
        assert!(config.network_credentials_grant.is_empty());
        assert_eq!(fs::read(paths.backup()).unwrap(), original);
        let durable: Value = serde_json::from_slice(&fs::read(paths.current()).unwrap()).unwrap();
        assert!(durable.get("networkCredentialsGrant").is_none());
        assert_eq!(
            PluginStateStore::load(dir.path(), [])
                .unwrap()
                .config(&plugin.plugin_id)
                .unwrap()
                .unwrap(),
            config
        );
    }
}

#[test]
fn recovery_persistence_failure_publishes_no_config_and_preserves_source() {
    let dir = TestDir::new("recovery-write-fail");
    let plugin = network_manifest("1.0.0", &["api.example.com"]);
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    let owner = dir.path().join(&plugin.plugin_id);
    let paths = AtomicPaths::new(&owner, "state.json");
    let source = fs::read(paths.current()).unwrap();
    fs::write(paths.backup(), &source).unwrap();
    fs::remove_file(paths.current()).unwrap();
    let outcome = load_owner_with_recovery(&owner, &plugin.plugin_id, |_, _| {
        Err(AtomicFileError::CandidateWrite)
    });
    assert!(matches!(outcome, Err(PluginStateError::Storage)));
    assert_eq!(fs::read(paths.backup()).unwrap(), source);
    assert!(!paths.current().exists());
}

#[test]
fn schema_two_roundtrip_has_command_map_and_canonical_roots() {
    let dir = TestDir::new("schema-two-red");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let plugin = manifest("com.example.schema", "schema", json!([]));
    install(&store, &plugin);
    let path = dir.path().join(&plugin.plugin_id).join("state.json");
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["schema"], 2);
    assert_eq!(
        value["commands"]["default"],
        json!({
            "defaultName":"schema", "nameOverride":null, "favorite":false
        })
    );
    assert!(value.get("defaultName").is_none());
    assert!(value.get("networkCredentialsGrant").is_none());
    assert!(parse_document(&serde_json::to_vec(&value).unwrap(), &plugin.plugin_id).is_some());
}

#[test]
fn schema_two_strict_structure_and_defaults() {
    let valid = json!({"schema":2,"pluginId":"com.example.schema","version":"1.0.0",
        "commands":{"constructor":{"defaultName":"schema","nameOverride":null,"favorite":false}},
        "installed":true,"enabled":true,"permissionGrants":[],"inventoryRevision":7,"settings":{}});
    let parse =
        |value: &Value| parse_document(&serde_json::to_vec(value).unwrap(), "com.example.schema");
    let defaults = parse(&valid).unwrap();
    assert_eq!(defaults.fault, None);
    assert_eq!(defaults.active_generation, 0);
    assert_eq!(defaults.package_digest, None);
    assert!(defaults.network_https_hosts_grant.is_empty());
    assert!(defaults.network_credentials_grant.is_empty());
    let canonical = serde_json::to_value(&defaults).unwrap();
    assert_eq!(canonical["fault"], Value::Null);
    assert_eq!(canonical["activeGeneration"], 0);
    assert_eq!(canonical["packageDigest"], Value::Null);
    assert_eq!(canonical["networkHttpsHostsGrant"], json!([]));
    assert!(canonical.get("networkCredentialsGrant").is_none());
    for field in [
        "commands",
        "pluginId",
        "version",
        "installed",
        "enabled",
        "permissionGrants",
        "inventoryRevision",
        "settings",
    ] {
        let mut bad = valid.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(parse(&bad).is_none(), "missing root {field}");
    }
    for field in ["defaultName", "nameOverride", "favorite"] {
        let mut bad = valid.clone();
        bad["commands"]["constructor"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(parse(&bad).is_none(), "missing entry {field}");
        let mut bad = valid.clone();
        bad[field] = json!(null);
        assert!(parse(&bad).is_none(), "old root {field}");
    }
    for (field, value) in [
        ("favorite", json!(null)),
        ("defaultName", json!("bad name")),
        ("nameOverride", json!("bad name")),
        ("unknown", json!(0)),
    ] {
        let mut bad = valid.clone();
        bad["commands"]["constructor"][field] = value;
        assert!(parse(&bad).is_none(), "invalid entry {field}");
    }
    for (field, value) in [
        ("unknown", json!(0)),
        ("schema", json!(3)),
        ("commands", json!({})),
        (
            "commands",
            json!({"bad id":{"defaultName":"valid","nameOverride":null,"favorite":false}}),
        ),
        ("networkHttpsHostsGrant", json!(null)),
        ("networkCredentialsGrant", json!(null)),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(parse(&bad).is_none(), "invalid root {field}");
    }
    let mut many = valid.clone();
    many["commands"] = Value::Object(
        (0..16)
            .map(|i| {
                (
                    format!("id{i}"),
                    json!({"defaultName":format!("name{i}"),"nameOverride":null,"favorite":false}),
                )
            })
            .collect(),
    );
    assert!(parse(&many).is_some());
    many["commands"]["extra"] = json!({"defaultName":"extra","nameOverride":null,"favorite":false});
    assert!(parse(&many).is_none());
    let text = serde_json::to_string(&valid).unwrap();
    let duplicate=text.replace("\"commands\":{", "\"commands\":{\"constructor\":{\"defaultName\":\"other\",\"nameOverride\":null,\"favorite\":false},");
    assert!(parse_document(duplicate.as_bytes(), "com.example.schema").is_none());
}

#[test]
fn schema_two_command_entries_require_objects_in_raw_bytes() {
    let parse = |entry: &str| {
        let raw = format!(
            r#"{{"schema":2,"pluginId":"com.example.schema","version":"1.0.0","commands":{{"default":{entry}}},"installed":true,"enabled":true,"permissionGrants":[],"inventoryRevision":7,"settings":{{}}}}"#
        );
        parse_document(raw.as_bytes(), "com.example.schema")
    };
    for entry in [
        r#"{"defaultName":"schema","nameOverride":null,"favorite":false}"#,
        r#"{"favorite":true,"nameOverride":"custom","defaultName":"schema"}"#,
    ] {
        assert!(parse(entry).is_some(), "valid object: {entry}");
    }
    for entry in [
        r#"["schema",null,false]"#,
        r#"["schema","custom",true]"#,
        r#"[]"#,
        r#"{"defaultName":"schema","favorite":false}"#,
        r#"{"defaultName":"schema","nameOverride":null,"favorite":false,"extra":0}"#,
        r#"{"defaultName":"schema","defaultName":"other","nameOverride":null,"favorite":false}"#,
        r#"{"defaultName":"schema","nameOverride":null,"nameOverride":"custom","favorite":false}"#,
        r#"{"defaultName":"schema","nameOverride":null,"favorite":false,"favorite":true}"#,
    ] {
        assert!(parse(entry).is_none(), "invalid command entry: {entry}");
    }
}

#[test]
fn schema_two_command_array_is_quarantined_and_recovers_revoked_backup() {
    let dir = TestDir::new("command-array-recovery");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let plugin = network_manifest("1.0.0", &["api.example.com"]);
    let original_config = store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    let owner = dir.path().join(&plugin.plugin_id);
    let paths = AtomicPaths::new(&owner, "state.json");
    let backup = fs::read(paths.current()).unwrap();
    fs::write(paths.backup(), &backup).unwrap();
    let mut current: Value = serde_json::from_slice(&backup).unwrap();
    current["commands"]["default"] = json!(["primary", null, false]);
    current["settings"] = json!({"latest":"not-in-backup"});
    let corrupt = serde_json::to_vec(&current).unwrap();
    fs::write(paths.current(), &corrupt).unwrap();

    let restored = PluginStateStore::load(dir.path(), []).unwrap();
    let config = restored.config(&plugin.plugin_id).unwrap().unwrap();
    assert_eq!(config.commands, original_config.commands);
    assert!(!config.settings.contains_key("latest"));
    assert!(!config
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert!(config.network_https_hosts_grant.is_empty());
    assert!(config.network_credentials_grant.is_empty());
    assert_eq!(fs::read(paths.backup()).unwrap(), backup);
    let quarantined: Vec<_> = fs::read_dir(&owner)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("state.json.invalid-")
        })
        .collect();
    assert_eq!(quarantined.len(), 1);
    assert_eq!(fs::read(&quarantined[0]).unwrap(), corrupt);
    let durable = fs::read(paths.current()).unwrap();
    assert_eq!(
        restored.lock().unwrap().by_plugin[&plugin.plugin_id].raw,
        durable
    );
    let document = parse_document(&durable, &plugin.plugin_id).unwrap();
    assert_eq!(
        document.commands,
        parse_document(&backup, &plugin.plugin_id).unwrap().commands
    );
    assert!(document.network_https_hosts_grant.is_empty());
    assert!(document.network_credentials_grant.is_empty());
    assert!(!document
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert_eq!(
        PluginStateStore::load(dir.path(), [])
            .unwrap()
            .config(&plugin.plugin_id)
            .unwrap()
            .unwrap(),
        config
    );
}

#[test]
fn repeated_stored_override_is_a_true_noop_but_explicit_default_is_a_change() {
    let dir = TestDir::new("override-noop");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let plugin = manifest("com.example.noop", "noop", json!([]));
    let before = install(&store, &plugin);
    assert_eq!(store.rename(&plugin.plugin_id, None).unwrap(), before);
    let explicit = store.rename(&plugin.plugin_id, Some("noop")).unwrap();
    assert!(explicit.inventory_revision > before.inventory_revision);
    assert_eq!(
        store.rename(&plugin.plugin_id, Some("noop")).unwrap(),
        explicit
    );
    let reset = store.rename(&plugin.plugin_id, None).unwrap();
    assert!(reset.inventory_revision > explicit.inventory_revision);
}

#[test]
fn state_activation_accepts_the_complete_validated_command_collection() {
    let dir = TestDir::new("collection-activation");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let mut wire = serde_json::to_value(manifest("com.example.multi", "first", json!([]))).unwrap();
    let mut command = wire.as_object_mut().unwrap().remove("command").unwrap();
    command["id"] = json!("constructor");
    let mut second = command.clone();
    second["id"] = json!("second");
    second["defaultName"] = json!("second");
    wire["commands"] = json!([command, second]);
    wire["minimumHostVersion"] = json!("0.5.0");
    let plugin: PublicManifestV1 = serde_json::from_value(wire).unwrap();
    assert!(store.install_or_upgrade(&plugin, BTreeSet::new()).is_ok());
    let value: Value = serde_json::from_slice(
        &fs::read(dir.path().join(&plugin.plugin_id).join("state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(value["commands"].as_object().unwrap().len(), 2);
}

fn legacy_bytes(mut value: Value, favorite: Option<bool>) -> Vec<u8> {
    let command = value.as_object_mut().unwrap().remove("commands").unwrap()["default"].clone();
    value["schema"] = json!(1);
    value["defaultName"] = command["defaultName"].clone();
    value["nameOverride"] = command["nameOverride"].clone();
    if let Some(favorite) = favorite {
        value["favorite"] = json!(favorite);
    }
    // Whitespace proves we keep authoritative bytes rather than reserializing the view.
    format!("  {}\n", serde_json::to_string_pretty(&value).unwrap()).into_bytes()
}

#[test]
fn schema_one_first_read_noops_and_first_real_mutation_failure_retry_preserve_exact_bytes() {
    for favorite in [None, Some(true)] {
        for rename in [false, true] {
            let dir = TestDir::new("schema-one-first-mutation");
            let plugin = manifest("com.example.legacy", "legacy", json!([]));
            let store = PluginStateStore::load(dir.path(), []).unwrap();
            let before = install(&store, &plugin);
            let owner = dir.path().join(&plugin.plugin_id);
            let paths = AtomicPaths::new(&owner, "state.json");
            let original = legacy_bytes(
                serde_json::from_slice(&fs::read(paths.current()).unwrap()).unwrap(),
                favorite,
            );
            fs::write(paths.current(), &original).unwrap();
            let loaded = load_owner_with_recovery(&owner, &plugin.plugin_id, |_, _| {
                panic!("pure normalization must not persist")
            })
            .unwrap()
            .unwrap();
            assert_eq!(loaded.raw, original);
            assert_eq!(
                loaded.document.inventory_revision,
                before.inventory_revision
            );
            assert_eq!(loaded.document.active_generation, before.active_generation);
            assert_eq!(
                loaded.document.commands["default"].favorite,
                favorite.unwrap_or(false)
            );
            let store = PluginStateStore::load(dir.path(), []).unwrap();
            let visible = store.config(&plugin.plugin_id).unwrap().unwrap();
            assert!(store
                .prepare_rename(&plugin.plugin_id, "default", None)
                .unwrap()
                .is_none());
            assert!(store
                .prepare_set_favorite(&plugin.plugin_id, "default", favorite.unwrap_or(false))
                .unwrap()
                .is_none());
            assert_eq!(
                store.inventory_revision().unwrap(),
                before.inventory_revision
            );
            assert_eq!(fs::read(paths.current()).unwrap(), original);
            let prepare = || {
                if rename {
                    store.prepare_rename(&plugin.plugin_id, "default", Some("renamed"))
                } else {
                    store.prepare_set_favorite(
                        &plugin.plugin_id,
                        "default",
                        !favorite.unwrap_or(false),
                    )
                }
            };
            let failed = prepare().unwrap().unwrap();
            fs::create_dir(paths.backup()).unwrap();
            assert_eq!(
                store.persist_prepared(&failed),
                DurableStateOutcome::NotCommitted
            );
            assert!(store.revalidate_previous(&failed));
            assert_eq!(store.config(&plugin.plugin_id).unwrap().unwrap(), visible);
            assert_eq!(fs::read(paths.current()).unwrap(), original);
            fs::remove_dir(paths.backup()).unwrap();
            let retry = prepare().unwrap().unwrap();
            assert_eq!(
                retry.candidate.document.inventory_revision,
                visible.inventory_revision + 1
            );
            assert_eq!(
                store.persist_prepared(&retry),
                DurableStateOutcome::Committed
            );
            let committed = store.publish_prepared(retry).unwrap();
            assert_eq!(committed.inventory_revision, visible.inventory_revision + 1);
            assert_eq!(fs::read(paths.backup()).unwrap(), original);
            let durable: Value =
                serde_json::from_slice(&fs::read(paths.current()).unwrap()).unwrap();
            assert_eq!(durable["schema"], 2);
            assert_eq!(
                store.lock().unwrap().by_plugin[&plugin.plugin_id].raw,
                fs::read(paths.current()).unwrap()
            );
            let inventory: Value =
                serde_json::from_slice(&fs::read(dir.path().join("inventory.json")).unwrap())
                    .unwrap();
            assert_eq!(inventory["schema"], 1);
        }
    }
}

fn multi_manifest(entries: &[(&str, &str)]) -> PublicManifestV1 {
    let mut value =
        serde_json::to_value(manifest("com.example.multi", "unused", json!([]))).unwrap();
    value.as_object_mut().unwrap().remove("command");
    value["minimumHostVersion"] = json!("0.5.0");
    value["commands"]=Value::Array(entries.iter().map(|(id,name)| json!({"id":id,"defaultName":name,"activationMode":"live","outputMode":"mainResult","inputRequired":false})).collect());
    let mut host = super::super::PublicPluginHost::current(super::super::PublicPlatform::Windows);
    host.version = [0, 5, 0];
    super::super::manifest::parse_manifest(&serde_json::to_vec(&value).unwrap(), &host).unwrap()
}

fn publish_change(
    store: &PluginStateStore,
    prepared: Option<PreparedStateCommit>,
) -> EffectivePluginConfig {
    let prepared = prepared.unwrap();
    assert_eq!(
        store.persist_prepared(&prepared),
        DurableStateOutcome::Committed
    );
    store.publish_prepared(prepared).unwrap()
}

#[test]
fn command_reconciliation_is_by_surviving_id_with_atomic_conflicts_and_fresh_readds() {
    let dir = TestDir::new("command-reconciliation");
    let store = PluginStateStore::load(dir.path(), ["reserved".into()]).unwrap();
    let first = multi_manifest(&[("constructor", "old"), ("b", "bee")]);
    install(&store, &first);
    publish_change(
        &store,
        store
            .prepare_rename(&first.plugin_id, "constructor", Some("custom"))
            .unwrap(),
    );
    publish_change(
        &store,
        store
            .prepare_set_favorite(&first.plugin_id, "constructor", true)
            .unwrap(),
    );
    publish_change(
        &store,
        store
            .prepare_set_favorite(&first.plugin_id, "b", true)
            .unwrap(),
    );
    let before = store.config(&first.plugin_id).unwrap().unwrap();
    for id in ["", "bad id", "missing"] {
        assert!(store
            .prepare_rename(&first.plugin_id, id, Some("new"))
            .is_err());
        assert!(store
            .prepare_set_favorite(&first.plugin_id, id, true)
            .is_err());
        assert!(before.command(id).is_none());
    }
    for name in ["bee", "reserved"] {
        assert!(matches!(
            store.prepare_rename(&first.plugin_id, "constructor", Some(name)),
            Err(PluginStateError::NameConflict { .. })
        ));
    }
    let conflicting = multi_manifest(&[("constructor", "new"), ("b", "custom")]);
    assert!(matches!(
        store.install_or_upgrade(&conflicting, BTreeSet::new()),
        Err(PluginStateError::NameConflict { .. })
    ));
    assert_eq!(store.config(&first.plugin_id).unwrap().unwrap(), before);
    let updated = install(
        &store,
        &multi_manifest(&[("b", "beetwo"), ("constructor", "custom"), ("c", "cee")]),
    );
    assert_eq!(
        updated
            .command("constructor")
            .unwrap()
            .name_override
            .as_deref(),
        Some("custom")
    );
    assert!(updated.command("constructor").unwrap().favorite);
    assert!(updated.command("b").unwrap().favorite);
    assert!(!updated.command("c").unwrap().favorite);
    assert_eq!(updated.command("c").unwrap().name_override, None);
    let reset = publish_change(
        &store,
        store
            .prepare_rename(&first.plugin_id, "constructor", None)
            .unwrap(),
    );
    assert_eq!(
        reset.command("constructor").unwrap().effective_name,
        "custom"
    );
    assert_eq!(reset.command("constructor").unwrap().name_override, None);
    drop(store);
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let updated = install(
        &store,
        &multi_manifest(&[("constructor", "next"), ("b", "bee")]),
    );
    assert_eq!(
        updated.command("constructor").unwrap().effective_name,
        "next"
    );
    install(&store, &multi_manifest(&[("b", "bee")]));
    drop(store);
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let readded = install(
        &store,
        &multi_manifest(&[("constructor", "fresh"), ("b", "bee")]),
    );
    assert_eq!(readded.command("constructor").unwrap().name_override, None);
    assert!(!readded.command("constructor").unwrap().favorite);
    assert!(readded.command("b").unwrap().favorite);
    publish_change(
        &store,
        store
            .prepare_rename(&first.plugin_id, "b", Some("kept"))
            .unwrap(),
    );
    let retained = publish_change(
        &store,
        store.prepare_uninstall(&first.plugin_id, true).unwrap(),
    );
    assert_eq!(retained.commands.len(), 2);
    assert!(retained.commands.values().all(|command| !command.favorite));
    assert_eq!(
        retained.command("b").unwrap().name_override.as_deref(),
        Some("kept")
    );
    let reinstalled = install(&store, &multi_manifest(&[("b", "bee")]));
    assert_eq!(reinstalled.command("b").unwrap().effective_name, "kept");
    assert!(!reinstalled.command("b").unwrap().favorite);
}

#[test]
fn legacy_upgrade_and_package_rollback_only_inherit_a_surviving_default() {
    let dir = TestDir::new("legacy-command-rollback");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let legacy = manifest("com.example.multi", "legacy", json!([]));
    install(&store, &legacy);
    store.rename(&legacy.plugin_id, Some("custom")).unwrap();
    publish_change(
        &store,
        store
            .prepare_set_favorite(&legacy.plugin_id, "default", true)
            .unwrap(),
    );
    let upgraded = install(
        &store,
        &multi_manifest(&[("other", "other"), ("default", "newdefault")]),
    );
    assert_eq!(
        upgraded.command("default").unwrap().effective_name,
        "custom"
    );
    assert!(upgraded.command("default").unwrap().favorite);
    assert!(!upgraded.command("other").unwrap().favorite);
    let rollback = install(&store, &legacy);
    assert_eq!(
        rollback.command("default").unwrap().effective_name,
        "custom"
    );
    install(&store, &multi_manifest(&[("other", "custom")]));
    drop(store);
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let rollback = install(&store, &legacy);
    assert_eq!(rollback.commands.len(), 1);
    assert_eq!(
        rollback.command("default").unwrap().effective_name,
        "legacy"
    );
    assert_eq!(rollback.command("default").unwrap().name_override, None);
    assert!(!rollback.command("default").unwrap().favorite);
}

#[test]
fn both_schemas_repair_security_durably_and_repair_failure_never_returns_config() {
    for schema in [1, 2] {
        for kind in ["semantic", "structural", "missing"] {
            for fail in [false, true] {
                let dir = TestDir::new("both-schema-recovery");
                let plugin = network_manifest("1.0.0", &["api.example.com"]);
                let store = PluginStateStore::load(dir.path(), []).unwrap();
                store
                    .install_or_upgrade(&plugin, network_permission_grants())
                    .unwrap();
                let owner = dir.path().join(&plugin.plugin_id);
                let paths = AtomicPaths::new(&owner, "state.json");
                let mut source = fs::read(paths.current()).unwrap();
                if schema == 1 {
                    source = legacy_bytes(serde_json::from_slice(&source).unwrap(), Some(true));
                }
                fs::write(paths.backup(), &source).unwrap();
                let mut current: Value = serde_json::from_slice(&source).unwrap();
                if kind == "missing" {
                    fs::remove_file(paths.current()).unwrap();
                } else {
                    current["settings"] = json!({"latest":"preserved"});
                    current["networkHttpsHostsGrant"] = if kind == "semantic" {
                        json!(["invalid host"])
                    } else {
                        json!(null)
                    };
                    fs::write(paths.current(), serde_json::to_vec(&current).unwrap()).unwrap();
                }
                let writes = std::cell::Cell::new(0);
                let recovered =
                    load_owner_with_recovery(&owner, &plugin.plugin_id, |paths, raw| {
                        writes.set(writes.get() + 1);
                        let candidate = parse_document(raw, &plugin.plugin_id).unwrap();
                        assert!(candidate.network_https_hosts_grant.is_empty());
                        assert!(candidate.network_credentials_grant.is_empty());
                        assert!(!candidate
                            .permission_grants
                            .contains(&PublicPermission::NetworkHttps));
                        if fail {
                            Err(AtomicFileError::CandidateWrite)
                        } else {
                            commit_with_backup(paths, None, raw)
                        }
                    });
                assert_eq!(writes.get(), 1);
                assert_eq!(fs::read(paths.backup()).unwrap(), source);
                if fail {
                    assert!(matches!(recovered, Err(PluginStateError::Storage)));
                } else {
                    let recovered = recovered.unwrap().unwrap();
                    assert_eq!(recovered.raw, fs::read(paths.current()).unwrap());
                    assert_eq!(recovered.document.schema, 2);
                    assert_eq!(
                        recovered.document.settings.contains_key("latest"),
                        kind == "semantic"
                    );
                    assert!(
                        load_owner_with_recovery(&owner, &plugin.plugin_id, |_, _| panic!(
                            "already repaired"
                        ))
                        .unwrap()
                        .is_some()
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_same_name_override_survives_restart_and_later_default_change() {
    let dir = TestDir::new("explicit-default-override");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let plugin = manifest("com.example.explicit", "old", json!([]));
    install(&store, &plugin);
    let explicit = store.rename(&plugin.plugin_id, Some("old")).unwrap();
    assert_eq!(
        explicit
            .command("default")
            .unwrap()
            .name_override
            .as_deref(),
        Some("old")
    );
    assert_eq!(
        store.rename(&plugin.plugin_id, Some("old")).unwrap(),
        explicit
    );
    drop(store);
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let updated = install(&store, &manifest(&plugin.plugin_id, "new", json!([])));
    assert_eq!(updated.command("default").unwrap().default_name, "new");
    assert_eq!(updated.command("default").unwrap().effective_name, "old");
    assert_eq!(
        updated.command("default").unwrap().name_override.as_deref(),
        Some("old")
    );
}

#[test]
fn valid_schema_one_credential_order_normalization_keeps_raw_and_does_not_repair() {
    let dir = TestDir::new("schema-one-credential-order");
    let store = PluginStateStore::load(dir.path(), []).unwrap();
    let plugin = network_manifest("1.0.0", &["api.example.com"]);
    store
        .install_or_upgrade(&plugin, network_permission_grants())
        .unwrap();
    let owner = dir.path().join(&plugin.plugin_id);
    let path = owner.join("state.json");
    let mut wire: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    wire["networkCredentialsGrant"] = json!([
        {"id":"z","settingKey":"token-z","host":"api.example.com","type":"bearer"},
        {"id":"a","settingKey":"token-a","host":"api.example.com","type":"bearer"}
    ]);
    let raw = legacy_bytes(wire, Some(true));
    fs::write(&path, &raw).unwrap();
    let loaded = load_owner_with_recovery(&owner, &plugin.plugin_id, |_, _| {
        panic!("valid ordering must not trigger repair")
    })
    .unwrap()
    .unwrap();
    assert_eq!(loaded.raw, raw);
    assert_eq!(loaded.document.network_credentials_grant[0].id, "a");
    assert!(loaded
        .document
        .permission_grants
        .contains(&PublicPermission::NetworkHttps));
    assert_eq!(fs::read(path).unwrap(), raw);
}
