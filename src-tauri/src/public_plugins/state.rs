use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::atomic_file::{
    commit_with_backup, quarantine_invalid, read_optional, AtomicFileError, AtomicPaths,
};

use super::{
    authorize_plugin_scope,
    manifest::{
        parse_canonical_version, valid_command_name, valid_plugin_id, valid_setting_key,
        PublicPermission, PublicSettingV1,
    },
    valid_json_value, PluginDataScope, PublicManifestV1,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublicPluginFault {
    RuntimeUnavailable,
    ConsecutiveFailures,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EffectivePluginConfig {
    pub(crate) plugin_id: String,
    pub(crate) version: String,
    pub(crate) effective_name: String,
    pub(crate) name_override: Option<String>,
    pub(crate) installed: bool,
    pub(crate) enabled: bool,
    pub(crate) favorite: bool,
    pub(crate) fault: Option<PublicPluginFault>,
    pub(crate) permission_grants: BTreeSet<PublicPermission>,
    pub(crate) inventory_revision: u64,
    pub(crate) active_generation: u64,
    pub(crate) package_digest: Option<String>,
    pub(crate) settings: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginStateError {
    Storage,
    InvalidPlugin,
    InvalidSetting,
    InvalidPermissions,
    RevisionExhausted,
    NameConflict { owner: Option<String> },
    InvalidScope,
}

impl fmt::Display for PluginStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Storage => "public plugin state storage failed",
            Self::InvalidPlugin => "public plugin state is invalid",
            Self::InvalidSetting => "public plugin setting is invalid",
            Self::InvalidPermissions => "public plugin permission grants are invalid",
            Self::RevisionExhausted => "public plugin inventory revision is exhausted",
            Self::NameConflict { .. } => "public plugin effective name conflicts",
            Self::InvalidScope => "public plugin caller scope is invalid",
        })
    }
}

impl std::error::Error for PluginStateError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginStateDocument {
    schema: u32,
    plugin_id: String,
    version: String,
    default_name: String,
    name_override: Option<String>,
    installed: bool,
    enabled: bool,
    #[serde(default)]
    favorite: bool,
    fault: Option<PublicPluginFault>,
    permission_grants: BTreeSet<PublicPermission>,
    inventory_revision: u64,
    #[serde(default)]
    active_generation: u64,
    #[serde(default)]
    package_digest: Option<String>,
    settings: BTreeMap<String, Value>,
}

impl PluginStateDocument {
    fn effective_name(&self) -> &str {
        self.name_override.as_deref().unwrap_or(&self.default_name)
    }

    fn view(&self) -> EffectivePluginConfig {
        EffectivePluginConfig {
            plugin_id: self.plugin_id.clone(),
            version: self.version.clone(),
            effective_name: self.effective_name().into(),
            name_override: self.name_override.clone(),
            installed: self.installed,
            enabled: self.enabled,
            favorite: self.favorite,
            fault: self.fault,
            permission_grants: self.permission_grants.clone(),
            inventory_revision: self.inventory_revision,
            active_generation: self.active_generation,
            package_digest: self.package_digest.clone(),
            settings: self.settings.clone(),
        }
    }
}

