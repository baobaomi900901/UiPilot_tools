use super::*;
use crate::plugin_share::Receiver;
impl PublicPluginManager {
    /// Read validated, granted manifests only. Discovery never starts a Runtime.
    pub(crate) fn share_catalog(&self) -> Result<Vec<Receiver>, PublicPluginManagementError> {
        let mut rows = Vec::new();
        for config in self.state.configs()? {
            let Some(b) = self.bundles.confirmed_bundle(&config.plugin_id)? else {
                continue;
            };
            if b.config != config
                || !config.installed
                || !config.enabled
                || config.fault.is_some()
                || b.runtime_recovery_needed
                || !config
                    .permission_grants
                    .contains(&PublicPermission::UiPanel)
            {
                continue;
            }
            let m = &b.runtime.manifest;
            if !m.permissions.contains(&PublicPermission::UiPanel) || m.panel.is_none() {
                continue;
            }
            for r in m
                .share
                .as_ref()
                .and_then(|s| s.receivers.as_ref())
                .into_iter()
                .flatten()
            {
                rows.push(Receiver {
                    plugin_id: config.plugin_id.clone(),
                    plugin_name: m.name.clone(),
                    receiver_id: r.id.clone(),
                    title: r.title.clone(),
                    command_id: r.command_id.clone(),
                    max_bytes: r.max_bytes as usize,
                    generation: b.runtime.generation,
                    activation_id: b.activation_id,
                    admission_epoch: b.admission_epoch,
                    disabled_reason: None,
                });
            }
        }
        Ok(rows)
    }
    pub(crate) fn share_source_enabled(
        &self,
        i: &crate::plugin_panel::PanelSessionIdentity,
    ) -> bool {
        self.bundles
            .confirmed_bundle(&i.plugin_id)
            .ok()
            .flatten()
            .is_some_and(|b| {
                b.config.installed
                    && b.config.enabled
                    && b.config.fault.is_none()
                    && !b.runtime_recovery_needed
                    && b.runtime.generation == i.generation
                    && b.activation_id == i.activation_id
                    && b.admission_epoch == i.admission_epoch
                    && b.config
                        .permission_grants
                        .contains(&PublicPermission::UiPanel)
                    && b.runtime
                        .manifest
                        .share
                        .as_ref()
                        .is_some_and(|s| s.source == Some(true))
            })
    }
}
