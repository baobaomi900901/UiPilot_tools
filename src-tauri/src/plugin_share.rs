//! Bounded, in-memory state for one ordinary Panel share. No runtime preparation.
use crate::plugin_panel::PanelSessionIdentity;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    pub(crate) r#type: String,
    pub(crate) text: String,
}
impl Payload {
    pub(crate) fn valid(&self) -> bool {
        self.r#type == "text/plain"
            && !self.text.trim().is_empty()
            && !self.text.contains('\0')
            && self.text.len() <= 16384
    }
}
pub(crate) fn envelope<T: Serialize>(value: &T) -> bool {
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 65536)
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Receiver {
    pub(crate) plugin_id: String,
    pub(crate) plugin_name: String,
    pub(crate) receiver_id: String,
    pub(crate) title: String,
    pub(crate) command_id: String,
    pub(crate) max_bytes: usize,
    pub(crate) generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) disabled_reason: Option<&'static str>,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    Choosing,
    Opening,
    Delivering,
    Accepted,
    Error,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    pub(crate) transfer_id: String,
    pub(crate) source_epoch: String,
    pub(crate) phase: Phase,
    pub(crate) receivers: Vec<Receiver>,
    pub(crate) target: Option<crate::commands::PluginPanelCommandResult>,
    pub(crate) target_label: Option<String>,
    pub(crate) error: Option<&'static str>,
}
pub(crate) struct Session {
    pub(crate) source: PanelSessionIdentity,
    pub(crate) payload: Option<Payload>,
    pub(crate) view: View,
    pub(crate) selected: Option<Receiver>,
    pub(crate) target: Option<PanelSessionIdentity>,
    pub(crate) request_id: Option<String>,
    pub(crate) claimed: bool,
    pub(crate) deadline: Instant,
}
impl Session {
    pub(crate) fn new(
        id: u64,
        source: PanelSessionIdentity,
        payload: Payload,
        mut rows: Vec<Receiver>,
    ) -> Self {
        rows.retain(|r| r.plugin_id != source.plugin_id);
        for r in &mut rows {
            if payload.text.len() > r.max_bytes {
                r.disabled_reason = Some("文字超过此接收者上限");
            }
        }
        rows.sort_by(|a, b| {
            (&a.plugin_name, &a.title, &a.plugin_id, &a.receiver_id).cmp(&(
                &b.plugin_name,
                &b.title,
                &b.plugin_id,
                &b.receiver_id,
            ))
        });
        Self {
            view: View {
                transfer_id: id.to_string(),
                source_epoch: source.session_epoch.to_string(),
                phase: Phase::Choosing,
                receivers: rows,
                target: None,
                target_label: None,
                error: None,
            },
            source,
            payload: Some(payload),
            selected: None,
            target: None,
            request_id: None,
            claimed: false,
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }
    pub(crate) fn locked(&self) -> bool {
        matches!(
            self.view.phase,
            Phase::Choosing | Phase::Opening | Phase::Delivering | Phase::Error
        )
    }
    pub(crate) fn fail(&mut self) {
        self.payload = None;
        self.view.phase = Phase::Error;
        self.view.receivers.clear();
        self.view.error = Some("分享接收失败，请关闭目标后重新分享");
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    pub(crate) version: u8,
    pub(crate) transfer_id: String,
    pub(crate) receiver_id: String,
    pub(crate) session_epoch: String,
    pub(crate) payload: Payload,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_share_payload_keeps_whitespace_and_utf8_limits() {
        let p = Payload {
            r#type: "text/plain".into(),
            text: "  edited\n文字  ".into(),
        };
        assert!(p.valid());
        assert_eq!(p.text, "  edited\n文字  ");
        for text in [" ".into(), "a\0b".into(), "字".repeat(5462)] {
            assert!(!Payload { text, ..p.clone() }.valid());
        }
        assert!(Payload {
            text: "x".repeat(16384),
            ..p.clone()
        }
        .valid());
        assert!(!envelope(&Payload {
            text: "\u{1}".repeat(16384),
            ..p
        }));
    }
}
