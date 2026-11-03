use super::*;
use crate::atomic_file;
use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    },
};

pub(super) const FILE_NAME: &str = "credentials-v1.dpapi";
const MAX_FILE: usize = 128 * 1024;
const MAX_VALUE: usize = 8192;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    revision: u64,
    items: Vec<Record>,
}
impl Document {
    fn inventory(&self) -> Inventory {
        Inventory {
            revision: self.revision.to_string(),
            items: self.items.iter().map(Record::view).collect(),
        }
    }
    fn check_revision(&self, value: &str) -> Result<(), Error> {
        let revision = value.parse::<u64>().map_err(|_| Error::invalid_input())?;
        if revision.to_string() != value {
            return Err(Error::invalid_input());
        }
        if self.revision != revision {
            return Err(Error::stale());
        }
        Ok(())
    }
    fn position(&self, service: Service) -> Result<usize, Error> {
        self.items
            .iter()
            .position(|item| item.service == service)
            .ok_or_else(Error::missing)
    }
    fn validate(&self) -> Result<(), Error> {
        if self.items.len() > 4 {
            return Err(Error::storage());
        }
        let mut seen = std::collections::BTreeSet::new();
        for item in &self.items {
            if !seen.insert(item.service)
                || !valid_name(&item.name)
                || !valid_values(item.service, &item.values)
                || (!item.values.is_empty() && !item.configured())
                || (item.enabled && !item.configured())
            {
                return Err(Error::storage());
            }
        }
        Ok(())
    }
}
struct ServiceLease {
    publication: crate::public_plugins::result_action::ResultActionLease,
    revision: u64,
    context: crate::public_plugins::PluginRequestContext,
    invalidate: Box<dyn Fn() + Send + Sync>,
}

