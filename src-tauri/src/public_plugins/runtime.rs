use std::{collections::BTreeMap, fmt, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    scheduler::{PluginContextAccessError, PluginRequestScheduler},
    PluginDataScope, PluginRequestContext, PluginSecretStore, PluginStateStore, PluginStorageStore,
    PublicManifestV1,
};

pub(crate) const PUBLIC_RUNTIME_LABEL_PREFIX: &str = "plugin-runtime-";

pub(crate) const PUBLIC_RUNTIME_BOOTSTRAP: &str = r#"
(() => {
  'use strict';
  let handler = null;
  let busy = false;
  const waitForInternals = () => new Promise((resolve) => {
    const tick = () => window.__TAURI_INTERNALS__ ? resolve(window.__TAURI_INTERNALS__) : setTimeout(tick, 0);
    tick();
  });
  const deepFreeze = (value, seen = new WeakSet()) => {
    if ((typeof value !== 'object' && typeof value !== 'function') || value === null || seen.has(value)) return value;
    seen.add(value);
    for (const key of Reflect.ownKeys(value)) deepFreeze(value[key], seen);
    return Object.freeze(value);
  };
  const fail = (code) => Object.assign(new Error(code), { name: code });
  waitForInternals().then(async (tauri) => {
    const invoke = tauri.invoke.bind(tauri);
    const listen = (event, callback) => invoke('plugin:event|listen', {
      event,
      target: { kind: 'Any' },
      handler: tauri.transformCallback((message) => callback(message.payload)),
    });
    await listen('uipilot-public-plugin-command', async (dispatch) => {
      if (!handler || busy) return;
      busy = true;
      let current = true;
      const requestContext = deepFreeze(dispatch.context);
      const context = () => {
        if (!current) throw fail('ExpiredRequestError');
        return requestContext;
      };
      const operation = (operation, key, value) => invoke('plugin_api_call', {
        request: { context: context(), operation, key, value },
      });
      const api = deepFreeze({
        storage: {
          get: (key) => operation('storageGet', key),
          set: (key, value) => operation('storageSet', key, value),
          remove: (key) => operation('storageRemove', key),
        },
        settings: {
          get: (key) => operation('settingGet', key),
          isSecretConfigured: (key) => operation('secretConfigured', key),
        },
      });
      try {
        const response = await handler(deepFreeze(dispatch.invocation), api);
        current = false;
        await invoke('complete_plugin_command', { completion: { context: requestContext, response } });
      } catch (_) {
        current = false;
        await invoke('complete_plugin_command', { completion: { context: requestContext, failed: true } });
      } finally {
        current = false;
        busy = false;
      }
    });
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    const entry = document.documentElement.dataset.runtimeEntry;
    const module = entry ? await import(entry) : null;
    if (!module || typeof module.onCommand !== 'function') throw new TypeError('onCommand export required');
    handler = module.onCommand;
    document.title = 'uipilot-public-plugin-ready';
  }).catch(() => { document.title = 'uipilot-public-plugin-failed'; });
})();
"#;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginApiRequest {
    pub(crate) context: PluginRequestContext,
    pub(crate) operation: PluginApiOperation,
    #[serde(default)]
    pub(crate) key: Option<String>,
    #[serde(default)]
    pub(crate) value: Option<Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PluginApiOperation {
    StorageGet,
    StorageSet,
    StorageRemove,
    SettingGet,
    SecretConfigured,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginRuntimeError {
    InvalidContext,
    ExpiredRequest,
    InvalidCaller,
    InvalidOperation,
    Storage,
    Unavailable,
}

impl fmt::Display for PluginRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidContext => "InvalidContext",
            Self::ExpiredRequest => "ExpiredRequestError",
            Self::InvalidCaller => "InvalidCaller",
            Self::InvalidOperation => "InvalidOperation",
            Self::Storage => "StorageError",
            Self::Unavailable => "RuntimeUnavailable",
        })
    }
}

impl std::error::Error for PluginRuntimeError {}

pub(crate) struct PluginRuntimeApi {
    scheduler: Arc<PluginRequestScheduler>,
    state: Arc<PluginStateStore>,
    storage: Arc<PluginStorageStore>,
    secrets: Arc<PluginSecretStore>,
}

impl PluginRuntimeApi {
    pub(crate) fn new(
        scheduler: Arc<PluginRequestScheduler>,
        state: Arc<PluginStateStore>,
        storage: Arc<PluginStorageStore>,
        secrets: Arc<PluginSecretStore>,
    ) -> Self {
        Self {
            scheduler,
            state,
            storage,
            secrets,
        }
    }

