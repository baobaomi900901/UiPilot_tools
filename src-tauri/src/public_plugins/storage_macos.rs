//! Internal macOS private JSON store. Scope isolates an owner, while the real
//! data gate authorizes the entire admitted operation, including durable I/O.
//! No gateway or production plugin host is installed by this module.
pub(crate) use super::storage_document::PluginStorageError;
use super::{
    data_call_gate::{PluginDataCallGate, PluginDataCallIdentity, PluginDataCallLease},
    private_file_macos::{PrivateFile, PrivateRoot},
    storage_document::{encode_document, parse_mac_document, validate_access, StorageDocument},
    valid_json_value, PluginDataScope,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
struct Owner {
    file: PrivateFile,
    document: StorageDocument,
    raw: Option<Vec<u8>>,
}
pub(crate) struct PluginStorageStore {
    root: PrivateRoot,
    gate: Arc<PluginDataCallGate>,
    owners: Mutex<BTreeMap<String, Owner>>,
}
impl PluginStorageStore {
    pub(crate) fn load(
        root: &Path,
        gate: Arc<PluginDataCallGate>,
    ) -> Result<Self, PluginStorageError> {
        Ok(Self {
            root: PrivateRoot::open(root).map_err(|_| PluginStorageError::Storage)?,
            gate,
            owners: Mutex::new(BTreeMap::new()),
        })
    }
    fn admit(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        plugin_id: &str,
        key: &str,
    ) -> Result<PluginDataCallLease, PluginStorageError> {
        validate_access(scope, plugin_id, key)?;
        if identity.plugin_id() != plugin_id {
            return Err(PluginStorageError::InvalidScope);
        }
        self.gate
            .try_acquire(identity)
            .map_err(|_| PluginStorageError::InvalidScope)
    }
    fn owner<'a>(
        &self,
        owners: &'a mut BTreeMap<String, Owner>,
        id: &str,
        create: bool,
    ) -> Result<Option<&'a mut Owner>, PluginStorageError> {
        self.root
            .validate()
            .map_err(|_| PluginStorageError::Storage)?;
        if !owners.contains_key(id) {
            let Some(file) = PrivateFile::open(&self.root, id, create)
                .map_err(|_| PluginStorageError::Storage)?
            else {
                return Ok(None);
            };
            let raw = file
                .read(&self.root)
                .map_err(|_| PluginStorageError::Storage)?;
            // A corrupt current file never becomes an empty store or backup.
            let document = match &raw {
                Some(bytes) => parse_mac_document(bytes, id).ok_or(PluginStorageError::Storage)?,
                None => StorageDocument {
                    schema: 1,
                    plugin_id: id.into(),
                    values: BTreeMap::new(),
                },
            };
            owners.insert(
                id.into(),
                Owner {
                    file,
                    raw,
                    document,
                },
            );
        }
        let owner = owners.get_mut(id).ok_or(PluginStorageError::Storage)?;
        if owner
            .file
            .read(&self.root)
            .map_err(|_| PluginStorageError::Storage)?
            != owner.raw
        {
            return Err(PluginStorageError::Storage);
        }
        Ok(Some(owner))
    }
    pub(crate) fn get(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        id: &str,
        key: &str,
    ) -> Result<Option<Value>, PluginStorageError> {
        let _lease = self.admit(scope, identity, id, key)?;
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| PluginStorageError::Storage)?;
        Ok(self
            .owner(&mut owners, id, false)?
            .and_then(|owner| owner.document.values.get(key).cloned()))
    }
    pub(crate) fn set(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        id: &str,
        key: &str,
        value: Value,
    ) -> Result<(), PluginStorageError> {
        let _lease = self.admit(scope, identity, id, key)?;
        if !valid_json_value(&value) {
            return Err(PluginStorageError::InvalidValue);
        }
        // Reject quota overflow before owner creation. Existing documents are
        // checked again after the value is combined with their other keys.
        let mut single = StorageDocument {
            schema: 1,
            plugin_id: id.into(),
            values: BTreeMap::new(),
        };
        single.values.insert(key.into(), value.clone());
        encode_document(&single)?;
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| PluginStorageError::Storage)?;
        let owner = self
            .owner(&mut owners, id, true)?
            .ok_or(PluginStorageError::Storage)?;
        let mut candidate = owner.document.clone();
        candidate.values.insert(key.into(), value);
        self.commit(owner, candidate)
    }
    pub(crate) fn remove(
        &self,
        scope: &PluginDataScope,
        identity: &PluginDataCallIdentity,
        id: &str,
        key: &str,
    ) -> Result<(), PluginStorageError> {
        let _lease = self.admit(scope, identity, id, key)?;
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| PluginStorageError::Storage)?;
        let Some(owner) = self.owner(&mut owners, id, false)? else {
            return Ok(());
        };
        if !owner.document.values.contains_key(key) {
            return Ok(());
        }
        let mut candidate = owner.document.clone();
        candidate.values.remove(key);
        self.commit(owner, candidate)
    }
    fn commit(
        &self,
        owner: &mut Owner,
        candidate: StorageDocument,
    ) -> Result<(), PluginStorageError> {
        let raw = encode_document(&candidate)?;
        // CommitUncertain maps to the existing fixed Storage error. PrivateFile
        // stays poisoned: do not retry or report rollback; drop/reopen to inspect.
        owner
            .file
            .commit(&self.root, owner.raw.as_deref(), &raw)
            .map_err(|_| PluginStorageError::Storage)?;
        owner.raw = Some(raw);
        owner.document = candidate;
        Ok(())
    }
}
#[cfg(test)]
#[path = "storage_macos_tests.rs"]
mod tests;
