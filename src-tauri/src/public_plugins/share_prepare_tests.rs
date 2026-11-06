use super::*;
use crate::public_plugins::share_prepare::SharePrepareError as E;
use crate::public_plugins::{
    runtime_label, PluginContextStatus, PluginInvocationTheme, PluginRequestOwner,
    PublicPluginManagementError, ScheduledPluginRequest,
};

fn fixture(
    name: &str,
) -> (
    TestRoot,
    Arc<PublicPluginService>,
    Arc<PublicPluginManager>,
    crate::plugin_share::catalog::Receiver,
) {
    let root = TestRoot::new(name);
    let mut value = manifest("panel");
    value["minimumHostVersion"] = json!("0.22.0");
    value["share"] = json!({"version":1,"receivers":[{"id":"receive","title":"Receive","commandId":"default","types":["text/plain"],"maxBytes":8192}]});
    let source = root.package();
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost::current(PublicPlatform::Windows),
            [],
            Arc::new(MessageCenterService::load(&app_data)),
        )
        .unwrap(),
    );
    let now = Instant::now();
    let prepared = manager
        .prepare(
            "main",
            PublicPluginInstallSource::DevelopmentDirectory { path: source },
            now,
        )
        .unwrap();
    manager
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([PublicPermission::UiPanel]),
            now,
            |_| true,
        )
        .unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    let receiver = manager.share_catalog().unwrap().remove(0);
    (root, service, manager, receiver)
}
fn completion(request: &ScheduledPluginRequest) -> PluginCommandCompletion {
    PluginCommandCompletion {
        context: request.context.clone(),
        failed: false,
        response: Some(
            json!({"requestId":request.context.request_id,"data":{"draft":"initialized"}}),
        ),
    }
}
#[test]
fn share_prepare_service_admits_without_panel_and_uses_independent_response_channel() {
    let (_root, service, manager, receiver) = fixture("share-prepare-channel");
    let now = Instant::now();
    let pending = service
        .begin_share_prepare(receiver.clone(), "host-transfer".into(), now)
        .unwrap();
    assert_eq!(
        service
            .begin_share_prepare(receiver, "other".into(), now)
            .err(),
        Some(E::Busy)
    );
    let (kind, invocation) = service
        .dispatch_invocation(&pending.dispatch, PluginInvocationTheme::Dark, "now".into())
        .unwrap();
    assert_eq!(kind, "command");
    assert_eq!(invocation["input"], "");
    assert!(invocation.get("image").is_none());
    assert_eq!(
        invocation["context"]["entry"],
        json!({"kind":"sharePrepare","receiverId":"receive"})
    );
    assert!(
        invocation.get("commandId").is_none(),
        "legacy command omission is preserved"
    );
    let completion = completion(&pending.dispatch);
    let outcome = manager
        .complete(
            &runtime_label(
                &completion.context.plugin_id,
                completion.context.plugin_generation,
            )
            .unwrap(),
            &completion,
            now,
        )
        .unwrap();
    assert_eq!(
        service.complete_share_prepare_with(&completion, &outcome, || {}),
        Some(true)
    );
    assert_eq!(
        pending.receiver.recv().unwrap().unwrap().data,
        json!({"draft":"initialized"})
    );
    assert!(pending.registration.result_is_current());
    assert!(service.submissions.lock().unwrap().by_token.is_empty());
    assert!(service.content_actions.lock().unwrap().is_empty());
    assert!(service.share_preparations.lock().unwrap().is_empty());
}
#[test]
fn share_prepare_cancel_after_sender_take_wins_and_late_completion_cannot_publish() {
    let (_root, service, manager, receiver) = fixture("share-prepare-cancel-take");
    let now = Instant::now();
    let pending = service
        .begin_share_prepare(receiver, "host-transfer".into(), now)
        .unwrap();
    let completion = completion(&pending.dispatch);
    let outcome = manager
        .complete(
            &runtime_label(
                &completion.context.plugin_id,
                completion.context.plugin_generation,
            )
            .unwrap(),
            &completion,
            now,
        )
        .unwrap();
    assert_eq!(
        service
            .complete_share_prepare_with(&completion, &outcome, || pending.registration.cancel()),
        None
    );
    assert_eq!(pending.receiver.recv().unwrap(), Err(E::Cancelled));
    assert!(!pending.registration.result_is_current());
    assert!(matches!(
        outcome.completed_owner,
        PluginRequestOwner::SharePrepare(_)
    ));
}
#[test]
fn share_prepare_drop_before_emit_aborts_slot_and_rejects_late_dispatch() {
    let (_root, service, manager, receiver) = fixture("share-prepare-drop");
    let now = Instant::now();
    let pending = service
        .begin_share_prepare(receiver.clone(), "host-transfer".into(), now)
        .unwrap();
    drop(pending.registration);
    assert_eq!(pending.receiver.recv().unwrap(), Err(E::Cancelled));
    let next = service
        .begin_share_prepare(receiver, "next".into(), now)
        .unwrap();
    assert_eq!(next.dispatch.deadline, pending.dispatch.deadline);
    assert!(service
        .emit_share_prepare_with(&pending.dispatch, || {
            panic!("cancelled undispatched handler must never emit alongside its replacement")
        })
        .is_err());
    assert_eq!(
        manager.scheduler().context_status(&next.dispatch.context),
        PluginContextStatus::Current
    );
    assert!(service.submissions.lock().unwrap().by_token.is_empty());
}

