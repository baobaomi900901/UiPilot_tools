use super::super::{
    package_macos::read_directory,
    private_file_macos::Failure,
    runtime_data_macos::{DataCall, DataOperation},
};
use super::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(crate) const ID: &str = "com.uipilot.demo-settings";
pub(crate) const REQUEST: &str = "public-request-0000000000000001";
pub(crate) struct Temp(PathBuf);
impl Temp {
    pub(crate) fn new() -> Self {
        let root = PathBuf::from(format!(
            "/private/tmp/uipilot-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["inventory", "storage", "settings", "package"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(root)
    }
    pub(crate) fn host(&self) -> MacPluginLifecycle {
        MacPluginLifecycle::load(
            &self.0.join("inventory"),
            &self.0.join("storage"),
            &self.0.join("settings"),
        )
        .unwrap()
    }
    pub(crate) fn package(&self, version: &str, clipboard: bool) -> VerifiedPackage {
        self.modified(version, clipboard, |_| {})
    }
    fn modified(
        &self,
        version: &str,
        clipboard: bool,
        edit: impl FnOnce(&mut Value),
    ) -> VerifiedPackage {
        let mut manifest: Value = serde_json::from_str(include_str!(
            "../../../examples/public-plugins/com.uipilot.demo-settings/package/plugin.json"
        ))
        .unwrap();
        manifest["version"] = json!(version);
        if clipboard {
            manifest["permissions"] = json!(["clipboard.write"]);
        }
        edit(&mut manifest);
        fs::write(
            self.0.join("package/plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            self.0.join("package/runtime.js"),
            format!("// version {version}\nexport function run() {{ return []; }}"),
        )
        .unwrap();
        read_directory(
            &self.0.join("package"),
            &PublicPluginHost::current(PublicPlatform::Macos),
        )
        .unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(crate) fn install(host: &mut MacPluginLifecycle, package: VerifiedPackage) {
    let prepared = host.prepare_install(package).unwrap();
    let approved = prepared.required_permissions().clone();
    assert_eq!(host.commit_install(prepared, &approved), Ok(true));
}
fn open(host: &mut MacPluginLifecycle) -> InstalledRequest {
    host.open_request(
        ID,
        "default",
        REQUEST,
        Instant::now() + Duration::from_secs(10),
        |_| Ok(Box::new(|| Ok(()))),
    )
    .unwrap()
}
fn get(
    request: &InstalledRequest,
    call_id: u64,
    key: &str,
) -> std::result::Result<Value, DataRuntimeError> {
    let reply = request.binding().call(DataCall {
        context: request.binding().context().clone(),
        call_id,
        request: DataOperation::SettingGet { key: key.into() },
    })?;
    reply.result
}
fn allowed(
    request: &InstalledRequest,
    call_id: u64,
) -> std::result::Result<Value, DataRuntimeError> {
    request
        .permission_call(
            request.binding().context(),
            call_id,
            PublicPermission::ClipboardWrite,
            || Ok(json!(true)),
        )?
        .result
}
#[test]
fn install_snapshots_source_and_restores_settings_grants_generation_without_starting_runtime() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    host.save_settings(ID, &BTreeMap::from([("greeting".into(), json!("持久 😀"))]))
        .unwrap();
    let request = open(&mut host);
    let generation = request.binding().context().plugin_generation;
    assert_eq!(get(&request, 1, "greeting"), Ok(json!("持久 😀")));
    assert_eq!(allowed(&request, 2), Ok(json!(true)));
    fs::write(t.0.join("package/runtime.js"), "throw 'source mutated'").unwrap();
    assert!(
        std::str::from_utf8(request.package().resource("runtime.js").unwrap())
            .unwrap()
            .contains("version 1.0.0")
    );
    drop(host);
    assert_eq!(
        get(&request, 3, "greeting"),
        Err(DataRuntimeError::InvalidContext)
    );
    drop(request);
    let mut host = t.host();
    let new = open(&mut host);
    assert_eq!(new.binding().context().plugin_generation, generation);
    assert_eq!(get(&new, 1, "greeting"), Ok(json!("持久 😀")));
    assert_eq!(allowed(&new, 2), Ok(json!(true)));
    assert!(!t.0.join("settings").join(ID).exists());
}
#[test]
fn consent_denial_stale_cross_store_and_expired_tokens_do_not_cancel_or_write() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", false));
    let request = open(&mut host);
    let before = host.raw.clone();
    let prepared = host.prepare_install(t.package("1.0.1", true)).unwrap();
    assert_eq!(
        host.commit_install(prepared, &BTreeSet::new()),
        Err(LifecycleError::ConsentRequired)
    );
    assert_eq!(host.raw, before);
    assert_eq!(get(&request, 1, "greeting"), Ok(json!("你好")));
    let prepared = host.prepare_install(t.package("1.0.1", false)).unwrap();
    let other = Temp::new();
    let mut otherhost = other.host();
    assert_eq!(
        otherhost.commit_install(prepared, &BTreeSet::new()),
        Err(LifecycleError::Stale)
    );
    let mut expired = host.prepare_install(t.package("1.0.1", false)).unwrap();
    expired.expires = Instant::now();
    assert_eq!(
        host.commit_install(expired, &BTreeSet::new()),
        Err(LifecycleError::Stale)
    );
    let stale = host.prepare_install(t.package("1.0.1", false)).unwrap();
    host.save_settings(ID, &BTreeMap::from([("greeting".into(), json!("new"))]))
        .unwrap();
    assert_eq!(
        host.commit_install(stale, &BTreeSet::new()),
        Err(LifecycleError::Stale)
    );
}
#[test]
fn upgrade_preserves_revoked_and_disabled_state_reconciles_settings_and_removed_permissions() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    let old = open(&mut host);
    assert_eq!(allowed(&old, 1), Ok(json!(true)));
    host.set_permission(ID, PublicPermission::ClipboardWrite, false)
        .unwrap();
    assert_eq!(allowed(&old, 2), Err(DataRuntimeError::InvalidContext));
    let revoked = open(&mut host);
    assert_eq!(
        allowed(&revoked, 1),
        Err(DataRuntimeError::InvalidOperation)
    );
    host.set_enabled(ID, false).unwrap();
    host.save_settings(
        ID,
        &BTreeMap::from([
            ("greeting".into(), json!("saved")),
            ("count".into(), json!(5)),
        ]),
    )
    .unwrap();
    let upgraded = t.modified("1.0.1", true, |m| {
        m["settings"][1]["max"] = json!(3);
    });
    let prepared = host.prepare_install(upgraded).unwrap();
    assert!(prepared.required_permissions().is_empty());
    host.commit_install(prepared, &BTreeSet::new()).unwrap();
    assert!(matches!(
        host.open_request(
            ID,
            "default",
            REQUEST,
            Instant::now() + Duration::from_secs(1),
            |_| panic!("must not attach")
        ),
        Err(LifecycleError::Disabled)
    ));
    assert!(host.document.plugins[ID].grants.is_empty());
    assert_eq!(host.document.plugins[ID].settings["count"], json!(2.0));
    host.set_enabled(ID, true).unwrap();
    let enabled = open(&mut host);
    assert_eq!(get(&enabled, 1, "greeting"), Ok(json!("saved")));
    assert_eq!(
        allowed(&enabled, 2),
        Err(DataRuntimeError::InvalidOperation)
    );
    host.set_permission(ID, PublicPermission::ClipboardWrite, true)
        .unwrap();
    assert_eq!(allowed(&enabled, 3), Err(DataRuntimeError::InvalidContext));
    install(&mut host, t.package("1.0.2", false));
    assert!(host.document.plugins[ID].grants.is_empty());
}
#[test]
fn invalid_settings_unknown_commands_permissions_conflicts_and_noops_preserve_live_request() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", false));
    let old = open(&mut host);
    let before = host.raw.clone();
    assert_eq!(host.set_enabled(ID, true), Ok(false));
    assert_eq!(host.save_settings(ID, &BTreeMap::new()), Ok(false));
    let p = host.prepare_install(t.package("1.0.0", false)).unwrap();
    assert_eq!(host.commit_install(p, &BTreeSet::new()), Ok(false));
    assert_eq!(
        host.save_settings(ID, &BTreeMap::from([("count".into(), json!(99))])),
        Err(LifecycleError::InvalidSetting)
    );
    assert_eq!(
        host.set_permission(ID, PublicPermission::ClipboardWrite, true),
        Err(LifecycleError::ConsentRequired)
    );
    assert!(host
        .open_request(
            ID,
            "missing",
            REQUEST,
            Instant::now() + Duration::from_secs(1),
            |_| panic!("must not attach")
        )
        .is_err());
    let conflict = t.modified("1.0.0", false, |m| {
        m["pluginId"] = json!("com.uipilot.other")
    });
    let p = host.prepare_install(conflict).unwrap();
    assert_eq!(
        host.commit_install(p, &BTreeSet::new()),
        Err(LifecycleError::CommandConflict)
    );
    assert_eq!(host.raw, before);
    assert_eq!(get(&old, 1, "greeting"), Ok(json!("你好")));
}
#[test]
fn before_rename_failure_keeps_old_document_but_never_revives_old_binding() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", false));
    let old = open(&mut host);
    let before = host.raw.clone();
    host.file.failure = Some(Failure::BeforeRename);
    assert_eq!(
        host.save_settings(ID, &BTreeMap::from([("greeting".into(), json!("lost"))])),
        Err(LifecycleError::Storage)
    );
    assert_eq!(host.raw, before);
    assert_eq!(host.file.read(&host.root).unwrap(), before);
    assert_eq!(
        get(&old, 1, "greeting"),
        Err(DataRuntimeError::InvalidContext)
    );
    let new = open(&mut host);
    assert_eq!(get(&new, 1, "greeting"), Ok(json!("你好")));
}
#[test]
fn uncertain_commit_closes_until_reopen_then_restores_actual_committed_document() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", false));
    let old = open(&mut host);
    host.file.failure = Some(Failure::AfterRename);
    assert_eq!(
        host.save_settings(
            ID,
            &BTreeMap::from([("greeting".into(), json!("committed"))])
        ),
        Err(LifecycleError::CommitUncertain)
    );
    assert_eq!(
        get(&old, 1, "greeting"),
        Err(DataRuntimeError::InvalidContext)
    );
    assert!(host.prepare_install(t.package("1.0.1", false)).is_err());
    drop(old);
    drop(host);
    let mut host = t.host();
    assert_eq!(get(&open(&mut host), 1, "greeting"), Ok(json!("committed")));
}
#[test]
fn cancellation_failure_never_commits_or_reopens_the_retired_epoch() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    let old = host
        .open_request(
            ID,
            "default",
            REQUEST,
            Instant::now() + Duration::from_secs(10),
            |_| Ok(Box::new(|| Err(DataRuntimeError::Storage))),
        )
        .unwrap();
    let before = host.raw.clone();
    assert_eq!(host.set_enabled(ID, false), Err(LifecycleError::Runtime));
    assert_eq!(host.raw, before);
    assert_eq!(
        get(&old, 1, "greeting"),
        Err(DataRuntimeError::InvalidContext)
    );
    assert!(host
        .open_request(
            ID,
            "default",
            REQUEST,
            Instant::now() + Duration::from_secs(1),
            |_| panic!()
        )
        .is_err());
}
#[test]
fn revoke_closes_new_admission_before_drain_and_blocks_inflight_reply_then_regrant_mints_new_authority(
) {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    let (admitted_tx, admitted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let old = host
        .open_request(
            ID,
            "default",
            REQUEST,
            Instant::now() + Duration::from_secs(10),
            |request| {
                let captured = request.clone();
                Ok(Box::new(move || {
                    assert_eq!(allowed(&captured, 2), Err(DataRuntimeError::InvalidContext));
                    release_tx.send(()).unwrap();
                    Ok(())
                }))
            },
        )
        .unwrap();
    let effect = old.clone();
    let worker = thread::spawn(move || {
        effect
            .permission_call(
                effect.binding().context(),
                1,
                PublicPermission::ClipboardWrite,
                || {
                    admitted_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(Value::Null)
                },
            )
            .map(|r| r.result)
    });
    admitted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    host.set_permission(ID, PublicPermission::ClipboardWrite, false)
        .unwrap();
    assert_eq!(
        worker.join().unwrap(),
        Err(DataRuntimeError::InvalidContext)
    );
    host.set_permission(ID, PublicPermission::ClipboardWrite, true)
        .unwrap();
    assert_eq!(allowed(&old, 3), Err(DataRuntimeError::InvalidContext));
    assert_eq!(allowed(&open(&mut host), 1), Ok(json!(true)));
}
#[test]
fn corrupt_inventory_and_external_change_are_not_recovered_as_empty_or_cached_authority() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    let old = open(&mut host);
    let path = t.0.join("inventory").join(OWNER).join("plugins.json");
    fs::write(&path, b"{\"schema\":1,\"schema\":1}").unwrap();
    assert!(host.prepare_install(t.package("1.0.1", true)).is_err());
    assert_eq!(allowed(&old, 1), Err(DataRuntimeError::InvalidContext));
    drop(host);
    assert!(MacPluginLifecycle::load(
        &t.0.join("inventory"),
        &t.0.join("storage"),
        &t.0.join("settings")
    )
    .is_err());
}
#[test]
fn restored_inventory_rejects_forged_owner_grants_settings_counters_and_modified_snapshot() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", false));
    let original = host.document.clone();
    let mut bad = original.clone();
    bad.plugins
        .get_mut(ID)
        .unwrap()
        .grants
        .insert(PublicPermission::ClipboardWrite);
    assert!(validate(&bad).is_err());
    let mut bad = original.clone();
    bad.plugins
        .get_mut(ID)
        .unwrap()
        .settings
        .insert("secret".into(), json!("x"));
    assert!(validate(&bad).is_err());
    let mut bad = original.clone();
    bad.plugins.get_mut(ID).unwrap().generation = 0;
    assert!(validate(&bad).is_err());
    let mut bad = original.clone();
    bad.plugins.get_mut(ID).unwrap().package["digest"] = json!("tampered");
    assert!(validate(&bad).is_err());
    let mut bad = original.clone();
    let entry = bad.plugins.remove(ID).unwrap();
    bad.plugins.insert("com.uipilot.other".into(), entry);
    assert!(validate(&bad).is_err());
    let mut bad = original;
    bad.revision = 99;
    assert!(validate(&bad).is_err());
}

