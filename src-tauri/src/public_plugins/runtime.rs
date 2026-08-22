use std::{fmt, sync::Arc, time::Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message_center::{
    MessagePostGuardEffect, MessagePublishOutcome, MessagePublishRequest, MessagePublisher,
};

use super::{
    delayed_messages::{
        DelayedMessageRegistration, DelayedMessageScheduleError, DelayedMessageScheduler,
    },
    scheduler::{PluginContextAccessError, PluginCurrentRequest, PluginRequestScheduler},
    PluginDataScope, PluginRequestContext, PluginSecretStore, PluginStateStore, PluginStorageStore,
    PublicManifestV1, PublicPermission,
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
      const operation = (operation, key, value, notification) => invoke('plugin_api_call', {
        request: { context: context(), operation, key, value, notification },
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
        notifications: {
          publish: (input) => {
            const snapshot = input && typeof input === 'object' && !Array.isArray(input)
              ? deepFreeze({ ...input })
              : input;
            return operation('notificationPublish', undefined, undefined, snapshot);
          },
          schedule: (input) => {
            const snapshot = input && typeof input === 'object' && !Array.isArray(input)
              ? deepFreeze({ ...input })
              : input;
            return operation('notificationSchedule', undefined, undefined, snapshot);
          },
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
    #[serde(default)]
    pub(crate) notification: Option<Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PluginApiOperation {
    StorageGet,
    StorageSet,
    StorageRemove,
    SettingGet,
    SecretConfigured,
    NotificationPublish,
    NotificationSchedule,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginNotificationPublishInput {
    content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginNotificationScheduleInput {
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    delay_ms: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginRuntimeError {
    InvalidContext,
    ExpiredRequest,
    InvalidCaller,
    InvalidOperation,
    PermissionDenied,
    InvalidNotification,
    InvalidDelay,
    ScheduleLimitExceeded,
    AlreadyPublished,
    MessageStoreUnavailable,
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
            Self::PermissionDenied => "PermissionDenied",
            Self::InvalidNotification => "InvalidNotification",
            Self::InvalidDelay => "InvalidDelay",
            Self::ScheduleLimitExceeded => "ScheduleLimitExceeded",
            Self::AlreadyPublished => "AlreadyPublished",
            Self::MessageStoreUnavailable => "MessageStoreUnavailable",
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
    delayed_messages: Arc<DelayedMessageScheduler>,
    messages: Arc<dyn MessagePublisher>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginApiExecution {
    pub(crate) result: Result<Value, PluginRuntimeError>,
    pub(crate) post_guard_effect: Option<MessagePostGuardEffect>,
}

impl PluginApiExecution {
    pub(crate) fn failed(error: PluginRuntimeError) -> Self {
        Self {
            result: Err(error),
            post_guard_effect: None,
        }
    }

    fn complete(
        result: Result<Value, PluginRuntimeError>,
        post_guard_effect: Option<MessagePostGuardEffect>,
    ) -> Self {
        Self {
            result,
            post_guard_effect,
        }
    }
}

impl PluginRuntimeApi {
    pub(crate) fn new(
        scheduler: Arc<PluginRequestScheduler>,
        state: Arc<PluginStateStore>,
        storage: Arc<PluginStorageStore>,
        secrets: Arc<PluginSecretStore>,
        delayed_messages: Arc<DelayedMessageScheduler>,
        messages: Arc<dyn MessagePublisher>,
    ) -> Self {
        Self {
            scheduler,
            state,
            storage,
            secrets,
            delayed_messages,
            messages,
        }
    }

    pub(crate) fn execute(
        &self,
        caller_label: &str,
        request: PluginApiRequest,
        manifest: &PublicManifestV1,
    ) -> PluginApiExecution {
        let Some(identity) = parse_runtime_label(caller_label) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidCaller);
        };
        if identity.plugin_id != request.context.plugin_id
            || identity.generation != request.context.plugin_generation
            || manifest.plugin_id != request.context.plugin_id
        {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidContext);
        }
        let Ok(scope) = PluginDataScope::new(&request.context.plugin_id) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidContext);
        };
        let context = request.context.clone();
        match self.scheduler.with_current(&context, |current| {
            self.execute_current(&scope, request, manifest, current)
        }) {
            Ok(execution) => execution,
            Err(error) => PluginApiExecution::failed(map_context_error(error)),
        }
    }

    fn execute_current(
        &self,
        scope: &PluginDataScope,
        request: PluginApiRequest,
        manifest: &PublicManifestV1,
        current: &mut PluginCurrentRequest<'_>,
    ) -> PluginApiExecution {
        if request.operation == PluginApiOperation::NotificationPublish {
            return self.publish_notification(request, manifest, current);
        }
        if request.operation == PluginApiOperation::NotificationSchedule {
            return self.schedule_notification(request, manifest, current);
        }
        if request.notification.is_some() {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidOperation);
        }
        PluginApiExecution::complete(self.execute_data_api(scope, request, manifest), None)
    }

    fn execute_data_api(
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
            PluginApiOperation::NotificationPublish | PluginApiOperation::NotificationSchedule => {
                unreachable!("handled before data APIs")
            }
        }
    }

    fn publish_notification(
        &self,
        request: PluginApiRequest,
        manifest: &PublicManifestV1,
        current: &mut PluginCurrentRequest<'_>,
    ) -> PluginApiExecution {
        let plugin_id = &request.context.plugin_id;
        let permitted = manifest
            .permissions
            .contains(&PublicPermission::NotificationsPublish)
            && self
                .state
                .config(plugin_id)
                .ok()
                .flatten()
                .is_some_and(|config| {
                    config.installed
                        && config.enabled
                        && config.fault.is_none()
                        && config.active_generation == request.context.plugin_generation
                        && config
                            .permission_grants
                            .contains(&PublicPermission::NotificationsPublish)
                });
        if !permitted {
            return PluginApiExecution::failed(PluginRuntimeError::PermissionDenied);
        }
        if request.key.is_some() || request.value.is_some() {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        }
        let Some(value) = request.notification else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        };
        let Ok(input) = serde_json::from_value::<PluginNotificationPublishInput>(value) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        };
        if !valid_notification_content(&input.content) {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        }
        if current.notification_published() {
            return PluginApiExecution::failed(PluginRuntimeError::AlreadyPublished);
        }

        match self.messages.commit_publish(MessagePublishRequest {
            plugin_id: plugin_id.clone(),
            plugin_name_snapshot: manifest.name.clone(),
            content: input.content,
        }) {
            MessagePublishOutcome::Published(message) => {
                current.mark_notification_published();
                PluginApiExecution::complete(
                    Ok(Value::Null),
                    Some(MessagePostGuardEffect::Published(message)),
                )
            }
            MessagePublishOutcome::BecameUnavailable => PluginApiExecution::complete(
                Err(PluginRuntimeError::MessageStoreUnavailable),
                Some(MessagePostGuardEffect::BecameUnavailable),
            ),
            MessagePublishOutcome::OperationFailed | MessagePublishOutcome::Unavailable => {
                PluginApiExecution::failed(PluginRuntimeError::MessageStoreUnavailable)
            }
        }
    }

    fn schedule_notification(
        &self,
        request: PluginApiRequest,
        manifest: &PublicManifestV1,
        current: &mut PluginCurrentRequest<'_>,
    ) -> PluginApiExecution {
        let plugin_id = &request.context.plugin_id;
        let permitted = manifest
            .permissions
            .contains(&PublicPermission::NotificationsPublish)
            && self
                .state
                .config(plugin_id)
                .ok()
                .flatten()
                .is_some_and(|config| {
                    config.installed
                        && config.enabled
                        && config.fault.is_none()
                        && config.active_generation == request.context.plugin_generation
                        && config
                            .permission_grants
                            .contains(&PublicPermission::NotificationsPublish)
                });
        if !permitted {
            return PluginApiExecution::failed(PluginRuntimeError::PermissionDenied);
        }
        if request.key.is_some() || request.value.is_some() {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        }
        let Some(value) = request.notification else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        };
        let Ok(input) = serde_json::from_value::<PluginNotificationScheduleInput>(value) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        };
        let Some(content) = input.content.as_ref().and_then(Value::as_str) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        };
        if !valid_notification_content(content) {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidNotification);
        }
        let Some(delay_ms) = input.delay_ms.as_ref().and_then(Value::as_u64) else {
            return PluginApiExecution::failed(PluginRuntimeError::InvalidDelay);
        };
        if current.notification_published() {
            return PluginApiExecution::failed(PluginRuntimeError::AlreadyPublished);
        }
        if !self.messages.is_available() {
            return PluginApiExecution::failed(PluginRuntimeError::MessageStoreUnavailable);
        }
        let registration = DelayedMessageRegistration {
            plugin_id: plugin_id.clone(),
            plugin_generation: request.context.plugin_generation,
            activation_id: current.activation_id(),
            plugin_name_snapshot: manifest.name.clone(),
            request_id: request.context.request_id,
            content: content.into(),
            delay_ms,
        };
        match self.delayed_messages.schedule(registration, Instant::now()) {
            Ok(_) => {
                current.mark_notification_published();
                PluginApiExecution::complete(Ok(Value::Null), None)
            }
            Err(DelayedMessageScheduleError::InvalidDelay) => {
                PluginApiExecution::failed(PluginRuntimeError::InvalidDelay)
            }
            Err(DelayedMessageScheduleError::LimitExceeded) => {
                PluginApiExecution::failed(PluginRuntimeError::ScheduleLimitExceeded)
            }
            Err(DelayedMessageScheduleError::InvalidRegistration) => {
                PluginApiExecution::failed(PluginRuntimeError::InvalidContext)
            }
            Err(DelayedMessageScheduleError::Unavailable) => {
                PluginApiExecution::failed(PluginRuntimeError::Unavailable)
            }
        }
    }
}

