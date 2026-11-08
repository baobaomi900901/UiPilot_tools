//! Real plugin owner/value contract; active request authority remains the data gate.
use super::manifest::{self, PublicPackageError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginDataScope {
    plugin_id: String,
}

impl PluginDataScope {
    pub(crate) fn new(plugin_id: &str) -> Result<Self, PublicPackageError> {
        manifest::valid_plugin_id(plugin_id)
            .then(|| Self {
                plugin_id: plugin_id.into(),
            })
            .ok_or(PublicPackageError::InvalidPackage)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidPluginScope;

pub(super) fn authorize_plugin_scope(
    scope: &PluginDataScope,
    plugin_id: &str,
) -> Result<(), InvalidPluginScope> {
    (scope.plugin_id == plugin_id)
        .then_some(())
        .ok_or(InvalidPluginScope)
}

pub(super) fn valid_json_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => true,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(f64::is_finite),
        serde_json::Value::Array(values) => values.iter().all(valid_json_value),
        serde_json::Value::Object(values) => values.iter().all(|(key, value)| {
            !matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                && valid_json_value(value)
        }),
    }
}