pub(crate) struct CredentialStore {
    root: PathBuf,
    state: Mutex<Option<Document>>,
    service_leases: Mutex<std::collections::BTreeMap<String, ServiceLease>>,
}
impl CredentialStore {
    pub(crate) fn for_app_data(root: &Path) -> Self {
        Self::new(root.join("credentials"))
    }
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Mutex::new(None),
            service_leases: Mutex::new(std::collections::BTreeMap::new()),
        }
    }
    #[cfg(test)]
    pub(crate) fn synthetic_youdao_for_test(&self) {
        *self.state.lock().unwrap() = Some(Document {
            revision: 1,
            items: vec![Record {
                service: Service::YoudaoTranslation,
                name: "Synthetic".into(),
                enabled: true,
                values: BTreeMap::from([
                    ("appId".into(), "synthetic-id".into()),
                    ("appKey".into(), "synthetic-key".into()),
                ]),
            }],
        });
    }
    #[cfg(test)]
    pub(crate) fn disable_youdao_for_test(&self) {
        self.set_enabled(EnabledInput {
            service: Service::YoudaoTranslation,
            enabled: false,
            expected_revision: "1".into(),
        })
        .unwrap();
    }
    fn lock(&self) -> Result<MutexGuard<'_, Option<Document>>, Error> {
        let mut state = self.state.lock().map_err(|_| Error::storage())?;
        if state.is_none() {
            *state = Some(self.read()?);
        }
        Ok(state)
    }
    pub(crate) fn with_youdao<T>(
        &self,
        context: &crate::public_plugins::PluginRequestContext,
        publication: crate::public_plugins::result_action::ResultActionLease,
        invalidate: Box<dyn Fn() + Send + Sync>,
        use_record: impl FnOnce(u64, &BTreeMap<String, String>) -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let guard = self.lock().map_err(|_| "credentialUnavailable")?;
        let document = guard.as_ref().ok_or("credentialUnavailable")?;
        let record = document
            .items
            .iter()
            .find(|r| r.service == Service::YoudaoTranslation)
            .filter(|r| r.configured())
            .ok_or("credentialNotConfigured")?;
        if !record.enabled {
            return Err("credentialDisabled");
        }
        let result = use_record(document.revision, &record.values)?;
        self.service_leases
            .lock()
            .map_err(|_| "credentialUnavailable")?
            .insert(
                context.plugin_id.clone(),
                ServiceLease {
                    publication,
                    revision: document.revision,
                    context: context.clone(),
                    invalidate,
                },
            );
        Ok(result)
    }
    pub(crate) fn with_completion<T>(
        &self,
        context: &crate::public_plugins::PluginRequestContext,
        complete: impl FnOnce(
            Option<(u64, crate::public_plugins::result_action::ResultActionLease)>,
        ) -> T,
    ) -> Result<T, Error> {
        let _state = self.state.lock().map_err(|_| Error::storage())?;
        let leases = self.service_leases.lock().map_err(|_| Error::storage())?;
        let revision = leases
            .get(&context.plugin_id)
            .filter(|lease| lease.context == *context)
            .map(|lease| (lease.revision, lease.publication.clone()));
        drop(leases);
        Ok(complete(revision))
    }
    pub(crate) fn with_revision<T>(
        &self,
        revision: u64,
        deliver: impl FnOnce() -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let guard = self.lock().map_err(|_| "credentialUnavailable")?;
        if guard.as_ref().ok_or("credentialUnavailable")?.revision != revision {
            return Err("expiredRequest");
        }
        deliver()
    }
    pub(super) fn list(&self) -> Result<Inventory, Error> {
        Ok(self
            .lock()?
            .as_ref()
            .ok_or_else(Error::storage)?
            .inventory())
    }
    fn mutate<F>(&self, expected_revision: &str, change: F) -> Result<Inventory, Error>
    where
        F: FnOnce(&mut Document) -> Result<(), Error>,
    {
        let mut guard = self.lock()?;
        let current = guard.as_ref().ok_or_else(Error::storage)?;
        current.check_revision(expected_revision)?;
        let mut candidate = current.clone();
        change(&mut candidate)?;
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .ok_or_else(Error::storage)?;
        candidate.validate()?;
        self.write(&candidate)?;
        let inventory = candidate.inventory();
        // Retire request contexts before publishing a successful configuration mutation.
        for invalidate in self
            .service_leases
            .lock()
            .map_err(|_| Error::storage())?
            .values()
        {
            (invalidate.invalidate)();
        }
        *guard = Some(candidate);
        Ok(inventory)
    }
    pub(super) fn add(&self, input: MutationInput) -> Result<Inventory, Error> {
        self.mutate(&input.expected_revision, |document| {
            if document
                .items
                .iter()
                .any(|item| item.service == input.service)
            {
                return Err(Error::invalid_input());
            }
            document.items.push(Record {
                service: input.service,
                name: input.service.default_name().into(),
                enabled: false,
                values: BTreeMap::new(),
            });
            Ok(())
        })
    }
    pub(super) fn delete(&self, input: MutationInput) -> Result<Inventory, Error> {
        self.mutate(&input.expected_revision, |document| {
            let position = document.position(input.service)?;
            document.items.remove(position);
            Ok(())
        })
    }
    pub(super) fn set_enabled(&self, input: EnabledInput) -> Result<Inventory, Error> {
        self.mutate(&input.expected_revision, |document| {
            let position = document.position(input.service)?;
            if input.enabled && !document.items[position].configured() {
                return Err(Error::not_configured());
            }
            document.items[position].enabled = input.enabled;
            Ok(())
        })
    }
    pub(super) async fn save_with<F, Fut>(
        &self,
        input: SaveInput,
        verify: F,
    ) -> Result<Inventory, Error>
    where
        F: FnOnce(Record) -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        if !valid_name(&input.name) || !valid_values(input.service, &input.values) {
            return Err(Error::invalid_input());
        }
        let candidate = {
            let guard = self.lock()?;
            let document = guard.as_ref().ok_or_else(Error::storage)?;
            document.check_revision(&input.expected_revision)?;
            let mut candidate = document.items[document.position(input.service)?].clone();
            candidate.name = input.name.trim().into();
            for (key, value) in &input.values {
                if !value.is_empty() {
                    candidate.values.insert(key.clone(), value.clone());
                }
            }
            if !candidate.configured() {
                return Err(Error::invalid_input());
            }
            candidate
        };
        // Never retain the lock across a network await. The commit checks the revision again.
        verify(candidate.clone()).await?;
        self.mutate(&input.expected_revision, |document| {
            let position = document.position(input.service)?;
            document.items[position] = candidate;
            Ok(())
        })
    }
    fn read(&self) -> Result<Document, Error> {
        check_root(&self.root, false)?;
        let path = self.root.join(FILE_NAME);
        check_file(&path)?;
        let Some(encrypted) =
            atomic_file::read_optional_bounded(&path, MAX_FILE).map_err(|_| Error::storage())?
        else {
            return Ok(Document::default());
        };
        let plaintext = crypt(&encrypted, &self.entropy(), false)?;
        let document: Document =
            serde_json::from_slice(&plaintext.0).map_err(|_| Error::storage())?;
        document.validate()?;
        Ok(document)
    }
    fn write(&self, document: &Document) -> Result<(), Error> {
        check_root(&self.root, true)?;
        let path = self.root.join(FILE_NAME);
        check_file(&path)?;
        let plaintext = SensitiveBytes(serde_json::to_vec(document).map_err(|_| Error::storage())?);
        let encrypted = crypt(&plaintext.0, &self.entropy(), true)?;
        if encrypted.0.len() > MAX_FILE {
            return Err(Error::storage());
        }
        // One replace contains metadata and secrets. No stale backup can restore a deletion.
        atomic_file::replace_current(&path, &encrypted.0).map_err(|_| Error::storage())
    }
    fn entropy(&self) -> Vec<u8> {
        let mut entropy = b"UIPILOT-CREDENTIAL-MANAGER-V1\0".to_vec();
        entropy.extend_from_slice(
            self.root
                .join(FILE_NAME)
                .to_string_lossy()
                .to_lowercase()
                .as_bytes(),
        );
        entropy
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control)
}
fn valid_values(service: Service, values: &BTreeMap<String, String>) -> bool {
    values.len() <= service.fields().len()
        && values.iter().all(|(key, value)| {
            service.fields().contains(&key.as_str())
                && value.len() <= MAX_VALUE
                && (value.is_empty()
                    || (!value.trim().is_empty()
                        && value.trim() == value
                        && !value.chars().any(char::is_control)))
        })
}
fn ordinary(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT.0
        == 0
}
fn check_root(root: &Path, create: bool) -> Result<(), Error> {
    if create {
        fs::create_dir_all(root).map_err(|_| Error::storage())?;
    }
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() && ordinary(&metadata) => Ok(()),
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::storage()),
    }
}
fn check_file(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file() && ordinary(&metadata) && metadata.len() <= MAX_FILE as u64 =>
        {
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::storage()),
    }
}
struct SensitiveBytes(Vec<u8>);
impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
fn crypt(bytes: &[u8], entropy: &[u8], protect: bool) -> Result<SensitiveBytes, Error> {
    let blob = |bytes: &[u8]| -> Result<CRYPT_INTEGER_BLOB, Error> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(bytes.len()).map_err(|_| Error::storage())?,
            pbData: bytes.as_ptr().cast_mut(),
        })
    };
    let input = blob(bytes)?;
    let entropy = blob(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if protect {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    }
    .map_err(|_| Error::storage())?;
    if output.pbData.is_null() {
        return Err(Error::storage());
    }
    let result = SensitiveBytes(
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec(),
    );
    for offset in 0..output.cbData as usize {
        unsafe { std::ptr::write_volatile(output.pbData.add(offset), 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    let freed = unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
    if !freed.is_invalid() {
        return Err(Error::storage());
    }
    Ok(result)
}
