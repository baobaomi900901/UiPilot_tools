use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use serde::Serialize;
use serde_json::Value;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl,
};

use crate::public_plugins::{
    inert_url, prepare_windows_webview, verify_windows_webview_muted, PluginInvocationTheme,
    PublicPluginManagementError, PublicPluginService, WebViewGuardOwner,
};

const CONTENT_READY_TIMEOUT: Duration = Duration::from_secs(5);
const CONTENT_ACK_TIMEOUT: Duration = Duration::from_secs(5);
/// Approximate launcher input chrome height inside the 720×420 main window.
const PANEL_TOP_OFFSET: f64 = 56.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PanelSessionIdentity {
    pub(crate) session_epoch: u64,
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) command_label: String,
    pub(crate) content_label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PanelOwner {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) command_label: String,
    pub(crate) request_id: String,
    pub(crate) submission_token: String,
    pub(crate) argument: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PanelPhase {
    AwaitingReady,
    AwaitingAck,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelCallError {
    InvalidCaller,
    ExpiredSession,
    Unavailable,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingArgument {
    pub(crate) request_id: String,
    pub(crate) submission_token: String,
    pub(crate) argument: String,
    pub(crate) theme: PluginInvocationTheme,
    pub(crate) invoked_at: String,
    pub(crate) data: Value,
}

struct LiveSession {
    identity: PanelSessionIdentity,
    phase: PanelPhase,
    current_request_id: String,
    pending: Option<PendingArgument>,
}

#[derive(Default)]
struct ControllerCore {
    next_epoch: u64,
    session: Option<LiveSession>,
}

#[derive(Default)]
pub(crate) struct PluginPanelController {
    core: Mutex<ControllerCore>,
    changed: Condvar,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPanelUpdate {
    pub(crate) request_id: String,
    pub(crate) input: String,
    pub(crate) platform: &'static str,
    pub(crate) theme: PluginInvocationTheme,
    pub(crate) invoked_at: String,
    pub(crate) session_epoch: String,
    pub(crate) data: Value,
}

pub(crate) const PUBLIC_PANEL_BOOTSTRAP: &str = r#"
(() => {
  'use strict';
  let handler = null;
  let invoke = null;
  let readySent = false;
  let storageSession = null;
  const deepFreeze = (value, seen = new WeakSet()) => {
    if ((typeof value !== 'object' && typeof value !== 'function') || value === null || seen.has(value)) return value;
    seen.add(value);
    for (const key of Reflect.ownKeys(value)) deepFreeze(value[key], seen);
    return Object.freeze(value);
  };
  const expiredStorage = deepFreeze({
    get: async () => { throw new Error('ExpiredWindowSessionError'); },
    set: async () => { throw new Error('ExpiredWindowSessionError'); },
    remove: async () => { throw new Error('ExpiredWindowSessionError'); },
  });
  const createStorageSession = (sessionEpoch) => {
    const session = { sessionEpoch, active: true };
    const call = async (command, args = {}) => {
      if (!session.active || storageSession !== session || !invoke) throw new Error('ExpiredWindowSessionError');
      return invoke(command, { sessionEpoch, ...args });
    };
    session.storage = deepFreeze({
      async get(key) { return call('plugin_panel_storage_get', { key }); },
      async set(key, value) { await call('plugin_panel_storage_set', { key, value: deepFreeze(value) }); },
      async remove(key) { await call('plugin_panel_storage_remove', { key }); },
    });
    return session;
  };
  const sendReady = async () => {
    if (!invoke || !handler || readySent) return;
    readySent = true;
    await invoke('plugin_panel_content_ready');
  };
  const api = deepFreeze({
    onUpdate(next) {
      if (handler || typeof next !== 'function') throw new TypeError('one onUpdate handler required');
      handler = next;
      void sendReady();
      return () => { if (handler === next) handler = null; };
    },
    get storage() { return storageSession ? storageSession.storage : expiredStorage; },
  });
  Object.defineProperty(window, 'uipilotPluginPanel', { value: api, configurable: false });
  Object.defineProperty(window, '__UIPILOT_PLUGIN_PANEL_PREPARE__', {
    configurable: false,
    value: ({ sessionEpoch }) => {
      if (typeof sessionEpoch !== 'string' || !/^(0|[1-9][0-9]*)$/.test(sessionEpoch) || sessionEpoch === '0') {
        throw new TypeError('invalid session epoch');
      }
      if (storageSession) storageSession.active = false;
      storageSession = createStorageSession(sessionEpoch);
    },
  });
  Object.defineProperty(window, '__UIPILOT_PLUGIN_PANEL_UPDATE__', {
    configurable: false,
    value: async (update) => {
      if (!handler || !invoke) throw new Error('content not ready');
      const root = document.documentElement;
      const dark = update.theme === 'dark';
      const tokens = dark
        ? ['#202020','#2b2b2b','#f5f5f5','#d9d9d9','#595959','#69b1ff','#ff7875']
        : ['#ffffff','#fafafa','#171717','#595959','#d9d9d9','#0067c0','#c62828'];
      const names = ['background','surface','text','text-muted','border','accent','danger'];
      names.forEach((name, index) => root.style.setProperty(`--uipilot-color-${name}`, tokens[index]));
      root.style.setProperty('--uipilot-font-family', 'Segoe UI, system-ui, sans-serif');
      await handler(deepFreeze(update));
      await invoke('plugin_panel_content_ack', { requestId: update.requestId });
    },
  });
  const wait = () => {
    const internals = window.__TAURI_INTERNALS__;
    if (!internals) return setTimeout(wait, 0);
    invoke = internals.invoke.bind(internals);
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    void sendReady();
  };
  wait();
})();
"#;

fn label_component(plugin_id: &str) -> Option<String> {
    let encoded = plugin_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    (!encoded.is_empty()).then_some(encoded)
}

fn decode_label_component(value: &str) -> Option<String> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

pub(crate) fn plugin_panel_content_label(plugin_id: &str) -> Option<String> {
    label_component(plugin_id).map(|value| format!("plugin-panel-content-{value}"))
}

pub(crate) fn plugin_id_from_panel_content_label(label: &str) -> Option<String> {
    decode_label_component(label.strip_prefix("plugin-panel-content-")?)
}

impl PluginPanelController {
    pub(crate) fn live_identity(&self) -> Option<PanelSessionIdentity> {
        self.core
            .lock()
            .ok()?
            .session
            .as_ref()
            .map(|session| session.identity.clone())
    }

    pub(crate) fn open_session(&self, owner: PanelOwner) -> Option<PanelSessionIdentity> {
        let content_label = plugin_panel_content_label(&owner.plugin_id)?;
        let mut core = self.core.lock().ok()?;
        let session_epoch = core.next_epoch.checked_add(1)?;
        core.next_epoch = session_epoch;
        let identity = PanelSessionIdentity {
            session_epoch,
            plugin_id: owner.plugin_id.clone(),
            generation: owner.plugin_generation,
            activation_id: owner.activation_id,
            admission_epoch: owner.admission_epoch,
            command_label: owner.command_label.clone(),
            content_label,
        };
        core.session = Some(LiveSession {
            identity: identity.clone(),
            phase: PanelPhase::AwaitingReady,
            current_request_id: owner.request_id,
            pending: None,
        });
        self.changed.notify_all();
        Some(identity)
    }

    pub(crate) fn buffer_pending(
        &self,
        session_epoch: u64,
        pending: PendingArgument,
    ) -> Result<(), PublicPluginManagementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let session = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if session.phase == PanelPhase::Ready {
            return Err(PublicPluginManagementError::Unavailable);
        }
        session.current_request_id = pending.request_id.clone();
        session.pending = Some(pending);
        Ok(())
    }

    pub(crate) fn take_pending(&self, session_epoch: u64) -> Option<PendingArgument> {
        let mut core = self.core.lock().ok()?;
        let session = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)?;
        session.pending.take()
    }

    pub(crate) fn mark_ready(&self, content_label: &str) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(session) = core.session.as_mut() else {
            return false;
        };
        if session.identity.content_label != content_label {
            return false;
        }
        if session.phase != PanelPhase::AwaitingReady {
            return session.phase == PanelPhase::AwaitingAck || session.phase == PanelPhase::Ready;
        }
        session.phase = PanelPhase::AwaitingAck;
        self.changed.notify_all();
        true
    }

    pub(crate) fn mark_ack(&self, content_label: &str, request_id: &str) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(session) = core.session.as_mut() else {
            return false;
        };
        if session.identity.content_label != content_label
            || session.current_request_id != request_id
        {
            return false;
        }
        if session.phase != PanelPhase::AwaitingAck && session.phase != PanelPhase::Ready {
            return false;
        }
        session.phase = PanelPhase::Ready;
        self.changed.notify_all();
        true
    }

    pub(crate) fn wait_until_ready(
        &self,
        session_epoch: u64,
        timeout: Duration,
    ) -> Result<(), PublicPluginManagementError> {
        let Ok(mut core) = self.core.lock() else {
            return Err(PublicPluginManagementError::Unavailable);
        };
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        loop {
            let ready = core.session.as_ref().is_some_and(|session| {
                session.identity.session_epoch == session_epoch
                    && matches!(session.phase, PanelPhase::AwaitingAck | PanelPhase::Ready)
            });
            if ready {
                return Ok(());
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(PublicPluginManagementError::RuntimeNotReady);
            }
            let (guard, result) = self
                .changed
                .wait_timeout(core, deadline.saturating_duration_since(now))
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            core = guard;
            if result.timed_out() {
                return Err(PublicPluginManagementError::RuntimeNotReady);
            }
        }
    }

    pub(crate) fn wait_until_acked(
        &self,
        session_epoch: u64,
        request_id: &str,
        timeout: Duration,
    ) -> Result<(), PublicPluginManagementError> {
        let Ok(mut core) = self.core.lock() else {
            return Err(PublicPluginManagementError::Unavailable);
        };
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        loop {
            let ready = core.session.as_ref().is_some_and(|session| {
                session.identity.session_epoch == session_epoch
                    && session.current_request_id == request_id
                    && session.phase == PanelPhase::Ready
            });
            if ready {
                return Ok(());
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(PublicPluginManagementError::Unavailable);
            }
            let (guard, result) = self
                .changed
                .wait_timeout(core, deadline.saturating_duration_since(now))
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            core = guard;
            if result.timed_out() {
                return Err(PublicPluginManagementError::Unavailable);
            }
        }
    }

    pub(crate) fn begin_storage_call(
        &self,
        content_label: &str,
        session_epoch: u64,
        mutable: bool,
    ) -> Result<PanelSessionIdentity, PanelCallError> {
        let plugin_id = plugin_id_from_panel_content_label(content_label)
            .ok_or(PanelCallError::InvalidCaller)?;
        let core = self
            .core
            .lock()
            .map_err(|_| PanelCallError::Unavailable)?;
        let session = core
            .session
            .as_ref()
            .ok_or(PanelCallError::ExpiredSession)?;
        if session.identity.content_label != content_label || session.identity.plugin_id != plugin_id
        {
            return Err(PanelCallError::InvalidCaller);
        }
        if session.identity.session_epoch != session_epoch {
            return Err(PanelCallError::ExpiredSession);
        }
        let allowed = match session.phase {
            PanelPhase::AwaitingReady => !mutable,
            PanelPhase::AwaitingAck | PanelPhase::Ready => true,
        };
        if !allowed {
            return Err(PanelCallError::ExpiredSession);
        }
        Ok(session.identity.clone())
    }

    pub(crate) fn teardown_session(&self, session_epoch: Option<u64>) -> Option<PanelSessionIdentity> {
        let mut core = self.core.lock().ok()?;
        let session = core.session.take()?;
        if let Some(expected) = session_epoch {
            if session.identity.session_epoch != expected {
                core.session = Some(session);
                return None;
            }
        }
        self.changed.notify_all();
        Some(session.identity)
    }

    pub(crate) fn set_current_request(
        &self,
        session_epoch: u64,
        request_id: &str,
    ) -> Result<(), PublicPluginManagementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let session = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        session.current_request_id = request_id.to_owned();
        if session.phase == PanelPhase::Ready {
            session.phase = PanelPhase::AwaitingAck;
        }
        Ok(())
    }
}

