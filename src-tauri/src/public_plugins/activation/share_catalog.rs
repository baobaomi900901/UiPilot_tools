//! Narrow read-only view over confirmed, validated activation bundles.
#![allow(dead_code)] // Consumed by subsequent native sharing integration.
use super::*;
use crate::plugin_share::{catalog::Receiver, types::PluginIdentity};

pub(super) fn eligible(manifest: &PublicManifestV1, config: &EffectivePluginConfig) -> bool {
    config.installed
        && config.enabled
        && config.fault.is_none()
        && manifest.share.as_ref().is_some_and(|s| s.version == 1)
        && manifest.panel.is_some()
        && manifest.legacy_command().is_some_and(|c| {
            c.activation_mode == PublicActivationMode::Submit
                && c.output_mode == PublicOutputMode::Panel
        })
        && manifest.permissions.contains(&PublicPermission::UiPanel)
        && config
            .permission_grants
            .contains(&PublicPermission::UiPanel)
}

pub(super) fn identity(bundle: &super::super::activation_bundle::ActivationBundle) -> PluginIdentity {
    PluginIdentity {
        plugin_id: bundle.config.plugin_id.clone(),
        generation: bundle.config.active_generation,
        activation_id: bundle.activation_id,
        admission_epoch: bundle.admission_epoch,
        inventory_revision: bundle.config.inventory_revision,
    }
}

impl PublicPluginManager {
    /// Only already-validated confirmed bundles, no candidate execution, runtime
    /// startup, disk rescan or body. Freeze this Vec for selector pagination.
    /// Admission/commit must revalidate; this snapshot grants no authority.
    pub(crate) fn share_catalog(&self) -> Result<Vec<Receiver>, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        let mut receivers = Vec::new();
        if self.host.platform != super::super::manifest::PublicPlatform::Windows {
            return Ok(receivers);
        }
        for config in self.state.configs()? {
            let bundle = match self.bundles.confirmed_bundle(&config.plugin_id) {
                Ok(Some(bundle)) => bundle,
                Ok(None) | Err(ReservationError::Busy) => continue,
                Err(error) => return Err(error.into()),
            };
            let manifest = &bundle.runtime.manifest;
            if bundle.config != config || !eligible(manifest, &config) {
                continue;
            }
            for receiver in manifest
                .share
                .as_ref()
                .and_then(|s| s.receivers.as_ref())
                .into_iter()
                .flatten()
            {
                receivers.push(Receiver {
                    plugin: identity(&bundle),
                    plugin_name: manifest.name.clone(),
                    receiver_id: receiver.id.clone(),
                    title: receiver.title.clone(),
                    command_id: receiver.command_id.clone(),
                    max_bytes: receiver.max_bytes as usize,
                });
            }
        }
        Ok(receivers)
    }

    /// Host-only source admission lookup. inventory_revision belongs to this
    /// plugin's config (including grants/settings), not the global inventory.
    pub(crate) fn share_source_identity(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginIdentity>, PublicPluginManagementError> {
        let _mutation = self.lock_mutation()?;
        if self.host.platform != super::super::manifest::PublicPlatform::Windows {
            return Ok(None);
        }
        let Some(bundle) = self.bundles.confirmed_bundle(plugin_id)? else {
            return Ok(None);
        };
        let Some(config) = self.state.config(plugin_id)? else {
            return Ok(None);
        };
        let manifest = &bundle.runtime.manifest;
        Ok((bundle.config == config
            && eligible(manifest, &config)
            && manifest
                .share
                .as_ref()
                .is_some_and(|s| s.source == Some(true)))
        .then(|| identity(&bundle)))
    }
}

#[cfg(test)]
mod plugin_share_tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (PublicManifestV1, EffectivePluginConfig) {
        let manifest = serde_json::from_value(json!({
            "schemaVersion":1,"pluginId":"com.test.receiver","name":"Receiver","version":"1.0.0",
            "apiVersion":1,"minimumHostVersion":"0.22.0","supportedPlatforms":["windows"],
            "command":{"defaultName":"/receiver","activationMode":"submit","outputMode":"panel","inputRequired":false},
            "runtime":{"entry":"runtime.js"},"panel":{"entry":"panel.html"},"permissions":["ui.panel"],
            "share":{"version":1,"receivers":[{"id":"receive","title":"Receive","commandId":"default","types":["text/plain"],"maxBytes":8192}]}
        })).unwrap();
        let config = EffectivePluginConfig {
            plugin_id: "com.test.receiver".into(),
            version: "1.0.0".into(),
            commands: BTreeMap::new(),
            installed: true,
            enabled: true,
            fault: None,
            permission_grants: BTreeSet::from([PublicPermission::UiPanel]),
            network_https_hosts_grant: BTreeSet::new(),
            network_credentials_grant: vec![],
            inventory_revision: 2,
            active_generation: 3,
            package_digest: Some("digest".into()),
            settings: BTreeMap::new(),
        };
        (manifest, config)
    }
    #[test]
    fn catalog_excludes_uninstalled_disabled_ungranted_nonpanel_and_undeclared() {
        let (manifest, config) = fixture();
        assert!(eligible(&manifest, &config));
        let mut c = config.clone();
        c.installed = false;
        assert!(!eligible(&manifest, &c));
        let mut c = config.clone();
        c.enabled = false;
        assert!(!eligible(&manifest, &c));
        let mut c = config.clone();
        c.permission_grants.clear();
        assert!(!eligible(&manifest, &c));
        let mut m = manifest.clone();
        m.panel = None;
        assert!(!eligible(&m, &config));
        let mut m = manifest.clone();
        m.share = None;
        assert!(!eligible(&m, &config));
        let mut value = serde_json::to_value(&manifest).unwrap();
        value["command"]["outputMode"] = json!("mainResult");
        let m = serde_json::from_value(value).unwrap();
        assert!(!eligible(&m, &config));
    }
}
