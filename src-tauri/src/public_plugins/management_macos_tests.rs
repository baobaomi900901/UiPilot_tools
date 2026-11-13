use super::*;
#[test]
fn revision_is_canonical_and_never_accepts_signed_or_padded_values() {
    assert_eq!(revision("0"), Ok(0));
    for value in ["", "+1", "01", "-1", " 1", "18446744073709551616"] {
        assert!(revision(value).is_err());
    }
}
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "uipilot-management-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let package = root.join("package");
        fs::create_dir(&package).unwrap();
        fs::write(
            package.join("plugin.json"),
            include_bytes!(
                "../../../examples/public-plugins/com.uipilot.demo-settings/package/plugin.json"
            ),
        )
        .unwrap();
        fs::write(
            package.join("runtime.js"),
            include_bytes!(
                "../../../examples/public-plugins/com.uipilot.demo-settings/package/runtime.js"
            ),
        )
        .unwrap();
        Self(root)
    }
    fn add_permission(&self) {
        let path = self.0.join("package/plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["permissions"] = json!(["clipboard.write"]);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn preview(&self, m: &mut Management) -> Preview {
        let rev = m.list().unwrap().revision;
        let selection = m.reserve(SourceKind::Directory, &rev).unwrap();
        m.selected(selection, Some(&self.0.join("package")))
            .unwrap()
            .unwrap()
    }
    fn install(&self, m: &mut Management) -> Snapshot {
        let p = self.preview(m);
        m.commit(&p.token, &p.expected_revision, p.required_permissions)
            .unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const ID: &str = "com.uipilot.demo-settings";

#[test]
fn tracked_reference_packages_respect_the_current_management_boundary() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("examples/public-plugins");
    let temp = Temp::new();
    let mut manager = Management::load(&temp.0).unwrap();
    let mut previewed = BTreeSet::new();
    let mut rejected = BTreeSet::new();
    for entry in fs::read_dir(examples).unwrap() {
        let package = entry.unwrap().path().join("package");
        if !package.join("plugin.json").is_file() {
            continue;
        }
        let name = package
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let revision = manager.list().unwrap().revision;
        let selection = manager.reserve(SourceKind::Directory, &revision).unwrap();
        match manager.selected(selection, Some(&package)) {
            Ok(Some(preview)) => {
                assert_eq!(preview.plugin.plugin_id, name);
                manager.cancel(&preview.token).unwrap();
                previewed.insert(name.to_owned());
            }
            Err(Error::Unsupported) => {
                assert!(manager.pending.is_none());
                rejected.insert(name.to_owned());
            }
            other => panic!("unexpected preview for {name}: {:?}", other.err()),
        }
    }
    assert_eq!(
        previewed,
        BTreeSet::from([
            "com.uipilot.demo-multi".into(),
            "com.uipilot.demo-return".into(),
            "com.uipilot.demo-settings".into(),
        ])
    );
    assert_eq!(rejected.len(), 14);
    assert!(rejected.contains("com.uipilot.note"));
    assert!(rejected.contains("com.uipilot.demo-recovery"));
    let inventory = manager.list().unwrap();
    assert_eq!(inventory.revision, "0");
    assert!(inventory.plugins.is_empty());
}

#[test]
fn lazy_load_has_no_startup_io_and_private_roots_reject_links_and_permissions() {
    let temp = Temp::new();
    let state = ManagementState::default();
    assert!(!temp.0.join("plugin-management-v1").exists());
    state.reset();
    assert!(!temp.0.join("plugin-management-v1").exists());
    fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o775)).unwrap();
    assert!(state.with(&temp.0, |m| m.list()).is_err());
    assert!(!temp.0.join("plugin-management-v1").exists());
    fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(state.with(&temp.0, |m| m.list()).unwrap().revision, "0");
    assert_eq!(
        fs::metadata(&temp.0).unwrap().permissions().mode() & 0o7777,
        0o755
    );
    assert_eq!(
        fs::metadata(temp.0.join("plugin-management-v1"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    let other = Temp::new();
    symlink(&temp.0, other.0.join("plugin-management-v1")).unwrap();
    assert!(Management::load(&other.0).is_err());
}
#[test]
fn opaque_pending_is_single_use_expiring_and_exact_grants_only() {
    let temp = Temp::new();
    temp.add_permission();
    let mut m = Management::load(&temp.0).unwrap();
    let p = temp.preview(&mut m);
    assert_eq!(
        p.required_permissions,
        vec![PublicPermission::ClipboardWrite]
    );
    assert!(m
        .commit(
            "forged",
            &p.expected_revision,
            p.required_permissions.clone()
        )
        .is_err());
    assert!(m.commit(&p.token, &p.expected_revision, vec![]).is_err());
    assert!(m
        .commit(&p.token, &p.expected_revision, p.required_permissions)
        .is_err());
    assert!(m.list().unwrap().plugins.is_empty());
    let p = temp.preview(&mut m);
    m.pending.as_mut().unwrap().expires = Instant::now();
    assert!(m
        .commit(&p.token, &p.expected_revision, p.required_permissions)
        .is_err());
    let p = temp.preview(&mut m);
    m.cancel(&p.token).unwrap();
    assert!(m
        .commit(&p.token, &p.expected_revision, p.required_permissions)
        .is_err());
    let p = temp.preview(&mut m);
    let other = Temp::new();
    let mut foreign = Management::load(&other.0).unwrap();
    assert!(foreign
        .commit(
            &p.token,
            &p.expected_revision,
            p.required_permissions.clone()
        )
        .is_err());
    let committed = m
        .commit(&p.token, &p.expected_revision, p.required_permissions)
        .unwrap();
    assert_eq!(committed.plugins.len(), 1);
    assert!(m.commit(&p.token, &committed.revision, vec![]).is_err());
}
#[test]
fn stale_revision_selection_and_reset_never_publish_an_old_choice() {
    let temp = Temp::new();
    let mut m = Management::load(&temp.0).unwrap();
    let installed = temp.install(&mut m);
    let selection = m
        .reserve(SourceKind::Directory, &installed.revision)
        .unwrap();
    let changed = m.enabled(ID, false, &installed.revision).unwrap();
    assert!(m
        .selected(selection, Some(&temp.0.join("package")))
        .is_err());
    assert!(m.enabled(ID, true, &installed.revision).is_err());
    let p = temp.preview(&mut m);
    assert!(!p.plugin.enabled);
    m.enabled(ID, true, &changed.revision).unwrap();
    assert!(m
        .commit(&p.token, &p.expected_revision, p.required_permissions)
        .is_err());
    let rev = m.list().unwrap().revision;
    let stale = m.reserve(SourceKind::Directory, &rev).unwrap();
    m.reset();
    let current = m.reserve(SourceKind::Directory, &rev).unwrap();
    assert!(m.selected(stale, Some(&temp.0.join("package"))).is_err());
    assert!(m.selected(current, None).unwrap().is_none());
}
#[test]
fn settings_save_is_atomic_and_reload_remove_reinstall_preserve_only_private_storage() {
    let temp = Temp::new();
    temp.add_permission();
    let mut m = Management::load(&temp.0).unwrap();
    let first = temp.install(&mut m);
    assert!(m
        .settings(
            ID,
            &BTreeMap::from([
                ("greeting".into(), json!("badpartial")),
                ("count".into(), json!(99))
            ]),
            &first.revision
        )
        .is_err());
    assert_eq!(
        m.list().unwrap().plugins[0].values["greeting"],
        json!("你好")
    );
    let saved = m
        .settings(
            ID,
            &BTreeMap::from([
                ("greeting".into(), json!("")),
                ("uppercase".into(), json!(false)),
            ]),
            &first.revision,
        )
        .unwrap();
    let denied = m
        .permission(ID, PublicPermission::ClipboardWrite, false, &saved.revision)
        .unwrap();
    drop(m);
    let mut m = Management::load(&temp.0).unwrap();
    let loaded = m.list().unwrap();
    assert_eq!(loaded.revision, denied.revision);
    assert_eq!(loaded.plugins[0].values["greeting"], json!(""));
    assert!(!loaded.plugins[0].permissions[0].granted);
    // An unrelated private-data marker is never traversed/deleted by removal.
    let marker = temp.0.join("plugin-management-v1/storage/private-marker");
    fs::write(&marker, b"retain").unwrap();
    assert!(m.remove(ID, &first.revision).is_err());
    let empty = m.remove(ID, &loaded.revision).unwrap();
    assert!(empty.plugins.is_empty());
    assert_ne!(empty.revision, "0");
    assert!(marker.exists());
    drop(m);
    let mut m = Management::load(&temp.0).unwrap();
    assert_eq!(m.list().unwrap().revision, empty.revision);
    let p = temp.preview(&mut m);
    assert_eq!(
        p.required_permissions,
        vec![PublicPermission::ClipboardWrite]
    );
    assert_eq!(p.plugin.values["greeting"], json!("你好"));
    m.commit(&p.token, &p.expected_revision, p.required_permissions)
        .unwrap();
    assert!(marker.exists());
}
#[test]
fn dialog_cancellation_and_secret_metadata_never_write_or_expose_secret_values() {
    let temp = Temp::new();
    let path = temp.0.join("package/plugin.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["settings"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"secret","key":"token","label":"private"}));
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut m = Management::load(&temp.0).unwrap();
    let s = m.reserve(SourceKind::Directory, "0").unwrap();
    assert!(m.selected(s, None).unwrap().is_none());
    assert_eq!(m.list().unwrap().revision, "0");
    let installed = temp.install(&mut m);
    assert!(installed.plugins[0].settings.iter().all(|s| !s.is_secret()));
    assert!(!installed.plugins[0].values.contains_key("token"));
    assert!(m
        .settings(
            ID,
            &BTreeMap::from([("token".into(), json!("never-write"))]),
            &installed.revision
        )
        .is_err());
}

#[test]
fn close_retires_immediately_while_management_io_mutex_is_occupied() {
    use std::{sync::mpsc, thread};
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    let selection = state
        .with(&temp.0, |m| m.reserve(SourceKind::Directory, "0"))
        .unwrap();
    let old = state.epoch().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_state = state.clone();
    let root = temp.0.clone();
    let worker = thread::spawn(move || {
        worker_state.with(&root, |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(())
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (reset_tx, reset_rx) = mpsc::channel();
    let reset_state = state.clone();
    let reset = thread::spawn(move || {
        reset_state.reset();
        reset_tx.send(()).unwrap();
    });
    let immediate = reset_rx.recv_timeout(Duration::from_millis(500)).is_ok();
    // Always release a deliberately blocked worker before reporting failure.
    release_tx.send(()).unwrap();
    reset.join().unwrap();
    let result = worker.join().unwrap();
    assert!(immediate, "reset must not wait on disk-operation mutex");
    assert_eq!(result, Err(Error::Stale));
    assert!(!state.selection_current(&selection));
    assert!(state
        .with_epoch::<()>(&temp.0, old, |_| panic!(
            "queued old command must never enter"
        ))
        .is_err());
    let new = state
        .with(&temp.0, |m| m.reserve(SourceKind::Directory, "0"))
        .unwrap();
    assert!(state.selection_current(&new));
    assert!(state
        .with(&temp.0, |m| m.selected(selection, None))
        .is_err());
    assert!(state.selection_current(&new));
    assert!(state
        .with(&temp.0, |m| m.selected(new, None))
        .unwrap()
        .is_none());
}
#[test]
fn valid_other_platform_package_is_unsupported_not_corrupt() {
    let temp = Temp::new();
    let path = temp.0.join("package/plugin.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["supportedPlatforms"] = json!(["windows"]);
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut m = Management::load(&temp.0).unwrap();
    let selection = m.reserve(SourceKind::Directory, "0").unwrap();
    assert!(matches!(
        m.selected(selection, Some(&temp.0.join("package"))),
        Err(Error::Unsupported)
    ));
    assert!(m.list().unwrap().plugins.is_empty());
}

#[test]
fn uncertain_commit_drops_poison_without_retry_and_explicit_reload_reads_actual_disk() {
    use super::super::private_file_macos::Failure;
    let temp = Temp::new();
    let state = ManagementState::default();
    let preview = state.with(&temp.0, |m| Ok(temp.preview(m))).unwrap();
    state
        .with(&temp.0, |m| {
            m.host.inject_commit_failure(Failure::AfterRename);
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        state.with(&temp.0, |m| m.commit(
            &preview.token,
            &preview.expected_revision,
            preview.required_permissions.clone()
        )),
        Err(Error::CommitUncertain)
    ));
    assert!(state.inner.lock().unwrap().is_none());
    let path = temp
        .0
        .join("plugin-management-v1/inventory/com.uipilot.inventory/plugins.json");
    let committed = fs::read(&path).unwrap();
    let snapshot = state.with(&temp.0, |m| m.list()).unwrap();
    assert_eq!(snapshot.revision, "1");
    assert_eq!(snapshot.plugins.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), committed);
    assert!(state
        .with(&temp.0, |m| m.commit(
            &preview.token,
            &snapshot.revision,
            preview.required_permissions
        ))
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), committed);
    fs::write(&path, b"not a valid inventory").unwrap();
    for _ in 0..2 {
        assert!(state.with(&temp.0, |m| m.list()).is_err());
        assert!(state.inner.lock().unwrap().is_none());
    }
    assert_eq!(fs::read(&path).unwrap(), b"not a valid inventory");
}

#[test]
fn storage_failure_retires_queued_commands_and_outstanding_native_selection() {
    let temp = Temp::new();
    let state = ManagementState::default();
    state
        .with(&temp.0, |m| {
            temp.install(m);
            Ok(())
        })
        .unwrap();
    let revision = state.with(&temp.0, |m| m.list()).unwrap().revision;
    let selection = state
        .with(&temp.0, |m| m.reserve(SourceKind::Directory, &revision))
        .unwrap();
    let queued = state.epoch().unwrap();
    fs::write(
        temp.0
            .join("plugin-management-v1/inventory/com.uipilot.inventory/plugins.json"),
        b"corrupt",
    )
    .unwrap();
    assert!(matches!(
        state.with(&temp.0, |m| m.list()),
        Err(Error::Storage)
    ));
    assert!(!state.selection_current(&selection));
    assert!(state
        .with_epoch::<()>(&temp.0, queued, |_| panic!(
            "old queued operation must not run"
        ))
        .is_err());
}

#[test]
fn command_preferences_follow_ids_across_rename_reopen_upgrade_and_removal() {
    let temp = Temp::new();
    let path = temp.0.join("package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut upper = manifest.as_object_mut().unwrap().remove("command").unwrap();
    upper["id"] = json!("upper"); upper["defaultName"] = json!("demo-upper");
    let mut lower = upper.clone(); lower["id"] = json!("lower"); lower["defaultName"] = json!("demo-lower");
    manifest["commands"] = json!([upper, lower]);
    manifest["minimumHostVersion"] = json!("0.5.0");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let mut m = Management::load(&temp.0).unwrap();
    let first = temp.install(&mut m);
    let saved = m.command_preference(ID, "upper", CommandPreference { name_override: Some("mac-upper".into()), favorite: true }, &first.revision).unwrap();
    assert_eq!(saved.plugins[0].commands[0].name, "mac-upper");
    assert!(saved.plugins[0].commands[0].favorite);
    assert_eq!(saved.plugins[0].commands[1].name, "demo-lower");
    assert!(!saved.plugins[0].commands[1].favorite);
    assert!(m.runtime_catalog.iter().any(|c| c.command_id == "upper" && c.name == "mac-upper" && c.favorite));
    assert!(!m.runtime_catalog.iter().any(|c| c.name == "demo-upper"));
    assert!(matches!(m.command_preference(ID, "upper", CommandPreference::default(), &first.revision), Err(Error::Stale)));
    drop(m);
    let mut m = Management::load(&temp.0).unwrap();
    assert_eq!(m.list().unwrap().plugins[0].commands[0].name, "mac-upper");
    manifest["version"] = json!("1.0.1");
    manifest["commands"][0]["defaultName"] = json!("new-upper");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let preview = temp.preview(&mut m);
    assert_eq!(preview.plugin.commands[0].name, "mac-upper");
    assert!(preview.plugin.commands[0].favorite);
    let upgraded = m.commit(&preview.token, &preview.expected_revision, preview.required_permissions).unwrap();
    let command = &upgraded.plugins[0].commands[0];
    assert_eq!(command.default_name, "new-upper"); assert_eq!(command.name, "mac-upper"); assert!(command.favorite);
    m.favorite(ID, "upper", false).unwrap();
    let current = m.list().unwrap();
    assert_eq!(current.plugins[0].commands[0].name, "mac-upper");
    let reset = m.command_preference(ID, "upper", CommandPreference::default(), &current.revision).unwrap();
    assert_eq!(reset.plugins[0].commands[0].name, "new-upper");
    m.remove(ID, &reset.revision).unwrap();
    let reinstalled = temp.install(&mut m);
    assert!(!reinstalled.plugins[0].commands[0].favorite);
    assert!(reinstalled.plugins[0].commands[0].name_override.is_none());
}

#[test]
fn invalid_or_conflicting_command_changes_never_mutate_the_inventory() {
    let temp = Temp::new();
    let mut m = Management::load(&temp.0).unwrap();
    let installed = temp.install(&mut m);
    for name in ["", "Bad", "/test", "two words", "find", "web-search"] {
        let error = m.command_preference(ID, "default", CommandPreference { name_override: Some(name.into()), favorite: true }, &installed.revision).err().unwrap();
        assert!(matches!(error, Error::InvalidCommand | Error::Conflict));
        let unchanged = m.list().unwrap();
        assert_eq!(unchanged.revision, installed.revision);
        assert_eq!(unchanged.plugins[0].commands[0].name, "settings-demo");
        assert!(!unchanged.plugins[0].commands[0].favorite);
    }
    assert!(matches!(m.command_preference(ID, "missing", CommandPreference::default(), &installed.revision), Err(Error::InvalidCommand)));
    let path = temp.0.join("package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["pluginId"] = json!("com.test.second"); manifest["command"]["defaultName"] = json!("second");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let current = temp.install(&mut m);
    assert!(matches!(m.command_preference(ID, "default", CommandPreference { name_override: Some("second".into()), favorite: false }, &current.revision), Err(Error::Conflict)));
    assert_eq!(m.list().unwrap().revision, current.revision);
}
