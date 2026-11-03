use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PanelStatus {
    pub(crate) message: String,
    pub(crate) tone: StatusTone,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StatusTone {
    Info,
    Success,
    Error,
}

impl PanelStatus {
    pub(crate) fn valid(&self) -> bool {
        self.message.chars().count() <= 512 && !self.message.contains('\0')
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PanelStatusEvent {
    pub(crate) session_epoch: String,
    pub(crate) sequence: String,
    pub(crate) status: PanelStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_status_limits_and_schema() {
        let status = |message: String| PanelStatus {
            message,
            tone: StatusTone::Info,
        };
        assert!(status(String::new()).valid());
        assert!(status("字".repeat(512)).valid());
        assert!(!status("x".repeat(513)).valid());
        assert!(!status("a\0b".into()).valid());
        assert!(
            serde_json::from_str::<PanelStatus>(r#"{"message":"ok","tone":"danger"}"#).is_err()
        );
        assert!(serde_json::from_str::<PanelStatus>(
            r#"{"message":"ok","tone":"info","html":true}"#
        )
        .is_err());
    }
}
