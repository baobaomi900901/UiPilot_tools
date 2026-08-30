use std::collections::HashSet;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};
use unicode_normalization::UnicodeNormalization;

use super::{PublicPackageError, PublicPluginHost};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PublicPlatform {
    Windows,
    Macos,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
pub(crate) enum PublicPermission {
    #[serde(rename = "ui.window")]
    UiWindow,
    #[serde(rename = "ui.panel")]
    UiPanel,
    #[serde(rename = "clipboard.write")]
    ClipboardWrite,
    #[serde(rename = "clipboard.read")]
    ClipboardRead,
    #[serde(rename = "clipboard.history.read")]
    ClipboardHistoryRead,
    #[serde(rename = "clipboard.history.paste")]
    ClipboardHistoryPaste,
    #[serde(rename = "network.https")]
    NetworkHttps,
    #[serde(rename = "files.userSelected")]
    FilesUserSelected,
    #[serde(rename = "files.index.readAll")]
    FilesIndexReadAll,
    #[serde(rename = "notifications.publish")]
    NotificationsPublish,
    #[serde(rename = "timer.control")]
    TimerControl,
    #[serde(rename = "background.schedule")]
    BackgroundSchedule,
}

impl PublicPermission {
    pub(super) fn is_available(self, platform: PublicPlatform) -> bool {
        matches!(self, Self::UiWindow | Self::UiPanel | Self::ClipboardWrite)
            || (matches!(
                self,
                Self::ClipboardHistoryRead
                    | Self::ClipboardHistoryPaste
                    | Self::NetworkHttps
                    | Self::NotificationsPublish
                    | Self::TimerControl
            ) && platform == PublicPlatform::Windows)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublicActivationMode {
    Live,
    Submit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublicOutputMode {
    MainResult,
    Window,
    Panel,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicCommandV1 {
    pub(crate) default_name: String,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) output_mode: PublicOutputMode,
    pub(crate) input_required: bool,
    #[serde(default)]
    pub(crate) input_placeholder: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicRuntimeV1 {
    pub(crate) entry: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicWindowV1 {
    pub(crate) entry: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicPanelV1 {
    pub(crate) entry: String,
    #[serde(default)]
    #[schemars(schema_with = "panel_host_keys_schema")]
    pub(crate) host_keys: Vec<PanelHostKeyDeclaration>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicNetworkV1 {
    #[schemars(schema_with = "network_https_hosts_schema")]
    pub(crate) https_hosts: Vec<String>,
}

fn deserialize_present_network<'de, D>(deserializer: D) -> Result<Option<PublicNetworkV1>, D::Error>
where
    D: Deserializer<'de>,
{
    PublicNetworkV1::deserialize(deserializer).map(Some)
}

fn present_network_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<PublicNetworkV1>()
}

fn network_https_hosts_schema(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = Vec::<String>::json_schema(generator);
    schema.insert("minItems".into(), 1.into());
    schema.insert("maxItems".into(), 8.into());
    schema.insert("uniqueItems".into(), true.into());
    schema.insert(
        "items".into(),
        serde_json::json!({
            "type": "string",
            "pattern": "^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"
        }),
    );
    schema
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
pub(crate) enum PanelHostKeyDeclaration {
    ArrowDown,
    ArrowUp,
    #[serde(rename = "Primary+N")]
    PrimaryN,
    Tab,
    #[serde(rename = "Shift+Tab")]
    ShiftTab,
    Enter,
}

fn panel_host_keys_schema(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = Vec::<PanelHostKeyDeclaration>::json_schema(generator);
    schema.insert("maxItems".into(), 8.into());
    schema.insert("uniqueItems".into(), true.into());
    schema
}

impl PublicPanelV1 {
    pub(crate) fn canonical_host_keys(&self) -> Vec<PanelHostKeyDeclaration> {
        let mut keys = self.host_keys.clone();
        keys.sort_unstable();
        keys
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(crate) enum PublicSettingV1 {
    #[serde(rename = "text")]
    Text {
        key: String,
        label: String,
        #[serde(default)]
        default: Option<String>,
    },
    #[serde(rename = "secret")]
    Secret { key: String, label: String },
    #[serde(rename = "number")]
    Number {
        key: String,
        label: String,
        #[serde(default)]
        default: Option<f64>,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
    },
    #[serde(rename = "boolean")]
    Boolean {
        key: String,
        label: String,
        #[serde(default)]
        default: Option<bool>,
    },
    #[serde(rename = "select")]
    Select {
        key: String,
        label: String,
        options: Vec<PublicSelectOptionV1>,
        #[serde(default)]
        default: Option<String>,
    },
}

impl PublicSettingV1 {
    pub(super) fn key(&self) -> &str {
        match self {
            Self::Text { key, .. }
            | Self::Secret { key, .. }
            | Self::Number { key, .. }
            | Self::Boolean { key, .. }
            | Self::Select { key, .. } => key,
        }
    }

    pub(super) fn is_secret(&self) -> bool {
        matches!(self, Self::Secret { .. })
    }

    pub(super) fn default_value(&self) -> Option<Value> {
        match self {
            Self::Text { default, .. } | Self::Select { default, .. } => {
                default.clone().map(Value::String)
            }
            Self::Number { default, .. } => default.and_then(Number::from_f64).map(Value::Number),
            Self::Boolean { default, .. } => default.map(Value::Bool),
            Self::Secret { .. } => None,
        }
    }

    pub(super) fn accepts_value(&self, value: &Value) -> bool {
        match self {
            Self::Text { .. } => value.as_str().is_some_and(plain_text),
            Self::Secret { .. } => false,
            Self::Number { min, max, .. } => value.as_f64().is_some_and(|value| {
                value.is_finite()
                    && min.is_none_or(|min| value >= min)
                    && max.is_none_or(|max| value <= max)
            }),
            Self::Boolean { .. } => value.is_boolean(),
            Self::Select { options, .. } => value
                .as_str()
                .is_some_and(|value| options.iter().any(|option| option.value == value)),
        }
    }

    fn validate(&self) -> bool {
        let valid_label = match self {
            Self::Text { label, .. }
            | Self::Secret { label, .. }
            | Self::Number { label, .. }
            | Self::Boolean { label, .. }
            | Self::Select { label, .. } => nonempty_plain_text(label),
        };
        if !valid_setting_key(self.key()) || !valid_label {
            return false;
        }
        match self {
            Self::Text { default, .. } => default.as_deref().is_none_or(plain_text),
            Self::Secret { .. } | Self::Boolean { .. } => true,
            Self::Number {
                default,
                min,
                max,
                step,
                ..
            } => {
                let finite = [*default, *min, *max, *step]
                    .into_iter()
                    .flatten()
                    .all(f64::is_finite);
                let ordered = min.zip(*max).is_none_or(|(min, max)| min <= max);
                let positive_step = step.is_none_or(|step| step > 0.0);
                let default_in_range = default.is_none_or(|default| {
                    min.is_none_or(|min| default >= min) && max.is_none_or(|max| default <= max)
                });
                finite && ordered && positive_step && default_in_range
            }
            Self::Select {
                options, default, ..
            } => {
                let mut values = HashSet::new();
                !options.is_empty()
                    && options.iter().all(|option| {
                        plain_text(&option.value)
                            && nonempty_plain_text(&option.label)
                            && values.insert(option.value.as_str())
                    })
                    && default
                        .as_deref()
                        .is_none_or(|default| values.contains(default))
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicSelectOptionV1 {
    pub(crate) value: String,
    pub(crate) label: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicManifestV1 {
    pub(crate) schema_version: u32,
    pub(crate) plugin_id: String,
    pub(crate) version: String,
    pub(crate) api_version: u32,
    pub(crate) minimum_host_version: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    pub(crate) supported_platforms: Vec<PublicPlatform>,
    pub(crate) command: PublicCommandV1,
    pub(crate) runtime: PublicRuntimeV1,
    #[serde(default)]
    pub(crate) window: Option<PublicWindowV1>,
    #[serde(default)]
    pub(crate) panel: Option<PublicPanelV1>,
    #[serde(
        default,
        deserialize_with = "deserialize_present_network",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(schema_with = "present_network_schema")]
    pub(crate) network: Option<PublicNetworkV1>,
    pub(crate) permissions: Vec<PublicPermission>,
    #[serde(default)]
    pub(crate) settings: Vec<PublicSettingV1>,
}

pub(crate) fn public_manifest_v1_schema() -> schemars::Schema {
    schemars::schema_for!(PublicManifestV1)
}

pub(super) fn parse_manifest(
    bytes: &[u8],
    host: &PublicPluginHost,
) -> Result<PublicManifestV1, PublicPackageError> {
    let mut manifest: PublicManifestV1 =
        serde_json::from_slice(bytes).map_err(|_| PublicPackageError::InvalidPackage)?;
    validate_manifest(&manifest, host)?;
    if let Some(network) = &mut manifest.network {
        network.https_hosts.sort_unstable();
    }
    if let Some(panel) = &mut manifest.panel {
        panel.host_keys.sort_unstable();
    }
    Ok(manifest)
}

fn validate_manifest(
    manifest: &PublicManifestV1,
    host: &PublicPluginHost,
) -> Result<(), PublicPackageError> {
    if manifest.schema_version != 1
        || !valid_plugin_id(&manifest.plugin_id)
        || parse_canonical_version(&manifest.version).is_none()
        || !nonempty_plain_text(&manifest.name)
        || !manifest.description.as_deref().is_none_or(plain_text)
        || manifest.supported_platforms.is_empty()
        || has_duplicates(manifest.supported_platforms.iter().copied())
        || !valid_command_name(&manifest.command.default_name)
        || !manifest
            .command
            .summary
            .as_deref()
            .is_none_or(valid_command_summary)
        || (manifest.command.input_required
            && !manifest
                .command
                .input_placeholder
                .as_deref()
                .is_some_and(nonempty_plain_text))
        || !manifest
            .command
            .input_placeholder
            .as_deref()
            .is_none_or(plain_text)
        || !valid_entry(&manifest.runtime.entry, "js")
        || manifest
            .window
            .as_ref()
            .is_some_and(|window| !valid_entry(&window.entry, "html"))
        || manifest
            .panel
            .as_ref()
            .is_some_and(|panel| !valid_entry(&panel.entry, "html"))
        || manifest.panel.as_ref().is_some_and(|panel| {
            panel.host_keys.len() > 8 || has_duplicates(panel.host_keys.iter().copied())
        })
        || manifest.network.as_ref().is_some_and(|network| {
            network.https_hosts.is_empty()
                || network.https_hosts.len() > 8
                || has_duplicates(network.https_hosts.iter().map(String::as_str))
                || network
                    .https_hosts
                    .iter()
                    .any(|host| !valid_https_host(host))
        })
        || has_duplicates(manifest.permissions.iter().copied())
        || manifest.settings.iter().any(|setting| !setting.validate())
        || has_duplicates(manifest.settings.iter().map(PublicSettingV1::key))
    {
        return Err(PublicPackageError::InvalidPackage);
    }

    if manifest.network.is_some()
        != manifest
            .permissions
            .contains(&PublicPermission::NetworkHttps)
    {
        return Err(PublicPackageError::InvalidPackage);
    }

    match manifest.command.output_mode {
        PublicOutputMode::Window
            if manifest.command.activation_mode != PublicActivationMode::Submit
                || manifest.window.is_none()
                || manifest.panel.is_some()
                || !manifest.permissions.contains(&PublicPermission::UiWindow)
                || manifest.permissions.contains(&PublicPermission::UiPanel) =>
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        PublicOutputMode::Panel
            if manifest.command.activation_mode != PublicActivationMode::Submit
                || manifest.panel.is_none()
                || manifest.window.is_some()
                || !manifest.permissions.contains(&PublicPermission::UiPanel)
                || manifest.permissions.contains(&PublicPermission::UiWindow)
                || manifest
                    .permissions
                    .contains(&PublicPermission::TimerControl) =>
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        PublicOutputMode::MainResult
            if manifest.window.is_some()
                || manifest.panel.is_some()
                || manifest.permissions.contains(&PublicPermission::UiWindow)
                || manifest.permissions.contains(&PublicPermission::UiPanel) =>
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        _ => {}
    }

    if manifest
        .permissions
        .contains(&PublicPermission::TimerControl)
        && (manifest.command.activation_mode != PublicActivationMode::Submit
            || manifest.command.output_mode != PublicOutputMode::Window
            || manifest.window.is_none()
            || manifest.panel.is_some()
            || !manifest.permissions.contains(&PublicPermission::UiWindow)
            || manifest.permissions.contains(&PublicPermission::UiPanel)
            || !manifest
                .permissions
                .contains(&PublicPermission::NotificationsPublish))
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let has_clipboard_history_read = manifest
        .permissions
        .contains(&PublicPermission::ClipboardHistoryRead);
    let has_clipboard_history_paste = manifest
        .permissions
        .contains(&PublicPermission::ClipboardHistoryPaste);
    if (has_clipboard_history_read || has_clipboard_history_paste)
        && (manifest.command.activation_mode != PublicActivationMode::Submit
            || manifest.command.output_mode != PublicOutputMode::Panel
            || manifest.panel.is_none()
            || manifest.window.is_some()
            || !manifest.permissions.contains(&PublicPermission::UiPanel)
            || manifest.permissions.contains(&PublicPermission::UiWindow)
            || (has_clipboard_history_paste && !has_clipboard_history_read))
    {
        return Err(PublicPackageError::InvalidPackage);
    }

    if !manifest.supported_platforms.contains(&host.platform) {
        return Err(PublicPackageError::IncompatiblePlatform);
    }
    let minimum_host = parse_canonical_version(&manifest.minimum_host_version)
        .ok_or(PublicPackageError::InvalidPackage)?;
    if manifest.api_version != 1
        || manifest.api_version != host.api_version
        || minimum_host > host.version
    {
        return Err(PublicPackageError::IncompatibleApi);
    }
    if manifest.network.is_some() && minimum_host < [0, 3, 2] {
        return Err(PublicPackageError::IncompatibleApi);
    }
    if manifest.command.output_mode == PublicOutputMode::Panel && minimum_host < [0, 3, 0] {
        return Err(PublicPackageError::IncompatibleApi);
    }
    if manifest.panel.as_ref().is_some_and(|panel| {
        panel.host_keys.iter().any(|key| {
            matches!(
                key,
                PanelHostKeyDeclaration::Tab
                    | PanelHostKeyDeclaration::ShiftTab
                    | PanelHostKeyDeclaration::Enter
            )
        })
    }) && minimum_host < [0, 3, 3]
    {
        return Err(PublicPackageError::IncompatibleApi);
    }
    if manifest
        .panel
        .as_ref()
        .is_some_and(|panel| !panel.host_keys.is_empty())
        && minimum_host < [0, 3, 1]
    {
        return Err(PublicPackageError::IncompatibleApi);
    }
    if (has_clipboard_history_read || has_clipboard_history_paste) && minimum_host < [0, 3, 3] {
        return Err(PublicPackageError::IncompatibleApi);
    }
    if manifest
        .permissions
        .iter()
        .any(|permission| !permission.is_available(host.platform))
    {
        return Err(PublicPackageError::UnsupportedPermission);
    }
    Ok(())
}

pub(super) fn valid_https_host(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 253
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
        || !value.contains('.')
        || value == "localhost"
        || value.ends_with(".localhost")
        || value.ends_with(".local")
        || is_ipv4_literal(value)
    {
        return false;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && !label.starts_with("xn--")
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn is_ipv4_literal(value: &str) -> bool {
    let labels = value.split('.').collect::<Vec<_>>();
    labels.len() == 4
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.bytes().all(|byte| byte.is_ascii_digit())
                && label.parse::<u8>().is_ok()
        })
}

pub(super) fn parse_canonical_version(value: &str) -> Option<[u32; 3]> {
    let mut values = [0; 3];
    let mut parts = value.split('.');
    for value in &mut values {
        let part = parts.next()?;
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        *value = part.parse().ok()?;
    }
    parts.next().is_none().then_some(values)
}

pub(crate) fn valid_plugin_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

pub(super) fn valid_command_name(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(super) fn valid_storage_key(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
        && !matches!(value, "__proto__" | "prototype" | "constructor")
}

pub(super) fn valid_setting_key(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
}

fn valid_entry(value: &str, expected_extension: &str) -> bool {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.len() > 240
        || value.split('/').count() > 8
    {
        return false;
    }
    let mut components = value.split('/');
    let valid_components = components.by_ref().all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component.len() <= 100
            && !component.ends_with(['.', ' '])
            && !component.contains(':')
            && component.nfc().eq(component.chars())
    });
    let Some(basename) = value.rsplit('/').next() else {
        return false;
    };
    let mut basename_parts = basename.split('.');
    let stem = basename_parts.next().unwrap_or_default();
    let extension = basename_parts.next();
    valid_components
        && !stem.is_empty()
        && extension == Some(expected_extension)
        && basename_parts.next().is_none()
}

fn has_duplicates<T>(values: impl IntoIterator<Item = T>) -> bool
where
    T: Eq + std::hash::Hash,
{
    let mut seen = HashSet::new();
    values.into_iter().any(|value| !seen.insert(value))
}

fn nonempty_plain_text(value: &str) -> bool {
    !value.trim().is_empty() && plain_text(value)
}

fn valid_command_summary(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 512 && !value.chars().any(char::is_control)
}

fn plain_text(value: &str) -> bool {
    !value.chars().any(|character| {
        character == '\0' || (character.is_control() && !"\r\n\t".contains(character))
    })
}
#[cfg(test)]
mod schema_tests {
    use super::*;

    fn manifest(summary: Option<Value>) -> Value {
        let mut manifest = serde_json::json!({
            "schemaVersion": 1,
            "pluginId": "com.uipilot.summary",
            "version": "1.0.0",
            "apiVersion": 1,
            "minimumHostVersion": "0.2.0",
            "name": "Summary Demo",
            "supportedPlatforms": ["windows"],
            "command": {
                "defaultName": "summary",
                "activationMode": "submit",
                "outputMode": "mainResult",
                "inputRequired": false
            },
            "runtime": { "entry": "dist/runtime.js" },
            "permissions": [],
            "settings": []
        });
        if let Some(summary) = summary {
            manifest["command"]["summary"] = summary;
        }
        manifest
    }

    fn parse(value: &Value) -> Result<PublicManifestV1, PublicPackageError> {
        parse_manifest(
            &serde_json::to_vec(value).unwrap(),
            &PublicPluginHost::current(PublicPlatform::Windows),
        )
    }

    #[test]
    fn generated_schema_covers_manifest_commands_settings_and_window_contracts() {
        let schema = serde_json::to_value(public_manifest_v1_schema()).unwrap();
        let serialized = serde_json::to_string(&schema).unwrap();
        for required in [
            "PublicManifestV1",
            "PublicCommandV1",
            "PublicSettingV1",
            "PublicPermission",
            "PublicWindowV1",
            "PublicPanelV1",
            "PublicNetworkV1",
            "additionalProperties",
            "ui.window",
            "ui.panel",
            "network.https",
            "clipboard.write",
            "clipboard.history.read",
            "clipboard.history.paste",
            "timer.control",
            "Shift+Tab",
            "Enter",
        ] {
            assert!(
                serialized.contains(required),
                "missing schema fragment: {required}"
            );
        }
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn command_summary_is_optional_bounded_single_line_plain_text() {
        assert!(parse(&manifest(None)).is_ok());

        let parsed = parse(&manifest(Some(serde_json::json!("Run the summary demo")))).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["command"]["summary"],
            "Run the summary demo"
        );

        for invalid in [
            String::new(),
            "   ".into(),
            "line\nbreak".into(),
            "control\u{0007}".into(),
            "x".repeat(513),
        ] {
            assert_eq!(
                parse(&manifest(Some(Value::String(invalid)))),
                Err(PublicPackageError::InvalidPackage)
            );
        }
    }

    #[test]
    fn timer_control_requires_the_exact_windows_window_permission_bundle() {
        let mut valid = manifest(None);
        valid["supportedPlatforms"] = serde_json::json!(["windows"]);
        valid["command"] = serde_json::json!({
            "defaultName": "timer",
            "activationMode": "submit",
            "outputMode": "window",
            "inputRequired": false
        });
        valid["window"] = serde_json::json!({ "entry": "dist/window.html" });
        valid["permissions"] =
            serde_json::json!(["ui.window", "notifications.publish", "timer.control"]);
        assert!(parse(&valid).is_ok());

        for (label, candidate) in [
            ("missing-window-permission", {
                let mut candidate = valid.clone();
                candidate["permissions"] =
                    serde_json::json!(["notifications.publish", "timer.control"]);
                candidate
            }),
            ("missing-notification-permission", {
                let mut candidate = valid.clone();
                candidate["permissions"] = serde_json::json!(["ui.window", "timer.control"]);
                candidate
            }),
            ("wrong-output-mode", {
                let mut candidate = valid.clone();
                candidate["command"]["outputMode"] = serde_json::json!("mainResult");
                candidate
            }),
            ("wrong-activation-mode", {
                let mut candidate = valid.clone();
                candidate["command"]["activationMode"] = serde_json::json!("live");
                candidate
            }),
            ("missing-window-entry", {
                let mut candidate = valid.clone();
                candidate.as_object_mut().unwrap().remove("window");
                candidate
            }),
            ("panel-with-timer", {
                let mut candidate = valid.clone();
                candidate["command"]["outputMode"] = serde_json::json!("panel");
                candidate["panel"] = serde_json::json!({ "entry": "dist/panel.html" });
                candidate.as_object_mut().unwrap().remove("window");
                candidate["permissions"] =
                    serde_json::json!(["ui.panel", "notifications.publish", "timer.control"]);
                candidate
            }),
        ] {
            assert_eq!(
                parse(&candidate),
                Err(PublicPackageError::InvalidPackage),
                "{label}"
            );
        }

        let mut macos = valid;
        macos["supportedPlatforms"] = serde_json::json!(["macos"]);
        assert_eq!(
            parse_manifest(
                &serde_json::to_vec(&macos).unwrap(),
                &PublicPluginHost::current(PublicPlatform::Macos),
            ),
            Err(PublicPackageError::UnsupportedPermission)
        );
    }

    #[test]
    fn panel_output_mode_accepts_legal_matrix_and_rejects_illegal_combinations() {
        let mut valid = manifest(None);
        valid["minimumHostVersion"] = serde_json::json!("0.3.0");
        valid["command"] = serde_json::json!({
            "defaultName": "panel",
            "activationMode": "submit",
            "outputMode": "panel",
            "inputRequired": false
        });
        valid["panel"] = serde_json::json!({ "entry": "dist/panel.html" });
        valid["permissions"] = serde_json::json!(["ui.panel"]);
        assert!(parse(&valid).is_ok());

        let mut low_host = valid.clone();
        low_host["minimumHostVersion"] = serde_json::json!("0.2.0");
        assert_eq!(
            parse(&low_host),
            Err(PublicPackageError::IncompatibleApi),
            "panel package must require host 0.3.0"
        );

        for (label, candidate) in [
            ("live-activation", {
                let mut candidate = valid.clone();
                candidate["command"]["activationMode"] = serde_json::json!("live");
                candidate
            }),
            ("missing-panel-entry", {
                let mut candidate = valid.clone();
                candidate.as_object_mut().unwrap().remove("panel");
                candidate
            }),
            ("missing-ui-panel", {
                let mut candidate = valid.clone();
                candidate["permissions"] = serde_json::json!([]);
                candidate
            }),
            ("with-window-entry", {
                let mut candidate = valid.clone();
                candidate["window"] = serde_json::json!({ "entry": "dist/window.html" });
                candidate
            }),
            ("with-ui-window", {
                let mut candidate = valid.clone();
                candidate["permissions"] = serde_json::json!(["ui.panel", "ui.window"]);
                candidate
            }),
            ("with-timer-control", {
                let mut candidate = valid.clone();
                candidate["permissions"] =
                    serde_json::json!(["ui.panel", "notifications.publish", "timer.control"]);
                candidate
            }),
            ("main-result-with-panel", {
                let mut candidate = valid.clone();
                candidate["command"]["outputMode"] = serde_json::json!("mainResult");
                candidate["command"]["activationMode"] = serde_json::json!("live");
                candidate
            }),
            ("main-result-with-ui-panel", {
                let mut candidate = manifest(None);
                candidate["permissions"] = serde_json::json!(["ui.panel"]);
                candidate
            }),
        ] {
            assert_eq!(
                parse(&candidate),
                Err(PublicPackageError::InvalidPackage),
                "{label}"
            );
        }

        let mut older_host = valid.clone();
        older_host["minimumHostVersion"] = serde_json::json!("0.3.0");
        assert_eq!(
            parse_manifest(
                &serde_json::to_vec(&older_host).unwrap(),
                &PublicPluginHost {
                    platform: PublicPlatform::Windows,
                    version: [0, 2, 0],
                    api_version: 1,
                },
            ),
            Err(PublicPackageError::IncompatibleApi)
        );
    }

    #[test]
    fn clipboard_history_permissions_require_windows_panel_and_are_not_clipboard_read() {
        let mut valid = manifest(None);
        valid["minimumHostVersion"] = serde_json::json!("0.3.3");
        valid["command"] = serde_json::json!({
            "defaultName": "cliphist",
            "activationMode": "submit",
            "outputMode": "panel",
            "inputRequired": false
        });
        valid["panel"] = serde_json::json!({ "entry": "dist/panel.html" });
        valid["permissions"] = serde_json::json!([
            "ui.panel",
            "clipboard.history.read",
            "clipboard.history.paste"
        ]);
        assert!(parse(&valid).is_ok());

        let mut paste_without_read = valid.clone();
        paste_without_read["permissions"] =
            serde_json::json!(["ui.panel", "clipboard.history.paste"]);
        assert_eq!(
            parse(&paste_without_read),
            Err(PublicPackageError::InvalidPackage)
        );

        let mut read_on_main_result = valid.clone();
        read_on_main_result["command"]["outputMode"] = serde_json::json!("mainResult");
        read_on_main_result["panel"] = serde_json::Value::Null;
        read_on_main_result["permissions"] = serde_json::json!(["clipboard.history.read"]);
        assert_eq!(
            parse(&read_on_main_result),
            Err(PublicPackageError::InvalidPackage)
        );

        let mut reserved_clipboard_read = valid.clone();
        reserved_clipboard_read["permissions"] = serde_json::json!(["ui.panel", "clipboard.read"]);
        assert_eq!(
            parse(&reserved_clipboard_read),
            Err(PublicPackageError::UnsupportedPermission),
            "reserved clipboard.read must not alias clipboard.history.read"
        );

        let mut macos_candidate = valid.clone();
        macos_candidate["supportedPlatforms"] = serde_json::json!(["windows", "macos"]);
        assert_eq!(
            parse_manifest(
                &serde_json::to_vec(&macos_candidate).unwrap(),
                &PublicPluginHost {
                    platform: PublicPlatform::Macos,
                    version: [0, 3, 3],
                    api_version: 1,
                },
            ),
            Err(PublicPackageError::UnsupportedPermission)
        );

        let mut low_host = valid.clone();
        low_host["minimumHostVersion"] = serde_json::json!("0.3.2");
        assert_eq!(parse(&low_host), Err(PublicPackageError::IncompatibleApi));
    }

    #[test]
    fn panel_host_keys_are_strict_and_require_host_0_3_3_for_extended_keys() {
        let mut panel = manifest(None);
        panel["minimumHostVersion"] = serde_json::json!("0.3.3");
        panel["command"] = serde_json::json!({
            "defaultName": "panel",
            "activationMode": "submit",
            "outputMode": "panel",
            "inputRequired": false
        });
        panel["panel"] = serde_json::json!({
            "entry": "dist/panel.html",
            "hostKeys": ["Enter", "Shift+Tab", "Tab", "Primary+N", "ArrowUp", "ArrowDown"]
        });
        panel["permissions"] = serde_json::json!(["ui.panel"]);

        let parsed = parse(&panel).expect("known host keys are valid on 0.3.3");
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["panel"]["hostKeys"],
            serde_json::json!([
                "ArrowDown",
                "ArrowUp",
                "Primary+N",
                "Tab",
                "Shift+Tab",
                "Enter"
            ])
        );

        let mut low_host = panel.clone();
        low_host["minimumHostVersion"] = serde_json::json!("0.3.2");
        assert_eq!(parse(&low_host), Err(PublicPackageError::IncompatibleApi));

        low_host["panel"]["hostKeys"] = serde_json::json!(["ArrowDown", "Primary+N"]);
        low_host["minimumHostVersion"] = serde_json::json!("0.3.1");
        assert!(
            parse(&low_host).is_ok(),
            "existing host key declarations remain a 0.3.1 panel"
        );

        for (label, host_keys) in [
            ("unknown", serde_json::json!(["Space"])),
            ("duplicate", serde_json::json!(["ArrowDown", "ArrowDown"])),
            ("wrong-type", serde_json::json!("ArrowDown")),
            (
                "over-limit",
                serde_json::json!([
                    "ArrowDown",
                    "ArrowUp",
                    "Primary+N",
                    "ArrowDown",
                    "ArrowUp",
                    "Primary+N",
                    "ArrowDown",
                    "ArrowUp",
                    "Primary+N"
                ]),
            ),
        ] {
            let mut candidate = panel.clone();
            candidate["panel"]["hostKeys"] = host_keys;
            assert!(parse(&candidate).is_err(), "{label}");
        }
    }

    #[test]
    fn plugin_network_manifest_contract_is_strict_versioned_and_canonical() {
        let old_serialized = serde_json::to_value(parse(&manifest(None)).unwrap()).unwrap();
        assert!(old_serialized.get("network").is_none());
        assert!(parse(&old_serialized).is_ok());
        let schema = serde_json::to_value(public_manifest_v1_schema()).unwrap();
        assert!(schema["properties"]["network"].get("default").is_none());

        let mut one_host = manifest(None);
        one_host["minimumHostVersion"] = serde_json::json!("0.3.2");
        one_host["permissions"] = serde_json::json!(["network.https"]);
        one_host["network"] = serde_json::json!({ "httpsHosts": ["api.example.com"] });
        assert_eq!(
            serde_json::to_value(parse(&one_host).unwrap()).unwrap()["network"]["httpsHosts"],
            serde_json::json!(["api.example.com"])
        );

        let mut eight_hosts = one_host.clone();
        eight_hosts["network"]["httpsHosts"] = serde_json::json!([
            "h.example.com",
            "g.example.com",
            "f.example.com",
            "e.example.com",
            "d.example.com",
            "c.example.com",
            "b.example.com",
            "a.example.com"
        ]);
        assert_eq!(
            serde_json::to_value(parse(&eight_hosts).unwrap()).unwrap()["network"]["httpsHosts"],
            serde_json::json!([
                "a.example.com",
                "b.example.com",
                "c.example.com",
                "d.example.com",
                "e.example.com",
                "f.example.com",
                "g.example.com",
                "h.example.com"
            ])
        );

        let mut missing_permission = one_host.clone();
        missing_permission["permissions"] = serde_json::json!([]);
        assert_eq!(
            parse(&missing_permission),
            Err(PublicPackageError::InvalidPackage)
        );

        let mut missing_network = one_host.clone();
        missing_network.as_object_mut().unwrap().remove("network");
        assert_eq!(
            parse(&missing_network),
            Err(PublicPackageError::InvalidPackage)
        );

        let mut explicit_null = manifest(None);
        explicit_null["network"] = serde_json::Value::Null;
        assert_eq!(
            parse(&explicit_null),
            Err(PublicPackageError::InvalidPackage)
        );

        let mut low_host = one_host.clone();
        low_host["minimumHostVersion"] = serde_json::json!("0.3.1");
        assert_eq!(parse(&low_host), Err(PublicPackageError::IncompatibleApi));

        for network in [
            serde_json::Value::Null,
            serde_json::json!({ "httpsHosts": "api.example.com" }),
            serde_json::json!({ "httpsHosts": [] }),
            serde_json::json!({ "httpsHosts": ["api.example.com", "api.example.com"] }),
            serde_json::json!({ "httpsHosts": [
                "a.example.com", "b.example.com", "c.example.com", "d.example.com",
                "e.example.com", "f.example.com", "g.example.com", "h.example.com",
                "i.example.com"
            ] }),
            serde_json::json!({ "httpsHosts": ["api.example.com"], "unknown": true }),
        ] {
            let mut candidate = one_host.clone();
            candidate["network"] = network;
            assert_eq!(parse(&candidate), Err(PublicPackageError::InvalidPackage));
        }

        let mut macos = one_host;
        macos["supportedPlatforms"] = serde_json::json!(["windows", "macos"]);
        assert_eq!(
            parse_manifest(
                &serde_json::to_vec(&macos).unwrap(),
                &PublicPluginHost::current(PublicPlatform::Macos)
            ),
            Err(PublicPackageError::UnsupportedPermission)
        );
        assert_eq!(
            PublicPluginHost::current(PublicPlatform::Windows).version,
            [0, 3, 3]
        );
    }

    #[test]
    fn plugin_network_manifest_host_policy_matches_shared_fixtures() {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../docs/plugin-sdk/tests/network-host-fixtures.json"
        ))
        .unwrap();
        for fixture in fixtures.as_array().unwrap() {
            let mut candidate = manifest(None);
            candidate["minimumHostVersion"] = serde_json::json!("0.3.2");
            candidate["permissions"] = serde_json::json!(["network.https"]);
            candidate["network"] =
                serde_json::json!({ "httpsHosts": [fixture["host"].as_str().unwrap()] });
            assert_eq!(
                parse(&candidate).is_ok(),
                fixture["valid"].as_bool().unwrap(),
                "fixture {}",
                fixture["name"].as_str().unwrap()
            );
        }
    }
}
