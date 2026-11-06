//! Body-free manager authority. A read guard is the commit exclusion barrier.
//! Never call manager, book, scheduler or UI code while holding that read guard.
#![allow(dead_code)] // Panel/coordinator consumers are introduced in Task 4D.
use super::*;
use crate::plugin_share::{catalog::Receiver, types::PluginIdentity};
use std::sync::{atomic::AtomicU8, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShareAuthorityIdentity {
    pub(crate) plugin: PluginIdentity,
    pub(crate) package_digest: String,
    pub(crate) command_id: String,
    pub(crate) receiver: Option<Receiver>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum ShareRevocation {
    Reservation = 1,
    Secret,
    Runtime,
    Closed,
    Terminal,
}

struct LeaseRecord {
    identity: ShareAuthorityIdentity,
    revision: u64,
    revoked: AtomicU8,
}

#[derive(Clone)]
pub(crate) struct ShareAuthorityLease(Arc<LeaseRecord>);
impl ShareAuthorityLease {
    pub(crate) fn identity(&self) -> &ShareAuthorityIdentity {
        &self.0.identity
    }
    pub(crate) fn revision(&self) -> u64 {
        self.0.revision
    }
    /// Sticky history survives notification drain, replacement and registry cleanup.
    pub(crate) fn revocation(&self) -> Option<ShareRevocation> {
        match self.0.revoked.load(Ordering::Acquire) {
            0 => None,
            1 => Some(ShareRevocation::Reservation),
            2 => Some(ShareRevocation::Secret),
            3 => Some(ShareRevocation::Runtime),
            4 => Some(ShareRevocation::Closed),
            _ => Some(ShareRevocation::Terminal),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShareAuthorityRevocation {
    pub(crate) identity: ShareAuthorityIdentity,
    pub(crate) revision: u64,
    pub(crate) reason: ShareRevocation,
}

#[derive(Default)]
pub(in crate::public_plugins) struct AuthorityState {
    terminal: bool,
    next_revision: u64,
    leases: Vec<Weak<LeaseRecord>>,
    blocked: BTreeSet<String>,
    unavailable_runtimes: BTreeSet<(String, u64, u64, u64)>,
    unavailable_generations: BTreeSet<(String, u64)>,
    notifications: Vec<ShareAuthorityRevocation>,
}

#[derive(Default)]
pub(crate) struct ShareAuthorityRegistry {
    state: RwLock<AuthorityState>,
}
pub(crate) struct ShareAuthorityReadGuard<'a> {
    _state: RwLockReadGuard<'a, AuthorityState>,
}

impl ShareAuthorityRegistry {
    pub(crate) fn read_pair<'a>(
        &'a self,
        source: &ShareAuthorityLease,
        target: &ShareAuthorityLease,
    ) -> Result<ShareAuthorityReadGuard<'a>, PublicPluginManagementError> {
        let state = match self.state.read() {
            Ok(state) => state,
            Err(poisoned) => {
                drop(poisoned);
                self.write().terminate();
                return Err(PublicPluginManagementError::Unavailable);
            }
        };
        let owns = |lease: &ShareAuthorityLease| {
            state
                .leases
                .iter()
                .any(|item| item.ptr_eq(&Arc::downgrade(&lease.0)))
        };
        if source.identity().receiver.is_some()
            || target.identity().receiver.is_none()
            || state.terminal
            || source.revocation().is_some()
            || target.revocation().is_some()
            || !owns(source)
            || !owns(target)
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok(ShareAuthorityReadGuard { _state: state })
    }
    pub(in crate::public_plugins) fn write(&self) -> RwLockWriteGuard<'_, AuthorityState> {
        match self.state.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.terminate();
                guard
            }
        }
    }
    /// Drain outside M/A/B; consumers must dispatch only after this method returns.
    pub(in crate::public_plugins) fn drain_revocations(&self) -> Vec<ShareAuthorityRevocation> {
        std::mem::take(&mut self.write().notifications)
    }
}

