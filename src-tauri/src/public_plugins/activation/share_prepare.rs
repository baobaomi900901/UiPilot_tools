//! Manager-first preparation admission and final identity validation.
use super::super::share_prepare::{SharePrepareError as E, SharePrepareOwner};
use super::*;

impl PublicPluginManager {
    fn validate_share_prepare_target(&self, owner: &SharePrepareOwner) -> Result<(), E> {
        let receiver = &owner.receiver;
        let plugin = &receiver.plugin;
        let bundle = self
            .bundles
            .confirmed_bundle(&plugin.plugin_id)
            .map_err(|_| E::Unavailable)?
            .ok_or(E::Expired)?;
        let config = self
            .state
            .config(&plugin.plugin_id)
            .map_err(|_| E::Unavailable)?
            .ok_or(E::Expired)?;
        let manifest = &bundle.runtime.manifest;
        if self.host.platform != super::super::manifest::PublicPlatform::Windows
            || bundle.config != config
            || !share_catalog::eligible(manifest, &config)
            || share_catalog::identity(&bundle) != *plugin
            || bundle.runtime.generation != plugin.generation
        {
            return Err(E::Expired);
        }
        if bundle.runtime_recovery_needed {
            return Err(E::Busy);
        }
        let declaration = manifest
            .share
            .as_ref()
            .and_then(|s| s.receivers.as_ref())
            .and_then(|receivers| receivers.iter().find(|r| r.id == receiver.receiver_id))
            .ok_or(E::Expired)?;
        let command = manifest
            .command_by_id(&declaration.command_id)
            .ok_or(E::Expired)?;
        if receiver.command_id != declaration.command_id
            || receiver.title != declaration.title
            || receiver.plugin_name != manifest.name
            || receiver.max_bytes != declaration.max_bytes as usize
            || command.activation_mode != PublicActivationMode::Submit
            || command.output_mode != PublicOutputMode::Panel
            || config.command(&receiver.command_id).is_none()
        {
            return Err(E::Expired);
        }
        Ok(())
    }
    pub(crate) fn admit_share_prepare(
        &self,
        owner: &SharePrepareOwner,
        now: Instant,
    ) -> Result<ScheduledPluginRequest, E> {
        let _mutation = self.lock_mutation().map_err(|_| E::Unavailable)?;
        self.validate_share_prepare_target(owner)?;
        let plugin = &owner.receiver.plugin;
        let reserved = self
            .scheduler
            .reserve(super::super::PluginRequestCandidate {
                command_id: owner.receiver.command_id.clone(),
                plugin_id: plugin.plugin_id.clone(),
                plugin_generation: plugin.generation,
                activation_id: plugin.activation_id,
                admission_epoch: plugin.admission_epoch,
                activation_mode: PublicActivationMode::Submit,
                input: String::new(),
                owner: super::super::PluginRequestOwner::SharePrepare(owner.clone()),
            })
            .map_err(|_| E::Unavailable)?;
        match self.enqueue_reserved(reserved, now) {
            Ok(PluginScheduleOutcome::Dispatched(request)) => Ok(request),
            Err(PluginScheduleError::Busy) => Err(E::Busy),
            _ => Err(E::Unavailable),
        }
    }
    /// Never call this while holding the future Panel/coordinator gate. It takes
    /// the manager mutation guard and invokes only an in-memory send/claim.
    pub(crate) fn with_share_prepare_target<T>(
        &self,
        owner: &SharePrepareOwner,
        action: impl FnOnce() -> T,
    ) -> Result<T, E> {
        let _mutation = self.lock_mutation().map_err(|_| E::Unavailable)?;
        self.validate_share_prepare_target(owner)?;
        Ok(action())
    }
    pub(crate) fn retire_share_prepare(&self, context: &PluginRequestContext) {
        if let Ok(transition) = self.network_authority.begin_transition() {
            let _ = self.scheduler.retire_context(context);
            let _ = self.scheduler.abort_unemitted_share_prepare(context);
            transition.invalidate_context(context);
        } else {
            let _ = self.scheduler.retire_context(context);
            let _ = self.scheduler.abort_unemitted_share_prepare(context);
        }
    }
}
