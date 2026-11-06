//! Reserved-target preparation and the logical commit transition.
use super::{
    catalog::Receiver,
    coordinator::ShareCoordinator,
    payload::{compact_envelope, revision, Payload},
    types::*,
    ShareError,
};
use std::time::Instant;

impl<I: IdSource> ShareCoordinator<I> {
    /// `current` comes from a fresh confirmed catalog lookup. `target` is a
    /// reserved identity; no native Webview needs to exist at this point.
    pub(crate) fn select(
        &mut self,
        owner: &Owner,
        transfer: &Ticket,
        selector: &Ticket,
        selected: &Receiver,
        current: Option<&Receiver>,
        target: Document,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.owned(owner, transfer)?;
        if t.state != State::Choosing || &t.selector != selector {
            return Err(ShareError::InvalidTicket);
        }
        let valid = current == Some(selected)
            && selected.plugin.plugin_id != t.source.plugin.plugin_id
            && target.plugin == selected.plugin
            && selected.command_id == "default"
            && target.webview_label != t.source.webview_label
            && target.nonce != t.source.nonce
            && target.epoch != t.source.epoch
            && !target.webview_label.is_empty()
            && !target.nonce.is_empty()
            && target.epoch != 0
            && t.body
                .as_ref()
                .is_some_and(|p| p.bytes() <= selected.max_bytes)
            && !self.spent.iter().any(|s| s.target == target);
        if !valid {
            self.finish_precommit(true, true);
            return Err(ShareError::Unavailable);
        }
        let t = self.transaction.as_mut().unwrap();
        t.state = State::Preparing;
        t.deadline = now + PREPARING;
        t.preparation = Some(Preparation {
            target,
            receiver: selected.clone(),
            initialization: None,
            prepared: false,
            confirmed: false,
            native_prepared: false,
            step: PrepareStep::Runtime,
            step_deadline: now + PREPARING,
        });
        Ok(())
    }
    pub(crate) fn begin_step(
        &mut self,
        owner: &Owner,
        transfer: &Ticket,
        step: PrepareStep,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        self.owned(owner, transfer)?;
        let t = self.preparing(transfer)?;
        let p = t.preparation.as_mut().unwrap();
        let allowed = match (p.step, step) {
            (PrepareStep::Initialization, PrepareStep::SourceConfirmation) => p.prepared,
            (PrepareStep::SourceConfirmation, PrepareStep::NativeSwitch) => p.confirmed,
            _ => false,
        };
        if !allowed {
            return Err(ShareError::InvalidState);
        }
        p.step = step;
        p.step_deadline = (now + SHORT).min(t.deadline);
        Ok(())
    }
    pub(crate) fn bind_initialization(
        &mut self,
        transfer: &Ticket,
        target: &Document,
        initialization: Initialization,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.preparing(transfer)?;

        let p = t.preparation.as_mut().unwrap();
        if &p.target != target {
            return Err(ShareError::InvalidIdentity);
        }
        if !matches!(p.step, PrepareStep::Runtime | PrepareStep::Initialization)
            || p.prepared
            || initialization.request_id.is_empty()
            || initialization.update_sequence == 0
            || p.initialization.as_ref().is_some_and(|old| {
                old.request_id != initialization.request_id
                    || old.update_sequence >= initialization.update_sequence
            })
        {
            return Err(ShareError::InvalidState);
        }
        if p.step == PrepareStep::Runtime {
            p.step = PrepareStep::Initialization;
            p.step_deadline = (now + SHORT).min(t.deadline);
        }
        p.initialization = Some(initialization);
        Ok(())
    }
    /// Call only after receiver registration and the real latest update ACK.
    pub(crate) fn prepared(
        &mut self,
        transfer: &Ticket,
        target: &Document,
        initialization: &Initialization,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.preparing(transfer)?;

        let p = t.preparation.as_mut().unwrap();
        if &p.target != target || p.initialization.as_ref() != Some(initialization) {
            return Err(ShareError::InvalidIdentity);
        }
        if p.step != PrepareStep::Initialization || p.prepared {
            return Err(ShareError::InvalidState);
        }
        p.prepared = true;
        Ok(())
    }
    pub(crate) fn confirm_source(
        &mut self,
        transfer: &Ticket,
        source: &Document,
        value: &str,
        payload: Payload,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let t = self.preparing(transfer)?;
        if &t.source != source {
            return Err(ShareError::InvalidIdentity);
        }
        let p = t.preparation.as_ref().unwrap();
        if p.step != PrepareStep::SourceConfirmation || !p.prepared || p.confirmed {
            return Err(ShareError::InvalidState);
        }
        let valid = revision(value).ok() == Some(t.revision)
            && payload.validate().is_ok()
            && payload.digest() == t.digest
            && compact_envelope(
                &serde_json::json!({"transferId":transfer,"revision":value,"payload":payload}),
            )
            .is_ok();
        if !valid {
            self.finish_precommit(true, true);
            return Err(ShareError::InvalidPayload);
        }
        self.transaction
            .as_mut()
            .unwrap()
            .preparation
            .as_mut()
            .unwrap()
            .confirmed = true;
        Ok(())
    }
    /// A receipt for native work already performed outside the gate; not proof
    /// by itself. CommitView must be read under the lifecycle/routing gate.
    pub(crate) fn native_prepared(
        &mut self,
        owner: &Owner,
        transfer: &Ticket,
        target: &Document,
        initialization: &Initialization,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        self.owned(owner, transfer)?;
        let t = self.preparing(transfer)?;

        let p = t.preparation.as_mut().unwrap();
        if &p.target != target || p.initialization.as_ref() != Some(initialization) {
            return Err(ShareError::InvalidIdentity);
        }
        if p.step != PrepareStep::NativeSwitch || !p.confirmed || p.native_prepared {
            return Err(ShareError::InvalidState);
        }
        p.native_prepared = true;
        Ok(())
    }
    /// The caller MUST apply this infallible logical routing transition before
    /// releasing the same gate: promote target + revoke source. Never perform
    /// OS calls here. Native preparation failure is a precommit failure; a later
    /// display failure never rolls this transition back.
    pub(crate) fn commit(
        &mut self,
        transfer: &Ticket,
        view: CommitView<'_>,
        now: Instant,
    ) -> Result<CommitTransition, ShareError> {
        self.tick(now);
        self.owned(view.owner, transfer)?;
        let source_current = self.source.as_ref().is_some_and(|s| {
            s.document == *view.active_source
                && s.registered
                && self
                    .transaction
                    .as_ref()
                    .is_some_and(|t| t.revision == s.revision)
        });
        let t = self.preparing(transfer)?;
        let p = t.preparation.as_ref().unwrap();
        if !source_current
            || !view.main_visible
            || &t.source != view.active_source
            || &p.target != view.pending_target
            || &p.receiver != view.receiver
            || p.initialization.as_ref() != Some(view.initialization)
            || !p.prepared
            || !p.confirmed
            || !p.native_prepared
        {
            return Err(ShareError::InvalidState);
        }
        let transition = CommitTransition {
            source: t.source.clone(),
            target: p.target.clone(),
            transfer: t.id.clone(),
            receiver_id: p.receiver.receiver_id.clone(),
        };
        t.state = State::Committed;
        t.deadline = now + SHORT;
        t.ack_eligible = true;
        t.document_alive = true;
        self.spent.push(Spent {
            transfer: transition.transfer.clone(),
            target: transition.target.clone(),
            issued: false,
        });
        Ok(transition)
    }
}