impl AuthorityState {
    fn issue(
        &mut self,
        identity: ShareAuthorityIdentity,
    ) -> Result<ShareAuthorityLease, PublicPluginManagementError> {
        if self.terminal
            || self.blocked.contains(&identity.plugin.plugin_id)
            || self.unavailable_generations.contains(&(
                identity.plugin.plugin_id.clone(),
                identity.plugin.generation,
            ))
            || self.unavailable_runtimes.contains(&(
                identity.plugin.plugin_id.clone(),
                identity.plugin.generation,
                identity.plugin.activation_id,
                identity.plugin.admission_epoch,
            ))
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        self.next_revision = self
            .next_revision
            .checked_add(1)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        self.leases.retain(|lease| lease.strong_count() != 0);
        let lease = ShareAuthorityLease(Arc::new(LeaseRecord {
            identity,
            revision: self.next_revision,
            revoked: AtomicU8::new(0),
        }));
        self.leases.push(Arc::downgrade(&lease.0));
        Ok(lease)
    }
    pub(in crate::public_plugins) fn revoke(
        &mut self,
        plugin: &str,
        generation: Option<u64>,
        reason: ShareRevocation,
    ) {
        for lease in self.leases.iter().filter_map(Weak::upgrade) {
            if lease.identity.plugin.plugin_id == plugin
                && generation.is_none_or(|g| g == lease.identity.plugin.generation)
                && lease
                    .revoked
                    .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                self.notifications.push(ShareAuthorityRevocation {
                    identity: lease.identity.clone(),
                    revision: lease.revision,
                    reason,
                });
            }
        }
    }
    pub(in crate::public_plugins) fn block(&mut self, plugin: &str, reason: ShareRevocation) {
        self.revoke(plugin, None, reason);
        self.blocked.insert(plugin.into());
    }
    pub(in crate::public_plugins) fn invalidate_runtime(
        &mut self,
        plugin: &str,
        generation: u64,
        activation: u64,
        epoch: u64,
    ) {
        for lease in self.leases.iter().filter_map(Weak::upgrade) {
            let identity = &lease.identity.plugin;
            if identity.plugin_id == plugin
                && identity.generation == generation
                && identity.activation_id == activation
                && identity.admission_epoch == epoch
                && lease
                    .revoked
                    .compare_exchange(
                        0,
                        ShareRevocation::Runtime as u8,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
            {
                self.notifications.push(ShareAuthorityRevocation {
                    identity: lease.identity.clone(),
                    revision: lease.revision,
                    reason: ShareRevocation::Runtime,
                });
            }
        }
        self.unavailable_runtimes
            .insert((plugin.into(), generation, activation, epoch));
    }
    pub(in crate::public_plugins) fn invalidate_generation(
        &mut self,
        plugin: &str,
        generation: u64,
    ) {
        self.revoke(plugin, Some(generation), ShareRevocation::Runtime);
        self.unavailable_generations
            .insert((plugin.into(), generation));
    }
    pub(in crate::public_plugins) fn recovered(&mut self, plugin: &str, generation: u64) {
        self.unavailable_generations
            .remove(&(plugin.into(), generation));
        self.unavailable_runtimes
            .retain(|(id, g, _, _)| id != plugin || *g != generation);
    }
    pub(in crate::public_plugins) fn available(&mut self, plugin: &str) {
        self.blocked.remove(plugin);
    }
    pub(in crate::public_plugins) fn terminate(&mut self) {
        self.terminal = true;
        for lease in self.leases.iter().filter_map(Weak::upgrade) {
            if lease
                .revoked
                .compare_exchange(
                    0,
                    ShareRevocation::Terminal as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                self.notifications.push(ShareAuthorityRevocation {
                    identity: lease.identity.clone(),
                    revision: lease.revision,
                    reason: ShareRevocation::Terminal,
                });
            }
        }
    }
}

/// Revokes under A before the underlying M becomes poisoned. Inner guards
/// created after M have already unwound; no callbacks run during this cleanup.
pub(super) struct ShareMutationGuard<'a> {
    _mutation: std::sync::MutexGuard<'a, ()>,
    authority: &'a ShareAuthorityRegistry,
}
impl<'a> ShareMutationGuard<'a> {
    pub(super) fn new(
        mutation: std::sync::MutexGuard<'a, ()>,
        authority: &'a ShareAuthorityRegistry,
    ) -> Self {
        Self {
            _mutation: mutation,
            authority,
        }
    }
}
impl Drop for ShareMutationGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.authority.write().terminate();
        }
    }
}

impl PublicPluginManager {
    /// Returns body-free notifications after M/A have both been released.
    /// Caller dispatches them outside all manager, Panel and coordinator guards.
    pub(crate) fn drain_share_authority_revocations(&self) -> Vec<ShareAuthorityRevocation> {
        let _mutation = self
            .mutation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.bundles.share_authority.drain_revocations()
    }

    pub(crate) fn share_authorities(&self) -> Arc<ShareAuthorityRegistry> {
        Arc::clone(&self.bundles.share_authority)
    }
    pub(crate) fn issue_share_source_authority(
        &self,
        plugin: &PluginIdentity,
        command_id: &str,
    ) -> Result<ShareAuthorityLease, PublicPluginManagementError> {
        self.issue_share_authority(plugin, command_id, None)
    }
    pub(crate) fn issue_share_receiver_authority(
        &self,
        receiver: &Receiver,
    ) -> Result<ShareAuthorityLease, PublicPluginManagementError> {
        self.issue_share_authority(&receiver.plugin, &receiver.command_id, Some(receiver))
    }
    fn issue_share_authority(
        &self,
        plugin: &PluginIdentity,
        command_id: &str,
        receiver: Option<&Receiver>,
    ) -> Result<ShareAuthorityLease, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let mut authority = self.bundles.share_authority.write();
        let bundle = self
            .bundles
            .confirmed_bundle(&plugin.plugin_id)?
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let config = self
            .state
            .config(&plugin.plugin_id)?
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let manifest = &bundle.runtime.manifest;
        if self.host.platform != super::super::manifest::PublicPlatform::Windows
            || bundle.config != config
            || !share_catalog::eligible(manifest, &config)
            || share_catalog::identity(&bundle) != *plugin
            || bundle.runtime_recovery_needed
            || bundle.runtime.generation != plugin.generation
            || config.package_digest.as_deref() != Some(bundle.runtime.digest.as_str())
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        let command = manifest
            .command_by_id(command_id)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if command.activation_mode != PublicActivationMode::Submit
            || command.output_mode != PublicOutputMode::Panel
            || config.command(command_id).is_none()
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        let share = manifest
            .share
            .as_ref()
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if let Some(receiver) = receiver {
            let declared = share
                .receivers
                .as_ref()
                .and_then(|items| items.iter().find(|item| item.id == receiver.receiver_id))
                .ok_or(PublicPluginManagementError::Unavailable)?;
            if declared.command_id != receiver.command_id
                || declared.title != receiver.title
                || declared.max_bytes as usize != receiver.max_bytes
                || manifest.name != receiver.plugin_name
            {
                return Err(PublicPluginManagementError::Unavailable);
            }
        } else if share.source != Some(true) {
            return Err(PublicPluginManagementError::Unavailable);
        }
        authority.issue(ShareAuthorityIdentity {
            plugin: plugin.clone(),
            package_digest: bundle.runtime.digest.clone(),
            command_id: command_id.into(),
            receiver: receiver.cloned(),
        })
    }
}
