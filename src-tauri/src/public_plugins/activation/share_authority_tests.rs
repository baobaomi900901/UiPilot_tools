fn share_authority_fixture(label: &str) -> (TestDir, Arc<PublicPluginManager>) {
    let dir = TestDir::new(label);
    write_package(&dir.source(), "1.0.0", "sharing");
    let path = dir.source().join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["minimumHostVersion"] = json!("0.22.0");
    manifest["command"]["activationMode"] = json!("submit");
    manifest["command"]["outputMode"] = json!("panel");
    manifest["panel"] = json!({"entry":"dist/panel.html"});
    manifest["permissions"] = json!(["ui.panel"]);
    manifest["settings"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"secret","key":"token","label":"Token"}));
    manifest["share"] = json!({"version":1,"source":true,"receivers":[{"id":"receive","title":"Receive","commandId":"default","types":["text/plain"],"maxBytes":8192}]});
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(dir.source().join("dist/panel.html"), "<!doctype html>").unwrap();
    let manager = Arc::new(manager(&dir));
    let now = Instant::now();
    let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
    manager
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([PublicPermission::UiPanel]),
            now,
            |_| true,
        )
        .unwrap();
    (dir, manager)
}

fn share_authority_pair(
    manager: &PublicPluginManager,
) -> (
    super::share_authority::ShareAuthorityLease,
    super::share_authority::ShareAuthorityLease,
) {
    let receiver = manager.share_catalog().unwrap().remove(0);
    let source = manager
        .issue_share_source_authority(&receiver.plugin, "default")
        .unwrap();
    let target = manager.issue_share_receiver_authority(&receiver).unwrap();
    (source, target)
}

#[test]
fn share_authority_reservation_revokes_both_and_rollback_only_allows_fresh_issuance() {
    let (_dir, manager) = share_authority_fixture("authority-reservation");
    let (source, target) = share_authority_pair(&manager);
    let registry = manager.share_authorities();
    assert!(registry.read_pair(&source, &target).is_ok());
    let reservation = manager.bundles.reserve("com.example.activation").unwrap();
    assert!(registry.read_pair(&source, &target).is_err());
    assert!(manager
        .issue_share_source_authority(&source.identity().plugin, "default")
        .is_err());
    drop(reservation);
    let fresh = share_authority_pair(&manager);
    assert!(registry.read_pair(&fresh.0, &fresh.1).is_ok());
    assert!(registry.read_pair(&source, &target).is_err());
    assert_ne!(source.revision(), fresh.0.revision());
    assert_eq!(manager.drain_share_authority_revocations().len(), 2);
    assert!(source.revocation().is_some());
}

#[test]
fn share_authority_nonreservation_revokers_are_irrevocable() {
    for action in ["secret", "generation", "close", "shutdown", "terminal"] {
        let (_dir, manager) = share_authority_fixture(action);
        let (source, target) = share_authority_pair(&manager);
        match action {
            "secret" => manager
                .save_secret("com.example.activation", "token", None)
                .unwrap(),
            "generation" => {
                manager.invalidate_runtime_generation(
                    "com.example.activation",
                    source.identity().plugin.generation,
                );
            }
            "close" => manager.close_network_authority("com.example.activation"),
            "shutdown" => {
                manager.shutdown_network();
            }
            _ => {
                let unrelated = manager.bundles.reserve("com.example.other").unwrap();
                unrelated.begin_committing().unwrap();
                drop(unrelated);
            }
        }
        assert!(
            manager
                .share_authorities()
                .read_pair(&source, &target)
                .is_err(),
            "{action}"
        );
        assert!(source.revocation().is_some(), "{action}");
    }
}

#[test]
fn share_authority_manager_settings_and_permission_reserve_before_durability() {
    use std::sync::{mpsc, Barrier};
    for permission in [false, true] {
        let (_dir, manager) = share_authority_fixture("authority-durable");
        let (source, target) = share_authority_pair(&manager);
        let registry = manager.share_authorities();
        let before = manager.state.config("com.example.activation").unwrap();
        let (reached, reached_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        *manager.before_state_durable.lock().unwrap() = Some(Arc::new({
            let release = release.clone();
            move || {
                reached.send(()).unwrap();
                release.wait();
            }
        }));
        let worker = std::thread::spawn({
            let manager = manager.clone();
            move || {
                if permission {
                    manager
                        .set_permission_access_with_effects(
                            "com.example.activation",
                            PublicPermission::UiPanel,
                            false,
                            &manager.state.inventory_revision().unwrap().to_string(),
                            &mut ManagementEffects::default(),
                        )
                        .unwrap();
                } else {
                    manager
                        .save_settings(
                            "com.example.activation",
                            &BTreeMap::from([("compact".into(), json!(true))]),
                        )
                        .unwrap();
                }
            }
        });
        reached_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            manager.state.config("com.example.activation").unwrap(),
            before
        );
        assert!(registry.read_pair(&source, &target).is_err());
        // Persistence hook holds neither M nor A: revocation publication is already visible.
        assert!(manager.mutation.try_lock().is_ok());
        assert_eq!(manager.drain_share_authority_revocations().len(), 2);
        release.wait();
        worker.join().unwrap();
        assert!(registry.read_pair(&source, &target).is_err());
        if !permission {
            let fresh = share_authority_pair(&manager);
            assert!(registry.read_pair(&fresh.0, &fresh.1).is_ok());
        }
    }
}

