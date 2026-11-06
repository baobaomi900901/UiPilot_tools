use super::{coordinator::ShareCoordinator, types::*, ShareError};
use std::time::Instant;

impl<I: IdSource> ShareCoordinator<I> {
    pub(crate) fn end(
        &mut self,
        owner: &Owner,
        transfer: &Ticket,
        reason: EndReason,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.owned(owner, transfer)?;
        if matches!(t.state, State::Choosing | State::Preparing) {
            let source_valid = !matches!(
                reason,
                EndReason::Navigation | EndReason::Hidden | EndReason::IdentityRevoked
            );
            self.finish_precommit(false, source_valid);
            return Ok(());
        }
        if reason == EndReason::ContinueEditing
            && !matches!(t.state, State::DeliveryFailed | State::DeliveryUnknown)
        {
            return Err(ShareError::InvalidState);
        }
        self.close_delivery(reason, now);
        Ok(())
    }
    pub(super) fn close_delivery(&mut self, reason: EndReason, now: Instant) {
        let Some(t) = &mut self.transaction else {
            return;
        };
        let retain = reason == EndReason::ContinueEditing;
        let destroy = matches!(
            reason,
            EndReason::Navigation | EndReason::Hidden | EndReason::IdentityRevoked
        );
        let already_safe = t.input_safe;
        t.state = State::Closed;
        t.ack_eligible = false;
        if !retain {
            t.body = None;
            t.recovery_deadline = None;
        }
        let Some(p) = &t.preparation else {
            return;
        };
        if !t.document_alive || t.destruction_requested || (already_safe && !destroy) {
            return;
        }
        if destroy {
            t.destruction_requested = true;
            t.cancellation = None;
            let effect = EffectIntent::new(t, Effect::DestroyTarget(p.target.clone()));
            self.effects.push(effect);
        } else if t.cancellation.is_none() {
            t.cancellation = Some(Cancellation {
                deadline: now + SHORT,
                retain_on_loss: retain,
            });
            let effect = EffectIntent::new(
                t,
                Effect::RevokeDelivery {
                    transfer: t.id.clone(),
                    target: p.target.clone(),
                },
            );
            self.effects.push(effect);
        } else if !retain {
            // A privacy-ending action supersedes ContinueEditing without moving
            // its original local-cancellation deadline.
            t.cancellation.as_mut().unwrap().retain_on_loss = false;
        }
    }
    /// Native IPC must bind this to the actual target document; a boolean from
    /// the launcher or an arbitrary plugin is never sufficient qualification.
    pub(crate) fn cancellation_ack(
        &mut self,
        transfer: &Ticket,
        target: &Document,
        _outcome: LocalCancellation,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.target_transaction(transfer, target)?;
        if t.state != State::Closed
            || !t.document_alive
            || t.destruction_requested
            || t.cancellation.is_none()
        {
            return Err(ShareError::InvalidState);
        }
        t.cancellation = None;
        t.body = None;
        t.recovery_deadline = None;
        t.input_safe = true;
        let effect = EffectIntent::new(t, Effect::EnableTargetInput(target.clone()));
        self.effects.push(effect);
        Ok(())
    }
    /// Confirmation of native destruction/session termination, not a request to
    /// destroy. Unexpected loss must first use document_lost to retain recovery.
    pub(crate) fn document_ended(&mut self, target: &Document, now: Instant) {
        self.tick(now);
        self.spent.retain(|s| &s.target != target);
        if let Some(t) = self
            .transaction
            .as_mut()
            .filter(|t| t.preparation.as_ref().is_some_and(|p| &p.target == target))
        {
            let preserve_recovery =
                !t.document_alive || (t.state == State::Closed && t.destruction_requested);
            t.document_alive = false;
            t.ack_eligible = false;
            t.cancellation = None;
            if !preserve_recovery || t.state == State::Accepted {
                t.state = State::Closed;
                t.body = None;
                t.recovery_deadline = None;
            }
        }
    }
    /// Every mutation calls tick first; the host must additionally schedule the
    /// returned next_deadline even when no IPC arrives. Times are monotonic.
    pub(crate) fn tick(&mut self, now: Instant) {
        if self.request.as_ref().is_some_and(|r| now >= r.deadline) {
            self.request = None;
        }
        let Some(t) = &self.transaction else {
            return;
        };
        match t.state {
            State::Choosing if now >= t.deadline => {
                self.finish_precommit(false, true);
                return;
            }
            State::Preparing
                if now >= t.deadline
                    || t.preparation
                        .as_ref()
                        .is_some_and(|p| now >= p.step_deadline) =>
            {
                self.finish_precommit(true, true);
                return;
            }
            _ => {}
        }
        let t = self.transaction.as_mut().unwrap();
        if t.state == State::Committed && now >= t.deadline {
            t.state = State::DeliveryUnknown;
            // Timer polling lateness must not extend the recovery lifetime.
            t.recovery_deadline.get_or_insert(t.deadline + RECOVERY);
        }
        if t.recovery_deadline.is_some_and(|deadline| now >= deadline) {
            self.close_delivery(EndReason::RecoveryExpired, now);
        }
        let t = self.transaction.as_mut().unwrap();
        if t.cancellation.as_ref().is_some_and(|c| now >= c.deadline) {
            let cancellation = t.cancellation.take().unwrap();
            t.destruction_requested = true;
            if !cancellation.retain_on_loss {
                t.body = None;
                t.recovery_deadline = None;
            }
            // Do not mark input_safe or pretend native destruction succeeded.
            // document_ended() is the required confirmation before reuse.
            if let Some(p) = &t.preparation {
                let effect = EffectIntent::new(t, Effect::DestroyTarget(p.target.clone()));
                self.effects.push(effect);
            }
        }
    }
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let mut deadlines = Vec::new();
        if let Some(r) = &self.request {
            deadlines.push(r.deadline);
        }
        if let Some(t) = &self.transaction {
            if matches!(
                t.state,
                State::Choosing | State::Preparing | State::Committed
            ) {
                deadlines.push(t.deadline);
            }
            if t.state == State::Preparing {
                if let Some(p) = &t.preparation {
                    deadlines.push(p.step_deadline);
                }
            }
            if let Some(deadline) = t.recovery_deadline {
                deadlines.push(deadline);
            }
            if let Some(c) = &t.cancellation {
                deadlines.push(c.deadline);
            }
        }
        deadlines.into_iter().min()
    }
}
