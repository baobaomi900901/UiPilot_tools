use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const MAX_FRAME: usize = 8 * 1024 * 1024;

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
    if request.request_id.len() != 64
        || !request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("ProtocolRejected");
    }
    Ok(())
}