#[derive(Clone)]
struct StoredState {
    document: PluginStateDocument,
    raw: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DurableStateOutcome {
    NotCommitted,
    Committed,
    Unknown,
}

pub(crate) struct PreparedStateCommit {
    plugin_id: String,
    base_revision: u64,
    previous: Option<StoredState>,
    candidate: StoredState,
    remove_after_publish: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InventoryRevisionDocument {
    schema: u32,
    revision: u64,
}

struct StateData {
    revision: u64,
    by_plugin: BTreeMap<String, StoredState>,
}

pub(crate) struct PluginStateStore {
    root: PathBuf,
    reserved_names: BTreeSet<String>,
    state: Mutex<StateData>,
}

impl PluginStateStore {
    pub(crate) fn load(
        root: &Path,
        reserved_names: impl IntoIterator<Item = String>,
    ) -> Result<Self, PluginStateError> {
        fs::create_dir_all(root).map_err(|_| PluginStateError::Storage)?;
        if !ordinary_directory(root) {
            return Err(PluginStateError::Storage);
        }
        let reserved_names = reserved_names
            .into_iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        let mut by_plugin = BTreeMap::new();
        for entry in fs::read_dir(root).map_err(|_| PluginStateError::Storage)? {
            let entry = entry.map_err(|_| PluginStateError::Storage)?;
            let plugin_id = entry.file_name().to_string_lossy().into_owned();
            if !valid_plugin_id(&plugin_id) || !ordinary_directory(&entry.path()) {
                continue;
            }
            if let Some(stored) = load_owner(&entry.path(), &plugin_id)? {
                by_plugin.insert(plugin_id, stored);
            }
        }

        let persisted_revision = load_inventory_revision(root)?;
        let owner_revision = by_plugin
            .values()
            .map(|stored| stored.document.inventory_revision)
            .max()
            .unwrap_or(0);
        let store = Self {
            root: root.to_path_buf(),
            reserved_names,
            state: Mutex::new(StateData {
                revision: persisted_revision.max(owner_revision),
                by_plugin,
            }),
        };
        store.validate_loaded_names()?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn install_or_upgrade(
        &self,
        manifest: &PublicManifestV1,
        permission_grants: BTreeSet<PublicPermission>,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        let generation = self
            .config(&manifest.plugin_id)?
            .map_or(Some(1), |config| config.active_generation.checked_add(1))
            .ok_or(PluginStateError::RevisionExhausted)?;
        self.activate(manifest, permission_grants, generation, None)
    }

    #[cfg(test)]
    pub(crate) fn activate(
        &self,
        manifest: &PublicManifestV1,
        permission_grants: BTreeSet<PublicPermission>,
        generation: u64,
        package_digest: Option<String>,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        let prepared =
            self.prepare_activation(manifest, permission_grants, generation, package_digest)?;
        if self.persist_prepared(&prepared) != DurableStateOutcome::Committed {
            return Err(PluginStateError::Storage);
        }
        self.publish_prepared(prepared)
    }

    pub(crate) fn prepare_activation(
        &self,
        manifest: &PublicManifestV1,
        permission_grants: BTreeSet<PublicPermission>,
        generation: u64,
        package_digest: Option<String>,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        if generation == 0
            || package_digest.as_deref().is_some_and(|digest| {
                digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        {
            return Err(PluginStateError::InvalidPlugin);
        }
        let declared = manifest
            .permissions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if declared != permission_grants {
            return Err(PluginStateError::InvalidPermissions);
        }
        let state = self.lock()?;
        let previous = state.by_plugin.get(&manifest.plugin_id);
        if previous.is_some_and(|stored| generation <= stored.document.active_generation) {
            return Err(PluginStateError::InvalidPlugin);
        }
        let override_name = previous.and_then(|stored| stored.document.name_override.clone());
        let effective_name = override_name
            .as_deref()
            .unwrap_or(&manifest.command.default_name);
        self.ensure_name_available(&state, &manifest.plugin_id, effective_name)?;
        let revision = next_revision(state.revision)?;
        let settings = reconcile_settings(
            previous.map(|stored| &stored.document.settings),
            &manifest.settings,
        );
        let (enabled, fault, favorite) = previous
            .filter(|stored| stored.document.installed)
            .map_or((true, None, false), |stored| {
                (
                    stored.document.enabled,
                    stored.document.fault,
                    stored.document.favorite,
                )
            });
        let document = PluginStateDocument {
            schema: 1,
            plugin_id: manifest.plugin_id.clone(),
            version: manifest.version.clone(),
            default_name: manifest.command.default_name.clone(),
            name_override: override_name,
            installed: true,
            enabled,
            favorite,
            fault,
            permission_grants,
            inventory_revision: revision,
            active_generation: generation,
            package_digest,
            settings,
        };
        let raw = serde_json::to_vec(&document).map_err(|_| PluginStateError::Storage)?;
        Ok(PreparedStateCommit {
            plugin_id: manifest.plugin_id.clone(),
            base_revision: state.revision,
            previous: previous.cloned(),
            candidate: StoredState { document, raw },
            remove_after_publish: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn rename(
        &self,
        plugin_id: &str,
        name_override: Option<&str>,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        if name_override.is_some_and(|name| !valid_command_name(name)) {
            return Err(PluginStateError::InvalidPlugin);
        }
        let mut state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let target = name_override.unwrap_or(&previous.document.default_name);
        self.ensure_name_available(&state, plugin_id, target)?;
        let revision = next_revision(state.revision)?;
        let mut document = previous.document.clone();
        document.name_override = name_override.map(str::to_owned);
        document.inventory_revision = revision;
        self.persist_revision(revision)?;
        let stored = self.persist(&document, Some(previous))?;
        let view = document.view();
        state.revision = revision;
        state.by_plugin.insert(plugin_id.into(), stored);
        Ok(view)
    }

    pub(crate) fn prepare_rename(
        &self,
        plugin_id: &str,
        name_override: Option<&str>,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        if name_override.is_some_and(|name| !valid_command_name(name)) {
            return Err(PluginStateError::InvalidPlugin);
        }
        let state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let target = name_override.unwrap_or(&previous.document.default_name);
        self.ensure_name_available(&state, plugin_id, target)?;
        let mut document = previous.document.clone();
        document.name_override = name_override.map(str::to_owned);
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, false)
    }

    #[cfg(test)]
    pub(crate) fn save_settings(
        &self,
        plugin_id: &str,
        definitions: &[PublicSettingV1],
        updates: &BTreeMap<String, Value>,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        for (key, value) in updates {
            let definition = definitions
                .iter()
                .find(|definition| definition.key() == key)
                .ok_or(PluginStateError::InvalidSetting)?;
            if definition.is_secret() || !definition.accepts_value(value) {
                return Err(PluginStateError::InvalidSetting);
            }
        }
        let mut state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let revision = next_revision(state.revision)?;
        let mut document = previous.document.clone();
        document.settings = reconcile_settings(Some(&document.settings), definitions);
        for (key, value) in updates {
            document.settings.insert(key.clone(), value.clone());
        }
        document.inventory_revision = revision;
        self.persist_revision(revision)?;
        let stored = self.persist(&document, Some(previous))?;
        let view = document.view();
        state.revision = revision;
        state.by_plugin.insert(plugin_id.into(), stored);
        Ok(view)
    }

    pub(crate) fn prepare_save_settings(
        &self,
        plugin_id: &str,
        definitions: &[PublicSettingV1],
        updates: &BTreeMap<String, Value>,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        for (key, value) in updates {
            let definition = definitions
                .iter()
                .find(|definition| definition.key() == key)
                .ok_or(PluginStateError::InvalidSetting)?;
            if definition.is_secret() || !definition.accepts_value(value) {
                return Err(PluginStateError::InvalidSetting);
            }
        }
        let state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let mut document = previous.document.clone();
        document.settings = reconcile_settings(Some(&document.settings), definitions);
        for (key, value) in updates {
            document.settings.insert(key.clone(), value.clone());
        }
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, false)
    }

    pub(crate) fn setting(
        &self,
        scope: &PluginDataScope,
        plugin_id: &str,
        definitions: &[PublicSettingV1],
        key: &str,
    ) -> Result<Option<Value>, PluginStateError> {
        authorize_plugin_scope(scope, plugin_id).map_err(|_| PluginStateError::InvalidScope)?;
        let definition = definitions
            .iter()
            .find(|definition| definition.key() == key)
            .ok_or(PluginStateError::InvalidSetting)?;
        if definition.is_secret() {
            return Err(PluginStateError::InvalidSetting);
        }
        let state = self.lock()?;
        let stored = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        Ok(stored
            .document
            .settings
            .get(key)
            .cloned()
            .or_else(|| definition.default_value()))
    }

    #[cfg(test)]
    pub(crate) fn set_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        self.update_lifecycle(plugin_id, |document| {
            document.enabled = enabled;
            if enabled {
                document.fault = None;
            }
        })
    }

    pub(crate) fn prepare_set_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        let state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let mut document = previous.document.clone();
        document.enabled = enabled;
        if enabled {
            document.fault = None;
        }
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, false)
    }

    pub(crate) fn prepare_set_favorite(
        &self,
        plugin_id: &str,
        favorite: bool,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        let state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let mut document = previous.document.clone();
        document.favorite = favorite;
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, false)
    }

