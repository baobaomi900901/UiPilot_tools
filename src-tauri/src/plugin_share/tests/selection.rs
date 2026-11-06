use super::*;

#[test]
fn tickets_require_exact_owner_source_and_single_use() {
    let mut c = ShareCoordinator::new(Ids::default());
    let now = Instant::now();
    let source = doc("source");
    assert!(c.menu_ticket(owner(), &source, now).is_err());
    assert!(c.publish_source(source.clone(), "2", now).is_err());
    c.publish_source(source.clone(), "1", now).unwrap();
    assert!(c.publish_source(source.clone(), "1", now).is_err());
    let menu = c.menu_ticket(owner(), &source, now).unwrap();
    let mut wrong = owner();
    wrong.document_nonce = "wrong".into();
    assert!(c.request_snapshot(&wrong, &menu, now).is_err());
    c.request_snapshot(&owner(), &menu, now).unwrap();
    assert!(c.request_snapshot(&owner(), &menu, now).is_err());
    let mut old = source.clone();
    old.epoch += 1;
    assert!(c.snapshot(&old, &menu, "1", payload("hello"), now).is_err());
    c.snapshot(&source, &menu, "1", payload("hello"), now)
        .unwrap();
    assert!(c
        .snapshot(&source, &menu, "1", payload("hello"), now)
        .is_err());
}

#[test]
fn immutable_snapshot_confirmation_mismatch_cancels_instead_of_replacing() {
    for (rev, body) in [("1", "changed"), ("2", " edited\n中文 ")] {
        let mut f = Flow::preparing();
        f.c.prepared(&f.id, &f.target, &init(), f.now).unwrap();
        f.c.begin_step(&owner(), &f.id, PrepareStep::SourceConfirmation, f.now)
            .unwrap();
        assert!(f
            .c
            .confirm_source(&f.id, &f.source, rev, payload(body), f.now)
            .is_err());
        assert_eq!(f.state(), State::PrepareFailed);
        assert!(!f.has_body());
        assert!(f.commit().is_err());
    }
}

#[test]
fn receiver_revalidation_rejects_disabled_updated_self_and_oversized_targets() {
    for mode in 0..4 {
        let mut f = Flow::choosing();
        let mut current = f.receiver.clone();
        if mode == 1 {
            current.plugin.generation += 1;
        }
        if mode == 2 {
            f.receiver = receiver("source");
            current = f.receiver.clone();
            f.target = f.source.clone();
        }
        if mode == 3 {
            f.receiver.max_bytes = 1;
            current = f.receiver.clone();
        }
        assert!(f
            .c
            .select(
                &owner(),
                &f.id,
                &f.selector,
                &f.receiver,
                if mode == 0 { None } else { Some(&current) },
                f.target.clone(),
                f.now
            )
            .is_err());
        assert!(f.c.transaction.as_ref().unwrap().preparation.is_none());
    }
    let mut f = Flow::preparing();
    assert!(f
        .c
        .select(
            &owner(),
            &f.id,
            &f.selector,
            &f.receiver,
            Some(&f.receiver),
            f.target.clone(),
            f.now
        )
        .is_err());
}

#[test]
fn commit_requires_current_owner_identities_prepared_confirmation_native_and_visibility() {
    let mut f = Flow::preparing();
    assert!(f.commit().is_err());
    for mode in 0..5 {
        let mut f = Flow::ready();
        let mut main = owner();
        let mut source = f.source.clone();
        let mut target = f.target.clone();
        let mut receiver = f.receiver.clone();
        if mode == 0 {
            main.navigation_epoch += 1;
        }
        if mode == 1 {
            source.plugin.inventory_revision += 1;
        }
        if mode == 2 {
            target.nonce = "stale".into();
        }
        if mode == 3 {
            receiver.plugin.admission_epoch += 1;
        }
        assert!(f
            .c
            .commit(
                &f.id,
                CommitView {
                    owner: &main,
                    active_source: &source,
                    pending_target: &target,
                    receiver: &receiver,
                    main_visible: mode != 4,
                    initialization: &init()
                },
                f.now
            )
            .is_err());
    }
}

#[test]
fn preparation_steps_and_total_deadline_cannot_be_extended() {
    let mut f = Flow::preparing();
    assert!(f
        .c
        .begin_step(&owner(), &f.id, PrepareStep::Initialization, f.now)
        .is_err());
    f.c.tick(f.now + SHORT);
    assert_eq!(f.state(), State::PrepareFailed);
    assert!(!f.has_body());
    let mut f = Flow::ready();
    f.c.tick(f.now + PREPARING);
    assert_eq!(f.state(), State::PrepareFailed);
    assert!(!f.has_body());
}

