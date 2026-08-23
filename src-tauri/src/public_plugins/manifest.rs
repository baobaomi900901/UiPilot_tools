use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
    #[serde(rename = "timer.control")]
    TimerControl,
    #[serde(rename = "background.schedule")]
    BackgroundSchedule,
}

impl PublicPermission {
    pub(super) fn is_available(self, platform: PublicPlatform) -> bool {
        matches!(self, Self::UiWindow | Self::ClipboardWrite)
            || (matches!(self, Self::NotificationsPublish | Self::TimerControl)
                && platform == PublicPlatform::Windows)
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

    if manifest
        .permissions
        .contains(&PublicPermission::TimerControl)
        && (manifest.command.activation_mode != PublicActivationMode::Submit
            || manifest.command.output_mode != PublicOutputMode::Window
            || manifest.window.is_none()
            || !manifest.permissions.contains(&PublicPermission::UiWindow)
            || !manifest
                .permissions
                .contains(&PublicPermission::NotificationsPublish))
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
    if manifest
        .permissions
        .iter()
        .any(|permission| !permission.is_available(host.platform))
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
            "additionalProperties",
            "ui.window",
            "clipboard.write",
            "timer.control",
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
}