    pub(crate) fn prepare_enable_with_generation(
        &self,
        plugin_id: &str,
        generation: u64,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        if generation == 0 {
            return Err(PluginStateError::InvalidPlugin);
        }
        let state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed || generation <= previous.document.active_generation {
            return Err(PluginStateError::InvalidPlugin);
        }
        let mut document = previous.document.clone();
        document.enabled = true;
        document.fault = None;
        document.active_generation = generation;
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, false)
    }

    pub(crate) fn disable_for_fault(
        &self,
        plugin_id: &str,
        fault: PublicPluginFault,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        self.update_lifecycle(plugin_id, |document| {
            document.enabled = false;
            document.fault = Some(fault);
        })
    }

    pub(crate) fn prepare_disable_for_fault(
        &self,
        plugin_id: &str,
        fault: PublicPluginFault,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        let state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let mut document = previous.document.clone();
        document.enabled = false;
        document.fault = Some(fault);
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, false)
    }

    pub(crate) fn configs(&self) -> Result<Vec<EffectivePluginConfig>, PluginStateError> {
        Ok(self
            .lock()?
            .by_plugin
            .values()
            .map(|stored| stored.document.view())
            .collect())
    }

    pub(crate) fn config(
        &self,
        plugin_id: &str,
    ) -> Result<Option<EffectivePluginConfig>, PluginStateError> {
        Ok(self
            .lock()?
            .by_plugin
            .get(plugin_id)
            .map(|stored| stored.document.view()))
    }

