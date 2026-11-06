//! All methods require the caller's shared Panel/launcher commit gate. This type
//! intentionally has no internal mutex: a second independent lock would let
//! lifecycle invalidation race routing/authority promotion. Never invoke native
//! or plugin code while that gate is held; drain `take_effects` afterward.
use super::{
    payload::{compact_envelope, revision, Payload},
    types::*,
    ShareError,
};
use std::time::Instant;

pub(crate) struct ShareCoordinator<I = SecureIds> {
    pub(super) ids: I,
    pub(super) source: Option<Source>,
    pub(super) menu: Option<Menu>,
    pub(super) request: Option<Request>,
    pub(super) transaction: Option<Transaction>,
    pub(super) spent: Vec<Spent>,
    pub(super) effects: Vec<EffectIntent>,
}
impl<I: IdSource> ShareCoordinator<I> {
    pub(crate) fn new(ids: I) -> Self {
        Self {
            ids,
            source: None,
            menu: None,
            request: None,
            transaction: None,
            spent: vec![],
            effects: vec![],
        }
    }

    /// Admission must first authenticate the actual active source Webview and
    /// verify its current manifest source:true and complete plugin identity.
    pub(crate) fn publish_source(
        &mut self,
        document: Document,
        value: &str,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let revision = revision(value)?;
        match &self.source {
            Some(source) if source.document == document => {
                if revision <= source.revision {
                    return Err(ShareError::InvalidRevision);
                }
            }
            _ if revision != 1 => return Err(ShareError::InvalidRevision),
            _ => {}
        }
        let same_document = self.source.as_ref().is_some_and(|s| s.document == document);
        self.invalidate_pending_source(same_document);
        self.source = Some(Source {
            document,
            revision,
            registered: true,
        });
        Ok(())
    }
    pub(crate) fn unregister_source(
        &mut self,
        source: &Document,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        if !self.source.as_ref().is_some_and(|s| &s.document == source) {
            return Err(ShareError::InvalidIdentity);
        }
        self.invalidate_pending_source(true);
        // Keep the revision watermark, but revoke availability. Re-registering
        // this document must advance it; resetting to 1 would resurrect tickets.
        self.source.as_mut().unwrap().registered = false;
        Ok(())
    }
    fn invalidate_pending_source(&mut self, source_valid: bool) {
        self.menu = None;
        self.request = None;
        if self
            .transaction
            .as_ref()
            .is_some_and(|t| matches!(t.state, State::Choosing | State::Preparing))
        {
            self.finish_precommit(false, source_valid);
        }
    }