#[test]
fn share_prepare_post_admission_invalidation_aborts_before_recheck_returns_error() {
    let (_root, service, manager, receiver) = fixture("share-prepare-admission-race");
    let now = Instant::now();
    let plugin_id = receiver.plugin.plugin_id.clone();
    assert_eq!(
        service
            .begin_share_prepare_with(receiver, "transfer".into(), now, || {
                manager
                    .scheduler()
                    .retire_plugin_requests(&plugin_id)
                    .unwrap();
            })
            .err(),
        Some(E::Expired)
    );
    assert!(service.share_preparations.lock().unwrap().is_empty());
    let current_receiver = manager.share_catalog().unwrap().remove(0);
    assert!(service
        .begin_share_prepare(current_receiver, "next".into(), now)
        .is_ok());
}

#[test]
fn share_prepare_emission_claim_then_cancel_keeps_physical_slot_and_original_deadline() {
    let (_root, service, manager, receiver) = fixture("share-prepare-emission-cancel");
    let now = Instant::now();
    let pending = service
        .begin_share_prepare(receiver.clone(), "transfer".into(), now)
        .unwrap();
    service
        .emit_share_prepare_with(&pending.dispatch, || {
            // The real native emitter can deliver before reporting failure. Cancel
            // here is also the deterministic claim-vs-cancel interleaving.
            pending.registration.cancel();
            assert_eq!(
                service
                    .begin_share_prepare(receiver.clone(), "next".into(), now)
                    .err(),
                Some(E::Busy)
            );
            Err(PublicPluginManagementError::Unavailable)
        })
        .unwrap_err();
    assert_eq!(pending.receiver.recv().unwrap(), Err(E::Cancelled));
    assert_eq!(pending.dispatch.deadline, now + Duration::from_secs(30));
    assert!(manager
        .scheduler()
        .expire_timeouts(pending.dispatch.deadline - Duration::from_nanos(1))
        .unwrap()
        .is_empty());
    let replacements = manager
        .scheduler()
        .expire_timeouts(pending.dispatch.deadline)
        .unwrap();
    assert_eq!(replacements.len(), 1);
    assert!(!replacements[0].counts_as_fault);
}
#[test]
fn share_prepare_revalidates_receiver_identity_and_rejects_invalid_initialization() {
    let (_root, service, manager, receiver) = fixture("share-prepare-identity");
    let now = Instant::now();
    for change in 0..5 {
        let mut forged = receiver.clone();
        match change {
            0 => forged.plugin.inventory_revision += 1,
            1 => forged.plugin.activation_id += 1,
            2 => forged.receiver_id = "other".into(),
            3 => forged.command_id = "other".into(),
            _ => forged.max_bytes += 1,
        }
        assert_eq!(
            service
                .begin_share_prepare(forged, "transfer".into(), now)
                .err(),
            Some(E::Expired)
        );
        assert!(service.share_preparations.lock().unwrap().is_empty());
    }
    for response in [
        json!({"requestId":"wrong","data":null}),
        json!({"results":[],"data":null}),
        json!({"data":null}),
    ] {
        let pending = service
            .begin_share_prepare(receiver.clone(), "transfer".into(), now)
            .unwrap();
        let mut completion = completion(&pending.dispatch);
        completion.response = Some(response);
        let outcome = manager
            .complete(
                &runtime_label(
                    &completion.context.plugin_id,
                    completion.context.plugin_generation,
                )
                .unwrap(),
                &completion,
                now,
            )
            .unwrap();
        assert_eq!(
            service.complete_share_prepare_with(&completion, &outcome, || {}),
            Some(false)
        );
        assert_eq!(pending.receiver.recv().unwrap(), Err(E::InvalidResponse));
    }
}

