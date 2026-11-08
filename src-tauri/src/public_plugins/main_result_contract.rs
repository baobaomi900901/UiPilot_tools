//! Legacy main-result wire contract, shared without a platform runtime.
//! Input is already serde_json::Value: raw duplicate JSON fields must be
//! rejected by the transport before conversion. This parser grants no actions.
use super::{request_contract::PluginRequestContext, runtime_error::PluginRuntimeError};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicMainResult {
    pub(crate) plugin_result_id: String,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) copy_text: Option<String>,
}

pub(super) const MAX_MAIN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_MAIN_RESULTS: usize = 20;
const MAX_RESULT_TITLE_CHARS: usize = 256;
const MAX_RESULT_SUBTITLE_CHARS: usize = 512;
const MAX_RESULT_DETAIL_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MainResultResponseWire {
    request_id: String,
    results: Vec<MainResultWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MainResultWire {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    default_action: Option<CopyTextActionWire>,
}

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum CopyTextActionWire {
    #[serde(rename = "copyText")]
    CopyText { text: String },
}

pub(crate) fn parse_main_result_response(
    context: &PluginRequestContext,
    value: Value,
) -> Result<Vec<PublicMainResult>, PluginRuntimeError> {
    if serde_json::to_vec(&value)
        .map_err(|_| PluginRuntimeError::InvalidOperation)?
        .len()
        > MAX_MAIN_RESPONSE_BYTES
    {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    let response = serde_json::from_value::<MainResultResponseWire>(value)
        .map_err(|_| PluginRuntimeError::InvalidOperation)?;
    if response.request_id != context.request_id || response.results.len() > MAX_MAIN_RESULTS {
        return Err(PluginRuntimeError::InvalidOperation);
    }
    let mut ids = BTreeSet::new();
    response
        .results
        .into_iter()
        .map(|result| {
            if result.id.is_empty()
                || !ids.insert(result.id.clone())
                || result.title.chars().count() > MAX_RESULT_TITLE_CHARS
                || result
                    .subtitle
                    .as_ref()
                    .is_some_and(|value| value.chars().count() > MAX_RESULT_SUBTITLE_CHARS)
                || result
                    .detail
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_RESULT_DETAIL_BYTES)
            {
                return Err(PluginRuntimeError::InvalidOperation);
            }
            let copy_text = result.default_action.map(|action| match action {
                CopyTextActionWire::CopyText { text } => text,
            });
            Ok(PublicMainResult {
                plugin_result_id: result.id,
                title: result.title,
                subtitle: result.subtitle,
                detail: result.detail,
                copy_text,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "main_result_contract_tests.rs"]
mod tests;