    /// Host-only real menu interaction, never a plugin-callable enumeration API.
    pub(crate) fn menu_ticket(
        &mut self,
        owner: Owner,
        source: &Document,
        now: Instant,
    ) -> Result<Ticket, ShareError> {
        self.tick(now);
        if !self
            .source
            .as_ref()
            .is_some_and(|s| &s.document == source && s.registered)
        {
            return Err(ShareError::Unavailable);
        }
        if let Some(t) = &self.transaction {
            match t.state {
                State::Choosing | State::Preparing => self.finish_precommit(false, true),
                State::Accepted => {}
                State::Cancelled | State::PrepareFailed => {}
                State::Closed if t.input_safe || !t.document_alive => {}
                _ => return Err(ShareError::InvalidState),
            }
        }
        self.transaction = None;
        self.request = None;
        let token = self.ids.next()?;
        self.menu = Some(Menu {
            token: token.clone(),
            owner,
            source: source.clone(),
        });
        Ok(token)
    }
    pub(crate) fn request_snapshot(
        &mut self,
        owner: &Owner,
        menu: &Ticket,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        let entry = self.menu.as_ref().ok_or(ShareError::InvalidTicket)?;
        if &entry.token != menu || &entry.owner != owner {
            return Err(ShareError::InvalidTicket);
        }
        self.request = Some(Request {
            menu: self.menu.take().unwrap(),
            deadline: now + SHORT,
        });
        Ok(())
    }
    pub(crate) fn snapshot(
        &mut self,
        source: &Document,
        menu: &Ticket,
        value: &str,
        payload: Payload,
        now: Instant,
    ) -> Result<(Ticket, Ticket), ShareError> {
        self.tick(now);
        let request = self.request.as_ref().ok_or(ShareError::InvalidTicket)?;
        if &request.menu.token != menu || &request.menu.source != source {
            return Err(ShareError::InvalidIdentity);
        }
        // Consume even malformed/failed responses from the actual source. There
        // is no retry or late-response path for this menu interaction.
        let request = self.request.take().unwrap();
        let revision = revision(value)?;
        if !self
            .source
            .as_ref()
            .is_some_and(|s| &s.document == source && s.revision == revision && s.registered)
        {
            return Err(ShareError::InvalidRevision);
        }
        payload.validate()?;
        compact_envelope(
            &serde_json::json!({"menuTicket":menu,"revision":value,"payload":payload}),
        )?;
        let id = self.ids.next()?;
        let selector = self.ids.next()?;
        self.transaction = Some(Transaction {
            id: id.clone(),
            selector: selector.clone(),
            owner: request.menu.owner,
            source: source.clone(),
            revision,
            digest: payload.digest(),
            body: Some(payload),
            state: State::Choosing,
            deadline: now + CHOOSING,
            preparation: None,
            ack_eligible: false,
            document_alive: false,
            destruction_requested: false,
            recovery_deadline: None,
            cancellation: None,
            input_safe: false,
        });
        Ok((id, selector))
    }
    pub(crate) fn cancel_before_commit(
        &mut self,
        owner: &Owner,
        transfer: Option<&Ticket>,
        source_valid: bool,
        failed: bool,
        now: Instant,
    ) -> Result<(), ShareError> {
        self.tick(now);
        if let Some(transfer) = transfer {
            let t = self.owned(owner, transfer)?;
            if !matches!(t.state, State::Choosing | State::Preparing) {
                return Err(ShareError::InvalidState);
            }
            self.finish_precommit(failed, source_valid);
        } else {
            let bound = self
                .request
                .as_ref()
                .map(|r| &r.menu.owner)
                .or_else(|| self.menu.as_ref().map(|m| &m.owner));
            if bound != Some(owner) {
                return Err(ShareError::InvalidIdentity);
            }
            self.menu = None;
            self.request = None;
        }
        Ok(())
    }
    pub(super) fn finish_precommit(&mut self, failed: bool, source_valid: bool) {
        let Some(t) = &mut self.transaction else {
            return;
        };
        if !matches!(t.state, State::Choosing | State::Preparing) {
            return;
        }
        t.state = if failed {
            State::PrepareFailed
        } else {
            State::Cancelled
        };
        t.body = None;
        if let Some(p) = &t.preparation {
            let effect = EffectIntent::new(
                t,
                Effect::RetirePreparation {
                    transfer: t.id.clone(),
                    target: p.target.clone(),
                },
            );
            self.effects.push(effect);
            let effect = EffectIntent::new(t, Effect::DestroyPending(p.target.clone()));
            self.effects.push(effect);
        }
        if source_valid {
            let effect = EffectIntent::new(t, Effect::RestoreSource(t.source.clone()));
            self.effects.push(effect);
        }
    }
    pub(super) fn owned(
        &self,
        owner: &Owner,
        transfer: &Ticket,
    ) -> Result<&Transaction, ShareError> {
        let t = self.transaction.as_ref().ok_or(ShareError::InvalidState)?;
        if &t.id != transfer || &t.owner != owner {
            return Err(ShareError::InvalidIdentity);
        }
        Ok(t)
    }
    pub(super) fn preparing(&mut self, transfer: &Ticket) -> Result<&mut Transaction, ShareError> {
        let t = self.transaction.as_mut().ok_or(ShareError::InvalidState)?;
        if &t.id != transfer {
            return Err(ShareError::InvalidIdentity);
        }
        if t.state != State::Preparing {
            return Err(ShareError::InvalidState);
        }
        Ok(t)
    }
    /// This is a qualification predicate, not an OS show call. Evaluate under
    /// the shared gate inside the serialized main-thread native action; protect
    /// the guard-to-apply interval with the controller's native operation order.
    pub(crate) fn exposure_is_current(
        &self,
        intent: &EffectIntent,
        view: ExposureView<'_>,
    ) -> bool {
        if !view.main_visible || &intent.owner != view.owner {
            return false;
        }
        let Some(t) = self
            .transaction
            .as_ref()
            .filter(|t| t.id == intent.transfer && t.owner == intent.owner)
        else {
            return false;
        };
        match &intent.effect {
            Effect::RestoreSource(document) => {
                matches!(t.state, State::Cancelled | State::PrepareFailed)
                    && document == &t.source
                    && document == view.active_document
                    && self
                        .source
                        .as_ref()
                        .is_some_and(|s| &s.document == document)
            }
            Effect::EnableTargetInput(document) => {
                matches!(t.state, State::Accepted | State::Closed)
                    && t.input_safe
                    && t.document_alive
                    && !t.destruction_requested
                    && t.cancellation.is_none()
                    && document == view.active_document
                    && t.preparation
                        .as_ref()
                        .is_some_and(|p| &p.target == document)
            }
            _ => false,
        }
    }
    pub(crate) fn take_effects(&mut self) -> Vec<EffectIntent> {
        std::mem::take(&mut self.effects)
    }
    pub(crate) fn state(&self) -> Option<State> {
        self.transaction.as_ref().map(|t| t.state)
    }
    pub(crate) fn selection_metadata(
        &mut self,
        owner: &Owner,
        transfer: &Ticket,
        now: Instant,
    ) -> Result<SnapshotMetadata, ShareError> {
        self.tick(now);
        let t = self.owned(owner, transfer)?;
        if t.state != State::Choosing {
            return Err(ShareError::InvalidState);
        }
        let payload = t.body.as_ref().ok_or(ShareError::InvalidState)?;
        Ok(SnapshotMetadata {
            content_type: "text/plain",
            bytes: payload.bytes(),
            source_plugin_id: t.source.plugin.plugin_id.clone(),
        })
    }
    /// Host recovery UI/copy only. A read also expires stale recovery, so delayed
    /// host timers cannot accidentally expose a body beyond its deadline.
    pub(crate) fn recovery_payload(
        &mut self,
        owner: &Owner,
        transfer: &Ticket,
        now: Instant,
    ) -> Result<Option<&Payload>, ShareError> {
        self.tick(now);
        self.owned(owner, transfer)?;
        Ok(self
            .transaction
            .as_ref()
            .filter(|t| t.recovery_deadline.is_some())
            .and_then(|t| t.body.as_ref()))
    }
}

impl Default for ShareCoordinator<SecureIds> {
    fn default() -> Self {
        Self::new(SecureIds)
    }
}
