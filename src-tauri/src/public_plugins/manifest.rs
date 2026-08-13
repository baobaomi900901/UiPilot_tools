use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use unicode_normalization::UnicodeNormalization;

use super::{PublicPackageError, PublicPluginHost};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PublicPlatform {
    Windows,
    Macos,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum PublicPermission {
    #[serde(rename = "ui.window")]
    UiWindow,
    #[serde(rename = "clipboard.write")]
    ClipboardWrite,
    #[serde(rename = "clipboard.read")]
    ClipboardRead,
    #[serde(rename = "network.https")]
    NetworkHttps,
    #[serde(rename = "files.userSelected")]
    FilesUserSelected,
    #[serde(rename = "files.index.readAll")]
    FilesIndexReadAll,
    #[serde(rename = "notifications.publish")]
    NotificationsPublish,
    #[serde(rename = "background.schedule")]
    BackgroundSchedule,
}

impl PublicPermission {
    pub(super) fn is_available(self) -> bool {
        matches!(self, Self::UiWindow | Self::ClipboardWrite)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublicActivationMode {
    Live,
    Submit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublicOutputMode {
    MainResult,
    Window,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicCommandV1 {
    pub(crate) default_name: String,
    pub(crate) activation_mode: PublicActivationMode,
    pub(crate) output_mode: PublicOutputMode,
    pub(crate) input_required: bool,
    #[serde(default)]
    pub(crate) input_placeholder: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicRuntimeV1 {
    pub(crate) entry: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicWindowV1 {
    pub(crate) entry: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicSelectOptionV1 {
    pub(crate) value: String,
    pub(crate) label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    pub(crate) permissions: Vec<PublicPermission>,
    #[serde(default)]
    pub(crate) settings: Vec<PublicSettingV1>,
}

pub(super) fn parse_manifest(
    bytes: &[u8],
    host: &PublicPluginHost,
) -> Result<PublicManifestV1, PublicPackageError> {
    let manifest: PublicManifestV1 =
        serde_json::from_slice(bytes).map_err(|_| PublicPackageError::InvalidPackage)?;
    validate_manifest(&manifest, host)?;
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
        || has_duplicates(manifest.permissions.iter().copied())
        || manifest.settings.iter().any(|setting| !setting.validate())
        || has_duplicates(manifest.settings.iter().map(PublicSettingV1::key))
    {
        return Err(PublicPackageError::InvalidPackage);
    }

    match manifest.command.output_mode {
        PublicOutputMode::Window
            if manifest.command.activation_mode != PublicActivationMode::Submit
                || manifest.window.is_none()
                || !manifest.permissions.contains(&PublicPermission::UiWindow) =>
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        PublicOutputMode::MainResult
            if manifest.window.is_some()
                || manifest.permissions.contains(&PublicPermission::UiWindow) =>
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        _ => {}
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
    if manifest
        .permissions
        .iter()
        .any(|permission| !permission.is_available())
    {
        return Err(PublicPackageError::UnsupportedPermission);
    }
    Ok(())
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

pub(super) fn valid_plugin_id(value: &str) -> bool {
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
    !matches!(value, "__proto__" | "prototype" | "constructor")
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

fn plain_text(value: &str) -> bool {
    !value.chars().any(|character| {
        character == '\0' || (character.is_control() && !"\r\n\t".contains(character))
    })
}