#[test]
fn counters_and_whole_inventory_budget_fail_before_cancel_or_write() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", false));
    let old = open(&mut host);
    let before = host.raw.clone();
    let mut candidate = host.document.clone();
    candidate.revision = u64::MAX;
    assert_eq!(host.commit(candidate, ID), Err(LifecycleError::Capacity));
    let huge = "x".repeat(STORAGE_QUOTA_BYTES);
    assert_eq!(
        host.save_settings(ID, &BTreeMap::from([("greeting".into(), json!(huge))])),
        Err(LifecycleError::Capacity)
    );
    assert_eq!(host.raw, before);
    assert_eq!(get(&old, 1, "greeting"), Ok(json!("你好")));
}
#[test]
fn owned_cleanup_runs_once_after_authority_closes_on_drop_or_disk_fault() {
    for corrupt in [false, true] {
        let t = Temp::new();
        let mut host = t.host();
        install(&mut host, t.package("1.0.0", true));
        let count = Arc::new(AtomicU64::new(0));
        let observed = count.clone();
        let request = host
            .open_request(
                ID,
                "default",
                REQUEST,
                Instant::now() + Duration::from_secs(10),
                move |request| {
                    let captured = request.clone();
                    Ok(Box::new(move || {
                        assert_eq!(allowed(&captured, 2), Err(DataRuntimeError::InvalidContext));
                        observed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }))
                },
            )
            .unwrap();
        assert_eq!(allowed(&request, 1), Ok(json!(true)));
        if corrupt {
            fs::write(
                t.0.join("inventory").join(OWNER).join("plugins.json"),
                b"{}",
            )
            .unwrap();
            assert!(host.set_enabled(ID, false).is_err());
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert!(host.set_enabled(ID, false).is_err());
        }
        drop(host);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(allowed(&request, 3), Err(DataRuntimeError::InvalidContext));
    }
}

