//! Wire DTOs shared without the WebView runtime or platform scheduler.
use super::manifest::valid_plugin_id;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginRequestContext {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) request_id: String,
}

impl PluginRequestContext {
    pub(super) fn valid_shape(&self) -> bool {
        valid_plugin_id(&self.plugin_id)
            && self.plugin_generation != 0
            && self
                .request_id
                .strip_prefix("public-request-")
                .is_some_and(|suffix| {
                    suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginInvocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) command_id: Option<String>,
    pub(crate) api_version: u32,
    pub(crate) request_id: String,
    pub(crate) input: String,
    pub(crate) context: PluginInvocationEnvironment,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginInvocationEnvironment {
    pub(crate) platform: PluginInvocationPlatform,
    pub(crate) theme: PluginInvocationTheme,
    pub(crate) invoked_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginInvocationPlatform {
    Windows,
    Macos,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginInvocationTheme {
    Dark,
    Light,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_invocation_wire_omits_command_id_and_rejects_unknown_fields() {
        let wire = json!({"apiVersion":1,"requestId":"public-request-0000000000000001","input":"中文  😀",
            "context":{"platform":"macos","theme":"dark","invokedAt":"2026-11-08T00:00:00Z"}});
        let invocation: PluginInvocation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&invocation).unwrap(), wire);
        let mut unknown = wire.clone();
        unknown["unknown"] = json!(true);
        assert!(serde_json::from_value::<PluginInvocation>(unknown).is_err());
        let mut command = wire;
        command["commandId"] = json!("command-one");
        let invocation: PluginInvocation = serde_json::from_value(command.clone()).unwrap();
        assert_eq!(serde_json::to_value(invocation).unwrap(), command);
    }
    #[test]
    fn request_context_retains_exact_shape_and_id_boundaries() {
        let valid = json!({"pluginId":"com.uipilot.fixture","pluginGeneration":1,"requestId":"public-request-0123456789abcdef"});
        assert!(
            serde_json::from_value::<PluginRequestContext>(valid.clone())
                .unwrap()
                .valid_shape()
        );
        for id in [
            "request-1",
            "public-request-0123456789abcde",
            "public-request-0123456789abcdef0",
            "public-request-0123456789abcdeg",
        ] {
            let mut wire = valid.clone();
            wire["requestId"] = json!(id);
            assert!(!serde_json::from_value::<PluginRequestContext>(wire)
                .unwrap()
                .valid_shape());
        }
        let mut unknown = valid;
        unknown["extra"] = json!(1);
        assert!(serde_json::from_value::<PluginRequestContext>(unknown).is_err());
    }
}
