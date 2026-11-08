//! Shared ordinary-setting rules. Persistence and request authorization remain
//! the caller's responsibility; secrets never enter this value path.
use std::collections::BTreeMap;

use serde_json::Value;

use super::manifest::PublicSettingV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InvalidSetting;

pub(super) fn public_definition<'a>(
    definitions: &'a [PublicSettingV1],
    key: &str,
) -> Result<&'a PublicSettingV1, InvalidSetting> {
    let definition = definitions
        .iter()
        .find(|definition| definition.key() == key)
        .ok_or(InvalidSetting)?;
    if definition.is_secret() {
        return Err(InvalidSetting);
    }
    Ok(definition)
}

pub(super) fn validate_updates(
    definitions: &[PublicSettingV1],
    updates: &BTreeMap<String, Value>,
) -> Result<(), InvalidSetting> {
    for (key, value) in updates {
        if !public_definition(definitions, key)?.accepts_value(value) {
            return Err(InvalidSetting);
        }
    }
    Ok(())
}

pub(super) fn stored_value(
    values: &BTreeMap<String, Value>,
    definition: &PublicSettingV1,
) -> Option<Value> {
    values
        .get(definition.key())
        .cloned()
        .or_else(|| definition.default_value())
}

pub(super) fn reconcile_settings(
    previous: Option<&BTreeMap<String, Value>>,
    definitions: &[PublicSettingV1],
) -> BTreeMap<String, Value> {
    let mut settings = BTreeMap::new();
    for definition in definitions {
        if definition.is_secret() {
            continue;
        }
        let value = previous
            .and_then(|values| values.get(definition.key()))
            .filter(|value| definition.accepts_value(value))
            .cloned()
            .or_else(|| definition.default_value());
        if let Some(value) = value {
            settings.insert(definition.key().into(), value);
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn definitions() -> Vec<PublicSettingV1> {
        serde_json::from_value(json!([
            {"type":"text", "key":"title", "label":"Title", "default":"default"},
            {"type":"number", "key":"count", "label":"Count", "min":0,"max":10,"step":2,"default":4},
            {"type":"boolean", "key":"enabled", "label":"Enabled", "default":true},
            {"type":"select", "key":"color", "label":"Color", "options":[
                {"label":"Red","value":"red"},{"label":"Blue","value":"blue"}],"default":"red"},
            {"type":"text", "key":"optional", "label":"Optional"},
            {"type":"secret", "key":"token", "label":"Token"}
        ])).unwrap()
    }

    fn values(value: Value) -> BTreeMap<String, Value> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn upgrade_retains_compatible_values_and_removes_obsolete_or_secret_values() {
        let before = values(json!({"title":"中文 😀", "count":0,"enabled":false,
            "color":"blue","optional":"", "obsolete":"old", "token":"synthetic-secret"}));
        let after = reconcile_settings(Some(&before), &definitions());
        assert_eq!(
            after,
            values(json!({"title":"中文 😀","count":0,"enabled":false,
            "color":"blue","optional":""}))
        );
    }

    #[test]
    fn incompatible_values_use_defaults_without_inventing_optional_or_secret_values() {
        let before = values(json!({"title":7,"count":11,"enabled":"false","color":"removed"}));
        let expected = values(json!({"title":"default","count":4.0,"enabled":true,"color":"red"}));
        assert_eq!(reconcile_settings(Some(&before), &definitions()), expected);
        assert_eq!(reconcile_settings(None, &definitions()), expected);
    }

    #[test]
    fn updates_reject_secrets_unknown_keys_types_and_bounds_without_changing_input() {
        let definitions = definitions();
        for bad in [
            json!({"token":"synthetic"}),
            json!({"unknown":true}),
            json!({"enabled":1}),
            json!({"count":-1}),
            json!({"count":11}),
            json!({"color":"removed"}),
            json!({"title":"bad\u{0000}text"}),
        ] {
            let updates = values(bad);
            let original = updates.clone();
            assert_eq!(
                validate_updates(&definitions, &updates),
                Err(InvalidSetting)
            );
            assert_eq!(updates, original);
        }
        // Existing v1 enforces range, not step multiples. Keep that contract.
        for valid in [
            json!({}),
            json!({"count":0}),
            json!({"count":10}),
            json!({"count":3}),
            json!({"title":"","enabled":false,"color":"blue"}),
        ] {
            assert_eq!(validate_updates(&definitions, &values(valid)), Ok(()));
        }
    }

    #[test]
    fn reads_reject_secret_and_unknown_definitions_and_preserve_missing_value_semantics() {
        let definitions = definitions();
        assert!(public_definition(&definitions, "token").is_err());
        assert!(public_definition(&definitions, "unknown").is_err());
        let empty = BTreeMap::new();
        assert_eq!(
            stored_value(&empty, public_definition(&definitions, "title").unwrap()),
            Some(json!("default"))
        );
        assert_eq!(
            stored_value(&empty, public_definition(&definitions, "optional").unwrap()),
            None
        );
        let stored = values(json!({"title":"","enabled":false,"count":0}));
        for key in ["title", "enabled", "count"] {
            assert_eq!(
                stored_value(&stored, public_definition(&definitions, key).unwrap()),
                stored.get(key).cloned()
            );
        }
    }
}