    pub(crate) fn execute(
        &self,
        caller_label: &str,
        request: PluginApiRequest,
        manifest: &PublicManifestV1,
    ) -> Result<Value, PluginRuntimeError> {
        let identity =
            parse_runtime_label(caller_label).ok_or(PluginRuntimeError::InvalidCaller)?;
        if identity.plugin_id != request.context.plugin_id
            || identity.generation != request.context.plugin_generation
            || manifest.plugin_id != request.context.plugin_id
        {
            return Err(PluginRuntimeError::InvalidContext);
        }
        let scope = PluginDataScope::new(&request.context.plugin_id)
            .map_err(|_| PluginRuntimeError::InvalidContext)?;
        let context = request.context.clone();
        self.scheduler
            .with_current(&context, || self.execute_current(&scope, request, manifest))
            .map_err(map_context_error)?
    }

    fn execute_current(
        &self,
        scope: &PluginDataScope,
        request: PluginApiRequest,
        manifest: &PublicManifestV1,
    ) -> Result<Value, PluginRuntimeError> {
        let plugin_id = &request.context.plugin_id;
        match request.operation {
            PluginApiOperation::StorageGet if request.value.is_none() => self
                .storage
                .get(
                    scope,
                    plugin_id,
                    request
                        .key
                        .as_deref()
                        .ok_or(PluginRuntimeError::InvalidOperation)?,
                )
                .map(|value| value.unwrap_or(Value::Null))
                .map_err(|_| PluginRuntimeError::Storage),
            PluginApiOperation::StorageSet => {
                let key = request.key.ok_or(PluginRuntimeError::InvalidOperation)?;
                let value = request.value.ok_or(PluginRuntimeError::InvalidOperation)?;
                self.storage
                    .set(scope, plugin_id, &key, value)
                    .map_err(|_| PluginRuntimeError::Storage)?;
                Ok(Value::Null)
            }
            PluginApiOperation::StorageRemove if request.value.is_none() => {
                self.storage
                    .remove(
                        scope,
                        plugin_id,
                        request
                            .key
                            .as_deref()
                            .ok_or(PluginRuntimeError::InvalidOperation)?,
                    )
                    .map_err(|_| PluginRuntimeError::Storage)?;
                Ok(Value::Null)
            }
            PluginApiOperation::SettingGet if request.value.is_none() => self
                .state
                .setting(
                    scope,
                    plugin_id,
                    &manifest.settings,
                    request
                        .key
                        .as_deref()
                        .ok_or(PluginRuntimeError::InvalidOperation)?,
                )
                .map(|value| value.unwrap_or(Value::Null))
                .map_err(|_| PluginRuntimeError::InvalidOperation),
            PluginApiOperation::SecretConfigured if request.value.is_none() => self
                .secrets
                .is_configured(
                    scope,
                    plugin_id,
                    request
                        .key
                        .as_deref()
                        .ok_or(PluginRuntimeError::InvalidOperation)?,
                )
                .map(Value::Bool)
                .map_err(|_| PluginRuntimeError::Storage),
            PluginApiOperation::StorageGet
            | PluginApiOperation::StorageRemove
            | PluginApiOperation::SettingGet
            | PluginApiOperation::SecretConfigured => Err(PluginRuntimeError::InvalidOperation),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginRuntimeIdentity {
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
}

pub(crate) fn runtime_label(plugin_id: &str, generation: u64) -> Option<String> {
    if generation == 0 || !super::manifest::valid_plugin_id(plugin_id) {
        return None;
    }
    Some(format!(
        "{PUBLIC_RUNTIME_LABEL_PREFIX}{}-g{generation:016x}",
        lower_hex(plugin_id.as_bytes())
    ))
}

pub(crate) fn parse_runtime_label(label: &str) -> Option<PluginRuntimeIdentity> {
    let encoded = label.strip_prefix(PUBLIC_RUNTIME_LABEL_PREFIX)?;
    let (plugin_hex, generation_hex) = encoded.rsplit_once("-g")?;
    if plugin_hex.is_empty()
        || plugin_hex.len() % 2 != 0
        || generation_hex.len() != 16
        || !generation_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let bytes = (0..plugin_hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&plugin_hex[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    let plugin_id = String::from_utf8(bytes).ok()?;
    let generation = u64::from_str_radix(generation_hex, 16).ok()?;
    (generation != 0 && super::manifest::valid_plugin_id(&plugin_id)).then_some(
        PluginRuntimeIdentity {
            plugin_id,
            generation,
        },
    )
}

fn map_context_error(error: PluginContextAccessError) -> PluginRuntimeError {
    match error {
        PluginContextAccessError::Expired => PluginRuntimeError::ExpiredRequest,
        PluginContextAccessError::Invalid => PluginRuntimeError::InvalidContext,
        PluginContextAccessError::Unavailable => PluginRuntimeError::Unavailable,
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginInvocation {
    pub(crate) api_version: u32,
    pub(crate) request_id: String,
    pub(crate) input: String,
    pub(crate) context: PluginInvocationEnvironment,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginInvocationEnvironment {
    pub(crate) platform: PluginInvocationPlatform,
    pub(crate) theme: PluginInvocationTheme,
    pub(crate) invoked_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginInvocationPlatform {
    Windows,
    Macos,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginInvocationTheme {
    Dark,
    Light,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginCommandDispatch {
    pub(crate) context: PluginRequestContext,
    pub(crate) invocation: PluginInvocation,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginCommandCompletion {
    pub(crate) context: PluginRequestContext,
    #[serde(default)]
    pub(crate) response: Option<Value>,
    #[serde(default)]
    pub(crate) failed: bool,
}

pub(crate) type PluginManifestMap = BTreeMap<String, PublicManifestV1>;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };

    use serde_json::json;

    use super::*;
    use crate::public_plugins::{
        scheduler::{PluginRequestCandidate, PluginScheduleOutcome, PluginSubmissionOwner},
        PublicActivationMode,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-public-runtime-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn manifest() -> PublicManifestV1 {
        serde_json::from_value(json!({
            "schemaVersion": 1,
            "pluginId": "com.example.runtime",
            "version": "1.0.0",
            "apiVersion": 1,
            "minimumHostVersion": "0.2.0",
            "name": "Runtime",
            "supportedPlatforms": ["windows"],
            "command": {
                "defaultName": "runtime",
                "activationMode": "live",
                "outputMode": "mainResult",
                "inputRequired": false
            },
            "runtime": { "entry": "runtime.js" },
            "permissions": [],
            "settings": [
                { "type": "text", "key": "prefix", "label": "Prefix", "default": "ok" },
                { "type": "secret", "key": "token", "label": "Token" }
            ]
        }))
        .unwrap()
    }

    fn request(context: PluginRequestContext, operation: PluginApiOperation) -> PluginApiRequest {
        PluginApiRequest {
            context,
            operation,
            key: Some("key".into()),
            value: None,
        }
    }

    #[test]
    fn guard_distinguishes_valid_forged_and_expired_contexts() {
        let dir = TestDir::new();
        let scheduler = Arc::new(PluginRequestScheduler::default());
        let state = Arc::new(
            PluginStateStore::load(&dir.path().join("state"), Vec::<String>::new()).unwrap(),
        );
        let storage = Arc::new(PluginStorageStore::load(&dir.path().join("storage")).unwrap());
        let secrets = Arc::new(PluginSecretStore::load(&dir.path().join("secrets")).unwrap());
        let api = PluginRuntimeApi::new(Arc::clone(&scheduler), state, storage, secrets);
        let manifest = manifest();
        let scheduled = match scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_mode: PublicActivationMode::Live,
                    input: "input".into(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 1,
                        control_value: "/runtime input".into(),
                        submission_token: "submission-1".into(),
                    },
                },
                Instant::now(),
            )
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("first request must dispatch"),
        };
        let label = runtime_label(&manifest.plugin_id, 1).unwrap();
        let mut get = request(scheduled.context.clone(), PluginApiOperation::StorageGet);
        assert_eq!(api.execute(&label, get.clone(), &manifest), Ok(Value::Null));

        get.context.request_id = "forged".into();
        assert_eq!(
            api.execute(&label, get, &manifest),
            Err(PluginRuntimeError::InvalidContext)
        );
        scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_mode: PublicActivationMode::Live,
                    input: "new".into(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 2,
                        control_value: "/runtime new".into(),
                        submission_token: "submission-2".into(),
                    },
                },
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            api.execute(
                &label,
                request(scheduled.context, PluginApiOperation::StorageGet),
                &manifest
            ),
            Err(PluginRuntimeError::ExpiredRequest)
        );
        assert_eq!(
            api.execute(
                "plugin-runtime-forged-g0000000000000001",
                request(
                    PluginRequestContext {
                        plugin_id: manifest.plugin_id.clone(),
                        plugin_generation: 1,
                        request_id: "public-request-0000000000000001".into(),
                    },
                    PluginApiOperation::StorageGet
                ),
                &manifest
            ),
            Err(PluginRuntimeError::InvalidCaller)
        );
    }

    #[test]
    fn runtime_labels_and_bootstrap_are_narrow() {
        let label = runtime_label("com.example.runtime", 42).unwrap();
        assert!(label.starts_with(PUBLIC_RUNTIME_LABEL_PREFIX));
        assert_eq!(
            parse_runtime_label(&label),
            Some(PluginRuntimeIdentity {
                plugin_id: "com.example.runtime".into(),
                generation: 42
            })
        );
        assert!(!label.starts_with("plugin-shell-"));
        assert!(!label.starts_with("plugin-content-"));
        assert!(PUBLIC_RUNTIME_BOOTSTRAP.contains("deepFreeze"));
        assert!(PUBLIC_RUNTIME_BOOTSTRAP.contains("onCommand"));
        assert!(!PUBLIC_RUNTIME_BOOTSTRAP.contains("api.resolve"));
    }
}
