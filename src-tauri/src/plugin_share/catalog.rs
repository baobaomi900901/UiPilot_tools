use super::{types::PluginIdentity, ShareError};
use serde::Serialize;
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Receiver {
    pub(crate) plugin: PluginIdentity,
    pub(crate) plugin_name: String,
    pub(crate) receiver_id: String,
    pub(crate) title: String,
    pub(crate) command_id: String,
    pub(crate) max_bytes: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Choice {
    pub(crate) receiver: Receiver,
    pub(crate) disabled_reason: Option<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    pub(crate) items: Vec<Choice>,
    pub(crate) next_offset: Option<usize>,
}
pub(crate) fn discover(
    receivers: &[Receiver],
    source: &str,
    bytes: usize,
    search: &str,
    offset: usize,
) -> Result<Page, ShareError> {
    let query = search.to_lowercase();
    let mut choices: Vec<_> = receivers
        .iter()
        .filter(|r| r.plugin.plugin_id != source)
        .filter(|r| {
            query.is_empty()
                || [
                    &r.title,
                    &r.plugin_name,
                    &r.plugin.plugin_id,
                    &r.receiver_id,
                ]
                .iter()
                .any(|s| s.to_lowercase().contains(&query))
        })
        .map(|r| Choice {
            receiver: r.clone(),
            disabled_reason: (bytes > r.max_bytes).then_some("payloadTooLarge"),
        })
        .collect();
    choices.sort_by(|a, b| {
        (
            &a.receiver.plugin_name,
            &a.receiver.title,
            &a.receiver.plugin.plugin_id,
            &a.receiver.receiver_id,
        )
            .cmp(&(
                &b.receiver.plugin_name,
                &b.receiver.title,
                &b.receiver.plugin.plugin_id,
                &b.receiver.receiver_id,
            ))
    });
    if offset > choices.len() {
        return Err(ShareError::InvalidTicket);
    }
    let total = choices.len();
    let mut page = Page {
        items: vec![],
        next_offset: None,
    };
    for choice in choices.into_iter().skip(offset).take(128) {
        page.items.push(choice);
        let next = offset + page.items.len();
        page.next_offset = (next < total).then_some(next);
        if super::payload::compact_envelope(&page).is_err() {
            page.items.pop();
            if page.items.is_empty() {
                return Err(ShareError::EnvelopeTooLarge);
            }
            page.next_offset = Some(offset + page.items.len());
            break;
        }
    }
    Ok(page)
}