pub(crate) fn content_ready(controller: &PluginPanelController, label: &str) -> bool {
    controller.mark_ready(label)
}

pub(crate) fn content_ack(
    controller: &PluginPanelController,
    label: &str,
    request_id: &str,
) -> bool {
    controller.mark_ack(label, request_id)
}

pub(crate) fn destroy_content(app: &AppHandle, content_label: &str) {
    if let Some(webview) = app.get_webview(content_label) {
        let _ = webview.close();
    }
}

pub(crate) fn teardown(
    app: &AppHandle,
    controller: &PluginPanelController,
    session_epoch: Option<u64>,
) {
    if let Some(identity) = controller.teardown_session(session_epoch) {
        destroy_content(app, &identity.content_label);
    }
}

fn panel_bounds(main: &tauri::Window) -> Result<(LogicalPosition<f64>, LogicalSize<f64>), ()> {
    let size = main.inner_size().map_err(|_| ())?;
    let scale = main.scale_factor().map_err(|_| ())?;
    let width = (size.width as f64 / scale).max(1.0);
    let height = (size.height as f64 / scale).max(1.0);
    let top = PANEL_TOP_OFFSET.min(height * 0.25);
    let panel_height = (height - top).max(1.0);
    Ok((
        LogicalPosition::new(0.0, top),
        LogicalSize::new(width, panel_height),
    ))
}

