use super::protocol::{decode_frame, encode_frame, validate_request, Request, MAX_FRAME};
use std::io::{Read, Write};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalPipeName(String);

impl LocalPipeName {
    pub(crate) fn from_nonce(nonce: &str) -> Result<Self, &'static str> {
        if nonce.len() != 64
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("ProtocolRejected");
        }
        Ok(Self(format!(r"\\.\pipe\uipilot-elevation-{nonce}")))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn read_frame<R: Read, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<T, &'static str> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| "ProtocolRejected")?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME - 4 {
        return Err("ProtocolRejected");
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|_| "ProtocolRejected")?;
    let mut frame = Vec::with_capacity(MAX_FRAME.min(length + 4));
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&payload);
    decode_frame(&frame)
}

pub(crate) fn write_frame<W: Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), &'static str> {
    let frame = encode_frame(value)?;
    writer
        .write_all(&frame)
        .map_err(|_| "TransportUnavailable")?;
    writer.flush().map_err(|_| "TransportUnavailable")
}

pub(crate) fn read_validated_request<R: Read>(reader: &mut R) -> Result<Request, &'static str> {
    let request: Request = read_frame(reader)?;
    validate_request(&request)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_elevation::protocol::RootSelector;
    use crate::process_elevation::{Operation, Request, Scope};

    fn request() -> Request {
        Request {
            version: 1,
            request_id: "a".repeat(64),
            scope: Scope::Single,
            operation: Operation::Terminate,
            root: RootSelector {
                pid: 7,
                creation_time: 11,
            },
        }
    }
    #[test]
    fn bounded_reader_and_writer_round_trip_a_typed_frame() {
        let value = request();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        assert_eq!(
            read_frame::<_, Request>(&mut bytes.as_slice()).unwrap(),
            value
        );
    }
    #[test]
    fn rejects_declared_payload_before_allocating_it() {
        let header = (MAX_FRAME as u32).to_le_bytes();
        assert_eq!(
            read_frame::<_, Request>(&mut header.as_slice()),
            Err("ProtocolRejected")
        );
    }

    #[test]
    fn pipe_name_is_local_and_nonce_bound() {
        let name = LocalPipeName::from_nonce(&"a".repeat(64)).unwrap();
        assert_eq!(
            name.as_str(),
            r"\\.\pipe\uipilot-elevation-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(LocalPipeName::from_nonce(&"A".repeat(64)).is_err());
        assert!(LocalPipeName::from_nonce(&"a".repeat(63)).is_err());
    }

    #[test]
    fn request_reader_applies_semantic_validation_before_returning() {
        let invalid = Request {
            version: 1,
            request_id: "a".repeat(64),
            scope: Scope::Single,
            operation: Operation::Terminate,
            root: RootSelector {
                pid: 0,
                creation_time: 11,
            },
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &invalid).unwrap();
        assert_eq!(
            read_validated_request(&mut bytes.as_slice()),
            Err("ProtocolRejected")
        );
    }
}
