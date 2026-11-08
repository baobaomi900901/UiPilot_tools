//! Internal runtime data wiring. No production plugin loader or OS sandbox.
use super::{
    call_session::{CallEndpoint, CallSession, ReplyPermit},
    data_call_gate::{PluginDataCallGate, PluginDataCallIdentity},
    manifest::PublicSettingV1,
    request_contract::PluginRequestContext,
    settings_macos::{MacSettingsStore, SettingsError, SettingsHandle},
    settings_values,
    storage::{PluginStorageError, PluginStorageStore},
    PluginDataScope,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DataRuntimeError {
    InvalidContext,
    InvalidOperation,
    Storage,
}
type Result<T> = std::result::Result<T, DataRuntimeError>;

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum DataOperation {
    StorageGet { key: String },
    StorageSet { key: String, value: Value },
    StorageRemove { key: String },
    SettingGet { key: String },
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DataCall {
    pub(crate) context: PluginRequestContext,
    pub(crate) call_id: u64,
    pub(crate) request: DataOperation,
}

// Fields and constructor are private. A transport receives a captured binding,
// never authority reconstructed from a child's JSON.
#[derive(Clone)]
pub(crate) struct RuntimeDataBinding {
    gate: Arc<PluginDataCallGate>,
    session: Arc<CallSession>,
    endpoint: CallEndpoint,
    context: PluginRequestContext,
    identity: PluginDataCallIdentity,
    scope: PluginDataScope,
    settings: BoundSettings,
    storage: Arc<PluginStorageStore>,
}

// A binding owns one immutable source. Installed requests never consult the
// independently persisted settings cache used by the legacy internal entry.
#[derive(Clone)]
enum BoundSettings {
    Persisted {
        store: Arc<MacSettingsStore>,
        handle: SettingsHandle,
    },
    Snapshot {
        definitions: Arc<[PublicSettingV1]>,
        values: Arc<BTreeMap<String, Value>>,
    },
}

pub(crate) struct DataReply {
    pub(crate) result: Result<Value>,
    pub(crate) permit: ReplyPermit,
}

impl RuntimeDataBinding {
    /// Main-thread-safe admission retirement only. Native transport cancellation
    /// and the host's actual close/drain happen separately on a worker.
    pub(crate) fn revoke_admission(&self) {
        let _ = self.gate.close(&self.identity);
        let _ = self.session.revoke_admission(&self.endpoint);
    }

    pub(crate) fn session(&self) -> &Arc<CallSession> {
        &self.session
    }
    pub(crate) fn endpoint(&self) -> &CallEndpoint {
        &self.endpoint
    }
    pub(crate) fn context(&self) -> &PluginRequestContext {
        &self.context
    }

    pub(crate) fn call(&self, request: DataCall) -> Result<DataReply> {
        let call = self
            .session
            .begin_call(&self.endpoint, &request.context, request.call_id)
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        let identity = call.identity();
        let id = identity.plugin_id();
        let result = match request.request {
            DataOperation::StorageGet { key } => self
                .storage
                .get(&self.scope, identity, id, &key)
                .map(|value| value.unwrap_or(Value::Null))
                .map_err(storage_error),
            DataOperation::StorageSet { key, value } => self
                .storage
                .set(&self.scope, identity, id, &key, value)
                .map(|()| Value::Null)
                .map_err(storage_error),
            DataOperation::StorageRemove { key } => self
                .storage
                .remove(&self.scope, identity, id, &key)
                .map(|()| Value::Null)
                .map_err(storage_error),
            DataOperation::SettingGet { key } => match &self.settings {
                BoundSettings::Persisted { store, handle } => store
                    .setting(&self.scope, identity, handle, &key)
                    .map(|value| value.unwrap_or(Value::Null))
                    .map_err(settings_error),
                BoundSettings::Snapshot {
                    definitions,
                    values,
                } => settings_values::public_definition(definitions, &key)
                    .map(|_| values.get(&key).cloned().unwrap_or(Value::Null))
                    .map_err(|_| DataRuntimeError::InvalidOperation),
            },
        };
        let permit = self
            .session
            .begin_reply(&self.endpoint, call)
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        Ok(DataReply { result, permit })
    }
}

/// Trusted-host controller for ONE active request. &mut transitions serialize
/// configuration; bindings can execute on workers without borrowing the host.
pub(crate) struct RuntimeDataHost {
    gate: Arc<PluginDataCallGate>,
    session: Arc<CallSession>,
    settings: Arc<MacSettingsStore>,
    storage: Arc<PluginStorageStore>,
    active: Option<RuntimeDataBinding>,
    next_epoch: u64,
    failed: bool,
}
impl RuntimeDataHost {
    pub(crate) fn load(storage_root: &Path, settings_root: &Path) -> Result<Self> {
        let gate = Arc::new(PluginDataCallGate::default());
        Ok(Self {
            session: Arc::new(CallSession::new(gate.clone())),
            settings: Arc::new(
                MacSettingsStore::load(settings_root, gate.clone()).map_err(settings_error)?,
            ),
            storage: Arc::new(
                PluginStorageStore::load(storage_root, gate.clone()).map_err(storage_error)?,
            ),
            gate,
            active: None,
            next_epoch: 0,
            failed: false,
        })
    }

    /// Cancel the old transport before waiting for reply/backend permits. The
    /// callback must return only once cancellation is effective, and must not
    /// wait for this host's drains. Failure permanently prevents reopening.
    pub(crate) fn close(&mut self, cancel: impl FnOnce() -> Result<()>) -> Result<()> {
        let Some(old) = self.active.take() else {
            return if self.failed {
                Err(DataRuntimeError::InvalidContext)
            } else {
                Ok(())
            };
        };
        self.failed = true;
        let gate_drain = self.gate.close(&old.identity);
        let session_drain = self.session.close(&old.endpoint);
        cancel()?;
        session_drain
            .map_err(|_| DataRuntimeError::InvalidContext)?
            .wait()
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        gate_drain
            .map_err(|_| DataRuntimeError::InvalidContext)?
            .wait()
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        self.failed = false;
        Ok(())
    }

    /// Explicitly starts a NEW request. This is not the settings UI's no-op save
    /// path. Definitions and updates come only from the trusted host, not RPC.
    pub(crate) fn open_request(
        &mut self,
        context: PluginRequestContext,
        definitions: &[PublicSettingV1],
        updates: &BTreeMap<String, Value>,
        deadline: Instant,
        cancel_previous: impl FnOnce() -> Result<()>,
    ) -> Result<RuntimeDataBinding> {
        self.open_with_settings(
            context,
            deadline,
            cancel_previous,
            |host, scope, identity| {
                let handle = host
                    .settings
                    .configure_with_updates(scope, identity, definitions, updates)
                    .map_err(settings_error)?;
                Ok(BoundSettings::Persisted {
                    store: host.settings.clone(),
                    handle,
                })
            },
        )
    }

    /// Trusted inventory entry. Values must already be the complete canonical
    /// ordinary-setting snapshot. No settings file is read or written here.
    pub(crate) fn open_installed_request(
        &mut self,
        context: PluginRequestContext,
        definitions: &[PublicSettingV1],
        values: &BTreeMap<String, Value>,
        deadline: Instant,
        cancel_previous: impl FnOnce() -> Result<()>,
    ) -> Result<RuntimeDataBinding> {
        // Reject invalid snapshots before retiring an existing request.
        let mut keys = std::collections::BTreeSet::new();
        if definitions
            .iter()
            .any(|definition| !definition.validate() || !keys.insert(definition.key()))
            || settings_values::validate_updates(definitions, values).is_err()
            || settings_values::reconcile_settings(Some(values), definitions) != *values
        {
            return Err(DataRuntimeError::InvalidOperation);
        }
        let settings = BoundSettings::Snapshot {
            definitions: Arc::from(definitions),
            values: Arc::new(values.clone()),
        };
        self.open_with_settings(context, deadline, cancel_previous, |_, _, _| Ok(settings))
    }

    fn open_with_settings(
        &mut self,
        context: PluginRequestContext,
        deadline: Instant,
        cancel_previous: impl FnOnce() -> Result<()>,
        prepare: impl FnOnce(&Self, &PluginDataScope, &PluginDataCallIdentity) -> Result<BoundSettings>,
    ) -> Result<RuntimeDataBinding> {
        if self.failed || !context.valid_shape() || Instant::now() >= deadline {
            return Err(DataRuntimeError::InvalidContext);
        }
        self.close(cancel_previous)?;
        if Instant::now() >= deadline {
            return Err(DataRuntimeError::InvalidContext);
        }
        self.next_epoch = self
            .next_epoch
            .checked_add(1)
            .ok_or(DataRuntimeError::InvalidContext)?;
        let epoch = self.next_epoch;
        let identity = PluginDataCallIdentity::new(
            &context.plugin_id,
            context.plugin_generation,
            epoch,
            epoch,
        )
        .map_err(|_| DataRuntimeError::InvalidContext)?;
        let scope = PluginDataScope::new(&context.plugin_id)
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        self.gate
            .activate(identity.clone())
            .map_err(|_| DataRuntimeError::InvalidContext)?;
        let prepared = (|| {
            let settings = prepare(self, &scope, &identity)?;
            let (endpoint, _) = self
                .session
                .open(context.clone(), epoch, epoch, deadline)
                .map_err(|_| DataRuntimeError::InvalidContext)?;
            Ok(RuntimeDataBinding {
                gate: self.gate.clone(),
                session: self.session.clone(),
                endpoint,
                context,
                identity: identity.clone(),
                scope,
                settings,
                storage: self.storage.clone(),
            })
        })();
        match prepared {
            Ok(binding) => {
                self.active = Some(binding.clone());
                Ok(binding)
            }
            Err(error) => {
                // No endpoint was published. Never reopen the previous epoch.
                if let Ok(drain) = self.gate.close(&identity) {
                    let _ = drain.wait();
                }
                Err(error)
            }
        }
    }
}
impl Drop for RuntimeDataHost {
    fn drop(&mut self) {
        // Drop cannot cancel a transport owned elsewhere or wait on its worker.
        // Fail closed now; adapters must explicitly close/cancel/drain first.
        if let Some(old) = self.active.take() {
            let _ = self.gate.close(&old.identity);
            let _ = self.session.close(&old.endpoint);
        }
    }
}
fn storage_error(error: PluginStorageError) -> DataRuntimeError {
    match error {
        PluginStorageError::InvalidKey | PluginStorageError::InvalidValue => {
            DataRuntimeError::InvalidOperation
        }
        PluginStorageError::Storage | PluginStorageError::QuotaExceeded => {
            DataRuntimeError::Storage
        }
        PluginStorageError::InvalidPlugin | PluginStorageError::InvalidScope => {
            DataRuntimeError::InvalidContext
        }
    }
}
fn settings_error(error: SettingsError) -> DataRuntimeError {
    match error {
        SettingsError::InvalidScope => DataRuntimeError::InvalidContext,
        SettingsError::InvalidDefinition | SettingsError::InvalidSetting => {
            DataRuntimeError::InvalidOperation
        }
        SettingsError::QuotaExceeded | SettingsError::Storage => DataRuntimeError::Storage,
    }
}

#[cfg(test)]
#[path = "runtime_data_macos_tests.rs"]
mod tests;
