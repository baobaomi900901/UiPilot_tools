//! Bounded, in-memory management observations. Never serialize configuration or errors.
use serde::{Deserialize, Deserializer, Serialize};
use std::{collections::HashMap, sync::Mutex};
fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}
use super::{
    manifest::{parse_canonical_version, valid_plugin_id},
    PublicPlatform, PublicPluginFault, PublicPluginManagementError,
};

pub(crate) const MAX_REPORT_BYTES: usize = 16384;
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Operation {
    Prepare,
    DevelopmentRefresh,
    Replace,
    Enable,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    Validation,
    Readiness,
    Persistence,
    Publication,
    Complete,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CommitOutcome {
    NotAttempted,
    Committed,
    NotCommitted,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Observation {
    pub(crate) operation: Operation,
    pub(crate) phase: Phase,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) error_code: Option<PublicPluginManagementError>,
    pub(crate) state_commit_outcome: CommitOutcome,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum DiagnosticState {
    #[serde(rename_all = "camelCase")]
    NotInstalled { inventory_revision: String },
    #[serde(rename_all = "camelCase")]
    Installed {
        package_version: String,
        generation: String,
        inventory_revision: String,
        enabled: bool,
        #[serde(deserialize_with = "required_nullable")]
        fault: Option<PublicPluginFault>,
    },
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiagnosticReport {
    pub(crate) schema_version: u8,
    pub(crate) host_version: String,
    pub(crate) platform: PublicPlatform,
    pub(crate) plugin_id: String,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) state: Option<DiagnosticState>,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) last_operation: Option<Observation>,
}
fn version(value: &str) -> bool {
    value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && parse_canonical_version(value).is_some()
}
fn decimal(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
        && value.parse::<u64>().is_ok()
}
impl DiagnosticReport {
    pub(crate) fn preview(&self) -> Result<String, PublicPluginManagementError> {
        let valid_state = match &self.state {
            None => true,
            Some(DiagnosticState::NotInstalled { inventory_revision }) => {
                decimal(inventory_revision)
            }
            Some(DiagnosticState::Installed {
                package_version,
                generation,
                inventory_revision,
                enabled,
                fault,
            }) => {
                version(package_version)
                    && decimal(generation)
                    && generation != "0"
                    && decimal(inventory_revision)
                    && (!enabled || fault.is_none())
            }
        };
        let phase_valid = self.last_operation.as_ref().is_none_or(|o| match o.phase {
            Phase::Validation | Phase::Readiness => {
                o.state_commit_outcome == CommitOutcome::NotAttempted
            }
            Phase::Persistence => true,
            Phase::Publication => o.state_commit_outcome == CommitOutcome::Committed,
            Phase::Complete => {
                matches!(
                    o.state_commit_outcome,
                    CommitOutcome::Committed | CommitOutcome::NotAttempted
                ) && o.error_code.is_none()
            }
        });
        if self.schema_version != 1
            || !version(&self.host_version)
            || !valid_plugin_id(&self.plugin_id)
            || !valid_state
            || !phase_valid
        {
            return Err(PublicPluginManagementError::InvalidPackage);
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        if text.len() > MAX_REPORT_BYTES {
            return Err(PublicPluginManagementError::InvalidPackage);
        }
        Ok(text)
    }
}

// A narrow adapter makes exact-preview output testable without touching a real clipboard.
pub(crate) fn copy_report(
    report: &DiagnosticReport,
    write: impl FnOnce(String) -> Result<(), ()>,
) -> Result<(), PublicPluginManagementError> {
    write(report.preview()?).map_err(|_| PublicPluginManagementError::Unavailable)
}

#[derive(Default)]
pub(super) struct Observations(Mutex<Slots>);
#[derive(Default)]
struct Slots {
    next: u64,
    entries: HashMap<String, (u64, Option<Observation>)>,
}
impl Observations {
    pub(super) fn start(&self, plugin_id: &str, operation: Operation) -> Option<Attempt<'_>> {
        let mut slots = self.0.lock().ok()?;
        slots.next = slots.next.checked_add(1)?;
        let seq = slots.next;
        if !slots.entries.contains_key(plugin_id) && slots.entries.len() == 64 {
            let oldest = slots
                .entries
                .iter()
                .min_by_key(|(_, (seq, _))| seq)
                .map(|(id, _)| id.clone())?;
            slots.entries.remove(&oldest);
        }
        slots.entries.insert(plugin_id.into(), (seq, None));
        Some(Attempt {
            store: self,
            plugin_id: plugin_id.into(),
            seq,
            observation: Observation {
                operation,
                phase: Phase::Validation,
                error_code: None,
                state_commit_outcome: CommitOutcome::NotAttempted,
            },
        })
    }
    pub(super) fn latest(&self, plugin_id: &str) -> Option<Observation> {
        self.0.lock().ok()?.entries.get(plugin_id)?.1.clone()
    }
}
pub(super) struct Attempt<'a> {
    store: &'a Observations,
    plugin_id: String,
    seq: u64,
    pub(super) observation: Observation,
}
impl Attempt<'_> {
    pub(super) fn finish<T>(mut self, result: &Result<T, PublicPluginManagementError>) {
        self.observation.error_code = result.as_ref().err().cloned();
        if let Ok(mut slots) = self.store.0.lock() {
            if let Some((seq, value)) = slots.entries.get_mut(&self.plugin_id) {
                if *seq == self.seq {
                    *value = Some(self.observation);
                }
            }
        }
    }
}
pub(super) fn phase(attempt: &mut Option<Attempt<'_>>, phase: Phase, outcome: CommitOutcome) {
    if let Some(attempt) = attempt {
        attempt.observation.phase = phase;
        attempt.observation.state_commit_outcome = outcome;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_sequence_and_eviction_never_resurrect_old_attempts() {
        let store = Observations::default();
        let old = store.start("com.example.test", Operation::Prepare).unwrap();
        let new = store.start("com.example.test", Operation::Enable).unwrap();
        new.finish(&Ok::<_, PublicPluginManagementError>(()));
        old.finish(&Err::<(), _>(PublicPluginManagementError::RuntimeNotReady));
        assert_eq!(
            store.latest("com.example.test").unwrap().operation,
            Operation::Enable
        );
        let evicted = store
            .start("com.example.evicted", Operation::Prepare)
            .unwrap();
        for i in 0..64 {
            store
                .start(&format!("com.example.p{i}"), Operation::Prepare)
                .unwrap()
                .finish(&Ok::<_, PublicPluginManagementError>(()));
        }
        evicted.finish(&Ok::<_, PublicPluginManagementError>(()));
        assert!(store.latest("com.example.evicted").is_none());
        assert_eq!(store.0.lock().unwrap().entries.len(), 64);
    }
    #[test]
    fn copy_emits_exact_validated_preview_and_fixed_failure() {
        let report: DiagnosticReport = serde_json::from_value(serde_json::json!({"schemaVersion":1,"hostVersion":"0.7.0","platform":"windows","pluginId":"com.example.test","state":null,"lastOperation":null})).unwrap();
        let preview = report.preview().unwrap();
        let mut copied = None;
        copy_report(&report, |text| {
            copied = Some(text);
            Ok(())
        })
        .unwrap();
        assert_eq!(copied.as_deref(), Some(preview.as_str()));
        assert_eq!(
            copy_report(&report, |_| Err(())),
            Err(PublicPluginManagementError::Unavailable)
        );
        let mut invalid = report;
        invalid.plugin_id = "PATH_PRIVATE_SENTINEL/secret".into();
        assert_eq!(
            copy_report(&invalid, |_| panic!(
                "invalid DTO must never reach clipboard"
            )),
            Err(PublicPluginManagementError::InvalidPackage)
        );
    }

    #[test]
    fn strict_report_rejects_private_fields_and_noncanonical_values() {
        let value = serde_json::json!({"schemaVersion":1,"hostVersion":"0.7.0","platform":"windows","pluginId":"com.example.test","state":{"status":"installed","packageVersion":"1.0.0","generation":"1","inventoryRevision":"1","enabled":true,"fault":null},"lastOperation":null});
        let report: DiagnosticReport = serde_json::from_value(value.clone()).unwrap();
        assert!(report.preview().is_ok());
        for key in ["settings", "path", "secret", "error", "requestId"] {
            let mut v = value.clone();
            v[key] = "PRIVATE_SENTINEL".into();
            assert!(serde_json::from_value::<DiagnosticReport>(v).is_err());
        }
        for bad in ["01", "-1", "18446744073709551616"] {
            let mut v = value.clone();
            v["state"]["generation"] = bad.into();
            assert!(serde_json::from_value::<DiagnosticReport>(v)
                .unwrap()
                .preview()
                .is_err());
        }
        for key in ["state", "lastOperation"] {
            let mut v = value.clone();
            v.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<DiagnosticReport>(v).is_err());
        }
        let mut missing = value.clone();
        missing["state"].as_object_mut().unwrap().remove("fault");
        assert!(serde_json::from_value::<DiagnosticReport>(missing).is_err());
        let mut v = value.clone();
        v["state"]["settings"] = "SENTINEL".into();
        assert!(serde_json::from_value::<DiagnosticReport>(v).is_err());
        let mut v = value;
        v["lastOperation"] = serde_json::json!({"operation":"replace","phase":"persistence","errorCode":"pluginUnavailable","stateCommitOutcome":"unknown"});
        assert!(serde_json::from_value::<DiagnosticReport>(v)
            .unwrap()
            .preview()
            .is_ok());
    }
}