#[test]
fn failed_attachment_closes_exposed_binding_and_does_not_revive_previous_request() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    let cleanup_count = Arc::new(AtomicU64::new(0));
    let count = cleanup_count.clone();
    let old = host
        .open_request(
            ID,
            "default",
            REQUEST,
            Instant::now() + Duration::from_secs(10),
            |_| {
                Ok(Box::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }))
            },
        )
        .unwrap();
    let mut exposed = None;
    let result = host.open_request(
        ID,
        "default",
        REQUEST,
        Instant::now() + Duration::from_secs(10),
        |request| {
            exposed = Some(request.clone());
            Err(LifecycleError::Runtime)
        },
    );
    assert!(matches!(result, Err(LifecycleError::Runtime)));
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
    assert_eq!(allowed(&old, 1), Err(DataRuntimeError::InvalidContext));
    assert_eq!(
        allowed(&exposed.unwrap(), 1),
        Err(DataRuntimeError::InvalidContext)
    );
    assert_eq!(allowed(&open(&mut host), 1), Ok(json!(true)));
    drop(host);
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
}

#[test]
fn successful_attachment_after_deadline_is_retired_once_before_returning_error() {
    let t = Temp::new();
    let mut host = t.host();
    install(&mut host, t.package("1.0.0", true));
    let deadline = Instant::now() + Duration::from_millis(200);
    let cleanup_count = Arc::new(AtomicU64::new(0));
    let count = cleanup_count.clone();
    let mut exposed = None;
    let result = host.open_request(ID, "default", REQUEST, deadline, |request| {
        exposed = Some(request.clone());
        thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
        );
        Ok(Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
    });
    assert!(matches!(result, Err(LifecycleError::Runtime)));
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        allowed(&exposed.unwrap(), 1),
        Err(DataRuntimeError::InvalidContext)
    );
    drop(host);
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
}

