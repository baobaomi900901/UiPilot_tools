//! Internal installed-package lifecycle. No production loader, RPC or OS grants.
//! One atomic inventory is authoritative for code, settings and consent.
use super::{
    manifest::{PublicPermission, PublicPlatform, PublicPluginHost},
    package_macos::{restore_snapshot, VerifiedPackage},
    private_file_macos::{FileError, FileKind, PrivateFile, PrivateRoot},
    request_contract::PluginRequestContext,
    runtime_data_macos::{DataReply, DataRuntimeError, RuntimeDataBinding, RuntimeDataHost},
    settings_values::{reconcile_settings, validate_updates},
    storage_document::{validate_mac_json, STORAGE_QUOTA_BYTES},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const OWNER: &str = "com.uipilot.inventory";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleError {
    InvalidPackage,
    InvalidState,
    Stale,
    ConsentRequired,
    NotInstalled,
    Disabled,
    InvalidSetting,
    InvalidCommand,
    CommandConflict,
    Capacity,
    Storage,
    CommitUncertain,
    Runtime,
}
pub(crate) type Retirement = Box<dyn FnOnce() -> std::result::Result<(), DataRuntimeError> + Send>;
type Result<T> = std::result::Result<T, LifecycleError>;
/// Host preferences are keyed by immutable command ID, never display name.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandPreference {
    pub(crate) name_override: Option<String>,
    pub(crate) favorite: bool,
}
impl CommandPreference {
    pub(crate) fn name<'a>(&'a self, default: &'a str) -> &'a str {
        self.name_override.as_deref().unwrap_or(default)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstalledDocument {
    package: Value,
    enabled: bool,
    grants: BTreeSet<PublicPermission>,
    generation: u64,
    settings: BTreeMap<String, Value>,
    #[serde(default)]
    commands: BTreeMap<String, CommandPreference>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Inventory {
    schema: u32,
    revision: u64,
    next_generation: u64,
    plugins: BTreeMap<String, InstalledDocument>,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            schema: 1,
            revision: 0,
            next_generation: 0,
            plugins: BTreeMap::new(),
        }
    }
}
/// Host-issued, single-consumption consent token. Never deserialized from RPC.
pub(crate) struct PreparedInstall {
    owner: Arc<()>,
    revision: u64,
    expires: Instant,
    package: VerifiedPackage,
    required: BTreeSet<PublicPermission>,
}
impl PreparedInstall {
    pub(crate) fn required_permissions(&self) -> &BTreeSet<PublicPermission> {
        &self.required
    }
    pub(crate) fn package(&self) -> &VerifiedPackage {
        &self.package
    }
}
#[derive(Clone)]
pub(crate) struct InstalledRequest {
    binding: RuntimeDataBinding,
    package: VerifiedPackage,
    command_id: String,
    grants: BTreeSet<PublicPermission>,
}
impl InstalledRequest {
    pub(crate) fn binding(&self) -> &RuntimeDataBinding {
        &self.binding
    }
    pub(crate) fn package(&self) -> &VerifiedPackage {
        &self.package
    }
    pub(crate) fn command_id(&self) -> &str {
        &self.command_id
    }
    pub(crate) fn clipboard_write_allowed(&self) -> bool {
        self.grants.contains(&PublicPermission::ClipboardWrite)
            && self
                .package
                .manifest()
                .permissions
                .contains(&PublicPermission::ClipboardWrite)
    }
    /// Internal Host admission only. The trusted effect is never supplied by a
    /// child. This does not implement a clipboard service or enable copy rows.
    pub(crate) fn permission_call(
        &self,
        context: &PluginRequestContext,
        call_id: u64,
        permission: PublicPermission,
        effect: impl FnOnce() -> std::result::Result<Value, DataRuntimeError>,
    ) -> std::result::Result<DataReply, DataRuntimeError> {
        let call = self
            .binding
            .session()
            .begin_call(self.binding.endpoint(), context, call_id)
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        let result = if self.grants.contains(&permission)
            && permission == PublicPermission::ClipboardWrite
        {
            effect()
        } else {
            Err(DataRuntimeError::InvalidOperation)
        };
        let permit = self
            .binding
            .session()
            .begin_reply(self.binding.endpoint(), call)
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        Ok(DataReply { result, permit })
    }
}

pub(crate) struct InstalledView {
    pub(crate) manifest: super::manifest::PublicManifestV1,
    pub(crate) enabled: bool,
    pub(crate) grants: BTreeSet<PublicPermission>,
    pub(crate) settings: BTreeMap<String, Value>,
    pub(crate) commands: BTreeMap<String, CommandPreference>,
}

pub(crate) struct MacPluginLifecycle {
    root: PrivateRoot,
    file: PrivateFile,
    raw: Option<Vec<u8>>,
    document: Inventory,
    packages: BTreeMap<String, VerifiedPackage>,
    owner: Arc<()>,
    runtime: Option<RuntimeDataHost>,
    retirement: Option<Retirement>,
}
impl MacPluginLifecycle {
    pub(crate) fn load(inventory: &Path, storage: &Path, settings: &Path) -> Result<Self> {
        let root = PrivateRoot::open(inventory).map_err(file_error)?;
        let file = PrivateFile::open_kind(&root, OWNER, true, FileKind::Lifecycle)
            .map_err(file_error)?
            .ok_or(LifecycleError::Storage)?;
        let raw = file.read(&root).map_err(file_error)?;
        let document = match &raw {
            None => Inventory::default(),
            Some(bytes) => {
                if bytes.len() > STORAGE_QUOTA_BYTES {
                    return Err(LifecycleError::Capacity);
                }
                validate_mac_json(bytes).ok_or(LifecycleError::InvalidState)?;
                serde_json::from_slice(bytes).map_err(|_| LifecycleError::InvalidState)?
            }
        };
        let packages = validate(&document)?;
        let runtime =
            RuntimeDataHost::load(storage, settings).map_err(|_| LifecycleError::Runtime)?;
        Ok(Self {
            root,
            file,
            raw,
            document,
            packages,
            owner: Arc::new(()),
            runtime: Some(runtime),
            retirement: None,
        })
    }
    fn current(&mut self) -> Result<()> {
        if self.runtime.is_none() {
            return Err(LifecycleError::InvalidState);
        }
        match self.file.read(&self.root) {
            Ok(raw) if raw == self.raw => Ok(()),
            _ => {
                // Retire the captured owner and cancel the captured transport,
                // including read-only management failures. No stale authority.
                let _ = self.close();
                self.runtime.take();
                Err(LifecycleError::InvalidState)
            }
        }
    }
    pub(crate) fn clipboard_write_current(&mut self, id: &str, generation: u64) -> Result<bool> {
        self.current()?;
        Ok(self.document.plugins.get(id).is_some_and(|entry| {
            entry.enabled
                && entry.generation == generation
                && entry.grants.contains(&PublicPermission::ClipboardWrite)
                && self.packages[id]
                    .manifest()
                    .permissions
                    .contains(&PublicPermission::ClipboardWrite)
        }))
    }
    #[cfg(test)]
    pub(crate) fn inject_commit_failure(&mut self, failure: super::private_file_macos::Failure) {
        self.file.failure = Some(failure);
    }
    /// Management reads validate the current durable file just like mutations.
    pub(crate) fn management_snapshot(&mut self) -> Result<(u64, Vec<InstalledView>)> {
        self.current()?;
        let rows = self
            .document
            .plugins
            .iter()
            .map(|(id, entry)| InstalledView {
                manifest: self.packages[id].manifest().clone(),
                enabled: entry.enabled,
                grants: entry.grants.clone(),
                settings: entry.settings.clone(),
                commands: entry.commands.clone(),
            })
            .collect();
        Ok((self.document.revision, rows))
    }
    pub(crate) fn remove_expected(&mut self, id: &str, revision: u64) -> Result<bool> {
        self.current()?;
        if revision != self.document.revision {
            return Err(LifecycleError::Stale);
        }
        let mut candidate = self.document.clone();
        if candidate.plugins.remove(id).is_none() {
            return Err(LifecycleError::NotInstalled);
        }
        // Retain plugin private storage; ordinary configuration belongs to the
        // removed inventory entry. No filesystem traversal or directory delete.
        self.commit(candidate, id)
    }
    pub(crate) fn prepare_install(&mut self, package: VerifiedPackage) -> Result<PreparedInstall> {
        self.current()?;
        policy(&package)?;
        let declared = declarations(&package);
        let previous = self
            .packages
            .get(&package.manifest().plugin_id)
            .map(declarations)
            .unwrap_or_default();
        Ok(PreparedInstall {
            owner: self.owner.clone(),
            revision: self.document.revision,
            expires: Instant::now() + Duration::from_secs(300),
            required: declared.difference(&previous).copied().collect(),
            package,
        })
    }
    pub(crate) fn commit_install(
        &mut self,
        prepared: PreparedInstall,
        approved: &BTreeSet<PublicPermission>,
    ) -> Result<bool> {
        self.current()?;
        if !Arc::ptr_eq(&self.owner, &prepared.owner)
            || prepared.revision != self.document.revision
            || Instant::now() >= prepared.expires
        {
            return Err(LifecycleError::Stale);
        }
        if approved != &prepared.required {
            return Err(LifecycleError::ConsentRequired);
        }
        let id = prepared.package.manifest().plugin_id.clone();
        let declared = declarations(&prepared.package);
        let old = self.document.plugins.get(&id);
        let grants = old
            .map(|p| {
                p.grants
                    .intersection(&declared)
                    .copied()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default()
            .union(approved)
            .copied()
            .collect();
        let settings = reconcile_settings(
            old.map(|p| &p.settings),
            &prepared.package.manifest().settings,
        );
        let mut candidate = self.document.clone();
        candidate.plugins.insert(
            id.clone(),
            InstalledDocument {
                package: serde_json::from_slice(prepared.package.encode_snapshot())
                    .map_err(|_| LifecycleError::InvalidPackage)?,
                enabled: old.map_or(true, |p| p.enabled),
                grants,
                generation: old.map_or(0, |p| p.generation),
                settings,
                commands: prepared
                    .package
                    .manifest()
                    .normalized_commands()
                    .filter_map(|(key, _)| {
                        old.and_then(|p| p.commands.get(key))
                            .map(|p| (key.into(), p.clone()))
                    })
                    .collect(),
            },
        );
        self.commit(candidate, &id)
    }
    pub(crate) fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<bool> {
        self.current()?;
        let mut candidate = self.document.clone();
        candidate
            .plugins
            .get_mut(id)
            .ok_or(LifecycleError::NotInstalled)?
            .enabled = enabled;
        self.commit(candidate, id)
    }
    pub(crate) fn set_permission(
        &mut self,
        id: &str,
        permission: PublicPermission,
        granted: bool,
    ) -> Result<bool> {
        self.current()?;
        let package = self.packages.get(id).ok_or(LifecycleError::NotInstalled)?;
        if permission != PublicPermission::ClipboardWrite
            || !declarations(package).contains(&permission)
        {
            return Err(LifecycleError::ConsentRequired);
        }
        let mut candidate = self.document.clone();
        let entry = candidate
            .plugins
            .get_mut(id)
            .ok_or(LifecycleError::NotInstalled)?;
        if granted {
            entry.grants.insert(permission);
        } else {
            entry.grants.remove(&permission);
        }
        self.commit(candidate, id)
    }
    pub(crate) fn save_settings(
        &mut self,
        id: &str,
        updates: &BTreeMap<String, Value>,
    ) -> Result<bool> {
        self.current()?;
        let package = self.packages.get(id).ok_or(LifecycleError::NotInstalled)?;
        validate_updates(&package.manifest().settings, updates)
            .map_err(|_| LifecycleError::InvalidSetting)?;
        let mut candidate = self.document.clone();
        candidate
            .plugins
            .get_mut(id)
            .ok_or(LifecycleError::NotInstalled)?
            .settings
            .extend(updates.clone());
        self.commit(candidate, id)
    }
    pub(crate) fn set_command_preference(
        &mut self,
        id: &str,
        command_id: &str,
        preference: CommandPreference,
    ) -> Result<bool> {
        self.current()?;
        let package = self.packages.get(id).ok_or(LifecycleError::NotInstalled)?;
        if !package
            .manifest()
            .normalized_commands()
            .any(|(key, _)| key == command_id)
            || preference
                .name_override
                .as_deref()
                .is_some_and(|name| !super::manifest::valid_command_name(name))
        {
            return Err(LifecycleError::InvalidCommand);
        }
        let mut candidate = self.document.clone();
        let entry = candidate
            .plugins
            .get_mut(id)
            .ok_or(LifecycleError::NotInstalled)?;
        if preference == CommandPreference::default() {
            entry.commands.remove(command_id);
        } else {
            entry.commands.insert(command_id.into(), preference);
        }
        self.commit(candidate, id)
    }
    fn commit(&mut self, mut candidate: Inventory, id: &str) -> Result<bool> {
        if candidate == self.document {
            return Ok(false);
        }
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .ok_or(LifecycleError::Capacity)?;
        candidate.next_generation = candidate
            .next_generation
            .checked_add(1)
            .ok_or(LifecycleError::Capacity)?;
        if let Some(entry) = candidate.plugins.get_mut(id) {
            entry.generation = candidate.next_generation;
        }
        let packages = validate(&candidate)?;
        let bytes = serde_json::to_vec(&candidate).map_err(|_| LifecycleError::Capacity)?;
        if bytes.len() > STORAGE_QUOTA_BYTES {
            return Err(LifecycleError::Capacity);
        }
        // Validate everything before retiring the old epoch. The callback must
        // retire any actual ResultRegistry owner and cancel/reap its transport.
        self.close()?;
        if let Err(error) = self.file.commit(&self.root, self.raw.as_deref(), &bytes) {
            if error == FileError::CommitUncertain {
                self.runtime.take();
            }
            return Err(file_error(error));
        }
        self.raw = Some(bytes);
        self.document = candidate;
        self.packages = packages;
        Ok(true)
    }
    /// Trusted attachment must use RAII to clean up partially constructed result
    /// owners and transports on Err. No retirement handle exists on that path.
    /// On success, the lifecycle owns the returned cleanup even if the deadline
    /// expired while attachment was running; cleanup must not wait on our drains.
    pub(crate) fn open_request(
        &mut self,
        id: &str,
        command_id: &str,
        request_id: &str,
        deadline: Instant,
        attach: impl FnOnce(&InstalledRequest) -> Result<Retirement>,
    ) -> Result<InstalledRequest> {
        self.current()?;
        let entry = self
            .document
            .plugins
            .get(id)
            .ok_or(LifecycleError::NotInstalled)?;
        if !entry.enabled {
            return Err(LifecycleError::Disabled);
        }
        let package = self
            .packages
            .get(id)
            .ok_or(LifecycleError::InvalidState)?
            .clone();
        if !package
            .manifest()
            .normalized_commands()
            .any(|(key, _)| key == command_id)
        {
            return Err(LifecycleError::InvalidPackage);
        }
        let context = PluginRequestContext {
            plugin_id: id.into(),
            plugin_generation: entry.generation,
            request_id: request_id.into(),
        };
        if !context.valid_shape() || Instant::now() >= deadline {
            return Err(LifecycleError::Runtime);
        }
        let settings = entry.settings.clone();
        let grants = entry.grants.clone();
        self.close()?;
        let binding = self
            .runtime
            .as_mut()
            .ok_or(LifecycleError::InvalidState)?
            .open_installed_request(
                context,
                &package.manifest().settings,
                &settings,
                deadline,
                || Ok(()),
            )
            .map_err(|_| LifecycleError::Runtime)?;
        let request = InstalledRequest {
            binding,
            package,
            command_id: command_id.into(),
            grants,
        };
        match attach(&request) {
            Ok(retirement) => {
                self.retirement = Some(retirement);
                if Instant::now() >= deadline {
                    let _ = self.close();
                    return Err(LifecycleError::Runtime);
                }
                Ok(request)
            }
            Err(error) => {
                let _ = self.close();
                Err(error)
            }
        }
    }
    /// Captured cleanup retires the actual result owner and cancels/reaps the
    /// transport before waiting. It must not wait for this host's drains.
    pub(crate) fn close(&mut self) -> Result<()> {
        let retirement = self.retirement.take();
        self.runtime
            .as_mut()
            .ok_or(LifecycleError::InvalidState)?
            .close(|| {
                if let Some(retire) = retirement {
                    retire()?;
                }
                Ok(())
            })
            .map_err(|_| LifecycleError::Runtime)
    }
}
impl Drop for MacPluginLifecycle {
    fn drop(&mut self) {
        // Drop closes authority and invokes owned cancellation, but does not
        // claim a permit drain. Explicit close is the verified drain boundary.
        self.runtime.take();
        if let Some(retire) = self.retirement.take() {
            let _ = retire();
        }
    }
}
fn declarations(package: &VerifiedPackage) -> BTreeSet<PublicPermission> {
    package.manifest().permissions.iter().copied().collect()
}
fn policy(package: &VerifiedPackage) -> Result<()> {
    // Other permissions require scope-aware separate consent/service contracts.
    if declarations(package)
        .iter()
        .any(|p| *p != PublicPermission::ClipboardWrite)
    {
        return Err(LifecycleError::ConsentRequired);
    }
    Ok(())
}
fn validate(document: &Inventory) -> Result<BTreeMap<String, VerifiedPackage>> {
    if document.schema != 1
        || document.revision != document.next_generation
        || (document.revision == 0 && !document.plugins.is_empty())
    {
        return Err(LifecycleError::InvalidState);
    }
    let mut names = BTreeSet::new();
    let mut generations = BTreeSet::new();
    let mut packages = BTreeMap::new();
    for (id, entry) in &document.plugins {
        let bytes = serde_json::to_vec(&entry.package).map_err(|_| LifecycleError::InvalidState)?;
        let package = restore_snapshot(&bytes, &PublicPluginHost::current(PublicPlatform::Macos))
            .map_err(|_| LifecycleError::InvalidPackage)?;
        policy(&package)?;
        if id != &package.manifest().plugin_id
            || entry.generation == 0
            || entry.generation > document.next_generation
            || !generations.insert(entry.generation)
            || !entry.grants.is_subset(&declarations(&package))
            || reconcile_settings(Some(&entry.settings), &package.manifest().settings)
                != entry.settings
        {
            return Err(LifecycleError::InvalidState);
        }
        if entry.commands.iter().any(|(id, preference)| {
            !package
                .manifest()
                .normalized_commands()
                .any(|(key, _)| key == id)
                || preference
                    .name_override
                    .as_deref()
                    .is_some_and(|name| !super::manifest::valid_command_name(name))
        }) {
            return Err(LifecycleError::InvalidCommand);
        }
        for (key, command) in package.manifest().normalized_commands() {
            let name = entry
                .commands
                .get(key)
                .and_then(|p| p.name_override.as_deref())
                .unwrap_or(&command.default_name);
            if super::launcher_runtime_macos::host_command_names().any(|reserved| reserved == name)
                || !names.insert(name.to_lowercase())
            {
                return Err(LifecycleError::CommandConflict);
            }
        }
        packages.insert(id.clone(), package);
    }
    Ok(packages)
}
fn file_error(error: FileError) -> LifecycleError {
    match error {
        FileError::CommitUncertain => LifecycleError::CommitUncertain,
        _ => LifecycleError::Storage,
    }
}
#[cfg(test)]
#[path = "lifecycle_macos_tests.rs"]
pub(crate) mod tests;
