//! Trusted-host ordinary settings persistence. No Runtime mutation API.
//! Handles carry only identity and immutable definitions, never a store or FD.
//! Lifecycle callers close/drain old epochs before configuring an admin epoch.
use super::{
    authorize_plugin_scope,
    data_call_gate::{PluginDataCallGate, PluginDataCallIdentity, PluginDataCallLease},
    manifest::{valid_setting_key, PublicSettingV1},
    private_file_macos::{FileKind, PrivateFile, PrivateRoot},
    settings_values::{public_definition, reconcile_settings, stored_value, validate_updates},
    storage_document::{check_values_quota, validate_mac_json, PluginStorageError},
    valid_json_value, PluginDataScope,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsError {
    InvalidScope,
    InvalidDefinition,
    InvalidSetting,
    QuotaExceeded,
    Storage,
}
impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidScope => "plugin settings scope expired or invalid",
            Self::InvalidDefinition => "plugin setting definitions invalid",
            Self::InvalidSetting => "plugin setting update invalid",
            Self::QuotaExceeded => "plugin settings quota exceeded",
            Self::Storage => "plugin settings storage failed",
        })
    }
}
impl std::error::Error for SettingsError {}
type Result<T> = std::result::Result<T, SettingsError>;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsDocument {
    schema: u32,
    plugin_id: String,
    revision: u64,
    values: BTreeMap<String, Value>,
}
#[derive(Clone)]
pub(crate) struct SettingsHandle {
    store: Arc<()>,
    owner: String,
    identity: PluginDataCallIdentity,
    generation: Arc<()>,
    revision: u64,
    definitions: Arc<[PublicSettingV1]>,
}
struct Owner {
    file: PrivateFile,
    document: SettingsDocument,
    raw: Option<Vec<u8>>,
    handle: Option<SettingsHandle>,
}
pub(crate) struct MacSettingsStore {
    root: PrivateRoot,
    gate: Arc<PluginDataCallGate>,
    identity: Arc<()>,
    owners: Mutex<BTreeMap<String, Owner>>,
}
impl MacSettingsStore {
    pub(crate) fn load(root: &Path, gate: Arc<PluginDataCallGate>) -> Result<Self> {
        Ok(Self {
            root: PrivateRoot::open(root).map_err(|_| SettingsError::Storage)?,
            gate,
            identity: Arc::new(()),
            owners: Mutex::new(BTreeMap::new()),
        })
    }
    fn admit(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
    ) -> Result<PluginDataCallLease> {
        authorize_plugin_scope(scope, identity.plugin_id())
            .map_err(|_| SettingsError::InvalidScope)?;
        self.gate
            .try_acquire(identity)
            .map_err(|_| SettingsError::InvalidScope)
    }
    fn check_handle(
        &self,
        identity: &PluginDataCallIdentity,
        handle: &SettingsHandle,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.identity, &handle.store)
            || handle.identity != *identity
            || handle.owner != identity.plugin_id()
        {
            return Err(SettingsError::InvalidScope);
        }
        Ok(())
    }
    fn check_disk(&self, owner: &Owner) -> Result<()> {
        if owner
            .file
            .read(&self.root)
            .map_err(|_| SettingsError::Storage)?
            != owner.raw
        {
            return Err(SettingsError::Storage);
        }
        Ok(())
    }
    fn persist(&self, owner: &mut Owner, candidate: SettingsDocument) -> Result<()> {
        let bytes = encode(&candidate)?;
        if owner.raw.as_deref() != Some(bytes.as_slice()) {
            owner
                .file
                .commit(&self.root, owner.raw.as_deref(), &bytes)
                .map_err(|_| SettingsError::Storage)?;
        }
        owner.document = candidate;
        owner.raw = Some(bytes);
        Ok(())
    }
    /// Trusted management entry only: the Runtime cannot supply definitions.
    /// Every successful configuration replaces the owner generation, even if
    /// reconciled values are unchanged. Failure never publishes a new handle.
    pub(crate) fn configure(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        definitions: &[PublicSettingV1],
    ) -> Result<SettingsHandle> {
        self.configure_with_updates(scope, identity, definitions, &BTreeMap::new())
    }
    /// Definition reconciliation and updates form one durable transaction.
    pub(crate) fn configure_with_updates(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        definitions: &[PublicSettingV1],
        updates: &BTreeMap<String, Value>,
    ) -> Result<SettingsHandle> {
        let _lease = self.admit(scope, identity)?;
        let mut keys = HashSet::new();
        if definitions
            .iter()
            .any(|definition| !definition.validate() || !keys.insert(definition.key()))
        {
            return Err(SettingsError::InvalidDefinition);
        }
        let id = identity.plugin_id();
        validate_updates(definitions, updates).map_err(|_| SettingsError::InvalidSetting)?;
        if updates.values().any(|value| !valid_json_value(value)) {
            return Err(SettingsError::InvalidSetting);
        }
        // An updates map is a subset of the final candidate. Reject its own
        // overflow before owner I/O; check the full reconciled candidate later.
        encode(&SettingsDocument {
            schema: 1,
            plugin_id: id.into(),
            revision: 0,
            values: updates.clone(),
        })?;
        let mut owners = self.owners.lock().map_err(|_| SettingsError::Storage)?;
        if !owners.contains_key(id) {
            let file = PrivateFile::open_kind(&self.root, id, true, FileKind::Settings)
                .map_err(|_| SettingsError::Storage)?
                .ok_or(SettingsError::Storage)?;
            let raw = file.read(&self.root).map_err(|_| SettingsError::Storage)?;
            let document = match &raw {
                Some(bytes) => parse(bytes, id).ok_or(SettingsError::Storage)?,
                None => SettingsDocument {
                    schema: 1,
                    plugin_id: id.into(),
                    revision: 0,
                    values: BTreeMap::new(),
                },
            };
            owners.insert(
                id.into(),
                Owner {
                    file,
                    document,
                    raw,
                    handle: None,
                },
            );
        }
        let owner = owners.get_mut(id).ok_or(SettingsError::Storage)?;
        self.check_disk(owner)?;
        let mut values = reconcile_settings(Some(&owner.document.values), definitions);
        values.extend(updates.clone());
        let revision = owner
            .document
            .revision
            .checked_add(1)
            .ok_or(SettingsError::Storage)?;
        self.persist(
            owner,
            SettingsDocument {
                schema: 1,
                plugin_id: id.into(),
                revision,
                values,
            },
        )?;
        let handle = SettingsHandle {
            store: self.identity.clone(),
            owner: id.into(),
            identity: identity.clone(),
            generation: Arc::new(()),
            revision,
            definitions: definitions.to_vec().into(),
        };
        owner.handle = Some(handle.clone());
        Ok(handle)
    }
    pub(crate) fn setting(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        handle: &SettingsHandle,
        key: &str,
    ) -> Result<Option<Value>> {
        let _lease = self.admit(scope, identity)?;
        self.check_handle(identity, handle)?;
        let mut owners = self.owners.lock().map_err(|_| SettingsError::Storage)?;
        // Validate generation before definitions or owner I/O.
        let owner = owners
            .get_mut(&handle.owner)
            .ok_or(SettingsError::InvalidScope)?;
        if !owner.handle.as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.generation, &handle.generation)
                && handle.revision == owner.document.revision
        }) {
            return Err(SettingsError::InvalidScope);
        }
        let definition = public_definition(&handle.definitions, key)
            .map_err(|_| SettingsError::InvalidSetting)?;
        self.check_disk(owner)?;
        Ok(stored_value(&owner.document.values, definition))
    }
    /// Trusted management only; validates the complete batch before publication.
    /// No-op returns the same generation. Changed values mint a new handle only
    /// after durable success. Rename uncertainty poisons the owner's file.
    pub(crate) fn save(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        handle: &SettingsHandle,
        updates: &BTreeMap<String, Value>,
    ) -> Result<SettingsHandle> {
        let _lease = self.admit(scope, identity)?;
        self.check_handle(identity, handle)?;
        let mut owners = self.owners.lock().map_err(|_| SettingsError::Storage)?;
        // Check stale identity before accessing definitions or disk.
        let owner = owners
            .get_mut(&handle.owner)
            .ok_or(SettingsError::InvalidScope)?;
        if !owner.handle.as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.generation, &handle.generation)
                && handle.revision == owner.document.revision
        }) {
            return Err(SettingsError::InvalidScope);
        }
        validate_updates(&handle.definitions, updates)
            .map_err(|_| SettingsError::InvalidSetting)?;
        if updates.values().any(|value| !valid_json_value(value)) {
            return Err(SettingsError::InvalidSetting);
        }
        self.check_disk(owner)?;
        let mut candidate = owner.document.clone();
        candidate.values.extend(updates.clone());
        if candidate == owner.document {
            return Ok(handle.clone());
        }
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .ok_or(SettingsError::Storage)?;
        self.persist(owner, candidate)?;
        let mut next = handle.clone();
        next.generation = Arc::new(());
        next.revision = owner.document.revision;
        owner.handle = Some(next.clone());
        Ok(next)
    }
}
fn encode(document: &SettingsDocument) -> Result<Vec<u8>> {
    check_values_quota(&document.values).map_err(|error| match error {
        PluginStorageError::QuotaExceeded => SettingsError::QuotaExceeded,
        _ => SettingsError::Storage,
    })?;
    serde_json::to_vec(document).map_err(|_| SettingsError::Storage)
}
fn parse(bytes: &[u8], owner: &str) -> Option<SettingsDocument> {
    validate_mac_json(bytes)?;
    let document: SettingsDocument = serde_json::from_slice(bytes).ok()?;
    if document.schema != 1
        || document.plugin_id != owner
        || document
            .values
            .iter()
            .any(|(key, value)| !valid_setting_key(key) || !valid_json_value(value))
    {
        return None;
    }
    encode(&document).ok()?;
    Some(document)
}
#[cfg(test)]
#[path = "settings_macos_tests.rs"]
mod tests;