pub(crate) fn mount(
    app: &AppHandle,
    controller: Arc<PluginPanelController>,
    owner: PanelOwner,
    panel_entry: &str,
    theme: PluginInvocationTheme,
    invoked_at: String,
    data: Value,
) -> Result<PanelSessionIdentity, PublicPluginManagementError> {
    teardown(app, controller.as_ref(), None);
    let identity = controller
        .open_session(owner.clone())
        .ok_or(PublicPluginManagementError::Unavailable)?;
    if let Err(error) = mount_webview(
        app,
        Arc::clone(&controller),
        &identity,
        panel_entry,
        PluginPanelUpdate {
            request_id: owner.request_id.clone(),
            input: owner.argument.clone(),
            platform: "windows",
            theme,
            invoked_at: invoked_at.clone(),
            session_epoch: identity.session_epoch.to_string(),
            data: data.clone(),
        },
    ) {
        teardown(app, &controller, Some(identity.session_epoch));
        return Err(error);
    }

    if let Err(error) = controller.wait_until_ready(identity.session_epoch, CONTENT_READY_TIMEOUT) {
        teardown(app, &controller, Some(identity.session_epoch));
        return Err(error);
    }

    if let Some(pending) = controller.take_pending(identity.session_epoch) {
        deliver_update(
            app,
            &controller,
            &identity,
            PluginPanelUpdate {
                request_id: pending.request_id,
                input: pending.argument,
                platform: "windows",
                theme: pending.theme,
                invoked_at: pending.invoked_at,
                session_epoch: identity.session_epoch.to_string(),
                data: pending.data,
            },
        )?;
    } else {
        deliver_update(
            app,
            &controller,
            &identity,
            PluginPanelUpdate {
                request_id: owner.request_id,
                input: owner.argument,
                platform: "windows",
                theme,
                invoked_at,
                session_epoch: identity.session_epoch.to_string(),
                data,
            },
        )?;
    }
    Ok(identity)
}

