//! Shared bounded content messages and logical retirement, independent of UI ownership.
use serde_json::Value;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContentActionError {
    InvalidRequest,
    Busy,
    Expired,
    Cancelled,
    Timeout,
    InvalidResponse,
    Failed,
    Unavailable,
}
fn bounded(value: &Value, depth: usize, count: &mut usize) -> bool {
    *count += 1;
    if depth > 32 || *count > 4096 {
        return false;
    }
    match value {
        Value::Array(values) => values.iter().all(|v| bounded(v, depth + 1, count)),
        Value::Object(values) => values.iter().all(|(k, v)| {
            !matches!(k.as_str(), "__proto__" | "prototype" | "constructor")
                && bounded(v, depth + 1, count)
        }),
        _ => true,
    }
}
pub(crate) fn valid_json(value: &Value) -> bool {
    bounded(value, 0, &mut 0)
        && serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 64 * 1024)
}

impl From<super::panel_action::PanelActionError> for ContentActionError {
    fn from(error: super::panel_action::PanelActionError) -> Self {
        use super::panel_action::PanelActionError as P;
        match error {
            P::InvalidPanelRequestError => Self::InvalidRequest,
            P::PanelRequestBusyError => Self::Busy,
            P::ExpiredPanelSession => Self::Expired,
            P::PanelRequestCancelledError => Self::Cancelled,
            P::PanelRequestTimeoutError => Self::Timeout,
            P::InvalidPanelResponseError => Self::InvalidResponse,
            P::PanelRequestFailedError => Self::Failed,
            P::PanelRequestUnavailableError => Self::Unavailable,
        }
    }
}
impl From<ContentActionError> for super::panel_action::PanelActionError {
    fn from(error: ContentActionError) -> Self {
        use ContentActionError as E;
        match error {
            E::InvalidRequest => Self::InvalidPanelRequestError,
            E::Busy => Self::PanelRequestBusyError,
            E::Expired => Self::ExpiredPanelSession,
            E::Cancelled => Self::PanelRequestCancelledError,
            E::Timeout => Self::PanelRequestTimeoutError,
            E::InvalidResponse => Self::InvalidPanelResponseError,
            E::Failed => Self::PanelRequestFailedError,
            E::Unavailable => Self::PanelRequestUnavailableError,
        }
    }
}
