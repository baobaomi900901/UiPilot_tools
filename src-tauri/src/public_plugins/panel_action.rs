//! Bounded panel business messages. Never log request or response contents.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum PanelActionError {
    InvalidPanelRequestError,
    PanelRequestBusyError,
    ExpiredPanelSession,
    PanelRequestCancelledError,
    PanelRequestTimeoutError,
    InvalidPanelResponseError,
    PanelRequestFailedError,
    PanelRequestUnavailableError,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PanelActionRequest {
    pub(crate) action: String,
    pub(crate) payload: Value,
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
fn valid_json(value: &Value) -> bool {
    bounded(value, 0, &mut 0)
        && serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 64 * 1024)
}
pub(crate) fn parse_epoch(value: &str) -> Result<u64, PanelActionError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|v| *v != 0 && v.to_string() == value)
        .ok_or(PanelActionError::ExpiredPanelSession)
}
impl PanelActionRequest {
    pub(crate) fn from_value(value: Value) -> Result<Self, PanelActionError> {
        if !value
            .as_object()
            .is_some_and(|o| o.len() == 2 && o.contains_key("action") && o.contains_key("payload"))
        {
            return Err(PanelActionError::InvalidPanelRequestError);
        }
        let request: Self = serde_json::from_value(value)
            .map_err(|_| PanelActionError::InvalidPanelRequestError)?;
        request.validate()?;
        Ok(request)
    }

    pub(crate) fn validate(&self) -> Result<(), PanelActionError> {
        let bytes = self.action.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 64
            || !bytes[0].is_ascii_lowercase()
            || !bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
            || matches!(
                self.action.as_str(),
                "__proto__" | "prototype" | "constructor"
            )
            || !valid_json(&serde_json::json!({"action": self.action, "payload": self.payload}))
        {
            return Err(PanelActionError::InvalidPanelRequestError);
        }
        Ok(())
    }
}
pub(crate) fn response_data(request_id: &str, value: Value) -> Result<Value, PanelActionError> {
    if !valid_json(&value) {
        return Err(PanelActionError::InvalidPanelResponseError);
    }
    let object = value
        .as_object()
        .ok_or(PanelActionError::InvalidPanelResponseError)?;
    if object.len() != 2
        || object.get("requestId").and_then(Value::as_str) != Some(request_id)
        || !object.contains_key("data")
    {
        return Err(PanelActionError::InvalidPanelResponseError);
    }
    Ok(object["data"].clone())
}
pub(crate) const JSON_BOOTSTRAP: &str = include_str!("panel_action_json.js");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_action_bounds_and_exact_reply() {
        assert!(PanelActionRequest::from_value(serde_json::json!(["save", null])).is_err());
        assert!(PanelActionRequest::from_value(serde_json::json!({"action":"save"})).is_err());
        assert!(PanelActionRequest::from_value(
            serde_json::json!({"action":"save", "payload":null, "extra":1})
        )
        .is_err());
        for epoch in ["0", "01", "+1", "-1", " 1", "18446744073709551616"] {
            assert!(parse_epoch(epoch).is_err());
        }
        assert_eq!(parse_epoch("1"), Ok(1));
        let input = PanelActionRequest {
            action: "save".into(),
            payload: Value::Null,
        };
        assert!(input.validate().is_ok());
        for action in ["", "Save", "constructor", "prototype", "a b"] {
            assert!(PanelActionRequest {
                action: action.into(),
                ..input.clone()
            }
            .validate()
            .is_err());
        }
        assert_eq!(
            response_data("id", serde_json::json!({"requestId":"id","data":null})),
            Ok(Value::Null)
        );
        assert!(response_data("id", serde_json::json!({"requestId":"wrong","data":null})).is_err());
        assert!(response_data(
            "id",
            serde_json::json!({"requestId":"id","data":{"constructor":1}})
        )
        .is_err());
        assert!(PanelActionRequest {
            payload: serde_json::json!(vec![0; 4096]),
            ..input.clone()
        }
        .validate()
        .is_err());
        assert!(PanelActionRequest {
            payload: serde_json::json!("x".repeat(65536)),
            ..input.clone()
        }
        .validate()
        .is_err());
        let mut nested = Value::Null;
        for _ in 0..33 {
            nested = serde_json::json!([nested]);
        }
        assert!(PanelActionRequest {
            payload: nested,
            ..input
        }
        .validate()
        .is_err());
    }
}
