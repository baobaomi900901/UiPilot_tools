use super::*;

#[test]
fn snapshot_timeout_cancellation_and_choosing_expiry_release_everything() {
    for cancel in [false, true] {
        let mut c = ShareCoordinator::new(Ids::default());
        let now = Instant::now();
        let source = doc("source");
        c.publish_source(source.clone(), "1", now).unwrap();
        let menu = c.menu_ticket(owner(), &source, now).unwrap();
        c.request_snapshot(&owner(), &menu, now).unwrap();
        if cancel {
            c.cancel_before_commit(&owner(), None, true, false, now)
                .unwrap();
        } else {
            c.tick(now + SHORT);
        }
        assert!(c.request.is_none());
        assert!(c
            .snapshot(&source, &menu, "1", payload("hello"), now + SHORT)
            .is_err());
        assert!(c.transaction.is_none());
    }
    let mut f = Flow::choosing();
    f.c.tick(f.now + CHOOSING);
    assert_eq!(f.state(), State::Cancelled);
    assert!(!f.has_body());
}

#[test]
fn invalidation_before_commit_wins_but_after_commit_never_rolls_back() {
    let mut f = Flow::ready();
    f.c.publish_source(f.source.clone(), "2", f.now).unwrap();
    assert_eq!(f.state(), State::Cancelled);
    assert!(!f.has_body());
    assert!(f.commit().is_err());
    let mut f = Flow::committed();
    f.c.publish_source(f.source.clone(), "2", f.now).unwrap();
    f.c.unregister_source(&f.source, f.now).unwrap();
    assert_eq!(f.state(), State::Committed);
    let delivered = f.c.claim(&f.id, &f.target, f.now).unwrap();
    assert_eq!(text(&delivered.payload), " edited\n中文 ");
}

#[test]
fn claim_is_one_time_identity_bound_and_deadline_enforced_even_if_response_lost() {
    let mut f = Flow::committed();
    let mut wrong = f.target.clone();
    wrong.nonce = "bad".into();
    assert!(f.c.claim(&f.id, &wrong, f.now).is_err());
    let response = f.c.claim(&f.id, &f.target, f.now).unwrap();
    drop(response);
    assert!(f.c.claim(&f.id, &f.target, f.now).is_err());
    assert_eq!(f.c.spent.len(), 1);
    assert!(f.c.spent[0].issued);
    let mut late = Flow::committed();
    assert!(late
        .c
        .claim(&late.id, &late.target, late.now + SHORT)
        .is_err());
    assert_eq!(late.state(), State::DeliveryUnknown);
}

#[test]
fn ack_requires_issued_body_and_late_ack_can_correct_unknown_once() {
    let mut f = Flow::committed();
    assert!(f.c.ack(&f.id, &f.target, true, f.now).is_err());
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.tick(f.now + SHORT);
    assert_eq!(f.state(), State::DeliveryUnknown);
    f.c.ack(&f.id, &f.target, true, f.later(3)).unwrap();
    assert_eq!(f.state(), State::Accepted);
    assert!(!f.has_body());
    f.c.ack(&f.id, &f.target, true, f.later(4)).unwrap();
    assert_eq!(f.c.spent.len(), 1);
    assert!(f.c.ack(&f.id, &f.target, false, f.later(4)).is_err());
    f.c.document_lost(&f.target, f.later(5));
    assert!(!f.has_body());
    f.c.document_ended(&f.target, f.later(5));
    assert!(f.c.spent.is_empty());
}

#[test]
fn rejection_is_terminal_and_ready_failure_does_not_require_body_claim() {
    let mut f = Flow::committed();
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.ack(&f.id, &f.target, false, f.now).unwrap();
    assert_eq!(f.state(), State::DeliveryFailed);
    assert!(f.has_body());
    assert!(f.c.ack(&f.id, &f.target, true, f.now).is_err());
    let mut f = Flow::committed();
    f.c.delivery_problem(&f.id, &f.target, true, f.now).unwrap();
    assert_eq!(f.state(), State::DeliveryFailed);
    assert!(f.c.claim(&f.id, &f.target, f.now).is_err());
}

#[test]
fn unexpected_loss_preserves_bounded_recovery_but_revokes_old_document_ack() {
    let mut f = Flow::committed();
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.document_lost(&f.target, f.now);
    let expiry = f.c.transaction.as_ref().unwrap().recovery_deadline;
    assert_eq!(f.state(), State::DeliveryUnknown);
    assert!(f.has_body());
    assert!(f.c.ack(&f.id, &f.target, true, f.now).is_err());
    f.c.document_lost(&f.target, f.later(30));
    assert_eq!(f.c.transaction.as_ref().unwrap().recovery_deadline, expiry);
    f.c.tick(f.now + RECOVERY);
    assert_eq!(f.state(), State::Closed);
    assert!(!f.has_body());
}

#[test]
fn active_end_always_releases_body_even_when_racing_unexpected_loss() {
    for loss_first in [false, true] {
        for reason in [
            EndReason::Navigation,
            EndReason::Hidden,
            EndReason::IdentityRevoked,
            EndReason::NewShare,
            EndReason::CloseRecovery,
        ] {
            let mut f = Flow::committed();
            f.c.claim(&f.id, &f.target, f.now).unwrap();
            if loss_first {
                f.c.document_lost(&f.target, f.now);
            }
            f.c.end(&owner(), &f.id, reason, f.now).unwrap();
            f.c.document_lost(&f.target, f.now);
            assert_eq!(f.state(), State::Closed);
            assert!(!f.has_body());
            assert!(f.c.ack(&f.id, &f.target, true, f.now).is_err());
        }
    }
}