    fn prepare_document(
        &self,
        state: &StateData,
        plugin_id: &str,
        document: PluginStateDocument,
        remove_after_publish: bool,
    ) -> Result<PreparedStateCommit, PluginStateError> {
        let previous = state.by_plugin.get(plugin_id).cloned();
        let raw = serde_json::to_vec(&document).map_err(|_| PluginStateError::Storage)?;
        Ok(PreparedStateCommit {
            plugin_id: plugin_id.to_owned(),
            base_revision: state.revision,
            previous,
            candidate: StoredState { document, raw },
            remove_after_publish,
        })
    }

    #[cfg(test)]
    pub(crate) fn uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<(), PluginStateError> {
        let mut state = self.lock()?;
        let Some(previous) = state.by_plugin.get(plugin_id) else {
            return Ok(());
        };
        if retain_data {
            let revision = next_revision(state.revision)?;
            let mut document = previous.document.clone();
            document.installed = false;
            document.enabled = false;
            document.favorite = false;
            document.fault = None;
            document.inventory_revision = revision;
            self.persist_revision(revision)?;
            let stored = self.persist(&document, Some(previous))?;
            state.revision = revision;
            state.by_plugin.insert(plugin_id.into(), stored);
        } else {
            let owner = owner_root(&self.root, plugin_id)?;
            let revision = next_revision(state.revision)?;
            self.persist_revision(revision)?;
            fs::remove_dir_all(owner).map_err(|_| PluginStateError::Storage)?;
            state.by_plugin.remove(plugin_id);
            state.revision = revision;
        }
        Ok(())
    }

    pub(crate) fn prepare_uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<Option<PreparedStateCommit>, PluginStateError> {
        let state = self.lock()?;
        let Some(previous) = state.by_plugin.get(plugin_id) else {
            return Ok(None);
        };
        let mut document = previous.document.clone();
        document.installed = false;
        document.enabled = false;
        document.favorite = false;
        document.fault = None;
        document.inventory_revision = next_revision(state.revision)?;
        self.prepare_document(&state, plugin_id, document, !retain_data)
            .map(Some)
    }

