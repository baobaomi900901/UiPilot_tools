mod providers;
mod store;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub(crate) use store::CredentialStore;
use tauri::{Emitter, State, WebviewWindow};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Service {
    YoudaoTranslation,
    BaiduTranslation,
    BaiduOcr,
    Deepseek,
}
impl Service {
    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::YoudaoTranslation | Self::BaiduTranslation => &["appId", "appKey"],
            Self::BaiduOcr => &["appId", "apiKey", "secretKey"],
            Self::Deepseek => &["apiKey"],
        }
    }
    fn default_name(self) -> &'static str {
        match self {
            Self::YoudaoTranslation => "有道翻译",
            Self::BaiduTranslation => "百度翻译",
            Self::BaiduOcr => "百度 OCR",
            Self::Deepseek => "DeepSeek",
        }
    }
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Inventory {
    revision: String,
    items: Vec<Entry>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Entry {
    service: Service,
    name: String,
    enabled: bool,
    configured: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MutationInput {
    service: Service,
    expected_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnabledInput {
    service: Service,
    enabled: bool,
    expected_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveInput {
    service: Service,
    name: String,
    values: BTreeMap<String, String>,
    expected_revision: String,
}
impl Drop for SaveInput {
    fn drop(&mut self) {
        wipe_values(&mut self.values);
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    service: Service,
    name: String,
    enabled: bool,
    values: BTreeMap<String, String>,
}
impl Record {
    fn configured(&self) -> bool {
        self.service
            .fields()
            .iter()
            .all(|key| self.values.get(*key).is_some_and(|value| !value.is_empty()))
    }
    fn view(&self) -> Entry {
        Entry {
            service: self.service,
            name: self.name.clone(),
            enabled: self.enabled,
            configured: self.configured(),
        }
    }
}
impl Drop for Record {
    fn drop(&mut self) {
        wipe_values(&mut self.values);
    }
}
fn wipe_values(values: &mut BTreeMap<String, String>) {
    for value in values.values_mut() {
        // Zeroing preserves UTF-8 validity and prevents dead-store elimination.
        unsafe {
            for byte in value.as_bytes_mut() {
                std::ptr::write_volatile(byte, 0);
            }
        }
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}
#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
pub(crate) struct Error {
    code: &'static str,
    message: &'static str,
}
impl Error {
    fn invalid_input() -> Self {
        Self {
            code: "invalidInput",
            message: "Credential input is invalid",
        }
    }
    fn stale() -> Self {
        Self {
            code: "staleInventory",
            message: "Credential configuration changed; reload and retry",
        }
    }
    fn missing() -> Self {
        Self {
            code: "credentialUnavailable",
            message: "Credential configuration is unavailable",
        }
    }
    fn not_configured() -> Self {
        Self {
            code: "notConfigured",
            message: "Validate and save this credential before enabling it",
        }
    }
    fn invalid_credentials() -> Self {
        Self {
            code: "invalidCredentials",
            message: "The service rejected these credentials",
        }
    }
    fn network() -> Self {
        Self {
            code: "networkFailed",
            message: "The service could not be reached",
        }
    }
    fn validation() -> Self {
        Self {
            code: "validationFailed",
            message: "The service could not validate this configuration",
        }
    }
    fn storage() -> Self {
        Self {
            code: "storageFailed",
            message: "Encrypted credential storage is unavailable",
        }
    }
}
fn require_main_label(label: &str) -> Result<(), Error> {
    if label == "main" {
        Ok(())
    } else {
        Err(Error {
            code: "invalidCaller",
            message: "Only the main window can manage credentials",
        })
    }
}
fn parse_input<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, Error> {
    // Never forward serde errors: unknown keys or invalid enum values may contain secrets.
    serde_json::from_value(value).map_err(|_| Error::invalid_input())
}
#[tauri::command]
pub(crate) fn list_credentials(
    window: WebviewWindow,
    store: State<'_, CredentialStore>,
) -> Result<Inventory, Error> {
    require_main_label(window.label())?;
    store.list()
}
#[tauri::command]
pub(crate) fn add_credential(
    window: WebviewWindow,
    store: State<'_, CredentialStore>,
    input: serde_json::Value,
) -> Result<Inventory, Error> {
    require_main_label(window.label())?;
    store.add(parse_input(input)?)
}
#[tauri::command]
pub(crate) async fn save_credential(
    window: WebviewWindow,
    store: State<'_, CredentialStore>,
    input: serde_json::Value,
) -> Result<Inventory, Error> {
    require_main_label(window.label())?;
    let result = store
        .save_with(parse_input(input)?, providers::verify)
        .await;
    if result.is_ok() {
        let _ = window.emit_to("main", "public-plugin-permissions-changed", ());
    }
    result
}
#[tauri::command]
pub(crate) fn set_credential_enabled(
    window: WebviewWindow,
    store: State<'_, CredentialStore>,
    input: serde_json::Value,
) -> Result<Inventory, Error> {
    require_main_label(window.label())?;
    let result = store.set_enabled(parse_input(input)?);
    if result.is_ok() {
        let _ = window.emit_to("main", "public-plugin-permissions-changed", ());
    }
    result
}
#[tauri::command]
pub(crate) fn delete_credential(
    window: WebviewWindow,
    store: State<'_, CredentialStore>,
    input: serde_json::Value,
) -> Result<Inventory, Error> {
    require_main_label(window.label())?;
    let result = store.delete(parse_input(input)?);
    if result.is_ok() {
        let _ = window.emit_to("main", "public-plugin-permissions-changed", ());
    }
    result
}
