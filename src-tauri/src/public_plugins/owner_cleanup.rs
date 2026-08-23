use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};

use crate::atomic_file::{commit_with_backup, quarantine_invalid, read_optional, AtomicPaths};

use super::manifest::valid_plugin_id;
use crate::settings::SettingsStore;

pub(super) const RECEIPTS_FILE_NAME: &str = "receipts.json";
const RECEIPTS_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PluginOwnerCleanupReceipt {
    schema: u32,
    pub(super) plugin_id: String,
    pub(super) transaction_id: String,
    pub(super) generation: u64,
    pub(super) activation_id: Option<u64>,
}

impl PluginOwnerCleanupReceipt {
    pub(super) fn new(
        plugin_id: &str,
        transaction_id: &str,
        generation: u64,
        activation_id: Option<u64>,
    ) -> Result<Self, OwnerCleanupError> {
        if !valid_plugin_id(plugin_id)
            || !valid_transaction_id(transaction_id)
            || generation == 0
            || activation_id.is_some_and(|value| value == 0)
        {
            return Err(OwnerCleanupError::InvalidIdentity);
        }
        Ok(Self {
            schema: RECEIPTS_SCHEMA,
            plugin_id: plugin_id.into(),
            transaction_id: transaction_id.into(),
            generation,
            activation_id,
        })
    }

