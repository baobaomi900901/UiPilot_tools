use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResultIconKind {
    Find,
    Calculator,
    WebSearch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum LauncherResultActivation {
    Completion {
        #[serde(rename = "completionText")]
        completion_text: String,
    },
    PluginCompletion {
        #[serde(rename = "completionText")]
        completion_text: String,
        #[serde(rename = "pluginId")]
        plugin_id: String,
        favorite: bool,
    },
    OpenFind {
        query: String,
    },
    ExecuteResult,
}

impl LauncherResultActivation {
    pub(crate) fn completion(completion_text: String) -> Option<Self> {
        valid_launcher_completion(&completion_text).then_some(Self::Completion { completion_text })
    }

    pub(crate) fn plugin_completion(
        completion_text: String,
        plugin_id: String,
        favorite: bool,
    ) -> Option<Self> {
        (valid_launcher_completion(&completion_text)
            && crate::public_plugins::valid_plugin_id(&plugin_id))
        .then_some(Self::PluginCompletion {
            completion_text,
            plugin_id,
            favorite,
        })
    }
}

pub(crate) fn valid_launcher_completion(value: &str) -> bool {
    if value.len() > 65_536 {
        return false;
    }
    let Some(value) = value.strip_prefix('/') else {
        return false;
    };
    let Some((command, argument)) = value.split_once(' ') else {
        return false;
    };
    if command.is_empty()
        || command.len() > 32
        || !command.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
        })
    {
        return false;
    }
    if argument.is_empty() {
        return true;
    }
    argument.trim() == argument
        && !argument
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponse {
    pub(crate) request_id: String,
    pub(crate) items: Vec<ResultItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) command_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) window_transfer_token: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) replace_local_results: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultItem {
    pub(crate) result_id: String,
    pub(crate) activation: LauncherResultActivation,
    pub(crate) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plugin_icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon_kind: Option<ResultIconKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) has_default_action: bool,
}

#[cfg(test)]
mod tests {
    use super::{valid_launcher_completion, LauncherResultActivation};

    #[test]
    fn launcher_completion_accepts_only_the_frozen_single_line_grammar() {
        let boundary = format!("/d {}", "a".repeat(65_533));
        let oversized = format!("/d {}", "a".repeat(65_534));
        let cases = [
            ("/demo-win ", true),
            ("/demo-win da", true),
            ("/demo-win da  value", true),
            (boundary.as_str(), true),
            (oversized.as_str(), false),
            ("/Demo-win ", false),
            ("/demo_win ", false),
            ("/demo-win", false),
            ("/demo-win  da", false),
            ("/demo-win da ", false),
            ("/demo-win \0", false),
            ("/demo-win da\rvalue", false),
            ("/demo-win da\nvalue", false),
            ("/demo-win da\u{2028}value", false),
            ("/demo-win da\u{2029}value", false),
            ("/demo-win da\u{0085}value", false),
        ];

        for (completion, expected) in cases {
            assert_eq!(
                valid_launcher_completion(completion),
                expected,
                "unexpected result for {completion:?}"
            );
        }
    }

    #[test]
    fn favorite_plugin_completion_serializes_identity_and_rejects_invalid_ownership() {
        let activation = LauncherResultActivation::plugin_completion(
            "/demo-win value".into(),
            "com.uipilot.demo-win".into(),
            true,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(activation).unwrap(),
            serde_json::json!({
                "kind": "pluginCompletion",
                "completionText": "/demo-win value",
                "pluginId": "com.uipilot.demo-win",
                "favorite": true
            })
        );
        assert!(LauncherResultActivation::plugin_completion(
            "/demo-win\nvalue".into(),
            "com.uipilot.demo-win".into(),
            false,
        )
        .is_none());
        assert!(LauncherResultActivation::plugin_completion(
            "/demo-win value".into(),
            "Invalid Plugin".into(),
            false,
        )
        .is_none());
    }
}