#[test]
fn cancellation_timeout_destroys_without_enabling_and_keeps_original_recovery_expiry() {
    let mut f = Flow::committed();
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.tick(f.now + SHORT);
    let expiry = f.c.transaction.as_ref().unwrap().recovery_deadline;
    f.c.end(&owner(), &f.id, EndReason::ContinueEditing, f.later(3))
        .unwrap();
    f.c.tick(f.later(5));
    assert!(f.has_body());
    assert!(!f.c.transaction.as_ref().unwrap().input_safe);
    assert_eq!(f.c.transaction.as_ref().unwrap().recovery_deadline, expiry);
    assert!(effects(&mut f.c).contains(&Effect::DestroyTarget(f.target.clone())));
    assert!(f
        .c
        .cancellation_ack(
            &f.id,
            &f.target,
            LocalCancellation::AlreadyConsumed,
            f.later(6)
        )
        .is_err());
    f.c.tick(expiry.unwrap());
    assert!(!f.has_body());
}

#[test]
fn recovery_expiry_revokes_bridge_and_new_share_waits_for_local_qualification() {
    let mut f = Flow::committed();
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.tick(f.now + SHORT);
    let expiry = f.c.transaction.as_ref().unwrap().recovery_deadline.unwrap();
    f.c.tick(expiry);
    assert!(!f.has_body());
    assert!(effects(&mut f.c)
        .iter()
        .any(|e| matches!(e, Effect::RevokeDelivery { .. })));
    f.c.publish_source(f.target.clone(), "1", expiry).unwrap();
    assert!(f.c.menu_ticket(owner(), &f.target, expiry).is_err());
    f.c.cancellation_ack(&f.id, &f.target, LocalCancellation::Revoked, expiry)
        .unwrap();
    assert!(f.c.menu_ticket(owner(), &f.target, expiry).is_ok());
}

#[test]
fn initialization_ack_cannot_satisfy_a_different_request_or_update() {
    let mut f = Flow::preparing();
    let mut wrong = init();
    wrong.request_id = "old-request".into();
    assert!(f.c.prepared(&f.id, &f.target, &wrong, f.now).is_err());
    let mut latest = init();
    latest.update_sequence = 2;
    f.c.bind_initialization(&f.id, &f.target, latest.clone(), f.now)
        .unwrap();
    assert!(f.c.prepared(&f.id, &f.target, &init(), f.now).is_err());
    f.c.prepared(&f.id, &f.target, &latest, f.now).unwrap();
}

#[test]
fn local_cancellation_cannot_restart_after_timeout_or_reopen_a_dying_document() {
    let mut f = Flow::committed();
    f.c.claim(&f.id, &f.target, f.now).unwrap();
    f.c.tick(f.now + SHORT);
    f.c.end(&owner(), &f.id, EndReason::ContinueEditing, f.later(3))
        .unwrap();
    f.c.tick(f.later(5));
    f.c.end(&owner(), &f.id, EndReason::NewShare, f.later(6))
        .unwrap();
    assert!(f
        .c
        .cancellation_ack(&f.id, &f.target, LocalCancellation::Revoked, f.later(6))
        .is_err());
    assert!(!f.c.transaction.as_ref().unwrap().input_safe);
}

#[test]
fn active_document_end_without_crash_classification_is_privacy_releasing() {
    let mut f = Flow::committed();
    f.c.document_ended(&f.target, f.now);
    assert_eq!(f.state(), State::Closed);
    assert!(!f.has_body());
    assert!(f.c.spent.is_empty());
}

#[test]
fn metadata_is_body_free_and_recovery_reads_enforce_expiry_and_owner() {
    let mut f = Flow::choosing();
    let metadata = f.c.selection_metadata(&owner(), &f.id, f.now).unwrap();
    assert_eq!(metadata.bytes, " edited\n中文 ".len());
    let encoded = compact_envelope(&metadata).unwrap();
    assert!(!encoded.contains("edited"));
    let mut f = Flow::committed();
    f.c.tick(f.now + SHORT);
    let mut wrong = owner();
    wrong.navigation_epoch += 1;
    assert!(f.c.recovery_payload(&wrong, &f.id, f.later(3)).is_err());
    assert_eq!(
        f.c.recovery_payload(&owner(), &f.id, f.later(3))
            .unwrap()
            .unwrap()
            .as_text(),
        " edited\n中文 "
    );
    let expiry = f.c.next_deadline().unwrap();
    assert!(f
        .c
        .recovery_payload(&owner(), &f.id, expiry)
        .unwrap()
        .is_none());
}

#[test]
fn stale_exposure_effects_cannot_open_documents_after_the_next_flow_begins() {
    for accepted in [false, true] {
        let mut f = if accepted {
            Flow::committed()
        } else {
            Flow::ready()
        };
        let active = if accepted {
            f.c.claim(&f.id, &f.target, f.now).unwrap();
            f.c.ack(&f.id, &f.target, true, f.now).unwrap();
            f.c.publish_source(f.target.clone(), "1", f.now).unwrap();
            f.target.clone()
        } else {
            f.c.publish_source(f.source.clone(), "2", f.now).unwrap();
            f.source.clone()
        };
        let intent =
            f.c.take_effects()
                .into_iter()
                .find(|i| {
                    matches!(
                        i.effect,
                        Effect::RestoreSource(_) | Effect::EnableTargetInput(_)
                    )
                })
                .unwrap();
        assert!(f.c.exposure_is_current(
            &intent,
            ExposureView {
                owner: &owner(),
                active_document: &active,
                main_visible: true
            }
        ));
        f.c.menu_ticket(owner(), &active, f.now).unwrap();
        assert!(!f.c.exposure_is_current(
            &intent,
            ExposureView {
                owner: &owner(),
                active_document: &active,
                main_visible: true
            }
        ));
    }
}