fn mount_webview(
    app: &AppHandle,
    controller: Arc<PluginPanelController>,
    identity: &PanelSessionIdentity,
    panel_entry: &str,
    _initial: PluginPanelUpdate,
) -> Result<(), PublicPluginManagementError> {
    if app.get_webview(&identity.content_label).is_some() {
        destroy_content(app, &identity.content_label);
    }
    let main = app
        .get_window("main")
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let (position, size) =
        panel_bounds(&main).map_err(|_| PublicPluginManagementError::Unavailable)?;
    let content_url = tauri::Url::parse(&format!(
        "uipilot-public-plugin://localhost/{}",
        panel_entry.trim_start_matches('/')
    ))
    .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let inert = inert_url().map_err(|_| PublicPluginManagementError::Unavailable)?;
    let content = WebviewBuilder::new(
        identity.content_label.clone(),
        WebviewUrl::CustomProtocol(inert),
    )
    .on_navigation(|url| {
        matches!(url.scheme(), "uipilot-public-plugin" | "http")
            && url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host.eq_ignore_ascii_case("uipilot-public-plugin.localhost")
            })
            && url.port().is_none()
    })
    .on_new_window(|_, _| NewWindowResponse::Deny)
    .on_download(|_, _| false);
    let content = main
        .add_child(content, position, size)
        .map_err(|_| PublicPluginManagementError::Unavailable)?;

    let guard_controller = Arc::clone(&controller);
    let guard_app = app.clone();
    let guard_label = identity.content_label.clone();
    let on_unmuted = Arc::new(move |_owner| {
        let _ = guard_controller.teardown_session(None);
        destroy_content(&guard_app, &guard_label);
    });

    if prepare_windows_webview(
        &content,
        app.state::<Arc<PublicPluginService>>().webview_guards(),
        WebViewGuardOwner::Content {
            label: identity.content_label.clone(),
            plugin_id: identity.plugin_id.clone(),
            session_generation: identity.session_epoch,
        },
        PUBLIC_PANEL_BOOTSTRAP,
        content_url,
        on_unmuted,
        CONTENT_READY_TIMEOUT,
    )
    .is_err()
    {
        destroy_content(app, &identity.content_label);
        return Err(PublicPluginManagementError::RuntimeNotReady);
    }
    let _ = verify_windows_webview_muted(&content, CONTENT_READY_TIMEOUT);
    Ok(())
}

