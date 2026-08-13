use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    },
};

use crate::atomic_file::replace_current;

use super::{
    authorize_plugin_scope,
    manifest::{valid_plugin_id, valid_setting_key},
    PluginDataScope,
};

const RECORD_MAGIC: &[u8] = b"UIPILOT-SECRET-V1\0";
const MAX_SECRET_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginSecretError {
    Storage,
    InvalidPlugin,
    InvalidKey,
    InvalidScope,
    ValueTooLarge,
    ProtectFailed,
}

impl fmt::Display for PluginSecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Storage => "public plugin secret storage failed",
            Self::InvalidPlugin => "public plugin secret owner is invalid",
            Self::InvalidKey => "public plugin secret setting key is invalid",
            Self::InvalidScope => "public plugin secret scope is invalid",
            Self::ValueTooLarge => "public plugin secret is too large",
            Self::ProtectFailed => "public plugin secret protection failed",
        })
    }
}

impl std::error::Error for PluginSecretError {}

pub(crate) struct PluginSecretStore {
    root: PathBuf,
}

impl PluginSecretStore {
    pub(crate) fn load(root: &Path) -> Result<Self, PluginSecretError> {
        fs::create_dir_all(root).map_err(|_| PluginSecretError::Storage)?;
        if !ordinary_directory(root) {
            return Err(PluginSecretError::Storage);
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub(crate) fn write(
        &self,
        plugin_id: &str,
        key: &str,
        plaintext: &str,
    ) -> Result<(), PluginSecretError> {
        validate_identity(plugin_id, key)?;
        if plaintext.len() > MAX_SECRET_BYTES {
            return Err(PluginSecretError::ValueTooLarge);
        }
        let ciphertext = protect(plaintext.as_bytes(), &entropy(plugin_id, key))?;
        let record = encode_record(&ciphertext)?;
        let owner = self.owner_root(plugin_id)?;
        fs::create_dir_all(&owner).map_err(|_| PluginSecretError::Storage)?;
        if !ordinary_directory(&owner) {
            return Err(PluginSecretError::Storage);
        }
        replace_current(&secret_path(&owner, key), &record).map_err(|_| PluginSecretError::Storage)
    }

    pub(crate) fn is_configured(
        &self,
        scope: &PluginDataScope,
        plugin_id: &str,
        key: &str,
    ) -> Result<bool, PluginSecretError> {
        authorize_plugin_scope(scope, plugin_id).map_err(|_| PluginSecretError::InvalidScope)?;
        validate_identity(plugin_id, key)?;
        let path = secret_path(&self.owner_root(plugin_id)?, key);
        match fs::read(path) {
            Ok(bytes) => Ok(decode_record(&bytes).is_some()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(PluginSecretError::Storage),
        }
    }

    pub(crate) fn remove(&self, plugin_id: &str, key: &str) -> Result<(), PluginSecretError> {
        validate_identity(plugin_id, key)?;
        let path = secret_path(&self.owner_root(plugin_id)?, key);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(PluginSecretError::Storage),
        }
    }

    pub(crate) fn uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<(), PluginSecretError> {
        if !valid_plugin_id(plugin_id) {
            return Err(PluginSecretError::InvalidPlugin);
        }
        if retain_data {
            return Ok(());
        }
        let owner = self.owner_root(plugin_id)?;
        if owner.exists() {
            fs::remove_dir_all(owner).map_err(|_| PluginSecretError::Storage)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn plaintext_for_test(&self, plugin_id: &str, key: &str) -> Result<Vec<u8>, PluginSecretError> {
        validate_identity(plugin_id, key)?;
        let record = fs::read(secret_path(&self.owner_root(plugin_id)?, key))
            .map_err(|_| PluginSecretError::Storage)?;
        let ciphertext = decode_record(&record).ok_or(PluginSecretError::Storage)?;
        unprotect(ciphertext, &entropy(plugin_id, key))
    }

    #[cfg(test)]
    fn record_path_for_test(
        &self,
        plugin_id: &str,
        key: &str,
    ) -> Result<PathBuf, PluginSecretError> {
        validate_identity(plugin_id, key)?;
        Ok(secret_path(&self.owner_root(plugin_id)?, key))
    }

    fn owner_root(&self, plugin_id: &str) -> Result<PathBuf, PluginSecretError> {
        valid_plugin_id(plugin_id)
            .then(|| self.root.join(plugin_id))
            .ok_or(PluginSecretError::InvalidPlugin)
    }
}

fn validate_identity(plugin_id: &str, key: &str) -> Result<(), PluginSecretError> {
    if !valid_plugin_id(plugin_id) {
        return Err(PluginSecretError::InvalidPlugin);
    }
    if !valid_setting_key(key) {
        return Err(PluginSecretError::InvalidKey);
    }
    Ok(())
}

fn entropy(plugin_id: &str, key: &str) -> Vec<u8> {
    let mut value = b"UIPILOT-PUBLIC-SECRET-V1\0".to_vec();
    value.extend_from_slice(plugin_id.as_bytes());
    value.push(0);
    value.extend_from_slice(key.as_bytes());
    value
}

fn protect(plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, PluginSecretError> {
    let input = blob(plaintext)?;
    let entropy = blob(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|_| PluginSecretError::ProtectFailed)?;
    take_output(output)
}

fn unprotect(ciphertext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, PluginSecretError> {
    let input = blob(ciphertext)?;
    let entropy = blob(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
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
    .map_err(|_| PluginSecretError::ProtectFailed)?;
    take_output(output)
}

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, PluginSecretError> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| PluginSecretError::ValueTooLarge)?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

fn take_output(output: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, PluginSecretError> {
    let result = if output.cbData == 0 {
        Vec::new()
    } else if output.pbData.is_null() {
        return Err(PluginSecretError::ProtectFailed);
    } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec()
    };
    if !output.pbData.is_null() {
        let freed = unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
        if !freed.is_invalid() {
            return Err(PluginSecretError::ProtectFailed);
        }
    }
    Ok(result)
}

fn encode_record(ciphertext: &[u8]) -> Result<Vec<u8>, PluginSecretError> {
    let length = u32::try_from(ciphertext.len()).map_err(|_| PluginSecretError::ValueTooLarge)?;
    let mut record = Vec::with_capacity(RECORD_MAGIC.len() + 4 + ciphertext.len());
    record.extend_from_slice(RECORD_MAGIC);
    record.extend_from_slice(&length.to_le_bytes());
    record.extend_from_slice(ciphertext);
    Ok(record)
}

fn decode_record(record: &[u8]) -> Option<&[u8]> {
    let length_start = RECORD_MAGIC.len();
    let data_start = length_start.checked_add(4)?;
    if record.get(..length_start)? != RECORD_MAGIC {
        return None;
    }
    let length =
        u32::from_le_bytes(record.get(length_start..data_start)?.try_into().ok()?) as usize;
    (record.len() == data_start.checked_add(length)?).then(|| &record[data_start..])
}

fn secret_path(owner: &Path, key: &str) -> PathBuf {
    owner.join(format!(
        "{}.secret",
        lower_hex(&Sha256::digest(key.as_bytes()))
    ))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[usize::from(byte >> 4)] as char);
        value.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    value
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
#[path = "secrets_tests.rs"]
mod tests;
