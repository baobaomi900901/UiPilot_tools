use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const MAX_FRAME: usize = 8 * 1024 * 1024;
const REQUEST_ID_BYTES: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub version: u16,
    pub request_id: String,
    pub scope: Scope,
    pub operation: Operation,
    pub root: RootSelector,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootSelector {
    pub pid: u32,
    pub creation_time: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Single,
    Tree,
    SameName,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Terminate,
    ForceTerminate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cancel {
    pub version: u16,
    pub request_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Terminal {
    pub version: u16,
    pub request_id: String,
    pub result: TerminalResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum TerminalResult {
    Cancelled,
    Rejected {
        code: StableError,
    },
    Completed {
        observed: u32,
        already_exited: u32,
        failed: u32,
        not_attempted: u32,
    },
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StableError {
    ProtocolRejected,
    SessionExpired,
    AccountMismatch,
    PeerRejected,
    PayloadTooLarge,
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, &'static str> {
    let payload = serde_json::to_vec(value).map_err(|_| "EncodeFailed")?;
    if payload.is_empty() || payload.len() > MAX_FRAME - 4 {
        return Err("PayloadTooLarge");
    }
    let length = u32::try_from(payload.len()).map_err(|_| "PayloadTooLarge")?;
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, &'static str> {
    if frame.len() < 4 || frame.len() > MAX_FRAME {
        return Err("ProtocolRejected");
    }
    let length =
        u32::from_le_bytes(frame[..4].try_into().map_err(|_| "ProtocolRejected")?) as usize;
    if length == 0 || length > MAX_FRAME - 4 || frame.len() != length + 4 {
        return Err("ProtocolRejected");
    }
    serde_json::from_slice(&frame[4..]).map_err(|_| "ProtocolRejected")
}

pub fn validate_request(request: &Request) -> Result<(), &'static str> {
    if request.version != 1 || request.root.pid == 0 || request.root.creation_time == 0 {
        return Err("ProtocolRejected");
    }
    if request.request_id.len() != REQUEST_ID_BYTES * 2
        || !request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("ProtocolRejected");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            version: 1,
            request_id: "a".repeat(64),
            scope: Scope::Single,
            operation: Operation::Terminate,
            root: RootSelector {
                pid: 42,
                creation_time: 99,
            },
        }
    }
    #[test]
    fn valid_frame_round_trips_exact_integer_and_variant_data() {
        let value = request();
        let decoded: Request = decode_frame(&encode_frame(&value).unwrap()).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(decoded.root.creation_time, 99);
    }
    #[test]
    fn rejects_unknown_fields_and_restart_payload_without_effects() {
        let unknown = br#"{"version":1,"requestId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","scope":"single","operation":"terminate","root":{"pid":42,"creationTime":99},"extra":true}"#;
        assert_eq!(
            decode_frame::<Request>(&[34, 0, 0, 0]).unwrap_err(),
            "ProtocolRejected"
        );
        let mut frame = (unknown.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(unknown);
        assert_eq!(
            decode_frame::<Request>(&frame).unwrap_err(),
            "ProtocolRejected"
        );
        let restart = br#"{"version":1,"requestId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","scope":"single","operation":"restart","root":{"pid":42,"creationTime":99}}"#;
        let mut frame = (restart.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(restart);
        assert_eq!(
            decode_frame::<Request>(&frame).unwrap_err(),
            "ProtocolRejected"
        );
    }
    #[test]
    fn rejects_truncated_oversized_trailing_and_empty_frames_before_allocation() {
        assert_eq!(
            decode_frame::<Request>(&[]).unwrap_err(),
            "ProtocolRejected"
        );
        assert_eq!(
            decode_frame::<Request>(&[3, 0, 0, 0, b'{']).unwrap_err(),
            "ProtocolRejected"
        );
        assert_eq!(
            decode_frame::<Request>(&((MAX_FRAME as u32).to_le_bytes())).unwrap_err(),
            "ProtocolRejected"
        );
        let mut frame = encode_frame(&request()).unwrap();
        frame.push(0);
        assert_eq!(
            decode_frame::<Request>(&frame).unwrap_err(),
            "ProtocolRejected"
        );
    }
    #[test]
    fn validates_exact_version_identity_and_nonzero_selector() {
        let mut value = request();
        assert_eq!(validate_request(&value), Ok(()));
        value.version = 2;
        assert_eq!(validate_request(&value), Err("ProtocolRejected"));
        value = request();
        value.root.pid = 0;
        assert_eq!(validate_request(&value), Err("ProtocolRejected"));
        value = request();
        value.request_id = "A".repeat(64);
        assert_eq!(validate_request(&value), Err("ProtocolRejected"));
    }
    #[test]
    fn rejects_payload_that_would_exceed_frame_bound() {
        let value = "x".repeat(MAX_FRAME);
        assert_eq!(encode_frame(&value).unwrap_err(), "PayloadTooLarge");
    }
}
