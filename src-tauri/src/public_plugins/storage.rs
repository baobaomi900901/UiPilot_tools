use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::atomic_file::{commit_with_backup, quarantine_invalid, read_optional, AtomicPaths};

use super::{
    authorize_plugin_scope,
    manifest::{valid_plugin_id, valid_storage_key},
    valid_json_value, PluginDataScope,
};

const STORAGE_QUOTA_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginStorageError {
    Storage,
    InvalidPlugin,
    InvalidKey,
    InvalidValue,
    InvalidScope,
    QuotaExceeded,
}

impl fmt::Display for PluginStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Storage => "public plugin private storage failed",
            Self::InvalidPlugin => "public plugin private storage owner is invalid",
            Self::InvalidKey => "public plugin private storage key is invalid",
            Self::InvalidValue => "public plugin private storage value is invalid",
            Self::InvalidScope => "public plugin private storage scope is invalid",
            Self::QuotaExceeded => "public plugin private storage quota exceeded",
        })
    }
}

impl std::error::Error for PluginStorageError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StorageDocument {
    schema: u32,
    plugin_id: String,
    values: BTreeMap<String, Value>,
}

struct StoredDocument {
    document: StorageDocument,
    raw: Vec<u8>,
}

pub(crate) struct PluginStorageStore {
    root: PathBuf,
    state: Mutex<BTreeMap<String, StoredDocument>>,
}

impl PluginStorageStore {
    pub(crate) fn load(root: &Path) -> Result<Self, PluginStorageError> {
        fs::create_dir_all(root).map_err(|_| PluginStorageError::Storage)?;
        if !ordinary_directory(root) {
            return Err(PluginStorageError::Storage);
        }
        let mut state = BTreeMap::new();
        for entry in fs::read_dir(root).map_err(|_| PluginStorageError::Storage)? {
            let entry = entry.map_err(|_| PluginStorageError::Storage)?;
            let plugin_id = entry.file_name().to_string_lossy().into_owned();
            if !valid_plugin_id(&plugin_id) || !ordinary_directory(&entry.path()) {
                continue;
            }
            if let Some(stored) = load_owner(&entry.path(), &plugin_id)? {
                state.insert(plugin_id, stored);
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(state),
        })
    }

    pub(crate) fn get(
        &self,
        scope: &PluginDataScope,
        plugin_id: &str,
        key: &str,
    ) -> Result<Option<Value>, PluginStorageError> {
        validate_access(scope, plugin_id, key)?;
        Ok(self
            .lock()?
            .get(plugin_id)
            .and_then(|stored| stored.document.values.get(key))
            .cloned())
    }

    pub(crate) fn set(
        &self,
        scope: &PluginDataScope,
        plugin_id: &str,
        key: &str,
        value: Value,
    ) -> Result<(), PluginStorageError> {
        validate_access(scope, plugin_id, key)?;
        if !valid_json_value(&value) {
            return Err(PluginStorageError::InvalidValue);
        }
        let mut state = self.lock()?;
        let previous = state.get(plugin_id);
        let mut document = previous.map_or_else(
            || StorageDocument {
                schema: 1,
                plugin_id: plugin_id.into(),
                values: BTreeMap::new(),
            },
            |stored| stored.document.clone(),
        );
        document.values.insert(key.into(), value);
        let stored = self.persist(&document, previous)?;
        state.insert(plugin_id.into(), stored);
        Ok(())
    }

    pub(crate) fn remove(
        &self,
        scope: &PluginDataScope,
        plugin_id: &str,
        key: &str,
    ) -> Result<(), PluginStorageError> {
        validate_access(scope, plugin_id, key)?;
        let mut state = self.lock()?;
        let Some(previous) = state.get(plugin_id) else {
            return Ok(());
        };
        if !previous.document.values.contains_key(key) {
            return Ok(());
        }
        let mut document = previous.document.clone();
        document.values.remove(key);
        let stored = self.persist(&document, Some(previous))?;
        state.insert(plugin_id.into(), stored);
        Ok(())
    }

    pub(crate) fn uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<(), PluginStorageError> {
        if !valid_plugin_id(plugin_id) {
            return Err(PluginStorageError::InvalidPlugin);
        }
        if retain_data {
            return Ok(());
        }
        let mut state = self.lock()?;
        let owner = self.root.join(plugin_id);
        if owner.exists() {
            fs::remove_dir_all(owner).map_err(|_| PluginStorageError::Storage)?;
        }
        state.remove(plugin_id);
        Ok(())
    }

    fn persist(
        &self,
        document: &StorageDocument,
        previous: Option<&StoredDocument>,
    ) -> Result<StoredDocument, PluginStorageError> {
        let values =
            serde_json::to_vec(&document.values).map_err(|_| PluginStorageError::InvalidValue)?;
        if values.len() > STORAGE_QUOTA_BYTES {
            return Err(PluginStorageError::QuotaExceeded);
        }
        let bytes = serde_json::to_vec(document).map_err(|_| PluginStorageError::InvalidValue)?;
        let owner = self.root.join(&document.plugin_id);
        fs::create_dir_all(&owner).map_err(|_| PluginStorageError::Storage)?;
        if !ordinary_directory(&owner) {
            return Err(PluginStorageError::Storage);
        }
        let paths = AtomicPaths::new(&owner, "storage.json");
        commit_with_backup(
            &paths,
            previous.map(|previous| previous.raw.as_slice()),
            &bytes,
        )
        .map_err(|_| PluginStorageError::Storage)?;
        Ok(StoredDocument {
            document: document.clone(),
            raw: bytes,
        })
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, StoredDocument>>, PluginStorageError>
    {
        self.state.lock().map_err(|_| PluginStorageError::Storage)
    }
}

fn validate_access(
    scope: &PluginDataScope,
    plugin_id: &str,
    key: &str,
) -> Result<(), PluginStorageError> {
    authorize_plugin_scope(scope, plugin_id).map_err(|_| PluginStorageError::InvalidScope)?;
    if !valid_plugin_id(plugin_id) {
        return Err(PluginStorageError::InvalidPlugin);
    }
    if !valid_storage_key(key) {
        return Err(PluginStorageError::InvalidKey);
    }
    Ok(())
}

fn load_owner(owner: &Path, plugin_id: &str) -> Result<Option<StoredDocument>, PluginStorageError> {
    let paths = AtomicPaths::new(owner, "storage.json");
    for path in [paths.current(), paths.backup()] {
        let Some(bytes) = read_optional(path).map_err(|_| PluginStorageError::Storage)? else {
            continue;
        };
        if let Some(document) = parse_document(&bytes, plugin_id) {
            return Ok(Some(StoredDocument {
                document,
                raw: bytes,
            }));
        }
        quarantine_invalid(path).map_err(|_| PluginStorageError::Storage)?;
    }
    Ok(None)
}

fn parse_document(bytes: &[u8], plugin_id: &str) -> Option<StorageDocument> {
    let document = serde_json::from_slice::<StorageDocument>(bytes).ok()?;
    let values = serde_json::to_vec(&document.values).ok()?;
    (document.schema == 1
        && document.plugin_id == plugin_id
        && valid_plugin_id(&document.plugin_id)
        && values.len() <= STORAGE_QUOTA_BYTES
        && document
            .values
            .iter()
            .all(|(key, value)| valid_storage_key(key) && valid_json_value(value)))
    .then_some(document)
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
#[path = "storage_tests.rs"]
mod tests;
