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
    Acknowledged,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelCallError {
    InvalidCaller,
    ExpiredSession,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingDispatch {
    pub(crate) request_id: String,
    pub(crate) submission_token: String,
    pub(crate) argument: String,
    pub(crate) theme: PluginInvocationTheme,
    pub(crate) invoked_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelSettlementError {
    Stale,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PanelAdmissionError<E> {
    Stale,
    Unavailable,
    Operation(E),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PanelDeliveryTicket {
    session_epoch: u64,
    request_id: String,
    submission_token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueDispatchOutcome {
    Buffered,
    Ready,
}

struct LiveSession {
    identity: PanelSessionIdentity,
    phase: PanelPhase,
    current_request_id: String,
    current_submission_token: String,
    pending: Option<PendingDispatch>,
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

pub(crate) const PUBLIC_PANEL_BOOTSTRAP_TEMPLATE: &str = r#"
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
    await invoke('plugin_panel_content_ready', { sessionEpoch: '__SESSION_EPOCH__' });
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
      await invoke('plugin_panel_content_ack', {
        requestId: update.requestId,
        sessionEpoch: update.sessionEpoch,
      });
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

fn panel_bootstrap(session_epoch: u64) -> String {
    PUBLIC_PANEL_BOOTSTRAP_TEMPLATE.replace("__SESSION_EPOCH__", &session_epoch.to_string())
}

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

pub(crate) fn plugin_panel_content_label(plugin_id: &str, session_epoch: u64) -> Option<String> {
    if session_epoch == 0 {
        return None;
    }
    label_component(plugin_id)
        .map(|value| format!("plugin-panel-content-{value}-s{session_epoch:016x}"))
}

pub(crate) fn plugin_id_from_panel_content_label(label: &str) -> Option<String> {
    let encoded = label.strip_prefix("plugin-panel-content-")?;
    let (plugin_id, session) = encoded.rsplit_once("-s")?;
    if session.len() != 16 || u64::from_str_radix(session, 16).ok()? == 0 {
        return None;
    }
    decode_label_component(plugin_id)
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
        let mut core = self.core.lock().ok()?;
        let session_epoch = core.next_epoch.checked_add(1)?;
        let content_label = plugin_panel_content_label(&owner.plugin_id, session_epoch)?;
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
            current_submission_token: owner.submission_token,
            pending: None,
        });
        self.changed.notify_all();
        Some(identity)
    }

    pub(crate) fn queue_dispatch(
        &self,
        session_epoch: u64,
        dispatch: PendingDispatch,
    ) -> Result<QueueDispatchOutcome, PublicPluginManagementError> {
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
            session.pending = None;
            session.current_request_id = dispatch.request_id;
            session.current_submission_token = dispatch.submission_token;
            self.changed.notify_all();
            return Ok(QueueDispatchOutcome::Ready);
        }
        if session.phase == PanelPhase::Acknowledged {
            session.pending = Some(dispatch);
            return Ok(QueueDispatchOutcome::Buffered);
        }
        if session.phase == PanelPhase::AwaitingReady {
            session.current_request_id = dispatch.request_id.clone();
            session.current_submission_token = dispatch.submission_token.clone();
        }
        session.pending = Some(dispatch);
        self.changed.notify_all();
        Ok(QueueDispatchOutcome::Buffered)
    }

    #[cfg(test)]
    pub(crate) fn wait_until_dispatchable(
        &self,
        session_epoch: u64,
        request_id: &str,
        submission_token: &str,
        _timeout: Duration,
    ) -> Result<PendingDispatch, PanelSettlementError> {
        match self.wait_until_dispatchable_and_admit(
            session_epoch,
            request_id,
            submission_token,
            || Ok::<_, ()>(()),
        ) {
            Ok((dispatch, ())) => Ok(dispatch),
            Err(PanelAdmissionError::Stale) => Err(PanelSettlementError::Stale),
            Err(PanelAdmissionError::Unavailable | PanelAdmissionError::Operation(())) => {
                Err(PanelSettlementError::Unavailable)
            }
        }
    }

    pub(crate) fn admit_current_dispatch<T, E>(
        &self,
        session_epoch: u64,
        request_id: &str,
        submission_token: &str,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, PanelAdmissionError<E>> {
        let core = self
            .core
            .lock()
            .map_err(|_| PanelAdmissionError::Unavailable)?;
        let session = core
            .session
            .as_ref()
            .filter(|session| session.identity.session_epoch == session_epoch)
            .ok_or(PanelAdmissionError::Stale)?;
        if session.phase != PanelPhase::Ready
            || session.current_request_id != request_id
            || session.current_submission_token != submission_token
        {
            return Err(PanelAdmissionError::Stale);
        }
        operation().map_err(PanelAdmissionError::Operation)
    }

    pub(crate) fn wait_until_dispatchable_and_admit<T, E>(
        &self,
        session_epoch: u64,
        request_id: &str,
        submission_token: &str,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<(PendingDispatch, T), PanelAdmissionError<E>> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelAdmissionError::Unavailable)?;
        loop {
            let session = core
                .session
                .as_mut()
                .filter(|session| session.identity.session_epoch == session_epoch)
                .ok_or(PanelAdmissionError::Stale)?;
            let pending_matches = session.pending.as_ref().is_some_and(|pending| {
                pending.request_id == request_id && pending.submission_token == submission_token
            });
            if !pending_matches {
                return Err(PanelAdmissionError::Stale);
            }
            if session.phase == PanelPhase::Ready {
                let dispatch = session.pending.take().ok_or(PanelAdmissionError::Stale)?;
                session.current_request_id = dispatch.request_id.clone();
                session.current_submission_token = dispatch.submission_token.clone();
                let result = operation().map_err(PanelAdmissionError::Operation)?;
                self.changed.notify_all();
                return Ok((dispatch, result));
            }
            core = self
                .changed
                .wait(core)
                .map_err(|_| PanelAdmissionError::Unavailable)?;
        }
    }

    #[cfg(test)]
    pub(crate) fn promote_pending_dispatch(&self, session_epoch: u64) -> Option<PendingDispatch> {
        let mut core = self.core.lock().ok()?;
        let session = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)?;
        if session.phase != PanelPhase::Ready {
            return None;
        }
        let dispatch = session.pending.take()?;
        session.current_request_id = dispatch.request_id.clone();
        session.current_submission_token = dispatch.submission_token.clone();
        Some(dispatch)
    }

    pub(crate) fn claim_delivery_settlement(
        &self,
        session_epoch: u64,
        request_id: &str,
        submission_token: &str,
    ) -> Result<(), PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let session = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)
            .ok_or(PanelSettlementError::Stale)?;
        if session.current_request_id != request_id
            || session.current_submission_token != submission_token
            || session.phase != PanelPhase::Ready
        {
            return Err(PanelSettlementError::Stale);
        }
        session.phase = PanelPhase::AwaitingAck;
        Ok(())
    }

    fn classify_delivery_failure(&self, ticket: &PanelDeliveryTicket) -> PanelSettlementError {
        let Ok(core) = self.core.lock() else {
            return PanelSettlementError::Unavailable;
        };
        let current = core.session.as_ref().is_some_and(|session| {
            session.identity.session_epoch == ticket.session_epoch
                && session.current_request_id == ticket.request_id
                && session.current_submission_token == ticket.submission_token
                && matches!(
                    session.phase,
                    PanelPhase::AwaitingAck | PanelPhase::Acknowledged | PanelPhase::Ready
                )
        });
        if current {
            PanelSettlementError::Unavailable
        } else {
            PanelSettlementError::Stale
        }
    }

    pub(crate) fn mark_ready(&self, content_label: &str, session_epoch: u64) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(session) = core.session.as_mut() else {
            return false;
        };
        if session.identity.content_label != content_label
            || session.identity.session_epoch != session_epoch
        {
            return false;
        }
        if session.phase != PanelPhase::AwaitingReady {
            return session.phase == PanelPhase::Ready;
        }
        session.phase = PanelPhase::Ready;
        self.changed.notify_all();
        true
    }

    pub(crate) fn mark_ack(
        &self,
        content_label: &str,
        session_epoch: u64,
        request_id: &str,
    ) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(session) = core.session.as_mut() else {
            return false;
        };
        if session.identity.content_label != content_label
            || session.identity.session_epoch != session_epoch
            || session.current_request_id != request_id
        {
            return false;
        }
        if session.phase != PanelPhase::AwaitingAck {
            return false;
        }
        session.phase = PanelPhase::Acknowledged;
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
                    && session.phase == PanelPhase::Ready
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
    ) -> Result<(), PanelSettlementError> {
        let Ok(mut core) = self.core.lock() else {
            return Err(PanelSettlementError::Unavailable);
        };
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .ok_or(PanelSettlementError::Unavailable)?;
        loop {
            match core.session.as_mut() {
                None => return Err(PanelSettlementError::Stale),
                Some(session) if session.identity.session_epoch != session_epoch => {
                    return Err(PanelSettlementError::Stale);
                }
                Some(session) if session.current_request_id != request_id => {
                    return Err(PanelSettlementError::Stale);
                }
                Some(session) if session.phase == PanelPhase::Acknowledged => {
                    session.phase = PanelPhase::Ready;
                    self.changed.notify_all();
                    return Ok(());
                }
                Some(session) if session.phase == PanelPhase::Ready => return Ok(()),
                Some(_) => {}
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(PanelSettlementError::Unavailable);
            }
            let (guard, _) = self
                .changed
                .wait_timeout(core, deadline.saturating_duration_since(now))
                .map_err(|_| PanelSettlementError::Unavailable)?;
            core = guard;
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
        let core = self.core.lock().map_err(|_| PanelCallError::Unavailable)?;
        let session = core
            .session
            .as_ref()
            .ok_or(PanelCallError::ExpiredSession)?;
        if session.identity.content_label != content_label
            || session.identity.plugin_id != plugin_id
        {
            return Err(PanelCallError::InvalidCaller);
        }
        if session.identity.session_epoch != session_epoch {
            return Err(PanelCallError::ExpiredSession);
        }
        let allowed = match session.phase {
            PanelPhase::AwaitingReady => !mutable,
            PanelPhase::AwaitingAck | PanelPhase::Acknowledged | PanelPhase::Ready => true,
        };
        if !allowed {
            return Err(PanelCallError::ExpiredSession);
        }
        Ok(session.identity.clone())
    }

    pub(crate) fn teardown_session(
        &self,
        session_epoch: Option<u64>,
    ) -> Option<PanelSessionIdentity> {
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

    pub(crate) fn teardown_current(
        &self,
        session_epoch: u64,
        request_id: &str,
        submission_token: &str,
    ) -> Option<PanelSessionIdentity> {
        let mut core = self.core.lock().ok()?;
        let current = core.session.as_ref().is_some_and(|session| {
            session.identity.session_epoch == session_epoch
                && session.current_request_id == request_id
                && session.current_submission_token == submission_token
        });
        if !current {
            return None;
        }
        let identity = core.session.take()?.identity;
        self.changed.notify_all();
        Some(identity)
    }

    #[cfg(test)]
    pub(crate) fn accepted_submission_token(
        &self,
        session_epoch: u64,
        submission_token: &str,
    ) -> bool {
        let Ok(core) = self.core.lock() else {
            return false;
        };
        core.session.as_ref().is_some_and(|session| {
            session.identity.session_epoch == session_epoch
                && session.current_submission_token == submission_token
        })
    }
}

pub(crate) fn content_ready(
    controller: &PluginPanelController,
    label: &str,
    session_epoch: u64,
) -> bool {
    controller.mark_ready(label, session_epoch)
}

pub(crate) fn content_ack(
    controller: &PluginPanelController,
    label: &str,
    session_epoch: u64,
    request_id: &str,
) -> bool {
    controller.mark_ack(label, session_epoch, request_id)
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
) -> Result<PanelSessionIdentity, PublicPluginManagementError> {
    teardown(app, controller.as_ref(), None);
    let identity = controller
        .open_session(owner)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    if let Err(error) = mount_webview(app, Arc::clone(&controller), &identity, panel_entry) {
        teardown(app, &controller, Some(identity.session_epoch));
        return Err(error);
    }
    controller
        .wait_until_ready(identity.session_epoch, CONTENT_READY_TIMEOUT)
        .inspect_err(|_| {
            teardown(app, &controller, Some(identity.session_epoch));
        })?;
    Ok(identity)
}

fn mount_webview(
    app: &AppHandle,
    controller: Arc<PluginPanelController>,
    identity: &PanelSessionIdentity,
    panel_entry: &str,
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
        panel_bootstrap(identity.session_epoch),
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
    ticket: &PanelDeliveryTicket,
    update: PluginPanelUpdate,
) -> Result<(), PanelSettlementError> {
    let content = settle_delivery_operation(
        controller,
        ticket,
        app.get_webview(&identity.content_label).ok_or(()),
    )?;
    let session_payload = settle_delivery_operation(
        controller,
        ticket,
        serde_json::to_string(&serde_json::json!({
            "sessionEpoch": identity.session_epoch.to_string(),
        })),
    )?;
    settle_delivery_operation(
        controller,
        ticket,
        content.eval(format!(
            "window.__UIPILOT_PLUGIN_PANEL_PREPARE__({session_payload});"
        )),
    )?;
    let payload = settle_delivery_operation(controller, ticket, serde_json::to_string(&update))?;
    settle_delivery_operation(
        controller,
        ticket,
        content.eval(format!(
            "window.__UIPILOT_PLUGIN_PANEL_UPDATE__({payload});"
        )),
    )?;
    controller.wait_until_acked(
        identity.session_epoch,
        &update.request_id,
        CONTENT_ACK_TIMEOUT,
    )
}

pub(crate) fn claim_panel_delivery(
    controller: &PluginPanelController,
    session_epoch: u64,
    request_id: &str,
    submission_token: &str,
) -> Result<PanelDeliveryTicket, PanelSettlementError> {
    controller.claim_delivery_settlement(session_epoch, request_id, submission_token)?;
    Ok(PanelDeliveryTicket {
        session_epoch,
        request_id: request_id.to_owned(),
        submission_token: submission_token.to_owned(),
    })
}

fn settle_delivery_operation<T, E>(
    controller: &PluginPanelController,
    ticket: &PanelDeliveryTicket,
    result: Result<T, E>,
) -> Result<T, PanelSettlementError> {
    result.map_err(|_| controller.classify_delivery_failure(ticket))
}

pub(crate) fn deliver_panel_update(
    app: &AppHandle,
    controller: &PluginPanelController,
    identity: &PanelSessionIdentity,
    submission_token: &str,
    update: PluginPanelUpdate,
) -> Result<(), PanelSettlementError> {
    let ticket = claim_panel_delivery(
        controller,
        identity.session_epoch,
        &update.request_id,
        submission_token,
    )?;
    deliver_update(app, controller, identity, &ticket, update)
}

pub(crate) fn queue_dispatch(
    controller: &PluginPanelController,
    session_epoch: u64,
    dispatch: PendingDispatch,
) -> Result<QueueDispatchOutcome, PublicPluginManagementError> {
    controller.queue_dispatch(session_epoch, dispatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
        let label = plugin_panel_content_label("com.uipilot.demo-panel", 42).unwrap();
        assert!(label.starts_with("plugin-panel-content-"));
        assert!(label.ends_with("-s000000000000002a"));
        assert_eq!(
            plugin_id_from_panel_content_label(&label).as_deref(),
            Some("com.uipilot.demo-panel")
        );
        assert!(plugin_id_from_panel_content_label("plugin-content-abc").is_none());
        assert!(plugin_id_from_panel_content_label("plugin-panel-content-zz").is_none());
        assert!(plugin_id_from_panel_content_label(
            "plugin-panel-content-636f6d-s0000000000000000"
        )
        .is_none());
        assert!(plugin_id_from_panel_content_label("plugin-panel-content-636f6d-s1").is_none());
    }

    #[test]
    fn session_epoch_bumps_and_teardown_drops_live_session() {
        let controller = PluginPanelController::default();
        let first = controller.open_session(owner("a")).unwrap();
        let second = controller.open_session(owner("b")).unwrap();
        assert_ne!(first.session_epoch, second.session_epoch);
        assert_ne!(first.content_label, second.content_label);
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

    fn pending_dispatch(
        request: &str,
        argument: &str,
        theme: PluginInvocationTheme,
    ) -> PendingDispatch {
        PendingDispatch {
            request_id: request.into(),
            submission_token: format!("token-{request}"),
            argument: argument.into(),
            theme,
            invoked_at: format!("t-{request}"),
        }
    }

    fn content_ready_session(controller: &PluginPanelController) -> PanelSessionIdentity {
        let identity = controller.open_session(owner("a")).unwrap();
        assert!(content_ready(
            controller,
            &identity.content_label,
            identity.session_epoch,
        ));
        identity
    }

    #[test]
    fn not_ready_keeps_only_latest_pending_argument() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch,
        ));
        let pending = controller
            .promote_pending_dispatch(identity.session_epoch)
            .unwrap();
        assert_eq!(pending.request_id, "b");
        assert_eq!(pending.argument, "two");
        assert!(controller
            .promote_pending_dispatch(identity.session_epoch)
            .is_none());
    }

    #[test]
    fn pending_dispatch_stores_argument_only_for_runtime_after_ready() {
        let source = include_str!("plugin_panel.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("plugin panel test module marker is missing");
        assert!(production.contains("struct PendingDispatch"));
        assert!(!production.contains("struct PendingArgument"));
        assert!(!production.contains("pending.data"));
        assert!(production.contains("pub(crate) fn promote_pending_dispatch"));
        assert!(production.contains("pub(crate) fn queue_dispatch"));
        assert!(production.contains("pub(crate) fn claim_delivery_settlement"));
        assert!(!production.contains("pub(crate) fn buffer_pending_dispatch"));
        assert!(!production.contains("pub(crate) fn bind_submission"));
        assert!(!production.contains("drain_pending_updates"));
        assert!(!production.contains("submit_update"));
    }

    #[test]
    fn first_runtime_delivery_settles_after_content_ready_without_spurious_ack() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch,
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "hello", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(!content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "a"
        ));
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .is_ok());
    }

    #[test]
    fn ready_admission_discards_stale_pending_before_later_promotion() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "buffered", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch,
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("c", "ready", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .promote_pending_dispatch(identity.session_epoch)
            .is_none());
        assert_eq!(
            controller.claim_delivery_settlement(identity.session_epoch, "b", "token-b"),
            Err(PanelSettlementError::Stale)
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "c", "token-c")
            .is_ok());
    }

    #[test]
    fn promote_pending_binds_atomically_and_later_ready_supersedes() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "buffered", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch,
        ));
        let promoted = controller
            .promote_pending_dispatch(identity.session_epoch)
            .unwrap();
        assert_eq!(promoted.request_id, "b");
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("c", "ready", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert_eq!(
            controller.claim_delivery_settlement(identity.session_epoch, "b", "token-b"),
            Err(PanelSettlementError::Stale)
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "c", "token-c")
            .is_ok());
    }

    #[test]
    fn superseding_queue_dispatch_invalidates_stale_runtime_settlement() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert_eq!(
            controller.claim_delivery_settlement(identity.session_epoch, "a", "token-a"),
            Err(PanelSettlementError::Stale)
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "b", "token-b")
            .is_ok());
    }

    #[test]
    fn runtime_admission_cannot_reverse_newer_enter_order() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("b", "two", PluginInvocationTheme::Light),
            )
            .unwrap();
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("c", "three", PluginInvocationTheme::Dark),
            )
            .unwrap();

        assert!(matches!(
            controller.admit_current_dispatch(
                identity.session_epoch,
                "b",
                "token-b",
                || Ok::<_, ()>("B"),
            ),
            Err(PanelAdmissionError::Stale)
        ));
        assert_eq!(
            controller
                .admit_current_dispatch(
                    identity.session_epoch,
                    "c",
                    "token-c",
                    || Ok::<_, ()>("C"),
                )
                .unwrap(),
            "C"
        );
    }

    #[test]
    fn concurrent_enter_admissions_keep_single_current_token() {
        use std::{
            sync::{Arc, Barrier},
            thread,
        };

        let controller = Arc::new(PluginPanelController::default());
        let identity = content_ready_session(&controller);
        let epoch = identity.session_epoch;
        let barrier = Arc::new(Barrier::new(2));
        let first = {
            let controller = Arc::clone(&controller);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                controller.queue_dispatch(
                    epoch,
                    PendingDispatch {
                        request_id: "a".into(),
                        submission_token: "token-a".into(),
                        argument: "a".into(),
                        theme: PluginInvocationTheme::Dark,
                        invoked_at: "t0".into(),
                    },
                )
            })
        };
        let second = {
            let controller = Arc::clone(&controller);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                controller.queue_dispatch(
                    epoch,
                    PendingDispatch {
                        request_id: "b".into(),
                        submission_token: "token-b".into(),
                        argument: "b".into(),
                        theme: PluginInvocationTheme::Light,
                        invoked_at: "t1".into(),
                    },
                )
            })
        };
        assert_eq!(first.join().unwrap().unwrap(), QueueDispatchOutcome::Ready);
        assert_eq!(second.join().unwrap().unwrap(), QueueDispatchOutcome::Ready);
        let accepts_a = controller.accepted_submission_token(epoch, "token-a");
        let accepts_b = controller.accepted_submission_token(epoch, "token-b");
        assert_ne!(accepts_a, accepts_b);
        if accepts_a {
            assert!(controller
                .claim_delivery_settlement(epoch, "b", "token-b")
                .is_err());
            assert!(controller
                .claim_delivery_settlement(epoch, "a", "token-a")
                .is_ok());
        } else {
            assert!(controller
                .claim_delivery_settlement(epoch, "a", "token-a")
                .is_err());
            assert!(controller
                .claim_delivery_settlement(epoch, "b", "token-b")
                .is_ok());
        }
    }

    #[test]
    fn ready_and_ack_are_bound_to_exact_label_request_and_session_epoch() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("request-1")).unwrap();
        assert!(!content_ready(
            &controller,
            "plugin-panel-content-forged",
            identity.session_epoch
        ));
        assert!(!content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch + 1
        ));
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch
        ));
        assert!(!content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "wrong-request"
        ));
        assert!(!content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "request-1"
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("request-1", "hello", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "request-1", "token-request-1")
            .is_ok());
        assert!(content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "request-1"
        ));
        controller.teardown_session(Some(identity.session_epoch));
        assert!(!content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "request-1"
        ));
    }

    #[test]
    fn stale_ack_from_previous_session_cannot_complete_new_session() {
        let controller = PluginPanelController::default();
        let first = controller.open_session(owner("a")).unwrap();
        let label = first.content_label.clone();
        let stale_epoch = first.session_epoch;
        assert!(content_ready(&controller, &label, stale_epoch));
        controller.teardown_session(None);
        let second = controller.open_session(owner("b")).unwrap();
        assert_ne!(second.content_label, label);
        assert!(!content_ack(&controller, &label, stale_epoch, "a"));
        assert!(content_ready(
            &controller,
            &second.content_label,
            second.session_epoch
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    second.session_epoch,
                    pending_dispatch("b", "hello", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(second.session_epoch, "b", "token-b")
            .is_ok());
        assert!(content_ack(
            &controller,
            &second.content_label,
            second.session_epoch,
            "b"
        ));
    }

    #[test]
    fn stale_ready_from_previous_session_cannot_unlock_new_session() {
        let controller = PluginPanelController::default();
        let first = controller.open_session(owner("a")).unwrap();
        let label = first.content_label.clone();
        let stale_epoch = first.session_epoch;
        controller.teardown_session(None);
        let second = controller.open_session(owner("b")).unwrap();
        assert_ne!(second.content_label, label);
        assert_ne!(second.session_epoch, stale_epoch);
        assert!(!content_ready(&controller, &label, stale_epoch));
        assert!(content_ready(
            &controller,
            &second.content_label,
            second.session_epoch
        ));
    }

    #[test]
    fn submission_token_is_bound_to_live_session() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert!(controller.accepted_submission_token(identity.session_epoch, "token-a"));
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch,
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(!controller.accepted_submission_token(identity.session_epoch, "token-a"));
        assert!(controller.accepted_submission_token(identity.session_epoch, "token-b"));
    }

    #[test]
    fn pending_buffered_during_ack_survives_until_runtime_dispatch() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .is_ok());
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert!(content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "a"
        ));
        assert!(controller
            .wait_until_acked(identity.session_epoch, "a", Duration::from_secs(1))
            .is_ok());
        let pending = controller
            .promote_pending_dispatch(identity.session_epoch)
            .unwrap();
        assert_eq!(pending.request_id, "b");
        assert_eq!(pending.argument, "two");
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "b", "token-b")
            .is_ok());
    }

    #[test]
    fn only_latest_buffered_enter_becomes_runtime_dispatchable() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("a", "one", PluginInvocationTheme::Dark),
            )
            .unwrap();
        controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .unwrap();
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("c", "three", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert!(content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "a"
        ));
        controller
            .wait_until_acked(identity.session_epoch, "a", Duration::from_secs(1))
            .unwrap();

        assert_eq!(
            controller.wait_until_dispatchable(
                identity.session_epoch,
                "b",
                "token-b",
                Duration::from_secs(1),
            ),
            Err(PanelSettlementError::Stale)
        );
        let promoted = controller
            .wait_until_dispatchable(
                identity.session_epoch,
                "c",
                "token-c",
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(promoted.argument, "three");
    }

    #[test]
    fn buffered_enter_is_stale_after_panel_teardown() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("a", "one", PluginInvocationTheme::Dark),
            )
            .unwrap();
        controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .unwrap();
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("b", "two", PluginInvocationTheme::Light),
            )
            .unwrap();
        controller.teardown_session(Some(identity.session_epoch));

        assert_eq!(
            controller.wait_until_dispatchable(
                identity.session_epoch,
                "b",
                "token-b",
                Duration::from_secs(1),
            ),
            Err(PanelSettlementError::Stale)
        );
    }

    #[test]
    fn late_submission_after_teardown_is_ignored() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        controller.teardown_session(Some(identity.session_epoch));
        assert!(controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("late", "x", PluginInvocationTheme::Dark),
            )
            .is_err());
    }

    #[test]
    fn empty_argument_is_a_valid_panel_submission() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        let dispatch = pending_dispatch("empty", "", PluginInvocationTheme::Dark);
        assert_eq!(dispatch.argument, "");
        assert_eq!(
            controller
                .queue_dispatch(identity.session_epoch, dispatch)
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "empty", "token-empty")
            .is_ok());
    }

    #[test]
    fn queue_dispatch_buffers_before_ready_and_promotes_after_content_ready() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        let dispatch = pending_dispatch("a", "hello", PluginInvocationTheme::Dark);
        assert_eq!(
            queue_dispatch(&controller, identity.session_epoch, dispatch.clone()).unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch
        ));
        let promoted = controller
            .promote_pending_dispatch(identity.session_epoch)
            .unwrap();
        assert_eq!(promoted.request_id, "a");
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .is_ok());
    }

    #[test]
    fn claim_panel_delivery_returns_stale_when_submission_superseded() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert_eq!(
            claim_panel_delivery(&controller, identity.session_epoch, "a", "token-a"),
            Err(PanelSettlementError::Stale)
        );
    }

    #[test]
    fn native_delivery_failure_is_stale_after_claimed_session_teardown() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        let ticket =
            claim_panel_delivery(&controller, identity.session_epoch, "a", "token-a").unwrap();
        controller.teardown_session(Some(identity.session_epoch));

        assert_eq!(
            settle_delivery_operation(&controller, &ticket, Err::<(), _>(())),
            Err(PanelSettlementError::Stale)
        );
    }

    #[test]
    fn native_delivery_failure_is_unavailable_for_live_claimed_session() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        let ticket =
            claim_panel_delivery(&controller, identity.session_epoch, "a", "token-a").unwrap();

        assert_eq!(
            settle_delivery_operation(&controller, &ticket, Err::<(), _>(())),
            Err(PanelSettlementError::Unavailable)
        );
    }

    #[test]
    fn stale_failure_cannot_teardown_a_newer_submission() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("a", "one", PluginInvocationTheme::Dark),
            )
            .unwrap();
        controller
            .queue_dispatch(
                identity.session_epoch,
                pending_dispatch("b", "two", PluginInvocationTheme::Light),
            )
            .unwrap();

        let stale = controller.teardown_current(identity.session_epoch, "a", "token-a");
        assert!(stale.is_none());
        assert!(controller.accepted_submission_token(identity.session_epoch, "token-b"));

        let current = controller
            .teardown_current(identity.session_epoch, "b", "token-b")
            .unwrap();
        assert_eq!(current, identity);
        assert!(controller.live_identity().is_none());
    }

    #[test]
    fn acknowledged_enter_buffers_before_wait_consumes_ack() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .is_ok());
        assert!(content_ack(
            &controller,
            &identity.content_label,
            identity.session_epoch,
            "a"
        ));
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("b", "two", PluginInvocationTheme::Light),
                )
                .unwrap(),
            QueueDispatchOutcome::Buffered
        );
        assert_eq!(
            controller
                .wait_until_acked(identity.session_epoch, "a", Duration::from_secs(1))
                .unwrap(),
            ()
        );
        assert!(controller.accepted_submission_token(identity.session_epoch, "token-a"));
        let pending = controller
            .promote_pending_dispatch(identity.session_epoch)
            .unwrap();
        assert_eq!(pending.request_id, "b");
    }

    #[test]
    fn wait_until_acked_returns_stale_after_teardown() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .is_ok());
        controller.teardown_session(Some(identity.session_epoch));
        assert_eq!(
            controller.wait_until_acked(identity.session_epoch, "a", Duration::from_secs(1)),
            Err(PanelSettlementError::Stale)
        );
    }

    #[test]
    fn wait_until_acked_returns_stale_when_session_epoch_superseded() {
        let controller = PluginPanelController::default();
        let first = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    first.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(first.session_epoch, "a", "token-a")
            .is_ok());
        controller.teardown_session(Some(first.session_epoch));
        let second = controller.open_session(owner("b")).unwrap();
        assert!(content_ready(
            &controller,
            &second.content_label,
            second.session_epoch,
        ));
        assert_eq!(
            controller.wait_until_acked(first.session_epoch, "a", Duration::from_secs(1)),
            Err(PanelSettlementError::Stale)
        );
    }

    #[test]
    fn wait_until_acked_times_out_with_unavailable_for_live_unacked_delivery() {
        let controller = PluginPanelController::default();
        let identity = content_ready_session(&controller);
        assert_eq!(
            controller
                .queue_dispatch(
                    identity.session_epoch,
                    pending_dispatch("a", "one", PluginInvocationTheme::Dark),
                )
                .unwrap(),
            QueueDispatchOutcome::Ready
        );
        assert!(controller
            .claim_delivery_settlement(identity.session_epoch, "a", "token-a")
            .is_ok());
        assert_eq!(
            controller.wait_until_acked(identity.session_epoch, "a", Duration::from_millis(1)),
            Err(PanelSettlementError::Unavailable)
        );
    }

    #[test]
    fn storage_calls_require_live_panel_session_and_epoch() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();
        assert_eq!(
            controller.begin_storage_call(
                "plugin-panel-content-forged",
                identity.session_epoch,
                false
            ),
            Err(PanelCallError::InvalidCaller)
        );
        assert_eq!(
            controller.begin_storage_call(
                &identity.content_label,
                identity.session_epoch + 1,
                false
            ),
            Err(PanelCallError::ExpiredSession)
        );
        assert!(controller
            .begin_storage_call(&identity.content_label, identity.session_epoch, false)
            .is_ok());
        assert_eq!(
            controller.begin_storage_call(&identity.content_label, identity.session_epoch, true),
            Err(PanelCallError::ExpiredSession)
        );
        assert!(content_ready(
            &controller,
            &identity.content_label,
            identity.session_epoch
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
        let bootstrap = panel_bootstrap(42);
        for required in [
            "uipilotPluginPanel",
            "onUpdate(next)",
            "plugin_panel_content_ready",
            "sessionEpoch: '42'",
            "plugin_panel_content_ack",
            "sessionEpoch: update.sessionEpoch",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "Reflect.deleteProperty(window, '__TAURI_INTERNALS__')",
            "__UIPILOT_PLUGIN_PANEL_UPDATE__",
            "sessionEpoch",
        ] {
            assert!(
                bootstrap.contains(required),
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
                !bootstrap.contains(forbidden),
                "forbidden bootstrap fragment: {forbidden}"
            );
        }
    }

    #[test]
    fn mount_waits_for_content_ready_without_runtime_data() {
        let source = include_str!("plugin_panel.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("plugin panel test module marker is missing");
        let mount_body = production
            .split("pub(crate) fn mount(")
            .nth(1)
            .and_then(|tail| tail.split("\nfn mount_webview").next())
            .expect("mount body is missing");
        assert!(!mount_body.contains("deliver_update"));
        assert!(!mount_body.contains("deliver_panel_update"));
        assert!(mount_body.contains("wait_until_ready"));
    }

    #[test]
    fn panel_bootstrap_is_owned_and_not_leaked() {
        let source = include_str!("plugin_panel.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("plugin panel test module marker is missing");
        assert!(!production.contains("Box::leak"));
        assert!(production.contains("fn panel_bootstrap(session_epoch: u64) -> String"));
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
            "panel_bootstrap(",
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
            ".initialization_script(PUBLIC_PANEL_BOOTSTRAP_TEMPLATE)",
            "navigate_main",
        ] {
            assert!(
                !production.contains(forbidden),
                "forbidden panel fragment: {forbidden}"
            );
        }
    }
}