#[test]
fn share_prepare_shutdown_and_generation_settlement_cancel_after_sender_take() {
    for shutdown in [false, true] {
        let (_root, service, manager, receiver) = fixture("share-prepare-lifecycle-take");
        let pending = service
            .begin_share_prepare(receiver.clone(), "host-transfer".into(), Instant::now())
            .unwrap();
        let completion = completion(&pending.dispatch);
        let outcome = manager
            .complete(
                &runtime_label(
                    &completion.context.plugin_id,
                    completion.context.plugin_generation,
                )
                .unwrap(),
                &completion,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            service.complete_share_prepare_with(&completion, &outcome, || {
                if shutdown {
                    service.shutdown();
                } else {
                    service.settle_panel_generation(
                        &receiver.plugin.plugin_id,
                        receiver.plugin.generation,
                    );
                }
            }),
            None
        );
        assert_eq!(pending.receiver.recv().unwrap(), Err(E::Cancelled));
        assert!(!pending.registration.result_is_current());
    }
}
#[test]
fn share_prepare_current_bundle_rechecked_after_sender_take_without_runtime_current_context() {
    let (_root, service, manager, receiver) = fixture("share-prepare-final-identity");
    let pending = service
        .begin_share_prepare(receiver.clone(), "host-transfer".into(), Instant::now())
        .unwrap();
    let completion = completion(&pending.dispatch);
    let outcome = manager
        .complete(
            &runtime_label(
                &completion.context.plugin_id,
                completion.context.plugin_generation,
            )
            .unwrap(),
            &completion,
            Instant::now(),
        )
        .unwrap();
    assert_eq!(
        service.complete_share_prepare_with(&completion, &outcome, || {
            manager
                .set_enabled_with_readiness(&receiver.plugin.plugin_id, false, |_| true)
                .unwrap();
        }),
        None
    );
    assert_eq!(pending.receiver.recv().unwrap(), Err(E::Expired));
}

#[test]
fn share_prepare_unemitted_replacement_by_ordinary_command_settles_and_never_queues_or_emits_old_handler(
) {
    let (_root, service, manager, receiver) = fixture("share-prepare-command-replace");
    let now = Instant::now();
    let pending = service
        .begin_share_prepare(receiver, "transfer".into(), now)
        .unwrap();
    let ordinary = service
        .schedule_command(
            manager.route("/demo ordinary").unwrap().unwrap(),
            1,
            "/demo ordinary".into(),
            now,
        )
        .unwrap();
    let ordinary_dispatch = ordinary
        .dispatch
        .expect("ordinary command must not wait behind a never-emitted handler");
    assert_eq!(
        pending
            .receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap(),
        Err(E::Cancelled)
    );
    assert!(!pending.registration.result_is_current());
    assert!(service
        .emit_share_prepare_with(&pending.dispatch, || panic!("replaced handler emitted"))
        .is_err());
    assert_eq!(
        manager
            .scheduler()
            .context_status(&ordinary_dispatch.context),
        PluginContextStatus::Current
    );
    let ordinary_completion = completion(&ordinary_dispatch);
    let outcome = manager
        .complete(
            &runtime_label(
                &ordinary_dispatch.context.plugin_id,
                ordinary_dispatch.context.plugin_generation,
            )
            .unwrap(),
            &ordinary_completion,
            now,
        )
        .unwrap();
    assert!(outcome.accepted);
    assert!(
        outcome.next.is_none(),
        "no orphaned waiter is left after the ordinary handler completes"
    );
}

#[test]
fn share_prepare_emitted_replacement_by_ordinary_command_waits_for_physical_completion() {
    let (_root, service, manager, receiver) = fixture("share-prepare-emitted-command-replace");
    let now = Instant::now();
    let pending = service
        .begin_share_prepare(receiver, "transfer".into(), now)
        .unwrap();
    service
        .emit_share_prepare_with(&pending.dispatch, || Ok(()))
        .unwrap();
    let ordinary = service
        .schedule_command(
            manager.route("/demo ordinary").unwrap().unwrap(),
            1,
            "/demo ordinary".into(),
            now,
        )
        .unwrap();
    assert!(
        ordinary.dispatch.is_none(),
        "an emitted handler still owns the physical slot"
    );
    assert_eq!(
        pending
            .receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap(),
        Err(E::Cancelled)
    );
    let done = completion(&pending.dispatch);
    let outcome = manager
        .complete(
            &runtime_label(&done.context.plugin_id, done.context.plugin_generation).unwrap(),
            &done,
            now,
        )
        .unwrap();
    assert!(!outcome.accepted);
    assert!(matches!(
        outcome.completed_owner,
        PluginRequestOwner::SharePrepare(_)
    ));
    assert!(matches!(
        outcome.next.unwrap().candidate.owner,
        PluginRequestOwner::Command(_)
    ));
}
