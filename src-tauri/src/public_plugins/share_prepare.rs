//! Host-only, body-free Runtime preparation ownership. No mounted content is required.
#![allow(dead_code)] // Task 4 owns these handles while preparing its pending Panel.
use super::*;
use crate::plugin_share::catalog::Receiver;

#[derive(Clone, Debug)]
pub(crate) struct SharePrepareLease(Arc<Mutex<bool>>);
impl Default for SharePrepareLease {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(true)))
    }
}
impl PartialEq for SharePrepareLease {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl SharePrepareLease {
    pub(crate) fn live(&self) -> bool {
        self.0.lock().is_ok_and(|live| *live)
    }
    pub(crate) fn cancel(&self) {
        if let Ok(mut live) = self.0.lock() {
            *live = false;
        }
    }
    fn with_live<T>(&self, action: impl FnOnce(bool) -> T) -> Option<T> {
        self.0.lock().ok().map(|live| action(*live))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SharePrepareOwner {
    pub(crate) receiver: Receiver,
    pub(crate) prepare_id: String,
    pub(crate) lease: SharePrepareLease,
}
impl SharePrepareOwner {
    pub(crate) fn valid_for(&self, candidate: &PluginRequestCandidate) -> bool {
        let plugin = &self.receiver.plugin;
        self.lease.live()
            && !self.prepare_id.is_empty()
            && self.prepare_id.len() <= 256
            && !self.prepare_id.contains('\0')
            && !self.receiver.receiver_id.is_empty()
            && plugin.inventory_revision != 0
            && plugin.plugin_id == candidate.plugin_id
            && plugin.generation == candidate.plugin_generation
            && plugin.activation_id == candidate.activation_id
            && plugin.admission_epoch == candidate.admission_epoch
            && self.receiver.command_id == candidate.command_id
            && candidate.activation_mode == PublicActivationMode::Submit
            && candidate.input.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SharePrepareError {
    Busy,
    Unavailable,
    Expired,
    Cancelled,
    Failed,
    InvalidResponse,
    TimedOut,
}

pub(super) struct PendingSharePrepare {
    owner: SharePrepareOwner,
    sender: Mutex<Option<mpsc::Sender<Result<PublicPanelResponse, SharePrepareError>>>>,
}
pub(crate) struct SharePreparation {
    pub(crate) dispatch: ScheduledPluginRequest,
    pub(crate) registration: SharePrepareRegistration,
    pub(crate) receiver: mpsc::Receiver<Result<PublicPanelResponse, SharePrepareError>>,
}
pub(crate) struct SharePrepareRegistration {
    context: PluginRequestContext,
    owner: SharePrepareOwner,
    service: std::sync::Weak<PublicPluginService>,
}
impl SharePrepareRegistration {
    pub(crate) fn context(&self) -> &PluginRequestContext {
        &self.context
    }
    pub(crate) fn owner(&self) -> &SharePrepareOwner {
        &self.owner
    }
    /// Pure cancellation qualification; safe under Task 4's gate. The caller
    /// separately checks current bundle authority in that gate's registry guard.
    pub(crate) fn result_is_current(&self) -> bool {
        self.owner.lease.live()
    }
    pub(crate) fn cancel(&self) {
        self.owner.lease.cancel();
        if let Some(service) = self.service.upgrade() {
            service.retire_share_prepare(&self.context);
        }
    }
}
impl Drop for SharePrepareRegistration {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl PublicPluginService {
    pub(crate) fn begin_share_prepare(
        self: &Arc<Self>,
        receiver: Receiver,
        prepare_id: String,
        now: Instant,
    ) -> Result<SharePreparation, SharePrepareError> {
        self.begin_share_prepare_with(receiver, prepare_id, now, || {})
    }
    pub(super) fn begin_share_prepare_with(
        self: &Arc<Self>,
        receiver: Receiver,
        prepare_id: String,
        now: Instant,
        after_admission: impl FnOnce(),
    ) -> Result<SharePreparation, SharePrepareError> {
        let owner = SharePrepareOwner {
            receiver,
            prepare_id,
            lease: SharePrepareLease::default(),
        };
        let manager = self.manager().map_err(|_| SharePrepareError::Unavailable)?;
        let dispatch = manager.admit_share_prepare(&owner, now)?;
        after_admission();
        let (sender, receiver) = mpsc::channel();
        let registration = SharePrepareRegistration {
            context: dispatch.context.clone(),
            owner: owner.clone(),
            service: Arc::downgrade(self),
        };
        self.share_preparations
            .lock()
            .map_err(|_| SharePrepareError::Unavailable)?
            .insert(
                dispatch.context.clone(),
                Arc::new(PendingSharePrepare {
                    owner: owner.clone(),
                    sender: Mutex::new(Some(sender)),
                }),
            );
        // A lifecycle mutation can win after manager admission but before map
        // insertion. Recheck under M before returning any dispatch to the caller.
        if manager.with_share_prepare_target(&owner, || {
            manager.scheduler().context_status(&dispatch.context) == PluginContextStatus::Current
        }) != Ok(true)
        {
            registration.cancel();
            return Err(SharePrepareError::Expired);
        }
        Ok(SharePreparation {
            dispatch,
            registration,
            receiver,
        })
    }
    pub(super) fn emit_share_prepare_with(
        &self,
        request: &ScheduledPluginRequest,
        emit: impl FnOnce() -> Result<(), PublicPluginManagementError>,
    ) -> Result<(), PublicPluginManagementError> {
        self.manager()?
            .scheduler()
            .claim_share_prepare_emission(&request.context)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        emit()
    }
    pub(crate) fn retire_share_prepare(&self, context: &PluginRequestContext) {
        if let Ok(manager) = self.manager() {
            manager.retire_share_prepare(context);
        }
        self.settle_share_prepare(context, SharePrepareError::Cancelled);
    }
    pub(super) fn settle_share_prepare(
        &self,
        context: &PluginRequestContext,
        error: SharePrepareError,
    ) {
        let pending = self
            .share_preparations
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(context));
        if let Some(pending) = pending {
            pending.owner.lease.cancel();
            if let Some(sender) = pending
                .sender
                .lock()
                .ok()
                .and_then(|mut sender| sender.take())
            {
                let _ = sender.send(Err(error));
            }
        }
    }
    pub(super) fn retire_share_preparations(
        &self,
        plugin_id: Option<&str>,
        generation: Option<u64>,
    ) {
        let contexts = self
            .share_preparations
            .lock()
            .ok()
            .map(|map| {
                map.keys()
                    .filter(|c| {
                        plugin_id.is_none_or(|id| c.plugin_id == id)
                            && generation.is_none_or(|g| c.plugin_generation == g)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for context in contexts {
            self.retire_share_prepare(&context);
        }
    }
    /// Ownership comes from physical scheduler completion, even after the
    /// registration/sender has been cancelled or removed. This never publishes
    /// launcher/content results. Returns fault accounting only for a live claim.
    pub(super) fn complete_share_prepare_with(
        &self,
        completion: &PluginCommandCompletion,
        outcome: &PluginCompletionOutcome,
        before_claim: impl FnOnce(),
    ) -> Option<bool> {
        let PluginRequestOwner::SharePrepare(owner) = &outcome.completed_owner else {
            return None;
        };
        // Keep the body-free owner in the map after taking its sender. Lifecycle
        // scans can still revoke the shared lease until the final send claim.
        let pending = self
            .share_preparations
            .lock()
            .ok()
            .and_then(|pending| pending.get(&completion.context).cloned());
        let Some(pending) = pending else {
            if let Ok(manager) = self.manager() {
                manager
                    .scheduler()
                    .discard_action_completion(&completion.context);
            }
            return None;
        };
        let sender = pending
            .sender
            .lock()
            .ok()
            .and_then(|mut sender| sender.take());
        before_claim();
        let Some(sender) = sender else {
            return None;
        };
        let result = if !outcome.accepted {
            Err(SharePrepareError::Cancelled)
        } else if completion.failed {
            Err(SharePrepareError::Failed)
        } else {
            completion
                .response
                .clone()
                .ok_or(SharePrepareError::InvalidResponse)
                .and_then(|value| {
                    parse_panel_response(&completion.context, value)
                        .map_err(|_| SharePrepareError::InvalidResponse)
                })
        };
        let manager = self.manager().ok();
        let mut accounting = None;
        let claimed = manager
            .as_ref()
            .and_then(|manager| {
                manager
                    .with_share_prepare_target(owner, || {
                        // Scheduler -> lease is the admission order. Never acquire the
                        // scheduler while holding the send/cancellation lease.
                        let completion_current = manager
                            .scheduler()
                            .claim_action_completion(&completion.context)
                            .is_ok();
                        owner.lease.with_live(|live| {
                            let current = live
                                && pending.owner == *owner
                                && outcome.accepted
                                && completion_current;
                            if current {
                                accounting = Some(result.is_ok());
                                let _ = sender.send(result);
                            } else {
                                let _ = sender.send(Err(SharePrepareError::Cancelled));
                            }
                        })
                    })
                    .ok()
            })
            .flatten();
        if claimed.is_none() {
            let _ = sender.send(Err(if owner.lease.live() {
                SharePrepareError::Expired
            } else {
                SharePrepareError::Cancelled
            }));
        }
        if let Ok(mut pending) = self.share_preparations.lock() {
            pending.remove(&completion.context);
        }
        if let Some(manager) = manager {
            manager
                .scheduler()
                .discard_action_completion(&completion.context);
        }
        accounting
    }
}
