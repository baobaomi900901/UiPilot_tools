use super::*;
use crate::public_plugins::share_prepare::{SharePrepareLease, SharePrepareOwner};

fn candidate() -> PluginRequestCandidate {
    let mut candidate = super::tests::candidate("", 1, PublicActivationMode::Submit);
    candidate.owner = PluginRequestOwner::SharePrepare(SharePrepareOwner {
        receiver: crate::plugin_share::catalog::Receiver {
            plugin: crate::plugin_share::types::PluginIdentity {
                plugin_id: candidate.plugin_id.clone(),
                generation: 1,
                activation_id: candidate.activation_id,
                admission_epoch: candidate.admission_epoch,
                inventory_revision: 1,
            },
            plugin_name: "Receiver".into(),
            receiver_id: "receive".into(),
            title: "Receive".into(),
            command_id: "default".into(),
            max_bytes: 8192,
        },
        prepare_id: "opaque-transfer".into(),
        lease: SharePrepareLease::default(),
    });
    candidate
}

#[test]
fn share_prepare_cancel_revokes_reads_but_keeps_slot_deadline_and_completion_kind() {
    let scheduler = PluginRequestScheduler::default();
    let now = Instant::now();
    let PluginScheduleOutcome::Dispatched(first) = scheduler.enqueue(candidate(), now).unwrap()
    else {
        panic!()
    };
    assert!(!scheduler
        .with_current(&first.context, |c| c.effects_allowed())
        .unwrap());
    assert_eq!(
        scheduler.enqueue(candidate(), now),
        Err(PluginScheduleError::Busy)
    );
    assert!(scheduler.cancel(&first.context, now).unwrap().is_none());
    assert_eq!(
        scheduler.context_status(&first.context),
        PluginContextStatus::Expired
    );
    assert_eq!(
        scheduler.enqueue(candidate(), now),
        Err(PluginScheduleError::Busy)
    );
    assert!(scheduler
        .expire_timeouts(first.deadline - Duration::from_nanos(1))
        .unwrap()
        .is_empty());
    let outcome = scheduler.complete(&first.context, now).unwrap();
    assert!(!outcome.accepted);
    assert!(matches!(
        outcome.completed_owner,
        PluginRequestOwner::SharePrepare(_)
    ));
    assert!(outcome.next.is_none());
    assert!(matches!(
        scheduler.enqueue(candidate(), now),
        Ok(PluginScheduleOutcome::Dispatched(_))
    ));
}

#[test]
fn share_prepare_busy_never_replaces_command_waiter_or_rebuilds() {
    let scheduler = PluginRequestScheduler::default();
    let now = Instant::now();
    let PluginScheduleOutcome::Dispatched(first) = scheduler
        .enqueue(
            super::tests::candidate("first", 1, PublicActivationMode::Submit),
            now,
        )
        .unwrap()
    else {
        panic!()
    };
    scheduler
        .enqueue(
            super::tests::candidate("waiter", 1, PublicActivationMode::Submit),
            now,
        )
        .unwrap();
    assert_eq!(
        scheduler.enqueue(candidate(), now),
        Err(PluginScheduleError::Busy)
    );
    let outcome = scheduler.complete(&first.context, now).unwrap();
    let waiter = outcome.next.unwrap();
    assert_eq!(waiter.candidate.input, "waiter");
    scheduler.expire_timeouts(waiter.deadline).unwrap();
    let mut next = candidate();
    next.plugin_generation = 2;
    if let PluginRequestOwner::SharePrepare(owner) = &mut next.owner {
        owner.receiver.plugin.generation = 2;
    }
    assert_eq!(scheduler.enqueue(next, now), Err(PluginScheduleError::Busy));
}

#[test]
fn share_prepare_retired_timeout_does_not_count_fault_or_reset_deadline() {
    let scheduler = PluginRequestScheduler::default();
    let now = Instant::now();
    let PluginScheduleOutcome::Dispatched(first) = scheduler.enqueue(candidate(), now).unwrap()
    else {
        panic!()
    };
    scheduler
        .cancel(&first.context, now + Duration::from_secs(29))
        .unwrap();
    let replacements = scheduler
        .expire_timeouts(now + Duration::from_secs(30))
        .unwrap();
    assert_eq!(replacements.len(), 1);
    assert!(!replacements[0].counts_as_fault);
    assert!(!replacements[0].has_waiting);
}

#[test]
fn share_prepare_candidate_rejects_input_mode_identity_and_dead_lease() {
    let scheduler = PluginRequestScheduler::default();
    for mutation in 0..4 {
        let mut next = candidate();
        match mutation {
            0 => next.input = "body".into(),
            1 => next.activation_mode = PublicActivationMode::Live,
            2 => next.activation_id += 1,
            _ => {
                if let PluginRequestOwner::SharePrepare(owner) = &next.owner {
                    owner.lease.cancel();
                }
            }
        }
        assert_eq!(
            scheduler.reserve(next),
            Err(PluginScheduleError::InvalidCandidate)
        );
    }
}

#[test]
fn share_prepare_waits_for_host_completion_settlement_without_requeue() {
    let scheduler = PluginRequestScheduler::default();
    let now = Instant::now();
    let PluginScheduleOutcome::Dispatched(first) = scheduler.enqueue(candidate(), now).unwrap()
    else {
        panic!()
    };
    assert!(scheduler.complete(&first.context, now).unwrap().accepted);
    assert_eq!(
        scheduler.enqueue(candidate(), now),
        Err(PluginScheduleError::Busy)
    );
    scheduler.discard_action_completion(&first.context);
    assert!(matches!(
        scheduler.enqueue(candidate(), now),
        Ok(PluginScheduleOutcome::Dispatched(_))
    ));
}