#[test]
fn share_authority_read_guard_excludes_real_reservation_secret_and_global_terminal() {
    use std::sync::mpsc;
    for action in ["settings", "secret", "terminal"] {
        let (_dir, manager) = share_authority_fixture(action);
        let (source, target) = share_authority_pair(&manager);
        let unrelated = if action == "terminal" {
            let reservation = manager.bundles.reserve("com.example.other").unwrap();
            reservation.begin_committing().unwrap();
            Some(reservation)
        } else {
            None
        };
        let registry = manager.share_authorities();
        let guard = registry.read_pair(&source, &target).unwrap();
        let (started, started_rx) = mpsc::channel();
        let (done, done_rx) = mpsc::channel();
        let worker = std::thread::spawn({
            let manager = manager.clone();
            move || {
                started.send(()).unwrap();
                match action {
                    "settings" => {
                        manager
                            .save_settings(
                                "com.example.activation",
                                &BTreeMap::from([("compact".into(), json!(true))]),
                            )
                            .unwrap();
                    }
                    "secret" => manager
                        .save_secret("com.example.activation", "token", None)
                        .unwrap(),
                    _ => drop(unrelated),
                }
                done.send(()).unwrap();
            }
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "{action}"
        );
        assert!(source.revocation().is_none());
        drop(guard);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        assert!(registry.read_pair(&source, &target).is_err());
    }
}

#[test]
fn share_authority_exact_receiver_source_and_registry_binding() {
    let (_dir, manager) = share_authority_fixture("authority-identity");
    let (source, target) = share_authority_pair(&manager);
    let mut receiver = manager.share_catalog().unwrap().remove(0);
    receiver.max_bytes += 1;
    assert!(manager.issue_share_receiver_authority(&receiver).is_err());
    let mut stale = source.identity().plugin.clone();
    stale.activation_id += 1;
    assert!(manager
        .issue_share_source_authority(&stale, "default")
        .is_err());
    assert!(manager
        .issue_share_source_authority(&source.identity().plugin, "other")
        .is_err());
    assert!(manager
        .share_authorities()
        .read_pair(&target, &source)
        .is_err());
    assert!(ShareAuthorityRegistry::default()
        .read_pair(&source, &target)
        .is_err());
    manager.invalidate_runtime_generation("com.example.activation", stale.generation + 1);
    assert!(manager
        .share_authorities()
        .read_pair(&source, &target)
        .is_ok());
}

#[test]
fn share_authority_timeout_revokes_before_scheduler_generation_becomes_observable() {
    use super::super::share_prepare::{SharePrepareLease, SharePrepareOwner};
    let (_dir, manager) = share_authority_fixture("authority-timeout");
    let (source, target) = share_authority_pair(&manager);
    let now = Instant::now();
    let receiver = manager.share_catalog().unwrap().remove(0);
    let owner = SharePrepareOwner {
        receiver,
        prepare_id: "authority-timeout".into(),
        lease: SharePrepareLease::default(),
    };
    let _request = manager.admit_share_prepare(&owner, now).unwrap();
    let replacements = manager
        .expire_network_timeouts(now + Duration::from_secs(31))
        .unwrap();
    assert_eq!(replacements.len(), 1);
    assert!(manager
        .share_authorities()
        .read_pair(&source, &target)
        .is_err());
    assert!(manager
        .issue_share_receiver_authority(&owner.receiver)
        .is_err());
}

#[test]
fn share_authority_replacement_waits_for_successful_runtime_finish() {
    use super::super::share_prepare::{SharePrepareLease, SharePrepareOwner};
    let (_dir, manager) = share_authority_fixture("authority-replacement");
    let (source, target) = share_authority_pair(&manager);
    let now = Instant::now();
    let receiver = manager.share_catalog().unwrap().remove(0);
    let owner = SharePrepareOwner {
        receiver,
        prepare_id: "replace".into(),
        lease: SharePrepareLease::default(),
    };
    manager.admit_share_prepare(&owner, now).unwrap();
    let replacement = manager
        .expire_network_timeouts(now + Duration::from_secs(31))
        .unwrap()
        .remove(0);
    let runtime = manager
        .replace_runtime_generation(
            &replacement.plugin_id,
            replacement.previous_generation,
            replacement.new_generation,
        )
        .unwrap();
    let next = manager.share_catalog().unwrap().remove(0);
    assert!(manager.issue_share_receiver_authority(&next).is_err());
    manager
        .finish_runtime_replacement(
            &runtime.plugin_id,
            runtime.generation,
            runtime.activation_id,
            runtime.admission_epoch,
            now + Duration::from_secs(32),
        )
        .unwrap();
    let fresh = share_authority_pair(&manager);
    assert!(manager
        .share_authorities()
        .read_pair(&fresh.0, &fresh.1)
        .is_ok());
    assert!(manager
        .share_authorities()
        .read_pair(&source, &target)
        .is_err());
    manager
        .mark_runtime_unavailable(
            &runtime.plugin_id,
            runtime.generation - 1,
            runtime.activation_id - 1,
        )
        .unwrap();
    assert!(manager
        .share_authorities()
        .read_pair(&fresh.0, &fresh.1)
        .is_ok());
}

#[test]
fn share_authority_generation_invalidation_cannot_interleave_manager_publication() {
    use std::sync::mpsc;
    let (_dir, manager) = share_authority_fixture("authority-generation-order");
    let (source, target) = share_authority_pair(&manager);
    let mutation = manager.lock_mutation().unwrap();
    let generation = source.identity().plugin.generation;
    let (started, started_rx) = mpsc::channel();
    let (done, done_rx) = mpsc::channel();
    let worker = std::thread::spawn({
        let manager = manager.clone();
        move || {
            started.send(()).unwrap();
            manager.invalidate_runtime_generation("com.example.activation", generation);
            done.send(()).unwrap();
        }
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed_early = done_rx.recv_timeout(Duration::from_millis(100)).is_ok();
    drop(mutation);
    worker.join().unwrap();
    assert!(
        !completed_early,
        "generation invalidation bypassed the publication mutex"
    );
    assert!(manager
        .share_authorities()
        .read_pair(&source, &target)
        .is_err());
}

#[test]
fn share_authority_poisoned_manager_revokes_at_unwind_boundary() {
    let (_dir, manager) = share_authority_fixture("authority-poison");
    let (source, target) = share_authority_pair(&manager);
    let worker = std::thread::spawn({
        let manager = manager.clone();
        move || {
            let _mutation = manager.lock_mutation().unwrap();
            panic!("synthetic manager panic");
        }
    });
    assert!(worker.join().is_err());
    assert!(manager
        .share_authorities()
        .read_pair(&source, &target)
        .is_err());
    assert!(source.revocation().is_some());
}

#[test]
fn share_authority_poisoned_registry_fails_closed_and_retains_terminal_history() {
    let (_dir, manager) = share_authority_fixture("authority-registry-poison");
    let (source, target) = share_authority_pair(&manager);
    let registry = manager.share_authorities();
    let worker = std::thread::spawn({
        let registry = registry.clone();
        move || {
            let _write = registry.write();
            panic!("synthetic authority panic");
        }
    });
    assert!(worker.join().is_err());
    assert!(registry.read_pair(&source, &target).is_err());
    assert_eq!(source.revocation(), Some(ShareRevocation::Terminal));
    assert_eq!(manager.drain_share_authority_revocations().len(), 2);
    assert!(manager
        .issue_share_source_authority(&source.identity().plugin, "default")
        .is_err());
}

#[test]
fn share_authority_secret_failure_cannot_reissue_until_reconfirmed_success() {
    let (_dir, manager) = share_authority_fixture("authority-secret-failure");
    let (source, target) = share_authority_pair(&manager);
    manager
        .secrets
        .fail_next_for_test(super::super::secrets::SecretTestFault::BeforeCommit);
    assert!(manager
        .save_secret(
            "com.example.activation",
            "token",
            Some("synthetic-test-token")
        )
        .is_err());
    assert!(manager
        .share_authorities()
        .read_pair(&source, &target)
        .is_err());
    assert!(manager
        .issue_share_source_authority(&source.identity().plugin, "default")
        .is_err());
    manager
        .save_secret("com.example.activation", "token", None)
        .unwrap();
    let fresh = share_authority_pair(&manager);
    assert_eq!(source.identity(), fresh.0.identity());
    assert_ne!(source.revision(), fresh.0.revision());
    assert!(manager
        .share_authorities()
        .read_pair(&fresh.0, &fresh.1)
        .is_ok());
    assert!(source.revocation().is_some());
}

#[test]
fn share_authority_recovery_failure_after_bundle_publication_cannot_issue() {
    for fail_network in [false, true] {
        let (_dir, manager) = share_authority_fixture("authority-recovery-publication");
        let (source, target) = share_authority_pair(&manager);
        let transaction = manager
            .begin_uninstall("com.example.activation", true)
            .unwrap()
            .unwrap();
        let recovery = manager
            .abort_uninstall_before_commit(transaction)
            .unwrap()
            .unwrap();
        let receiver = manager.share_catalog().unwrap().remove(0);
        assert!(manager.issue_share_receiver_authority(&receiver).is_err());
        if fail_network {
            manager.network_authority.shutdown();
            assert!(manager.finish_runtime_recovery(&recovery).is_err());
            assert!(manager.issue_share_receiver_authority(&receiver).is_err());
        } else {
            assert!(manager.finish_runtime_recovery(&recovery).unwrap());
            assert!(manager.issue_share_receiver_authority(&receiver).is_ok());
        }
        assert!(manager
            .share_authorities()
            .read_pair(&source, &target)
            .is_err());
    }
}