    pub(crate) fn cleanup_uninstalled_owner(
        &self,
        plugin_id: &str,
    ) -> Result<(), PluginStateError> {
        let owner = owner_root(&self.root, plugin_id)?;
        match fs::remove_dir_all(owner) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(PluginStateError::Storage),
        }
    }

    fn update_lifecycle(
        &self,
        plugin_id: &str,
        update: impl FnOnce(&mut PluginStateDocument),
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        let mut state = self.lock()?;
        let previous = state
            .by_plugin
            .get(plugin_id)
            .ok_or(PluginStateError::InvalidPlugin)?;
        if !previous.document.installed {
            return Err(PluginStateError::InvalidPlugin);
        }
        let revision = next_revision(state.revision)?;
        let mut document = previous.document.clone();
        update(&mut document);
        document.inventory_revision = revision;
        self.persist_revision(revision)?;
        let stored = self.persist(&document, Some(previous))?;
        let view = document.view();
        state.revision = revision;
        state.by_plugin.insert(plugin_id.into(), stored);
        Ok(view)
    }

    fn validate_loaded_names(&self) -> Result<(), PluginStateError> {
        let state = self.lock()?;
        for (plugin_id, stored) in &state.by_plugin {
            if stored.document.installed {
                self.ensure_name_available(&state, plugin_id, stored.document.effective_name())?;
            }
        }
        Ok(())
    }

    fn ensure_name_available(
        &self,
        state: &StateData,
        plugin_id: &str,
        name: &str,
    ) -> Result<(), PluginStateError> {
        let folded = name.to_ascii_lowercase();
        if self.reserved_names.contains(&folded) {
            return Err(PluginStateError::NameConflict { owner: None });
        }
        if let Some(owner) = state.by_plugin.values().find_map(|stored| {
            let document = &stored.document;
            (document.installed
                && document.plugin_id != plugin_id
                && document.effective_name().to_ascii_lowercase() == folded)
                .then(|| document.plugin_id.clone())
        }) {
            return Err(PluginStateError::NameConflict { owner: Some(owner) });
        }
        Ok(())
    }

    fn persist(
        &self,
        document: &PluginStateDocument,
        previous: Option<&StoredState>,
    ) -> Result<StoredState, PluginStateError> {
        let bytes = serde_json::to_vec(document).map_err(|_| PluginStateError::Storage)?;
        let owner = owner_root(&self.root, &document.plugin_id)?;
        fs::create_dir_all(&owner).map_err(|_| PluginStateError::Storage)?;
        if !ordinary_directory(&owner) {
            return Err(PluginStateError::Storage);
        }
        let paths = AtomicPaths::new(&owner, "state.json");
        commit_with_backup(
            &paths,
            previous.map(|previous| previous.raw.as_slice()),
            &bytes,
        )
        .map_err(|_| PluginStateError::Storage)?;
        Ok(StoredState {
            document: document.clone(),
            raw: bytes,
        })
    }

    pub(crate) fn persist_prepared(&self, prepared: &PreparedStateCommit) -> DurableStateOutcome {
        let owner = match owner_root(&self.root, &prepared.plugin_id) {
            Ok(owner) => owner,
            Err(_) => return DurableStateOutcome::NotCommitted,
        };
        if fs::create_dir_all(&owner).is_err() || !ordinary_directory(&owner) {
            return DurableStateOutcome::NotCommitted;
        }
        let paths = AtomicPaths::new(&owner, "state.json");
        let outcome = classify_durable_result(commit_with_backup(
            &paths,
            prepared
                .previous
                .as_ref()
                .map(|previous| previous.raw.as_slice()),
            &prepared.candidate.raw,
        ));
        if outcome == DurableStateOutcome::Committed {
            let _ = self.persist_revision(prepared.candidate.document.inventory_revision);
        }
        outcome
    }

    pub(crate) fn revalidate_previous(&self, prepared: &PreparedStateCommit) -> bool {
        let Ok(owner) = owner_root(&self.root, &prepared.plugin_id) else {
            return false;
        };
        let paths = AtomicPaths::new(&owner, "state.json");
        match (read_optional(paths.current()), prepared.previous.as_ref()) {
            (Ok(None), None) => true,
            (Ok(Some(actual)), Some(previous)) => actual == previous.raw,
            _ => false,
        }
    }

    pub(crate) fn publish_prepared(
        &self,
        prepared: PreparedStateCommit,
    ) -> Result<EffectivePluginConfig, PluginStateError> {
        let mut state = self.lock()?;
        let previous_matches = match (
            state.by_plugin.get(&prepared.plugin_id),
            prepared.previous.as_ref(),
        ) {
            (None, None) => true,
            (Some(current), Some(previous)) => current.raw == previous.raw,
            _ => false,
        };
        if state.revision != prepared.base_revision || !previous_matches {
            return Err(PluginStateError::Storage);
        }
        let view = prepared.candidate.document.view();
        state.revision = prepared.candidate.document.inventory_revision;
        if prepared.remove_after_publish {
            state.by_plugin.remove(&prepared.plugin_id);
        } else {
            state
                .by_plugin
                .insert(prepared.plugin_id, prepared.candidate);
        }
        Ok(view)
    }

    fn persist_revision(&self, revision: u64) -> Result<(), PluginStateError> {
        let bytes = serde_json::to_vec(&InventoryRevisionDocument {
            schema: 1,
            revision,
        })
        .map_err(|_| PluginStateError::Storage)?;
        let paths = AtomicPaths::new(&self.root, "inventory.json");
        let previous = read_optional(paths.current()).map_err(|_| PluginStateError::Storage)?;
        commit_with_backup(&paths, previous.as_deref(), &bytes)
            .map_err(|_| PluginStateError::Storage)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, StateData>, PluginStateError> {
        self.state.lock().map_err(|_| PluginStateError::Storage)
    }
}