    fn valid(&self) -> bool {
        self.schema == RECEIPTS_SCHEMA
            && valid_plugin_id(&self.plugin_id)
            && valid_transaction_id(&self.transaction_id)
            && self.generation != 0
            && self.activation_id.is_none_or(|value| value != 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OwnerCleanupError {
    InvalidIdentity,
    AlreadyPending,
    Storage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiptDocument {
    schema: u32,
    receipts: BTreeMap<String, PluginOwnerCleanupReceipt>,
}

impl Default for ReceiptDocument {
    fn default() -> Self {
        Self {
            schema: RECEIPTS_SCHEMA,
            receipts: BTreeMap::new(),
        }
    }
}

impl ReceiptDocument {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let document = serde_json::from_slice::<Self>(bytes).ok()?;
        (document.schema == RECEIPTS_SCHEMA
            && document
                .receipts
                .iter()
                .all(|(plugin_id, receipt)| plugin_id == &receipt.plugin_id && receipt.valid()))
        .then_some(document)
    }
}

struct ReceiptState {
    document: ReceiptDocument,
    raw: Option<Vec<u8>>,
}

pub(super) struct PluginOwnerCleanupStore {
    paths: AtomicPaths,
    state: Mutex<ReceiptState>,
}

impl PluginOwnerCleanupStore {
    pub(super) fn load(root: &Path) -> Result<Self, OwnerCleanupError> {
        fs::create_dir_all(root).map_err(|_| OwnerCleanupError::Storage)?;
        if !ordinary_directory(root) {
            return Err(OwnerCleanupError::Storage);
        }
        let paths = AtomicPaths::new(root, RECEIPTS_FILE_NAME);
        let current = read_optional(paths.current()).map_err(|_| OwnerCleanupError::Storage)?;
        let backup = read_optional(paths.backup()).map_err(|_| OwnerCleanupError::Storage)?;
        let (document, raw) = match current {
            Some(bytes) => match ReceiptDocument::parse(&bytes) {
                Some(document) => (document, Some(bytes)),
                None => {
                    let backup = backup
                        .and_then(|bytes| {
                            ReceiptDocument::parse(&bytes).map(|value| (value, bytes))
                        })
                        .ok_or(OwnerCleanupError::Storage)?;
                    quarantine_invalid(paths.current()).map_err(|_| OwnerCleanupError::Storage)?;
                    (backup.0, Some(backup.1))
                }
            },
            None => match backup {
                Some(bytes) => ReceiptDocument::parse(&bytes)
                    .map(|document| (document, Some(bytes)))
                    .ok_or(OwnerCleanupError::Storage)?,
                None => (ReceiptDocument::default(), None),
            },
        };
        Ok(Self {
            paths,
            state: Mutex::new(ReceiptState { document, raw }),
        })
    }

    pub(super) fn commit(
        &self,
        receipt: PluginOwnerCleanupReceipt,
    ) -> Result<(), OwnerCleanupError> {
        if !receipt.valid() {
            return Err(OwnerCleanupError::InvalidIdentity);
        }
        let mut state = self.lock()?;
        if let Some(current) = state.document.receipts.get(&receipt.plugin_id) {
            if current != &receipt {
                return Err(OwnerCleanupError::AlreadyPending);
            }
        }
        let mut candidate = state.document.clone();
        candidate
            .receipts
            .insert(receipt.plugin_id.clone(), receipt);
        self.persist(&mut state, candidate)
    }

    pub(super) fn clear(
        &self,
        plugin_id: &str,
        transaction_id: &str,
    ) -> Result<bool, OwnerCleanupError> {
        if !valid_plugin_id(plugin_id) || !valid_transaction_id(transaction_id) {
            return Err(OwnerCleanupError::InvalidIdentity);
        }
        let mut state = self.lock()?;
        let matches = state
            .document
            .receipts
            .get(plugin_id)
            .is_some_and(|receipt| receipt.transaction_id == transaction_id);
        if !matches {
            return Ok(false);
        }
        let mut candidate = state.document.clone();
        candidate.receipts.remove(plugin_id);
        self.persist(&mut state, candidate)?;
        Ok(true)
    }

    pub(super) fn is_blocked(&self, plugin_id: &str) -> Result<bool, OwnerCleanupError> {
        if !valid_plugin_id(plugin_id) {
            return Err(OwnerCleanupError::InvalidIdentity);
        }
        Ok(self.lock()?.document.receipts.contains_key(plugin_id))
    }

    #[cfg(test)]
    pub(super) fn receipt(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginOwnerCleanupReceipt>, OwnerCleanupError> {
        if !valid_plugin_id(plugin_id) {
            return Err(OwnerCleanupError::InvalidIdentity);
        }
        Ok(self.lock()?.document.receipts.get(plugin_id).cloned())
    }

    pub(super) fn pending(&self) -> Result<Vec<PluginOwnerCleanupReceipt>, OwnerCleanupError> {
        Ok(self.lock()?.document.receipts.values().cloned().collect())
    }

    fn persist(
        &self,
        state: &mut ReceiptState,
        candidate: ReceiptDocument,
    ) -> Result<(), OwnerCleanupError> {
        let bytes = serde_json::to_vec(&candidate).map_err(|_| OwnerCleanupError::Storage)?;
        commit_with_backup(&self.paths, state.raw.as_deref(), &bytes)
            .map_err(|_| OwnerCleanupError::Storage)?;
        state.document = candidate;
        state.raw = Some(bytes);
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, ReceiptState>, OwnerCleanupError> {
        self.state.lock().map_err(|_| OwnerCleanupError::Storage)
    }
}

pub(super) fn retry_pending_owner_cleanup(
    app_data_dir: &Path,
    settings: &SettingsStore,
) -> Result<(), OwnerCleanupError> {
    let store = PluginOwnerCleanupStore::load(&app_data_dir.join("public-plugin-owner-cleanup"))?;
    let mut failed = false;
    for receipt in store.pending()? {
        if cleanup_fixed_owners(app_data_dir, settings, &receipt).is_err() {
            failed = true;
            continue;
        }
        if !store.clear(&receipt.plugin_id, &receipt.transaction_id)? {
            failed = true;
        }
    }
    if failed {
        Err(OwnerCleanupError::Storage)
    } else {
        Ok(())
    }
}

fn cleanup_fixed_owners(
    app_data_dir: &Path,
    settings: &SettingsStore,
    receipt: &PluginOwnerCleanupReceipt,
) -> Result<(), OwnerCleanupError> {
    if !receipt.valid() {
        return Err(OwnerCleanupError::InvalidIdentity);
    }
    let public_root = app_data_dir.join("public-plugins");
    for owner in ["storage", "secrets", "state", "packages"] {
        remove_owned_directory(&public_root.join(owner).join(&receipt.plugin_id))?;
    }
    settings
        .remove_plugin_window_position(&receipt.plugin_id)
        .map_err(|_| OwnerCleanupError::Storage)
}

pub(super) fn remove_owned_directory(path: &Path) -> Result<(), OwnerCleanupError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(OwnerCleanupError::Storage),
    };
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(OwnerCleanupError::Storage);
    }
    make_tree_writable(path)?;
    fs::remove_dir_all(path).map_err(|_| OwnerCleanupError::Storage)
}

fn make_tree_writable(path: &Path) -> Result<(), OwnerCleanupError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| OwnerCleanupError::Storage)?;
    if is_reparse_point(&metadata) {
        return Err(OwnerCleanupError::Storage);
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| OwnerCleanupError::Storage)? {
            make_tree_writable(&entry.map_err(|_| OwnerCleanupError::Storage)?.path())?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(OwnerCleanupError::Storage);
    }
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).map_err(|_| OwnerCleanupError::Storage)?;
    }
    Ok(())
}

