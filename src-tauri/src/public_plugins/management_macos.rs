//! Shared installed-inventory authority for trusted management and runtime binding.
//! No source evaluation or OS grant; transport execution occurs outside its lock.
use super::{
    lifecycle_macos::{
        InstalledRequest, InstalledView, LifecycleError, MacPluginLifecycle, PreparedInstall,
        Retirement,
    },
    manifest::{
        PublicActivationMode, PublicManifestV1, PublicOutputMode, PublicPermission, PublicPlatform,
        PublicPluginHost, PublicSettingV1,
    },
    package_macos::{read_archive, read_directory},
    private_file_macos::{ensure_management_root, ensure_private_child},
    settings_values::reconcile_settings,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Storage,
    Stale,
    InvalidPackage,
    Unsupported,
    ConsentRequired,
    InvalidSetting,
    Conflict,
    Capacity,
    CommitUncertain,
    Busy,
}
impl Error {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Storage => "pluginManagementStorage",
            Self::Stale => "pluginManagementStale",
            Self::InvalidPackage => "pluginManagementInvalidPackage",
            Self::Unsupported => "pluginManagementUnsupported",
            Self::ConsentRequired => "pluginManagementConsentRequired",
            Self::InvalidSetting => "pluginManagementInvalidSetting",
            Self::Conflict => "pluginManagementConflict",
            Self::Capacity => "pluginManagementCapacity",
            Self::CommitUncertain => "pluginManagementCommitUncertain",
            Self::Busy => "pluginManagementBusy",
        }
    }
}
impl From<LifecycleError> for Error {
    fn from(error: LifecycleError) -> Self {
        match error {
            LifecycleError::Stale | LifecycleError::NotInstalled => Self::Stale,
            LifecycleError::InvalidPackage => Self::InvalidPackage,
            LifecycleError::ConsentRequired => Self::ConsentRequired,
            LifecycleError::InvalidSetting => Self::InvalidSetting,
            LifecycleError::CommandConflict => Self::Conflict,
            LifecycleError::Capacity => Self::Capacity,
            LifecycleError::CommitUncertain => Self::CommitUncertain,
            _ => Self::Storage,
        }
    }
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionRow {
    pub(crate) permission: PublicPermission,
    pub(crate) granted: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandRow {
    pub(crate) id: String,
    pub(crate) name: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginRow {
    pub(crate) plugin_id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) version: String,
    pub(crate) enabled: bool,
    pub(crate) permissions: Vec<PermissionRow>,
    pub(crate) settings: Vec<PublicSettingV1>,
    pub(crate) values: BTreeMap<String, Value>,
    pub(crate) commands: Vec<CommandRow>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub(crate) revision: String,
    pub(crate) plugins: Vec<PluginRow>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    pub(crate) token: String,
    pub(crate) expected_revision: String,
    pub(crate) plugin: PluginRow,
    pub(crate) required_permissions: Vec<PublicPermission>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SourceKind {
    Archive,
    Directory,
}

pub(crate) fn revision(value: &str) -> Result<u64> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Error::Stale);
    }
    value.parse().map_err(|_| Error::Stale)
}
fn row(
    manifest: &PublicManifestV1,
    enabled: bool,
    grants: &BTreeSet<PublicPermission>,
    values: BTreeMap<String, Value>,
) -> PluginRow {
    PluginRow {
        plugin_id: manifest.plugin_id.clone(),
        name: manifest.name.clone(),
        description: manifest.description.clone(),
        version: manifest.version.clone(),
        enabled,
        permissions: manifest
            .permissions
            .iter()
            .map(|p| PermissionRow {
                permission: *p,
                granted: grants.contains(p),
            })
            .collect(),
        settings: manifest
            .settings
            .iter()
            .filter(|s| !s.is_secret())
            .cloned()
            .collect(),
        values,
        commands: manifest
            .normalized_commands()
            .map(|(id, cmd)| CommandRow {
                id: id.into(),
                name: cmd.default_name.clone(),
            })
            .collect(),
    }
}
fn view(view: InstalledView) -> PluginRow {
    row(&view.manifest, view.enabled, &view.grants, view.settings)
}
struct Pending {
    token: String,
    expires: Instant,
    prepared: PreparedInstall,
}
struct SelectionOwner {
    active: AtomicBool,
}
#[derive(Clone)]
pub(crate) struct Selection {
    owner: Arc<SelectionOwner>,
    epoch: Arc<()>,
    revision: u64,
    kind: SourceKind,
}
#[derive(Clone)]
pub(crate) struct RuntimeCommand {
    pub(crate) plugin_id: String,
    pub(crate) command_id: String,
    pub(crate) name: String,
    pub(crate) mode: PublicActivationMode,
    pub(crate) input_required: bool,
    pub(crate) multi: bool,
    pub(crate) revision: String,
}
pub(crate) trait RuntimeInvalidation: Send + Sync {
    fn invalidate_runtime(&self, retired: &ManagementEpoch);
}