fn classify_durable_result(result: Result<(), AtomicFileError>) -> DurableStateOutcome {
    match result {
        Ok(()) => DurableStateOutcome::Committed,
        Err(
            AtomicFileError::CandidateWrite
            | AtomicFileError::BackupWrite
            | AtomicFileError::BackupReplace,
        ) => DurableStateOutcome::NotCommitted,
        Err(
            AtomicFileError::Read
            | AtomicFileError::CurrentReplace
            | AtomicFileError::InvalidQuarantine,
        ) => DurableStateOutcome::Unknown,
    }
}

#[cfg(test)]
mod durable_outcome_tests {
    use super::{classify_durable_result, AtomicFileError, DurableStateOutcome};

    #[test]
    fn durable_helper_exposes_only_the_three_frozen_outcomes() {
        let cases = [
            (Ok(()), DurableStateOutcome::Committed),
            (
                Err(AtomicFileError::CandidateWrite),
                DurableStateOutcome::NotCommitted,
            ),
            (
                Err(AtomicFileError::BackupWrite),
                DurableStateOutcome::NotCommitted,
            ),
            (
                Err(AtomicFileError::BackupReplace),
                DurableStateOutcome::NotCommitted,
            ),
            (
                Err(AtomicFileError::CurrentReplace),
                DurableStateOutcome::Unknown,
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(classify_durable_result(input), expected);
        }
    }
}

fn load_inventory_revision(root: &Path) -> Result<u64, PluginStateError> {
    let paths = AtomicPaths::new(root, "inventory.json");
    for path in [paths.current(), paths.backup()] {
        let Some(bytes) = read_optional(path).map_err(|_| PluginStateError::Storage)? else {
            continue;
        };
        if let Ok(document) = serde_json::from_slice::<InventoryRevisionDocument>(&bytes) {
            if document.schema == 1 {
                return Ok(document.revision);
            }
        }
        quarantine_invalid(path).map_err(|_| PluginStateError::Storage)?;
    }
    Ok(0)
}

fn load_owner(owner: &Path, plugin_id: &str) -> Result<Option<StoredState>, PluginStateError> {
    let paths = AtomicPaths::new(owner, "state.json");
    for path in [paths.current(), paths.backup()] {
        let Some(bytes) = read_optional(path).map_err(|_| PluginStateError::Storage)? else {
            continue;
        };
        if let Some(document) = parse_document(&bytes, plugin_id) {
            return Ok(Some(StoredState {
                document,
                raw: bytes,
            }));
        }
        quarantine_invalid(path).map_err(|_| PluginStateError::Storage)?;
    }
    Ok(None)
}

fn parse_document(bytes: &[u8], plugin_id: &str) -> Option<PluginStateDocument> {
    let document = serde_json::from_slice::<PluginStateDocument>(bytes).ok()?;
    (document.schema == 1
        && document.plugin_id == plugin_id
        && valid_plugin_id(&document.plugin_id)
        && parse_canonical_version(&document.version).is_some()
        && valid_command_name(&document.default_name)
        && document
            .name_override
            .as_deref()
            .is_none_or(valid_command_name)
        && (document.package_digest.is_none()
            || (document.active_generation != 0
                && document.package_digest.as_deref().is_some_and(|digest| {
                    digest.len() == 64
                        && digest
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                })))
        && document
            .settings
            .iter()
            .all(|(key, value)| valid_setting_key(key) && valid_json_value(value)))
    .then_some(document)
}

fn reconcile_settings(
    previous: Option<&BTreeMap<String, Value>>,
    definitions: &[PublicSettingV1],
) -> BTreeMap<String, Value> {
    let mut settings = BTreeMap::new();
    for definition in definitions {
        if definition.is_secret() {
            continue;
        }
        let value = previous
            .and_then(|values| values.get(definition.key()))
            .filter(|value| definition.accepts_value(value))
            .cloned()
            .or_else(|| definition.default_value());
        if let Some(value) = value {
            settings.insert(definition.key().into(), value);
        }
    }
    settings
}

fn next_revision(revision: u64) -> Result<u64, PluginStateError> {
    revision
        .checked_add(1)
        .ok_or(PluginStateError::RevisionExhausted)
}

fn owner_root(root: &Path, plugin_id: &str) -> Result<PathBuf, PluginStateError> {
    valid_plugin_id(plugin_id)
        .then(|| root.join(plugin_id))
        .ok_or(PluginStateError::InvalidPlugin)
}

fn ordinary_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !is_reparse_point(&metadata))
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