fn valid_transaction_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        retry_pending_owner_cleanup, OwnerCleanupError, PluginOwnerCleanupReceipt,
        PluginOwnerCleanupStore, RECEIPTS_FILE_NAME,
    };
    use crate::settings::{SettingsStore, WindowPosition};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-public-owner-cleanup-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn receipt_for(plugin_id: &str, transaction_id: &str) -> PluginOwnerCleanupReceipt {
        PluginOwnerCleanupReceipt::new(plugin_id, transaction_id, 7, Some(11)).unwrap()
    }

    #[test]
    fn committed_receipt_blocks_after_reload_until_matching_clear() {
        let dir = TestDir::new("reload");
        let store = PluginOwnerCleanupStore::load(dir.path()).unwrap();
        store
            .commit(receipt_for(
                "com.example.cleanup",
                "00000000000000000000000000000001",
            ))
            .unwrap();

        let reloaded = PluginOwnerCleanupStore::load(dir.path()).unwrap();
        assert!(reloaded.is_blocked("com.example.cleanup").unwrap());
        assert_eq!(
            reloaded
                .receipt("com.example.cleanup")
                .unwrap()
                .unwrap()
                .generation,
            7
        );

        assert!(!reloaded
            .clear("com.example.cleanup", "ffffffffffffffffffffffffffffffff")
            .unwrap());
        assert!(reloaded.is_blocked("com.example.cleanup").unwrap());
        assert!(reloaded
            .clear("com.example.cleanup", "00000000000000000000000000000001")
            .unwrap());
        assert!(!PluginOwnerCleanupStore::load(dir.path())
            .unwrap()
            .is_blocked("com.example.cleanup")
            .unwrap());
    }

    #[test]
    fn corrupt_current_uses_valid_backup_without_unblocking_owner() {
        let dir = TestDir::new("backup");
        let store = PluginOwnerCleanupStore::load(dir.path()).unwrap();
        store
            .commit(receipt_for(
                "com.example.cleanup",
                "00000000000000000000000000000001",
            ))
            .unwrap();
        store
            .commit(receipt_for(
                "com.example.second",
                "00000000000000000000000000000002",
            ))
            .unwrap();
        fs::write(dir.path().join(RECEIPTS_FILE_NAME), b"not-json").unwrap();

        let recovered = PluginOwnerCleanupStore::load(dir.path()).unwrap();
        let pending = recovered.receipt("com.example.cleanup").unwrap().unwrap();
        assert_eq!(pending.transaction_id, "00000000000000000000000000000001");
        assert!(recovered.is_blocked("com.example.cleanup").unwrap());
    }

    #[test]
    fn malformed_identity_is_rejected_without_persisting() {
        let dir = TestDir::new("identity");
        let store = PluginOwnerCleanupStore::load(dir.path()).unwrap();
        assert_eq!(
            PluginOwnerCleanupReceipt::new("../escape", "bad", 0, Some(0)),
            Err(OwnerCleanupError::InvalidIdentity)
        );
        assert!(store.pending().unwrap().is_empty());
    }

    #[test]
    fn startup_retry_removes_every_fixed_owner_and_clears_receipt_last() {
        let dir = TestDir::new("cleanup-targets");
        let settings = SettingsStore::load(dir.path()).unwrap();
        settings
            .set_plugin_window_position("com.example.cleanup", WindowPosition { x: 12, y: 34 })
            .unwrap();
        for owner in ["storage", "secrets", "state", "packages"] {
            let path = dir
                .path()
                .join("public-plugins")
                .join(owner)
                .join("com.example.cleanup");
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("owned"), b"value").unwrap();
        }
        let store =
            PluginOwnerCleanupStore::load(&dir.path().join("public-plugin-owner-cleanup")).unwrap();
        store
            .commit(receipt_for(
                "com.example.cleanup",
                "00000000000000000000000000000001",
            ))
            .unwrap();

        retry_pending_owner_cleanup(dir.path(), &settings).unwrap();

        for owner in ["storage", "secrets", "state", "packages"] {
            assert!(!dir
                .path()
                .join("public-plugins")
                .join(owner)
                .join("com.example.cleanup")
                .exists());
        }
        assert_eq!(settings.plugin_window_position("com.example.cleanup"), None);
        assert!(
            !PluginOwnerCleanupStore::load(&dir.path().join("public-plugin-owner-cleanup"))
                .unwrap()
                .is_blocked("com.example.cleanup")
                .unwrap()
        );
    }

    #[test]
    fn failed_target_keeps_receipt_for_later_retry() {
        let dir = TestDir::new("cleanup-failure");
        let settings = SettingsStore::load(dir.path()).unwrap();
        let package_owner = dir
            .path()
            .join("public-plugins")
            .join("packages")
            .join("com.example.cleanup");
        fs::create_dir_all(package_owner.parent().unwrap()).unwrap();
        fs::write(&package_owner, b"not-a-directory").unwrap();
        let store =
            PluginOwnerCleanupStore::load(&dir.path().join("public-plugin-owner-cleanup")).unwrap();
        store
            .commit(receipt_for(
                "com.example.cleanup",
                "00000000000000000000000000000001",
            ))
            .unwrap();

        assert_eq!(
            retry_pending_owner_cleanup(dir.path(), &settings),
            Err(OwnerCleanupError::Storage)
        );
        assert!(
            PluginOwnerCleanupStore::load(&dir.path().join("public-plugin-owner-cleanup"))
                .unwrap()
                .is_blocked("com.example.cleanup")
                .unwrap()
        );
    }
}