fn deliver_update(
    app: &AppHandle,
    controller: &PluginPanelController,
    identity: &PanelSessionIdentity,
    update: PluginPanelUpdate,
) -> Result<(), PublicPluginManagementError> {
    controller.set_current_request(identity.session_epoch, &update.request_id)?;
    let content = app
        .get_webview(&identity.content_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let session_payload = serde_json::to_string(&serde_json::json!({
        "sessionEpoch": identity.session_epoch.to_string(),
    }))
    .map_err(|_| PublicPluginManagementError::Unavailable)?;
    content
        .eval(format!(
            "window.__UIPILOT_PLUGIN_PANEL_PREPARE__({session_payload});"
        ))
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let payload =
        serde_json::to_string(&update).map_err(|_| PublicPluginManagementError::Unavailable)?;
    content
        .eval(format!(
            "window.__UIPILOT_PLUGIN_PANEL_UPDATE__({payload});"
        ))
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    controller.wait_until_acked(
        identity.session_epoch,
        &update.request_id,
        CONTENT_ACK_TIMEOUT,
    )
}

pub(crate) fn submit_update(
    app: &AppHandle,
    controller: &PluginPanelController,
    session_epoch: u64,
    request_id: String,
    submission_token: String,
    argument: String,
    theme: PluginInvocationTheme,
    invoked_at: String,
    data: Value,
) -> Result<(), PublicPluginManagementError> {
    let identity = controller
        .live_identity()
        .filter(|identity| identity.session_epoch == session_epoch)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let phase_ready = {
        let core = controller
            .core
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        core.session.as_ref().is_some_and(|session| {
            session.identity.session_epoch == session_epoch && session.phase == PanelPhase::Ready
        })
    };
    if !phase_ready {
        controller.buffer_pending(
            session_epoch,
            PendingArgument {
                request_id,
                submission_token,
                argument,
                theme,
                invoked_at,
                data,
            },
        )?;
        return Ok(());
    }
    let _ = submission_token;
    deliver_update(
        app,
        controller,
        &identity,
        PluginPanelUpdate {
            request_id,
            input: argument,
            platform: "windows",
            theme,
            invoked_at,
            session_epoch: session_epoch.to_string(),
            data,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(request: &str) -> PanelOwner {
        PanelOwner {
            plugin_id: "com.uipilot.demo-panel".into(),
            plugin_generation: 1,
            activation_id: 2,
            admission_epoch: 3,
            command_label: "bbm".into(),
            request_id: request.into(),
            submission_token: format!("token-{request}"),
            argument: "hello".into(),
        }
    }

    #[test]
    fn panel_labels_are_hex_encoded_and_round_trip() {
        let label = plugin_panel_content_label("com.uipilot.demo-panel").unwrap();
        assert!(label.starts_with("plugin-panel-content-"));
        assert_eq!(
            plugin_id_from_panel_content_label(&label).as_deref(),
            Some("com.uipilot.demo-panel")
        );
        assert!(plugin_id_from_panel_content_label("plugin-content-abc").is_none());
        assert!(plugin_id_from_panel_content_label("plugin-panel-content-zz").is_none());
    }

    #[test]
    fn session_epoch_bumps_and_teardown_drops_live_session() {
        let controller = PluginPanelController::default();
        let first = controller.open_session(owner("a")).unwrap();
        let second = controller.open_session(owner("b")).unwrap();
        assert_ne!(first.session_epoch, second.session_epoch);
        assert_eq!(
            controller.live_identity().map(|value| value.session_epoch),
            Some(second.session_epoch)
        );
        assert!(controller
            .teardown_session(Some(first.session_epoch))
            .is_none());
        assert!(controller
            .teardown_session(Some(second.session_epoch))
            .is_some());
        assert!(controller.live_identity().is_none());
    }

    #[test]
    fn not_ready_keeps_only_latest_pending_argument() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        controller
            .buffer_pending(
                identity.session_epoch,
                PendingArgument {
                    request_id: "a".into(),
                    submission_token: "ta".into(),
                    argument: "one".into(),
                    theme: PluginInvocationTheme::Dark,
                    invoked_at: "t0".into(),
                    data: Value::Null,
                },
            )
            .unwrap();
        controller
            .buffer_pending(
                identity.session_epoch,
                PendingArgument {
                    request_id: "b".into(),
                    submission_token: "tb".into(),
                    argument: "two".into(),
                    theme: PluginInvocationTheme::Light,
                    invoked_at: "t1".into(),
                    data: serde_json::json!({"ok": true}),
                },
            )
            .unwrap();
        let pending = controller.take_pending(identity.session_epoch).unwrap();
        assert_eq!(pending.request_id, "b");
        assert_eq!(pending.argument, "two");
        assert!(controller.take_pending(identity.session_epoch).is_none());
    }

    #[test]
    fn ready_and_ack_are_bound_to_exact_label_and_request() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("request-1")).unwrap();
        assert!(!content_ready(&controller, "plugin-panel-content-forged"));
        assert!(content_ready(&controller, &identity.content_label));
        assert!(!content_ack(
            &controller,
            &identity.content_label,
            "wrong-request"
        ));
        assert!(content_ack(
            &controller,
            &identity.content_label,
            "request-1"
        ));
        controller.teardown_session(Some(identity.session_epoch));
        assert!(!content_ack(
            &controller,
            &identity.content_label,
            "request-1"
        ));
    }

    #[test]
    fn late_submission_after_teardown_is_ignored() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        controller.teardown_session(Some(identity.session_epoch));
        assert!(controller
            .buffer_pending(
                identity.session_epoch,
                PendingArgument {
                    request_id: "late".into(),
                    submission_token: "late".into(),
                    argument: "x".into(),
                    theme: PluginInvocationTheme::Dark,
                    invoked_at: "t".into(),
                    data: Value::Null,
                },
            )
            .is_err());
    }

    #[test]
    fn storage_calls_require_live_panel_session_and_epoch() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert_eq!(
            controller.begin_storage_call("plugin-panel-content-forged", identity.session_epoch, false),
            Err(PanelCallError::InvalidCaller)
        );
        assert_eq!(
            controller.begin_storage_call(&identity.content_label, identity.session_epoch + 1, false),
            Err(PanelCallError::ExpiredSession)
        );
        assert!(controller
            .begin_storage_call(&identity.content_label, identity.session_epoch, false)
            .is_ok());
        assert_eq!(
            controller.begin_storage_call(&identity.content_label, identity.session_epoch, true),
            Err(PanelCallError::ExpiredSession)
        );
        assert!(content_ready(&controller, &identity.content_label));
        assert!(content_ack(
            &controller,
            &identity.content_label,
            "a"
        ));
        assert!(controller
            .begin_storage_call(&identity.content_label, identity.session_epoch, true)
            .is_ok());
        controller.teardown_session(Some(identity.session_epoch));
        assert_eq!(
            controller.begin_storage_call(&identity.content_label, identity.session_epoch, false),
            Err(PanelCallError::ExpiredSession)
        );
    }

    #[test]
    fn panel_bootstrap_exposes_update_and_storage_only() {
        for required in [
            "uipilotPluginPanel",
            "onUpdate(next)",
            "plugin_panel_content_ready",
            "plugin_panel_content_ack",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "Reflect.deleteProperty(window, '__TAURI_INTERNALS__')",
            "__UIPILOT_PLUGIN_PANEL_UPDATE__",
            "sessionEpoch",
        ] {
            assert!(
                PUBLIC_PANEL_BOOTSTRAP.contains(required),
                "missing bootstrap fragment: {required}"
            );
        }
        for forbidden in [
            "plugin_window_",
            "timer",
            "close:",
            "iframe",
            "srcdoc",
            "fetch(",
            "WebSocket",
            "clipboard",
            "notifications",
        ] {
            assert!(
                !PUBLIC_PANEL_BOOTSTRAP.contains(forbidden),
                "forbidden bootstrap fragment: {forbidden}"
            );
        }
    }

    #[test]
    fn host_panel_uses_child_webview_isolation_contract() {
        let source = include_str!("plugin_panel.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("plugin panel test module marker is missing");
        for required in [
            "WebviewBuilder::new(",
            "WebviewUrl::CustomProtocol(inert)",
            "prepare_windows_webview(",
            "PUBLIC_PANEL_BOOTSTRAP",
            ".on_navigation(",
            ".on_new_window(|_, _| NewWindowResponse::Deny)",
            ".on_download(|_, _| false)",
            ".add_child(",
            "get_window(\"main\")",
            "plugin-panel-content-",
        ] {
            assert!(
                production.contains(required),
                "missing isolation fragment: {required}"
            );
        }
        for forbidden in [
            "NewWindowResponse::Allow",
            "WebviewUrl::External",
            "file://",
            "<iframe",
            "srcdoc",
            "innerHTML",
            ".initialization_script(PUBLIC_PANEL_BOOTSTRAP)",
            "navigate_main",
        ] {
            assert!(
                !production.contains(forbidden),
                "forbidden panel fragment: {forbidden}"
            );
        }
    }
}