fn valid_notification_content(content: &str) -> bool {
    !content.trim().is_empty()
        && content.chars().count() <= 500
        && !content.chars().any(char::is_control)
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

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, VecDeque},
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Mutex,
        },
        time::{Duration, Instant},
    };

    use serde_json::json;

    use super::*;
    use crate::message_center::{
        MessageCenterService, MessagePostGuardEffect, MessagePublishOutcome, MessagePublishRequest,
        MessagePublished, MessagePublisher,
    };
    use crate::public_plugins::{
        delayed_messages::{DelayedMessageRegistration, DelayedMessageScheduler},
        scheduler::{PluginRequestCandidate, PluginScheduleOutcome, PluginSubmissionOwner},
        PublicActivationMode, PublicPermission, ScheduledPluginRequest,
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
            notification: None,
        }
    }

    fn notification_manifest() -> PublicManifestV1 {
        let mut manifest = manifest();
        manifest.permissions = vec![PublicPermission::NotificationsPublish];
        manifest
    }

    fn notification_request(
        context: PluginRequestContext,
        notification: Option<Value>,
    ) -> PluginApiRequest {
        PluginApiRequest {
            context,
            operation: PluginApiOperation::NotificationPublish,
            key: None,
            value: None,
            notification,
        }
    }

    fn scheduled_notification_request(
        context: PluginRequestContext,
        notification: Option<Value>,
    ) -> PluginApiRequest {
        PluginApiRequest {
            context,
            operation: PluginApiOperation::NotificationSchedule,
            key: None,
            value: None,
            notification,
        }
    }

    fn runtime_fixture(
        dir: &TestDir,
        persisted_manifest: &PublicManifestV1,
        grants: BTreeSet<PublicPermission>,
        publisher: Arc<dyn MessagePublisher>,
    ) -> (
        PluginRuntimeApi,
        Arc<PluginRequestScheduler>,
        Arc<DelayedMessageScheduler>,
        ScheduledPluginRequest,
        String,
    ) {
        let scheduler = Arc::new(PluginRequestScheduler::default());
        let state = Arc::new(
            PluginStateStore::load(&dir.path().join("state"), Vec::<String>::new()).unwrap(),
        );
        state
            .install_or_upgrade(persisted_manifest, grants)
            .unwrap();
        let storage = Arc::new(PluginStorageStore::load(&dir.path().join("storage")).unwrap());
        let secrets = Arc::new(PluginSecretStore::load(&dir.path().join("secrets")).unwrap());
        let delayed_messages = Arc::new(DelayedMessageScheduler::default());
        let api = PluginRuntimeApi::new(
            Arc::clone(&scheduler),
            state,
            storage,
            secrets,
            Arc::clone(&delayed_messages),
            publisher,
        );
        let scheduled = match scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: persisted_manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_id: 1,
                    activation_mode: PublicActivationMode::Submit,
                    input: "input".into(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 1,
                        control_value: "/runtime input".into(),
                        submission_token: "submission-notification".into(),
                    },
                },
                Instant::now(),
            )
            .unwrap()
        {
            PluginScheduleOutcome::Dispatched(request) => request,
            PluginScheduleOutcome::Waiting { .. } => panic!("first request must dispatch"),
        };
        let label = runtime_label(&persisted_manifest.plugin_id, 1).unwrap();
        (api, scheduler, delayed_messages, scheduled, label)
    }

    #[derive(Clone, Copy)]
    enum FakePublishOutcome {
        Succeed,
        OperationFailed,
    }

    struct FakePublisher {
        available: AtomicBool,
        outcomes: Mutex<VecDeque<FakePublishOutcome>>,
        calls: Mutex<Vec<MessagePublishRequest>>,
    }

    impl FakePublisher {
        fn new(outcomes: impl IntoIterator<Item = FakePublishOutcome>) -> Self {
            Self {
                available: AtomicBool::new(true),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn unavailable() -> Self {
            Self {
                available: AtomicBool::new(false),
                outcomes: Mutex::new(VecDeque::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<MessagePublishRequest> {
            self.calls.lock().unwrap().clone()
        }

        fn set_available(&self, available: bool) {
            self.available.store(available, Ordering::Release);
        }
    }

    impl MessagePublisher for FakePublisher {
        fn is_available(&self) -> bool {
            self.available.load(Ordering::Acquire)
        }

        fn commit_publish(&self, request: MessagePublishRequest) -> MessagePublishOutcome {
            let mut calls = self.calls.lock().unwrap();
            calls.push(request.clone());
            match self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(FakePublishOutcome::Succeed)
            {
                FakePublishOutcome::Succeed => MessagePublishOutcome::Published(MessagePublished {
                    id: calls.len().to_string(),
                    plugin_id: request.plugin_id,
                    plugin_name_snapshot: request.plugin_name_snapshot,
                    created_at: "2026-08-19T01:02:03Z".into(),
                    content: request.content,
                    revision: calls.len().to_string(),
                    unread_count: calls.len(),
                }),
                FakePublishOutcome::OperationFailed => MessagePublishOutcome::OperationFailed,
            }
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
        let publisher = Arc::new(FakePublisher::new([]));
        let api = PluginRuntimeApi::new(
            Arc::clone(&scheduler),
            state,
            storage,
            secrets,
            Arc::new(DelayedMessageScheduler::default()),
            publisher,
        );
        let manifest = manifest();
        let scheduled = match scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_id: 1,
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
        assert_eq!(
            api.execute(&label, get.clone(), &manifest).result,
            Ok(Value::Null)
        );

        get.context.request_id = "forged".into();
        assert_eq!(
            api.execute(&label, get, &manifest).result,
            Err(PluginRuntimeError::InvalidContext)
        );
        scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_id: 1,
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
            )
            .result,
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
            )
            .result,
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

    #[test]
    fn notification_permission_is_checked_before_store_access() {
        let dir = TestDir::new();
        let persisted = manifest();
        let declared = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([FakePublishOutcome::Succeed]));
        let (api, _, _, scheduled, label) =
            runtime_fixture(&dir, &persisted, BTreeSet::new(), publisher.clone());
        let input = Some(json!({ "content": "message" }));

        assert_eq!(
            api.execute(
                &label,
                notification_request(scheduled.context.clone(), input.clone()),
                &persisted,
            )
            .result,
            Err(PluginRuntimeError::PermissionDenied)
        );
        assert_eq!(
            api.execute(
                &label,
                notification_request(scheduled.context, input),
                &declared,
            )
            .result,
            Err(PluginRuntimeError::PermissionDenied)
        );
        assert!(publisher.calls().is_empty());
    }

    #[test]
    fn notification_input_validation_preserves_the_500_scalar_original() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([FakePublishOutcome::Succeed]));
        let (api, _, _, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher.clone(),
        );

        for invalid in [
            None,
            Some(json!({})),
            Some(json!({ "content": 1 })),
            Some(json!({ "content": "   " })),
            Some(json!({ "content": "line one\nline two" })),
            Some(json!({ "content": "control\u{0000}" })),
            Some(json!({ "content": "x".repeat(501) })),
            Some(json!({ "content": "valid", "unknown": true })),
        ] {
            let execution = api.execute(
                &label,
                notification_request(scheduled.context.clone(), invalid),
                &manifest,
            );
            assert_eq!(
                execution.result,
                Err(PluginRuntimeError::InvalidNotification)
            );
            assert_eq!(execution.post_guard_effect, None);
        }

        let original = format!("  {}  ", "界".repeat(496));
        let execution = api.execute(
            &label,
            notification_request(
                scheduled.context,
                Some(json!({ "content": original.clone() })),
            ),
            &manifest,
        );
        assert_eq!(execution.result, Ok(Value::Null));
        assert_eq!(publisher.calls()[0].content, original);
    }

    #[test]
    fn one_request_can_publish_only_once() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([FakePublishOutcome::Succeed]));
        let (api, _, _, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher.clone(),
        );
        let request =
            notification_request(scheduled.context, Some(json!({ "content": "only once" })));

        assert_eq!(
            api.execute(&label, request.clone(), &manifest).result,
            Ok(Value::Null)
        );
        assert_eq!(
            api.execute(&label, request, &manifest).result,
            Err(PluginRuntimeError::AlreadyPublished)
        );
        assert_eq!(publisher.calls().len(), 1);
    }

    #[test]
    fn failed_persistence_does_not_consume_the_request_allowance() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([
            FakePublishOutcome::OperationFailed,
            FakePublishOutcome::Succeed,
        ]));
        let (api, _, _, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher.clone(),
        );
        let request = notification_request(scheduled.context, Some(json!({ "content": "retry" })));

        let failed = api.execute(&label, request.clone(), &manifest);
        assert_eq!(
            failed.result,
            Err(PluginRuntimeError::MessageStoreUnavailable)
        );
        assert_eq!(failed.post_guard_effect, None);
        assert_eq!(
            api.execute(&label, request, &manifest).result,
            Ok(Value::Null)
        );
        assert_eq!(publisher.calls().len(), 2);
    }

    #[test]
    fn committed_publish_stays_successful_when_a_new_request_supersedes_it() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let message_center = Arc::new(MessageCenterService::load(dir.path()));
        let (api, scheduler, _, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            message_center.clone(),
        );

        let execution = api.execute(
            &label,
            notification_request(
                scheduled.context.clone(),
                Some(json!({ "content": "committed" })),
            ),
            &manifest,
        );
        scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_id: 1,
                    activation_mode: PublicActivationMode::Submit,
                    input: "new".into(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 2,
                        control_value: "/runtime new".into(),
                        submission_token: "submission-new".into(),
                    },
                },
                Instant::now(),
            )
            .unwrap();

        assert_eq!(execution.result, Ok(Value::Null));
        assert!(matches!(
            execution.post_guard_effect,
            Some(MessagePostGuardEffect::Published(ref message))
                if message.content == "committed"
        ));
        assert_eq!(
            scheduler.context_status(&scheduled.context),
            crate::public_plugins::PluginContextStatus::Expired
        );
        assert_eq!(message_center.summary().unwrap().unread_count, 1);
    }

    #[test]
    fn exhaustion_returns_one_deferred_unavailable_effect() {
        let dir = TestDir::new();
        let root = dir.path().join("message-center");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("messages.json"),
            serde_json::to_vec(&json!({
                "schema": 1,
                "revision": u64::MAX.to_string(),
                "nextMessageId": "1",
                "messages": []
            }))
            .unwrap(),
        )
        .unwrap();
        let manifest = notification_manifest();
        let message_center = Arc::new(MessageCenterService::load(dir.path()));
        let (api, _, _, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            message_center,
        );
        let request =
            notification_request(scheduled.context, Some(json!({ "content": "exhausted" })));

        let first = api.execute(&label, request.clone(), &manifest);
        assert_eq!(
            first.result,
            Err(PluginRuntimeError::MessageStoreUnavailable)
        );
        assert_eq!(
            first.post_guard_effect,
            Some(MessagePostGuardEffect::BecameUnavailable)
        );
        let second = api.execute(&label, request, &manifest);
        assert_eq!(
            second.result,
            Err(PluginRuntimeError::MessageStoreUnavailable)
        );
        assert_eq!(second.post_guard_effect, None);
    }

    #[test]
    fn accepted_schedule_survives_request_supersession_with_immutable_identity() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([]));
        let (api, scheduler, delayed_messages, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher,
        );
        let before = Instant::now();

        let execution = api.execute(
            &label,
            scheduled_notification_request(
                scheduled.context.clone(),
                Some(json!({ "content": "later", "delayMs": 10_000 })),
            ),
            &manifest,
        );
        scheduler
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: manifest.plugin_id.clone(),
                    plugin_generation: 1,
                    activation_id: 1,
                    activation_mode: PublicActivationMode::Submit,
                    input: "new".into(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch: 2,
                        control_value: "/runtime new".into(),
                        submission_token: "submission-new".into(),
                    },
                },
                Instant::now(),
            )
            .unwrap();

        assert_eq!(execution.result, Ok(Value::Null));
        assert_eq!(execution.post_guard_effect, None);
        assert_eq!(
            scheduler.context_status(&scheduled.context),
            crate::public_plugins::PluginContextStatus::Expired
        );
        let message = delayed_messages
            .claim_due(before + Duration::from_secs(11))
            .unwrap()
            .unwrap();
        assert_eq!(message.plugin_id, manifest.plugin_id);
        assert_eq!(message.plugin_generation, 1);
        assert_eq!(message.plugin_name_snapshot, manifest.name);
        assert_eq!(message.request_id, scheduled.context.request_id);
        assert_eq!(message.content, "later");
    }

    #[test]
    fn unavailable_store_rejects_schedule_without_consuming_notification_allowance() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::unavailable());
        let (api, _, delayed_messages, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher.clone(),
        );

        let schedule = scheduled_notification_request(
            scheduled.context.clone(),
            Some(json!({ "content": "later", "delayMs": 10_000 })),
        );
        assert_eq!(
            api.execute(&label, schedule, &manifest).result,
            Err(PluginRuntimeError::MessageStoreUnavailable)
        );
        assert_eq!(
            delayed_messages.claim_due(Instant::now() + Duration::from_secs(11)),
            Ok(None)
        );
        publisher.set_available(true);
        assert_eq!(
            api.execute(
                &label,
                notification_request(scheduled.context, Some(json!({ "content": "immediate" })),),
                &manifest,
            )
            .result,
            Ok(Value::Null)
        );
        assert_eq!(publisher.calls().len(), 1);
    }

    #[test]
    fn schedule_validation_distinguishes_content_and_delay_without_consuming_allowance() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([]));
        let (api, _, delayed_messages, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher,
        );

        for (notification, expected) in [
            (
                json!({ "content": "   ", "delayMs": 10_000 }),
                PluginRuntimeError::InvalidNotification,
            ),
            (
                json!({ "content": "later" }),
                PluginRuntimeError::InvalidDelay,
            ),
            (
                json!({ "content": "later", "delayMs": -1 }),
                PluginRuntimeError::InvalidDelay,
            ),
            (
                json!({ "content": "later", "delayMs": 1.5 }),
                PluginRuntimeError::InvalidDelay,
            ),
            (
                json!({ "content": "later", "delayMs": 999 }),
                PluginRuntimeError::InvalidDelay,
            ),
            (
                json!({ "content": "later", "delayMs": 86_400_001 }),
                PluginRuntimeError::InvalidDelay,
            ),
            (
                json!({ "content": "later", "delayMs": 10_000, "unknown": true }),
                PluginRuntimeError::InvalidNotification,
            ),
        ] {
            assert_eq!(
                api.execute(
                    &label,
                    scheduled_notification_request(scheduled.context.clone(), Some(notification),),
                    &manifest,
                )
                .result,
                Err(expected)
            );
        }

        assert_eq!(
            api.execute(
                &label,
                scheduled_notification_request(
                    scheduled.context,
                    Some(json!({ "content": "valid", "delayMs": 10_000 })),
                ),
                &manifest,
            )
            .result,
            Ok(Value::Null)
        );
        assert!(delayed_messages
            .claim_due(Instant::now() + Duration::from_secs(11))
            .unwrap()
            .is_some());
    }

    #[test]
    fn immediate_and_delayed_notifications_share_one_request_allowance() {
        for schedule_first in [true, false] {
            let dir = TestDir::new();
            let manifest = notification_manifest();
            let publisher = Arc::new(FakePublisher::new([FakePublishOutcome::Succeed]));
            let (api, _, delayed_messages, scheduled, label) = runtime_fixture(
                &dir,
                &manifest,
                BTreeSet::from([PublicPermission::NotificationsPublish]),
                publisher.clone(),
            );
            let schedule = scheduled_notification_request(
                scheduled.context.clone(),
                Some(json!({ "content": "later", "delayMs": 10_000 })),
            );
            let publish =
                notification_request(scheduled.context, Some(json!({ "content": "immediate" })));
            let (first, second) = if schedule_first {
                (schedule, publish)
            } else {
                (publish, schedule)
            };

            assert_eq!(
                api.execute(&label, first, &manifest).result,
                Ok(Value::Null)
            );
            assert_eq!(
                api.execute(&label, second, &manifest).result,
                Err(PluginRuntimeError::AlreadyPublished),
                "schedule_first={schedule_first}"
            );
            let queued = delayed_messages
                .claim_due(Instant::now() + Duration::from_secs(11))
                .unwrap();
            assert_eq!(queued.is_some(), schedule_first);
            assert_eq!(publisher.calls().len(), usize::from(!schedule_first));
        }
    }

    #[test]
    fn schedule_limit_error_does_not_consume_request_allowance() {
        let dir = TestDir::new();
        let manifest = notification_manifest();
        let publisher = Arc::new(FakePublisher::new([]));
        let (api, _, delayed_messages, scheduled, label) = runtime_fixture(
            &dir,
            &manifest,
            BTreeSet::from([PublicPermission::NotificationsPublish]),
            publisher,
        );
        let now = Instant::now();
        for index in 0..32 {
            delayed_messages
                .schedule(
                    DelayedMessageRegistration {
                        plugin_id: manifest.plugin_id.clone(),
                        plugin_generation: 1,
                        activation_id: 1,
                        plugin_name_snapshot: manifest.name.clone(),
                        request_id: format!("prefill-{index}"),
                        content: "prefill".into(),
                        delay_ms: 10_000,
                    },
                    now,
                )
                .unwrap();
        }
        let request = scheduled_notification_request(
            scheduled.context,
            Some(json!({ "content": "later", "delayMs": 10_000 })),
        );

        assert_eq!(
            api.execute(&label, request.clone(), &manifest).result,
            Err(PluginRuntimeError::ScheduleLimitExceeded)
        );
        assert_eq!(delayed_messages.cancel_plugin(&manifest.plugin_id), Ok(32));
        assert_eq!(
            api.execute(&label, request, &manifest).result,
            Ok(Value::Null)
        );
    }
}