impl Temp {
    pub(crate) fn corrupt_inventory(&self) {
        fs::write(
            self.0.join("inventory").join(OWNER).join("plugins.json"),
            b"{}",
        )
        .unwrap();
    }
}

#[test]
fn old_inventory_without_preferences_loads_and_upgrade_removes_orphaned_command_preferences() {
    let temp = Temp::new();
    let mut host = temp.host();
    install(&mut host, temp.package("1.0.0", false));
    let mut old: Value = serde_json::from_slice(host.raw.as_ref().unwrap()).unwrap();
    old["plugins"][ID].as_object_mut().unwrap().remove("commands");
    let old_bytes = serde_json::to_vec(&old).unwrap();
    host.file.commit(&host.root, host.raw.as_deref(), &old_bytes).unwrap();
    drop(host);
    let mut host = temp.host();
    assert!(host.management_snapshot().unwrap().1[0].commands.is_empty());
    host.set_command_preference(ID, "default", CommandPreference { name_override: Some("custom".into()), favorite: true }).unwrap();
    install(&mut host, temp.modified("1.0.1", false, |manifest| {
        let mut command = manifest.as_object_mut().unwrap().remove("command").unwrap();
        command["id"] = json!("replacement");
        manifest["commands"] = json!([command]);
        manifest["minimumHostVersion"] = json!("0.5.0");
    }));
    assert!(host.management_snapshot().unwrap().1[0].commands.is_empty());
}

#[test]
fn changing_command_preference_retires_old_runtime_authority() {
    let temp = Temp::new();
    let mut host = temp.host();
    install(&mut host, temp.package("1.0.0", false));
    let old = open(&mut host);
    assert!(get(&old, 1, "greeting").is_ok());
    host.set_command_preference(ID, "default", CommandPreference { name_override: Some("renamed".into()), favorite: true }).unwrap();
    assert_eq!(get(&old, 2, "greeting"), Err(DataRuntimeError::InvalidContext));
    assert!(get(&open(&mut host), 1, "greeting").is_ok());
}
