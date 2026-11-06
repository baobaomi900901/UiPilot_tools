use super::ShareError;
use serde::{Deserialize, Serialize};

pub(crate) const BODY_LIMIT: usize = 16_384;
pub(crate) const ENVELOPE_LIMIT: usize = 65_536;

#[derive(Clone, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

impl Payload {
    pub(crate) fn validate(&self) -> Result<(), ShareError> {
        if self.kind != "text/plain" || self.text.trim().is_empty() || self.text.contains('\0') {
            return Err(ShareError::InvalidPayload);
        }
        if self.text.len() > BODY_LIMIT {
            return Err(ShareError::TooLarge);
        }
        Ok(())
    }
    pub(crate) fn as_text(&self) -> &str {
        &self.text
    }
    pub(crate) fn bytes(&self) -> usize {
        self.text.len()
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(self.text.as_bytes()).into()
    }
}

pub(crate) fn compact_envelope<T: Serialize>(value: &T) -> Result<String, ShareError> {
    let encoded = serde_json::to_string(value).map_err(|_| ShareError::InvalidPayload)?;
    if encoded.len() > ENVELOPE_LIMIT {
        return Err(ShareError::EnvelopeTooLarge);
    }
    Ok(encoded)
}

pub(crate) fn revision(value: &str) -> Result<u64, ShareError> {
    if value.starts_with('0') || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ShareError::InvalidRevision);
    }
    value.parse().map_err(|_| ShareError::InvalidRevision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(text: &str) -> Payload {
        serde_json::from_value(json!({"type":"text/plain", "text":text})).unwrap()
    }

    #[test]
    fn plugin_share_payload_preserves_text_and_enforces_exact_contract() {
        let original = "  中\n文  ";
        let p = payload(original);
        assert!(p.validate().is_ok());
        assert_eq!(p.text, original);
        for bad in ["", " \n\t", "a\0b"] {
            assert_eq!(payload(bad).validate(), Err(ShareError::InvalidPayload));
        }
        assert!(payload(&"中".repeat(5461)).validate().is_ok());
        assert!(payload(&"x".repeat(BODY_LIMIT)).validate().is_ok());
        assert_eq!(
            payload(&"x".repeat(BODY_LIMIT + 1)).validate(),
            Err(ShareError::TooLarge)
        );
        let bad_type: Payload =
            serde_json::from_value(json!({"type":"text/html","text":"x"})).unwrap();
        assert_eq!(bad_type.validate(), Err(ShareError::InvalidPayload));
        assert!(
            serde_json::from_str::<Payload>(r#"{"type":"text/plain","text":"\uD800"}"#).is_err()
        );
        assert!(serde_json::from_value::<Payload>(
            json!({"type":"text/plain","text":"x","extra":0})
        )
        .is_err());
    }

    #[test]
    fn plugin_share_entire_compact_envelope_is_bounded_in_utf8() {
        let legal = payload(&format!("x{}", "\u{0001}".repeat(11_000)));
        assert!(legal.validate().is_ok());
        assert!(matches!(
            compact_envelope(&json!({"payload":legal,"ticket":"t"})),
            Err(ShareError::EnvelopeTooLarge)
        ));
        assert!(compact_envelope(&"x".repeat(ENVELOPE_LIMIT - 2)).is_ok());
        assert!(compact_envelope(&"x".repeat(ENVELOPE_LIMIT - 1)).is_err());
    }

    #[test]
    fn plugin_share_revision_is_canonical_nonzero_u64() {
        for bad in ["0", "01", "+1", "-1", " 1", "18446744073709551616"] {
            assert_eq!(revision(bad), Err(ShareError::InvalidRevision));
        }
        assert_eq!(revision("18446744073709551615"), Ok(u64::MAX));
    }
}
