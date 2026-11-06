use super::{coordinator::ShareCoordinator, payload::compact_envelope, types::*, ShareError};
use std::time::Instant;

impl<I: IdSource> ShareCoordinator<I> {
    /// Caller must authenticate the exact current native target Webview and
    /// current bundle under the shared gate. The bridge also owns its distinct
    /// pending/consumed/revoked intent and Ready synchronous-completion barrier.
    pub(crate) fn claim(
        &mut self,
        transfer: &Ticket,
        target: &Document,
        now: Instant,
    ) -> Result<Delivery, ShareError> {
        self.tick(now);
        let t = self.target_transaction(transfer, target)?;
        if !matches!(t.state, State::Committed | State::DeliveryUnknown)
            || !t.ack_eligible
            || !t.document_alive
            || now >= t.deadline
        {
            return Err(ShareError::Expired);
        }
        let p = t.preparation.as_ref().unwrap();
        let event = Delivery {
            version: 1,
            transfer_id: transfer.clone(),
            receiver_id: p.receiver.receiver_id.clone(),
            session_epoch: target.epoch.to_string(),
            payload: t.body.as_ref().ok_or(ShareError::InvalidState)?.clone(),
        };
        if let Err(error) = compact_envelope(&event) {
            self.delivery_problem(transfer, target, true, now)?;
            return Err(error);
        }
        let record = self
            .spent
            .iter_mut()
            .find(|s| &s.transfer == transfer && &s.target == target)
            .ok_or(ShareError::InvalidIdentity)?;
        if record.issued {
            return Err(ShareError::InvalidState);
        }
        // Mark before returning any bytes, including when transport later fails.
        record.issued = true;
        Ok(event)
    }
    pub(crate) fn ack(
        &mut self,
        transfer: &Ticket,
        target: &Document,
        accepted: bool,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let issued = self
            .spent
            .iter()
            .any(|s| &s.transfer == transfer && &s.target == target && s.issued);
        let t = self.target_transaction(transfer, target)?;
        if !issued || !t.document_alive || !t.ack_eligible {
            return Err(ShareError::InvalidIdentity);
        }
        if (t.state == State::Accepted && accepted)
            || (t.state == State::DeliveryFailed && !accepted)
        {
            return Ok(());
        }
        if !matches!(t.state, State::Committed | State::DeliveryUnknown) {
            return Err(ShareError::InvalidState);
        }
        if accepted {
            t.state = State::Accepted;
            t.body = None;
            t.recovery_deadline = None;
            t.input_safe = true;
            let effect = EffectIntent::new(t, Effect::EnableTargetInput(target.clone()));
            self.effects.push(effect);
        } else {
            t.state = State::DeliveryFailed;
            t.recovery_deadline.get_or_insert(now + RECOVERY);
        }
        Ok(())
    }
    /// `definite` means a Ready/receiver contract failure; transport uncertainty
    /// must pass false, including eval whose execution may already have happened.
    pub(crate) fn delivery_problem(
        &mut self,
        transfer: &Ticket,
        target: &Document,
        definite: bool,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.target_transaction(transfer, target)?;
        if !matches!(t.state, State::Committed | State::DeliveryUnknown)
            || !t.document_alive
            || !t.ack_eligible
        {
            return Err(ShareError::InvalidState);
        }
        t.state = if definite {
            State::DeliveryFailed
        } else {
            State::DeliveryUnknown
        };
        t.recovery_deadline.get_or_insert(now + RECOVERY);
        Ok(())
    }
    /// Called for CONFIRMED unexpected document loss. Active user/lifecycle
    /// revocation instead uses end(), which releases recovery immediately.
    pub(crate) fn document_lost(&mut self, target: &Document, now: Instant) {
        self.tick(now);
        let Some(t) = self
            .transaction
            .as_mut()
            .filter(|t| t.preparation.as_ref().is_some_and(|p| &p.target == target))
        else {
            return;
        };
        if t.state == State::Preparing {
            self.finish_precommit(true, true);
            return;
        }
        t.document_alive = false;
        t.ack_eligible = false;
        t.cancellation = None;
        if matches!(
            t.state,
            State::Committed | State::DeliveryUnknown | State::DeliveryFailed
        ) {
            t.state = State::DeliveryUnknown;
            t.recovery_deadline.get_or_insert(now + RECOVERY);
        }
        // Closed/Accepted never regain recovery or ACK qualification.
    }
    pub(super) fn target_transaction(
        &mut self,
        transfer: &Ticket,
        target: &Document,
    ) -> Result<&mut Transaction, ShareError> {
        let t = self.transaction.as_mut().ok_or(ShareError::InvalidState)?;
        if &t.id != transfer || !t.preparation.as_ref().is_some_and(|p| &p.target == target) {
            return Err(ShareError::InvalidIdentity);
        }
        Ok(t)
    }
}
