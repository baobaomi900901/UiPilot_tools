//! Window public errors; JSON limits and validation are shared with Panel.
use super::content_action::ContentActionError;
pub(crate) use super::panel_action::PanelActionRequest as WindowActionRequest;
use serde::Serialize;
use serde_json::Value;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum WindowActionError {
    InvalidWindowRequestError,
    WindowRequestBusyError,
    ExpiredWindowSessionError,
    WindowRequestCancelledError,
    WindowRequestTimeoutError,
    InvalidWindowResponseError,
    WindowRequestFailedError,
    WindowRequestUnavailableError,
}
impl From<ContentActionError> for WindowActionError {
    fn from(error: ContentActionError) -> Self {
        use ContentActionError as E;
        match error {
            E::InvalidRequest => Self::InvalidWindowRequestError,
            E::Busy => Self::WindowRequestBusyError,
            E::Expired => Self::ExpiredWindowSessionError,
            E::Cancelled => Self::WindowRequestCancelledError,
            E::Timeout => Self::WindowRequestTimeoutError,
            E::InvalidResponse => Self::InvalidWindowResponseError,
            E::Failed => Self::WindowRequestFailedError,
            E::Unavailable => Self::WindowRequestUnavailableError,
        }
    }
}
impl From<super::panel_action::PanelActionError> for WindowActionError {
    fn from(error: super::panel_action::PanelActionError) -> Self {
        ContentActionError::from(error).into()
    }
}
pub(crate) fn parse_session(value: &str) -> Result<u64, WindowActionError> {
    super::panel_action::parse_epoch(value).map_err(Into::into)
}
pub(crate) fn parse_request(value: Value) -> Result<WindowActionRequest, WindowActionError> {
    WindowActionRequest::from_value(value).map_err(Into::into)
}
