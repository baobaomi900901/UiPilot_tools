// Decode the discriminator and each strict DTO directly from authoritative bytes.
// In particular, never round-trip through Value before checking duplicate command IDs.
use super::*;
use serde::de::{Error, MapAccess, Visitor};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyStateDocument {
    schema: u32,
    plugin_id: String,
    version: String,
    default_name: String,
    name_override: Option<String>,
    installed: bool,
    enabled: bool,
    #[serde(default)]
    favorite: bool,
    fault: Option<PublicPluginFault>,
    permission_grants: BTreeSet<PublicPermission>,
    #[serde(default)]
    network_https_hosts_grant: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    network_credentials_grant: Vec<PluginNetworkCredential>,
    inventory_revision: u64,
    #[serde(default)]
    active_generation: u64,
    #[serde(default)]
    package_digest: Option<String>,
    settings: BTreeMap<String, Value>,
}

pub(super) fn decode_document(bytes: &[u8]) -> Option<PluginStateDocument> {
    #[derive(Deserialize)]
    struct Schema {
        schema: u32,
    }
    let mut document = match serde_json::from_slice::<Schema>(bytes).ok()?.schema {
        1 => {
            let old: LegacyStateDocument = serde_json::from_slice(bytes).ok()?;
            Some(PluginStateDocument {
                schema: 2,
                commands: BTreeMap::from([(
                    "default".into(),
                    CommandState {
                        default_name: old.default_name,
                        name_override: old.name_override,
                        favorite: old.favorite,
                    },
                )]),
                plugin_id: old.plugin_id,
                version: old.version,
                installed: old.installed,
                enabled: old.enabled,
                fault: old.fault,
                permission_grants: old.permission_grants,
                network_https_hosts_grant: old.network_https_hosts_grant,
                network_credentials_grant: old.network_credentials_grant,
                inventory_revision: old.inventory_revision,
                active_generation: old.active_generation,
                package_digest: old.package_digest,
                settings: old.settings,
            })
        }
        2 => serde_json::from_slice(bytes).ok(),
        _ => None,
    }?;
    // Ordering is representation normalization, never a security-repair write.
    document
        .network_credentials_grant
        .sort_by(|a, b| a.id.cmp(&b.id));
    Some(document)
}

impl<'de> Deserialize<'de> for CommandState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Command;
        impl<'de> Visitor<'de> for Command {
            type Value = CommandState;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a command state object")
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Fields {
                    default_name: String,
                    #[serde(deserialize_with = "required_override")]
                    name_override: Option<String>,
                    favorite: bool,
                }
                // Reuse strict field decoding on the original map, without accepting
                // serde's positional struct representation or buffering through Value.
                let fields =
                    Fields::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(CommandState {
                    default_name: fields.default_name,
                    name_override: fields.name_override,
                    favorite: fields.favorite,
                })
            }
        }
        deserializer.deserialize_map(Command)
    }
}

pub(super) fn required_override<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

pub(super) fn command_map<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, CommandState>, D::Error> {
    struct Commands;
    impl<'de> Visitor<'de> for Commands {
        type Value = BTreeMap<String, CommandState>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("one to sixteen unique command IDs")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut commands = BTreeMap::new();
            while let Some((id, command)) = map.next_entry::<String, CommandState>()? {
                if !valid_command_name(&id)
                    || commands.len() == 16
                    || commands.insert(id, command).is_some()
                {
                    return Err(M::Error::custom("invalid or duplicate command ID"));
                }
            }
            if commands.is_empty() {
                return Err(M::Error::custom("empty commands"));
            }
            Ok(commands)
        }
    }
    deserializer.deserialize_map(Commands)
}
