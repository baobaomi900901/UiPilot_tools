use super::{
    authorize_plugin_scope,
    manifest::{valid_plugin_id, valid_storage_key},
    valid_json_value, PluginDataScope,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

pub(super) const STORAGE_QUOTA_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginStorageError {
    Storage,
    InvalidPlugin,
    InvalidKey,
    InvalidValue,
    InvalidScope,
    QuotaExceeded,
}

impl fmt::Display for PluginStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Storage => "public plugin private storage failed",
            Self::InvalidPlugin => "public plugin private storage owner is invalid",
            Self::InvalidKey => "public plugin private storage key is invalid",
            Self::InvalidValue => "public plugin private storage value is invalid",
            Self::InvalidScope => "public plugin private storage scope is invalid",
            Self::QuotaExceeded => "public plugin private storage quota exceeded",
        })
    }
}

impl std::error::Error for PluginStorageError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StorageDocument {
    pub(super) schema: u32,
    pub(super) plugin_id: String,
    pub(super) values: BTreeMap<String, Value>,
}

pub(super) fn validate_access(
    scope: &PluginDataScope,
    plugin_id: &str,
    key: &str,
) -> Result<(), PluginStorageError> {
    authorize_plugin_scope(scope, plugin_id).map_err(|_| PluginStorageError::InvalidScope)?;
    if !valid_plugin_id(plugin_id) {
        return Err(PluginStorageError::InvalidPlugin);
    }
    if !valid_storage_key(key) {
        return Err(PluginStorageError::InvalidKey);
    }
    Ok(())
}

pub(super) fn parse_document(bytes: &[u8], plugin_id: &str) -> Option<StorageDocument> {
    let document = serde_json::from_slice::<StorageDocument>(bytes).ok()?;
    let values = serde_json::to_vec(&document.values).ok()?;
    (document.schema == 1
        && document.plugin_id == plugin_id
        && valid_plugin_id(&document.plugin_id)
        && values.len() <= STORAGE_QUOTA_BYTES
        && document
            .values
            .iter()
            .all(|(key, value)| valid_storage_key(key) && valid_json_value(value)))
    .then_some(document)
}

pub(super) fn check_values_quota(
    values: &BTreeMap<String, Value>,
) -> Result<(), PluginStorageError> {
    let bytes = serde_json::to_vec(values).map_err(|_| PluginStorageError::InvalidValue)?;
    if bytes.len() > STORAGE_QUOTA_BYTES {
        return Err(PluginStorageError::QuotaExceeded);
    }
    Ok(())
}
pub(super) fn encode_document(document: &StorageDocument) -> Result<Vec<u8>, PluginStorageError> {
    check_values_quota(&document.values)?;
    serde_json::to_vec(document).map_err(|_| PluginStorageError::InvalidValue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn shared_schema_encoding_and_owner_validation_stay_compatible() {
        let document = StorageDocument {
            schema: 1,
            plugin_id: "com.example.a".into(),
            values: BTreeMap::from([("v".into(), json!({"中文":[0,false,null,"😀"]}))]),
        };
        let raw = encode_document(&document).unwrap();
        assert_eq!(
            String::from_utf8(raw.clone()).unwrap(),
            r#"{"schema":1,"pluginId":"com.example.a","values":{"v":{"中文":[0,false,null,"😀"]}}}"#
        );
        assert_eq!(parse_document(&raw, "com.example.a"), Some(document));
        assert!(parse_document(&raw, "com.example.b").is_none());
        assert!(parse_document(
            br#"{"schema":1,"pluginId":"com.example.a","values":{},"extra":1}"#,
            "com.example.a"
        )
        .is_none());
        let scope = PluginDataScope::new("com.example.a").unwrap();
        assert_eq!(
            validate_access(&scope, "com.example.b", "v"),
            Err(PluginStorageError::InvalidScope)
        );
        for key in [
            "",
            "Upper",
            "__proto__",
            "prototype",
            "constructor",
            "has/slash",
        ] {
            assert_eq!(
                validate_access(&scope, "com.example.a", key),
                Err(PluginStorageError::InvalidKey)
            );
        }
    }
    #[test]
    fn maximum_owner_envelope_fits_mac_budget_without_changing_value_quota() {
        let id = "a".repeat(64);
        assert!(valid_plugin_id(&id));
        let mut document = StorageDocument {
            schema: 1,
            plugin_id: id,
            values: BTreeMap::new(),
        };
        document
            .values
            .insert("v".into(), json!("x".repeat(STORAGE_QUOTA_BYTES - 8)));
        let values = serde_json::to_vec(&document.values).unwrap();
        assert_eq!(values.len(), STORAGE_QUOTA_BYTES);
        assert!(encode_document(&document).unwrap().len() <= STORAGE_QUOTA_BYTES + 256);
        document.values.insert(
            "v".into(),
            json!("x".repeat(STORAGE_QUOTA_BYTES - 9) + "😀"),
        );
        assert_eq!(
            encode_document(&document),
            Err(PluginStorageError::QuotaExceeded)
        );
    }
}

// Mac's private current file is fail-closed; preserve Windows' existing parser
// and backup/quarantine behavior. Run this bounded raw check before Value can
// discard duplicate keys. serde_json's normal recursion limit remains enabled.
#[cfg(target_os = "macos")]
pub(super) fn parse_mac_document(bytes: &[u8], owner: &str) -> Option<StorageDocument> {
    validate_mac_json(bytes)?;
    parse_document(bytes, owner)
}

#[cfg(target_os = "macos")]
pub(super) fn validate_mac_json(bytes: &[u8]) -> Option<()> {
    validate_bounded_mac_json(bytes, STORAGE_QUOTA_BYTES + 256)
}

#[cfg(target_os = "macos")]
pub(super) fn validate_bounded_mac_json(bytes: &[u8], max_bytes: usize) -> Option<()> {
    use serde::de::{self, MapAccess, SeqAccess, Visitor};
    struct Unique;
    impl<'de> Deserialize<'de> for Unique {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Check;
            impl<'de> Visitor<'de> for Check {
                type Value = Unique;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("unique JSON keys")
                }
                fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_str<E: de::Error>(self, _: &str) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Unique, A::Error> {
                    while sequence.next_element::<Unique>()?.is_some() {}
                    Ok(Unique)
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
                    let mut keys = std::collections::HashSet::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if !keys.insert(key) {
                            return Err(de::Error::custom("duplicate key"));
                        }
                        map.next_value::<Unique>()?;
                    }
                    Ok(Unique)
                }
            }
            deserializer.deserialize_any(Check)
        }
    }
    if bytes.len() > max_bytes {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    Unique::deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(())
}