/// One authority belongs to the trusted main WebView. Its mutex serializes
/// inventory mutations and request binding, never transport execution or a chooser.
pub(crate) struct Management {
    epoch: Arc<()>,
    host: MacPluginLifecycle,
    appdata: PathBuf,
    runtime_catalog: Vec<RuntimeCommand>,
    runtime_owner: Option<Arc<()>>,
    pending: Option<Pending>,
    selection: Option<Selection>,
}
impl Management {
    fn load(appdata: &Path) -> Result<Self> {
        let base = ensure_management_root(appdata).map_err(|_| Error::Storage)?;
        let inventory = ensure_private_child(&base, "inventory").map_err(|_| Error::Storage)?;
        let storage = ensure_private_child(&base, "storage").map_err(|_| Error::Storage)?;
        let settings = ensure_private_child(&base, "settings").map_err(|_| Error::Storage)?;
        Ok(Self {
            epoch: Arc::new(()),
            host: MacPluginLifecycle::load(&inventory, &storage, &settings)?,
            appdata: appdata.to_owned(),
            runtime_catalog: Vec::new(),
            runtime_owner: None,
            pending: None,
            selection: None,
        })
    }
    pub(crate) fn list(&mut self) -> Result<Snapshot> {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.expires)
        {
            self.pending = None;
        }
        let (revision, rows) = self.host.management_snapshot()?;
        self.runtime_catalog = rows
            .iter()
            .filter(|row| row.enabled)
            .flat_map(|row| {
                row.manifest
                    .normalized_commands()
                    .filter(|(_, c)| c.output_mode == PublicOutputMode::MainResult)
                    .map(|(id, c)| RuntimeCommand {
                        plugin_id: row.manifest.plugin_id.clone(),
                        command_id: id.into(),
                        name: c.default_name.clone(),
                        mode: c.activation_mode,
                        input_required: c.input_required,
                        multi: row.manifest.is_multi_command(),
                        revision: revision.to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Ok(Snapshot {
            revision: revision.to_string(),
            plugins: rows.into_iter().map(view).collect(),
        })
    }
    fn expected(&mut self, expected: &str) -> Result<Snapshot> {
        let expected = revision(expected)?;
        let snapshot = self.list()?;
        if revision(&snapshot.revision)? != expected {
            return Err(Error::Stale);
        }
        Ok(snapshot)
    }
    pub(crate) fn reserve(&mut self, kind: SourceKind, expected: &str) -> Result<Selection> {
        let current = self.expected(expected)?;
        if self.selection.is_some() {
            return Err(Error::Busy);
        }
        self.pending = None;
        let selection = Selection {
            owner: Arc::new(SelectionOwner {
                active: AtomicBool::new(true),
            }),
            epoch: self.epoch.clone(),
            revision: revision(&current.revision)?,
            kind,
        };
        self.selection = Some(selection.clone());
        Ok(selection)
    }
    pub(crate) fn selected(
        &mut self,
        selection: Selection,
        path: Option<&Path>,
    ) -> Result<Option<Preview>> {
        if !self
            .selection
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(&s.owner, &selection.owner))
        {
            return Err(Error::Stale);
        }
        self.selection = None;
        selection.owner.active.store(false, Ordering::Release);
        let current = self.expected(&selection.revision.to_string())?;
        let Some(path) = path else {
            return Ok(None);
        };
        let host = PublicPluginHost::current(PublicPlatform::Macos);
        let package = match selection.kind {
            SourceKind::Archive => read_archive(path, &host),
            SourceKind::Directory => read_directory(path, &host),
        }
        .map_err(|error| match error {
            super::manifest::PublicPackageError::InvalidPackage => Error::InvalidPackage,
            _ => Error::Unsupported,
        })?;
        let prepared = self.host.prepare_install(package)?;
        let manifest = prepared.package().manifest();
        let old = current
            .plugins
            .iter()
            .find(|p| p.plugin_id == manifest.plugin_id);
        let grants = old
            .map(|p| {
                p.permissions
                    .iter()
                    .filter(|p| p.granted)
                    .map(|p| p.permission)
                    .collect()
            })
            .unwrap_or_default();
        let values = reconcile_settings(old.map(|p| &p.values), &manifest.settings);
        let plugin = row(manifest, old.is_none_or(|p| p.enabled), &grants, values);
        let mut random = [0u8; 32];
        // macOS public libc CSPRNG, no file path or user state access.
        if unsafe { libc::getentropy(random.as_mut_ptr().cast(), random.len()) } != 0 {
            return Err(Error::Storage);
        }
        let token = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let preview = Preview {
            token: token.clone(),
            expected_revision: current.revision,
            plugin,
            required_permissions: prepared.required_permissions().iter().copied().collect(),
        };
        self.pending = Some(Pending {
            token,
            expires: Instant::now() + Duration::from_secs(300),
            prepared,
        });
        Ok(Some(preview))
    }
    pub(crate) fn cancel(&mut self, token: &str) -> Result<()> {
        if !self.pending.as_ref().is_some_and(|p| p.token == token) {
            return Err(Error::Stale);
        }
        self.pending = None;
        Ok(())
    }
    pub(crate) fn commit(
        &mut self,
        token: &str,
        expected: &str,
        approved: Vec<PublicPermission>,
    ) -> Result<Snapshot> {
        if !self.pending.as_ref().is_some_and(|p| p.token == token) {
            return Err(Error::Stale);
        }
        // Consume even a failed matching attempt. An unknown token cannot consume
        // a different pending installation.
        let pending = self.pending.take().unwrap();
        if Instant::now() >= pending.expires {
            return Err(Error::Stale);
        }
        self.expected(expected)?;
        let unique: BTreeSet<_> = approved.iter().copied().collect();
        if unique.len() != approved.len() {
            return Err(Error::ConsentRequired);
        }
        self.host.commit_install(pending.prepared, &unique)?;
        self.list()
    }
    pub(crate) fn enabled(&mut self, id: &str, enabled: bool, expected: &str) -> Result<Snapshot> {
        self.expected(expected)?;
        self.host.set_enabled(id, enabled)?;
        self.list()
    }
    pub(crate) fn permission(
        &mut self,
        id: &str,
        permission: PublicPermission,
        granted: bool,
        expected: &str,
    ) -> Result<Snapshot> {
        self.expected(expected)?;
        self.host.set_permission(id, permission, granted)?;
        self.list()
    }
    pub(crate) fn settings(
        &mut self,
        id: &str,
        updates: &BTreeMap<String, Value>,
        expected: &str,
    ) -> Result<Snapshot> {
        self.expected(expected)?;
        self.host.save_settings(id, updates)?;
        self.list()
    }
    pub(crate) fn remove(&mut self, id: &str, expected: &str) -> Result<Snapshot> {
        self.host.remove_expected(id, revision(expected)?)?;
        self.list()
    }
    fn reset(&mut self) {
        if let Some(selection) = self.selection.take() {
            selection.owner.active.store(false, Ordering::Release);
        }
        self.pending = None;
    }
}
/// Default has no paths, files or startup side effects. Load failure is local to
/// the management command and can be retried; it never prevents application boot.
#[derive(Clone)]
pub(crate) struct ManagementEpoch(Arc<()>);
impl ManagementEpoch {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
#[derive(Default)]
pub(crate) struct ManagementState {
    inner: Mutex<Option<Management>>,
    // Short ownership lock, never held over the management mutex or any IO.
    epoch: Mutex<Arc<()>>,
    runtime_hook: Mutex<Option<Weak<dyn RuntimeInvalidation>>>,
    #[cfg(test)]
    pub(crate) before_reset_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl ManagementState {
    pub(crate) fn epoch(&self) -> Result<ManagementEpoch> {
        Ok(ManagementEpoch(
            self.epoch.lock().map_err(|_| Error::Storage)?.clone(),
        ))
    }
    fn current_epoch(&self, epoch: &Arc<()>) -> bool {
        self.epoch
            .lock()
            .ok()
            .is_some_and(|e| Arc::ptr_eq(&e, epoch))
    }
    pub(crate) fn with<T>(
        &self,
        appdata: &Path,
        operation: impl FnOnce(&mut Management) -> Result<T>,
    ) -> Result<T> {
        self.with_epoch(appdata, self.epoch()?, operation)
    }
    pub(crate) fn with_epoch<T>(
        &self,
        appdata: &Path,
        epoch: ManagementEpoch,
        operation: impl FnOnce(&mut Management) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.inner.lock().map_err(|_| Error::Storage)?;
        if !self.current_epoch(&epoch.0) {
            return Err(Error::Stale);
        }
        if state.is_none() {
            *state = Some(Management::load(appdata)?);
        }
        let manager = state.as_mut().unwrap();
        if !Arc::ptr_eq(&manager.epoch, &epoch.0) {
            manager.reset();
            manager.epoch = epoch.0.clone();
        }
        if !self.current_epoch(&epoch.0) {
            manager.reset();
            return Err(Error::Stale);
        }
        // A close can retire ownership while an already-admitted disk operation
        // finishes. It never waits on this IO lock and never claims disk rollback.
        let result = operation(manager);
        let retired = !self.current_epoch(&epoch.0);
        if matches!(&result, Err(Error::Storage | Error::CommitUncertain)) {
            // Never retry a mutation. Release the poisoned controller/locks and
            // discard consent; the next explicit command may strictly reload
            // the actual durable document, including a completed uncertain write.
            manager.reset();
            state.take();
            if let Ok(mut current) = self.epoch.lock() {
                if Arc::ptr_eq(&current, &epoch.0) {
                    *current = Arc::new(());
                }
            }
            // Preserve the material uncertainty even if a window close raced it.
            return result;
        }
        if retired {
            manager.reset();
            return Err(Error::Stale);
        }
        result
    }
    pub(crate) fn selection_current(&self, selection: &Selection) -> bool {
        self.current_epoch(&selection.epoch) && selection.owner.active.load(Ordering::Acquire)
    }
    pub(crate) fn reset(&self) {
        let retired = match self.epoch.lock() {
            Ok(mut epoch) => ManagementEpoch(std::mem::replace(&mut *epoch, Arc::new(()))),
            Err(_) => return,
        };
        #[cfg(test)]
        {
            let hook = self.before_reset_hook.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        let hook = self
            .runtime_hook
            .lock()
            .ok()
            .and_then(|h| h.as_ref().and_then(Weak::upgrade));
        if let Some(hook) = hook {
            hook.invalidate_runtime(&retired);
        }
    }
    pub(crate) fn register_runtime(&self, hook: Weak<dyn RuntimeInvalidation>) -> bool {
        let Ok(mut current) = self.runtime_hook.lock() else {
            return false;
        };
        if current.as_ref().and_then(Weak::upgrade).is_some() {
            return false;
        }
        *current = Some(hook);
        true
    }
    pub(crate) fn runtime_epoch_current(&self, epoch: &ManagementEpoch) -> bool {
        self.current_epoch(&epoch.0)
    }
    /// Only an available, explicitly routed command reaches this lazy inventory
    /// read. Revalidate persisted snapshots; never follow original source paths.
    pub(crate) fn runtime_command(
        &self,
        appdata: &Path,
        epoch: &ManagementEpoch,
        name: &str,
    ) -> Result<Option<RuntimeCommand>> {
        self.with_epoch(appdata, epoch.clone(), |manager| {
            manager.list()?;
            Ok(manager
                .runtime_catalog
                .iter()
                .find(|c| c.name == name)
                .cloned())
        })
    }
    pub(crate) fn open_runtime(
        &self,
        appdata: &Path,
        epoch: &ManagementEpoch,
        command: &RuntimeCommand,
        owner: Arc<()>,
        request: &str,
        deadline: Instant,
        before_open: impl FnOnce() -> bool,
        attach: impl FnOnce(&InstalledRequest) -> std::result::Result<Retirement, LifecycleError>,
    ) -> Result<InstalledRequest> {
        self.with_epoch(appdata, epoch.clone(), |manager| {
            if manager.appdata != appdata
                || !manager.runtime_catalog.iter().any(|c| {
                    c.plugin_id == command.plugin_id
                        && c.command_id == command.command_id
                        && c.revision == command.revision
                })
            {
                return Err(Error::Stale);
            }
            // An older query may have waited for this IO lock while a newer
            // request opened. Check BEFORE lifecycle.open_request closes anyone.
            if !before_open() {
                return Err(Error::Stale);
            }
            let installed = manager.host.open_request(
                &command.plugin_id,
                &command.command_id,
                request,
                deadline,
                attach,
            )?;
            manager.runtime_owner = Some(owner);
            Ok(installed)
        })
    }
    /// Background close touches only an already-loaded, captured request. A late
    /// cleanup cannot close a newer owner and never lazily creates a directory.
    pub(crate) fn close_runtime(&self, owner: &Arc<()>) -> Result<()> {
        let mut state = self.inner.lock().map_err(|_| Error::Storage)?;
        if let Some(manager) = state.as_mut() {
            if manager
                .runtime_owner
                .as_ref()
                .is_some_and(|id| Arc::ptr_eq(id, owner))
            {
                manager.host.close()?;
                manager.runtime_owner = None;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "management_macos_tests.rs"]
mod tests;
