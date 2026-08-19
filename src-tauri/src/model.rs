use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResultIconKind {
    Calculator,
    WebSearch,
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
    pub(crate) completion_text: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) has_default_action: bool,
}
