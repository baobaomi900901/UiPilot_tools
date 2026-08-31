use serde::Serialize;

use crate::settings::BuiltinFeature;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResultIconKind {
    Find,
    Calculator,
    WebSearch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ResultFavoriteTarget {
    PublicPlugin {
        #[serde(rename = "pluginId")]
        plugin_id: String,
    },
    Builtin {
        feature: BuiltinFeature,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultFavorite {
    pub(crate) target: ResultFavoriteTarget,
    pub(crate) favorite: bool,
}

impl ResultFavorite {
    pub(crate) fn public_plugin(plugin_id: String, favorite: bool) -> Option<Self> {
        crate::public_plugins::valid_plugin_id(&plugin_id).then_some(Self {
            target: ResultFavoriteTarget::PublicPlugin { plugin_id },
            favorite,
        })
    }

    pub(crate) fn builtin(feature: BuiltinFeature, favorite: bool) -> Self {
        Self {
            target: ResultFavoriteTarget::Builtin { feature },
            favorite,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum LauncherResultActivation {
    Completion {
        #[serde(rename = "completionText")]
        completion_text: String,
    },
    WindowActivation {
        #[serde(rename = "pluginId")]
        plugin_id: String,
        #[serde(rename = "commandLabel")]
        command_label: String,
        #[serde(rename = "initialArgument")]
        initial_argument: String,
        favorite: bool,
    },
    MainResultActivation {
        #[serde(rename = "pluginId")]
        plugin_id: String,
        #[serde(rename = "commandLabel")]
        command_label: String,
        #[serde(rename = "initialArgument")]
        initial_argument: String,
        favorite: bool,
    },
    PanelActivation {
        #[serde(rename = "pluginId")]
        plugin_id: String,
        #[serde(rename = "initialArgument")]
        initial_argument: String,
        favorite: bool,
    },
    OpenQuicklinks,
    OpenFind {
        query: String,
    },
    ExecuteResult,
}

impl LauncherResultActivation {
    pub(crate) fn completion(completion_text: String) -> Option<Self> {
        valid_launcher_completion(&completion_text).then_some(Self::Completion { completion_text })
    }

    pub(crate) fn window_activation(
        plugin_id: String,
        command_label: String,
        initial_argument: String,
        favorite: bool,
    ) -> Option<Self> {
        (crate::public_plugins::valid_plugin_id(&plugin_id)
            && valid_launcher_command(&command_label)
            && valid_panel_initial_argument(&initial_argument))
        .then_some(Self::WindowActivation {
            plugin_id,
            command_label,
            initial_argument,
            favorite,
        })
    }

    pub(crate) fn main_result_activation(
        plugin_id: String,
        command_label: String,
        initial_argument: String,
        favorite: bool,
    ) -> Option<Self> {
        (crate::public_plugins::valid_plugin_id(&plugin_id)
            && valid_launcher_command(&command_label)
            && valid_panel_initial_argument(&initial_argument))
        .then_some(Self::MainResultActivation {
            plugin_id,
            command_label,
            initial_argument,
            favorite,
        })
    }

    pub(crate) fn panel_activation(
        plugin_id: String,
        initial_argument: String,
        favorite: bool,
    ) -> Option<Self> {
        (crate::public_plugins::valid_plugin_id(&plugin_id)
            && valid_panel_initial_argument(&initial_argument))
        .then_some(Self::PanelActivation {
            plugin_id,
            initial_argument,
            favorite,
        })
    }
}

pub(crate) fn valid_panel_initial_argument(value: &str) -> bool {
    value.len() <= 65_536
        && value.trim() == value
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
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
    if !valid_launcher_command(command) {
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

pub(crate) fn valid_launcher_command(command: &str) -> bool {
    !command.is_empty()
        && command.len() <= 32
        && command.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
        })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MainResultCommandContext {
    pub(crate) plugin_id: String,
    pub(crate) command_label: String,
    pub(crate) argument: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponse {
    pub(crate) request_id: String,
    pub(crate) items: Vec<ResultItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) command_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) main_result_command: Option<MainResultCommandContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) window_transfer_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auto_execute_result_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) favorite: Option<ResultFavorite>,
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
    fn window_activation_serializes_command_and_rejects_invalid_values() {
        let activation = LauncherResultActivation::window_activation(
            "com.uipilot.pomodoro".into(),
            "pomodoro".into(),
            "focus".into(),
            true,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(activation).unwrap(),
            serde_json::json!({
                "kind": "windowActivation",
                "pluginId": "com.uipilot.pomodoro",
                "commandLabel": "pomodoro",
                "initialArgument": "focus",
                "favorite": true
            })
        );
        assert!(LauncherResultActivation::window_activation(
            "Invalid Plugin".into(),
            "pomodoro".into(),
            "".into(),
            false,
        )
        .is_none());
        assert!(LauncherResultActivation::window_activation(
            "com.uipilot.pomodoro".into(),
            "Pomodoro".into(),
            "".into(),
            false,
        )
        .is_none());
        assert!(LauncherResultActivation::window_activation(
            "com.uipilot.pomodoro".into(),
            "pomodoro".into(),
            "bad\narg".into(),
            false,
        )
        .is_none());
    }

    #[test]
    fn panel_activation_serializes_initial_argument_and_rejects_invalid_identity() {
        let activation = LauncherResultActivation::panel_activation(
            "com.uipilot.demo-panel".into(),
            "hello".into(),
            true,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(activation).unwrap(),
            serde_json::json!({
                "kind": "panelActivation",
                "pluginId": "com.uipilot.demo-panel",
                "initialArgument": "hello",
                "favorite": true
            })
        );
        assert!(LauncherResultActivation::panel_activation(
            "Invalid Plugin".into(),
            "".into(),
            false,
        )
        .is_none());
        assert!(LauncherResultActivation::panel_activation(
            "com.uipilot.demo-panel".into(),
            "bad\narg".into(),
            false,
        )
        .is_none());
    }
}