#[test]
fn continue_editing_revokes_host_first_and_requires_exact_local_confirmation() {
    let mut f = Flow::committed();
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.tick(f.now + SHORT);
    f.c.end(&owner(), &f.id, EndReason::ContinueEditing, f.later(3))
        .unwrap();
    assert!(f.has_body());
    assert!(!f.c.transaction.as_ref().unwrap().input_safe);
    assert!(f.c.ack(&f.id, &f.target, true, f.later(3)).is_err());
    let mut wrong = f.target.clone();
    wrong.epoch += 1;
    assert!(f
        .c
        .cancellation_ack(&f.id, &wrong, LocalCancellation::Revoked, f.later(3))
        .is_err());
    f.c.cancellation_ack(&f.id, &f.target, LocalCancellation::Revoked, f.later(3))
        .unwrap();
    assert!(!f.has_body());
    assert!(f.c.transaction.as_ref().unwrap().input_safe);
    assert!(effects(&mut f.c).contains(&Effect::EnableTargetInput(f.target.clone())));
}

#[test]
fn runtime_preparation_uses_total_deadline_until_update_handshake_begins() {
    let mut f = Flow::choosing();
    f.c.select(
        &owner(),
        &f.id,
        &f.selector,
        &f.receiver,
        Some(&f.receiver),
        f.target.clone(),
        f.now,
    )
    .unwrap();
    f.c.tick(f.later(25));
    assert_eq!(f.state(), State::Preparing);
    f.c.bind_initialization(&f.id, &f.target, init(), f.later(25))
        .unwrap();
    f.c.tick(f.later(27));
    assert_eq!(f.state(), State::PrepareFailed);
    assert!(effects(&mut f.c)
        .iter()
        .any(|e| matches!(e, Effect::RetirePreparation { .. })));
    let mut f = Flow::choosing();
    f.c.select(
        &owner(),
        &f.id,
        &f.selector,
        &f.receiver,
        Some(&f.receiver),
        f.target.clone(),
        f.now,
    )
    .unwrap();
    f.c.tick(f.now + PREPARING);
    assert_eq!(f.state(), State::PrepareFailed);
    assert!(!f.has_body());
}

#[test]
fn unregister_requires_new_revision_and_never_reuses_old_tickets() {
    let mut f = Flow::choosing();
    f.c.unregister_source(&f.source, f.now).unwrap();
    assert!(!f.has_body());
    assert!(f.c.menu_ticket(owner(), &f.source, f.now).is_err());
    assert!(f.c.publish_source(f.source.clone(), "1", f.now).is_err());
    f.c.publish_source(f.source.clone(), "2", f.now).unwrap();
    assert!(f.c.menu_ticket(owner(), &f.source, f.now).is_ok());
}

#[test]
fn commit_checks_current_initialization_and_source_edit_restores_valid_source() {
    let mut f = Flow::ready();
    let mut current = init();
    current.update_sequence += 1;
    assert!(f
        .c
        .commit(
            &f.id,
            CommitView {
                owner: &owner(),
                active_source: &f.source,
                pending_target: &f.target,
                receiver: &f.receiver,
                main_visible: true,
                initialization: &current
            },
            f.now
        )
        .is_err());
    let mut f = Flow::ready();
    f.c.publish_source(f.source.clone(), "2", f.now).unwrap();
    assert!(effects(&mut f.c).contains(&Effect::RestoreSource(f.source)));
}

#[test]
fn target_only_precommit_revocation_restores_source_but_source_revocation_does_not() {
    let mut f = Flow::preparing();
    f.c.cancel_before_commit(&owner(), Some(&f.id), true, true, f.now)
        .unwrap();
    assert_eq!(f.state(), State::PrepareFailed);
    assert!(effects(&mut f.c).contains(&Effect::RestoreSource(f.source)));
    let mut f = Flow::preparing();
    f.c.end(&owner(), &f.id, EndReason::IdentityRevoked, f.now)
        .unwrap();
    assert_eq!(f.state(), State::Cancelled);
    assert!(!effects(&mut f.c)
        .iter()
        .any(|e| matches!(e, Effect::RestoreSource(_))));
}
