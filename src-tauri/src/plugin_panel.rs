use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{mpsc, Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
};

#[cfg(windows)]
use webview2_com::FocusChangedEventHandler;

use crate::public_plugins::{
    inert_url, prepare_windows_webview, verify_windows_webview_muted, PanelHostKeyDeclaration,
    PluginInvocationTheme, PublicPluginManagementError, PublicPluginService, WebViewGuardOwner,
};

const CONTENT_READY_TIMEOUT: Duration = Duration::from_secs(5);
const CONTENT_ACK_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const HOST_INPUT_FOCUS_TIMEOUT: Duration = Duration::from_secs(2);
const INTERNAL_BLUR_GRACE: Duration = Duration::from_millis(250);
const CONTENT_BLUR_RECHECK_DELAY: Duration = Duration::from_millis(50);
const HOST_KEY_QUEUE_CAPACITY: usize = 8;
pub(crate) const HOST_KEY_ACK_TIMEOUT: Duration = Duration::from_secs(2);
// Mirrors the fixed launcher slot: 12px outer padding, 44px input, 8px gap,
// and a 24px status row above the 12px bottom padding.
const PANEL_HORIZONTAL_INSET: f64 = 12.0;
const PANEL_TOP_OFFSET: f64 = 64.0;
const PANEL_BOTTOM_INSET: f64 = 36.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PanelSessionIdentity {
    pub(crate) session_epoch: u64,
    pub(crate) plugin_id: String,
    pub(crate) generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) command_label: String,
    pub(crate) content_label: String,
    pub(crate) host_keys: Vec<PanelHostKeyDeclaration>,
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
    pub(crate) host_keys: Vec<PanelHostKeyDeclaration>,
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
pub(crate) struct AppFocusLossTicket {
    session_epoch: Option<u64>,
    focus_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostInputFocusIdentity {
    pub(crate) session_epoch: u64,
    pub(crate) focus_request_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostInputFocusPhase {
    Prepared,
    NativeClaimed,
    AwaitingAck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostInputFocusTicket {
    identity: HostInputFocusIdentity,
    phase: HostInputFocusPhase,
    confirmed_main_focus_revision: Option<u64>,
    deadline: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostInputFocusOutcome {
    Focused,
    Noop,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostInputFocusAdvance {
    Advanced,
    Noop,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueDispatchOutcome {
    Buffered,
    Ready,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum PluginPanelHostKey {
    ArrowDown,
    ArrowUp,
    #[serde(rename = "n")]
    N,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HostKeyEnqueueInput {
    pub(crate) client_sequence: u64,
    pub(crate) declaration: PanelHostKeyDeclaration,
    pub(crate) key: PluginPanelHostKey,
    pub(crate) ctrl_key: bool,
    pub(crate) meta_key: bool,
    pub(crate) shift_key: bool,
    pub(crate) alt_key: bool,
}

impl HostKeyEnqueueInput {
    pub(crate) fn valid_chord(self) -> bool {
        match self.declaration {
            PanelHostKeyDeclaration::ArrowDown => {
                self.key == PluginPanelHostKey::ArrowDown
                    && !self.ctrl_key
                    && !self.meta_key
                    && !self.shift_key
                    && !self.alt_key
            }
            PanelHostKeyDeclaration::ArrowUp => {
                self.key == PluginPanelHostKey::ArrowUp
                    && !self.ctrl_key
                    && !self.meta_key
                    && !self.shift_key
                    && !self.alt_key
            }
            PanelHostKeyDeclaration::PrimaryN => {
                self.key == PluginPanelHostKey::N
                    && !self.shift_key
                    && !self.alt_key
                    && if cfg!(target_os = "macos") {
                        self.meta_key && !self.ctrl_key
                    } else {
                        self.ctrl_key && !self.meta_key
                    }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HostKeyEnqueueOutcome {
    Enqueued { route_sequence: u64 },
    DroppedQueueFull,
    Noop,
    ProtocolViolation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostKeyEnqueueDecision {
    pub(crate) outcome: HostKeyEnqueueOutcome,
    pub(crate) start_pump: bool,
    pub(crate) terminate_session: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostKeyDeliveryPhase {
    Prepared,
    NativeFocused,
    DeliveredAwaitingAck,
    Accomplished,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostKeyDeliveryTicket {
    pub(crate) session_epoch: u64,
    pub(crate) content_label: String,
    pub(crate) route_sequence: u64,
    pub(crate) declaration: PanelHostKeyDeclaration,
    pub(crate) key: PluginPanelHostKey,
    pub(crate) ctrl_key: bool,
    pub(crate) meta_key: bool,
    pub(crate) shift_key: bool,
    pub(crate) alt_key: bool,
}

#[derive(Clone, Debug)]
struct HostKeyInFlight {
    ticket: HostKeyDeliveryTicket,
    phase: HostKeyDeliveryPhase,
    ack_deadline: Option<Instant>,
}

#[derive(Clone, Debug)]
struct HostKeyRouteState {
    next_route_sequence: u64,
    next_expected_client_sequence: u64,
    receiver_armed: bool,
    queue: VecDeque<HostKeyDeliveryTicket>,
    in_flight: Option<HostKeyInFlight>,
    pump_running: bool,
}

impl Default for HostKeyRouteState {
    fn default() -> Self {
        Self {
            next_route_sequence: 1,
            next_expected_client_sequence: 1,
            receiver_armed: false,
            queue: VecDeque::new(),
            in_flight: None,
            pump_running: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostKeyAckOutcome {
    Pending,
    Acknowledged,
    TimedOut,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PanelHideTicketIdentity {
    pub(crate) session_epoch: u64,
    pub(crate) hide_ticket_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelHideTicketPhase {
    Admitted,
    Observed,
    Committed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PanelHideTicket {
    identity: PanelHideTicketIdentity,
    phase: PanelHideTicketPhase,
}

struct LiveSession {
    identity: PanelSessionIdentity,
    phase: PanelPhase,
    current_request_id: String,
    current_submission_token: String,
    pending: Option<PendingDispatch>,
    content_focused: bool,
    host_key_route: HostKeyRouteState,
}

#[derive(Default)]
struct ControllerCore {
    next_epoch: u64,
    next_focus_request_id: u64,
    next_hide_ticket_id: u64,
    session: Option<LiveSession>,
    hide_ticket: Option<PanelHideTicket>,
    host_input_focus: Option<HostInputFocusTicket>,
    host_input_focus_settlements: BTreeMap<u64, HostInputFocusOutcome>,
    native_host_input_focus_claims: BTreeSet<u64>,
    focus_revision: u64,
    main_content_focused: bool,
    internal_blur_until: Option<Instant>,
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
  let hostKeyHandler = null;
  let hostKeyRegistrationViolation = false;
  let invoke = null;
  let readySent = false;
  let storageSession = null;
  const deepFreeze = (value, seen = new WeakSet()) => {
    if ((typeof value !== 'object' && typeof value !== 'function') || value === null || seen.has(value)) return value;
    seen.add(value);
    for (const key of Reflect.ownKeys(value)) deepFreeze(value[key], seen);
    return Object.freeze(value);
  };
  const hostKeys = deepFreeze(__HOST_KEYS__);
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
    if (!invoke || !handler || readySent || (hostKeys.length > 0 && !hostKeyHandler)) return;
    readySent = true;
    await invoke('plugin_panel_content_ready', {
      sessionEpoch: '__SESSION_EPOCH__',
      hostKeyReceiverRegistered: hostKeyHandler !== null,
      hostKeyRegistrationViolation,
    });
  };
  const requestPanelHide = () => {
    if (!invoke) return Promise.reject(new Error('ExpiredWindowSessionError'));
    return invoke('plugin_panel_request_hide_admit', { sessionEpoch: '__SESSION_EPOCH__' }).then((result) => {
      if (result?.outcome === 'noop') return;
      if (
        result?.outcome !== 'admitted' || typeof result.hideTicketId !== 'string' ||
        !/^[1-9][0-9]*$/.test(result.hideTicketId)
      ) throw new Error('windowFailed');
      return new Promise((resolve) => {
        resolve();
        void invoke('plugin_panel_request_hide_admit_observed', {
          sessionEpoch: '__SESSION_EPOCH__',
          hideTicketId: result.hideTicketId,
        }).catch(() => undefined);
        setTimeout(() => {
          void invoke('plugin_panel_request_hide_commit', {
            sessionEpoch: '__SESSION_EPOCH__',
            hideTicketId: result.hideTicketId,
          }).catch(() => undefined);
        }, 0);
      });
    });
  };
  const api = deepFreeze({
    onUpdate(next) {
      if (handler || typeof next !== 'function') throw new TypeError('one onUpdate handler required');
      handler = next;
      void sendReady();
      return () => { if (handler === next) handler = null; };
    },
    onHostKey(next) {
      if (hostKeyHandler || typeof next !== 'function') throw new TypeError('one onHostKey handler required');
      if (hostKeys.length === 0) {
        hostKeyRegistrationViolation = true;
        void sendReady();
        throw new TypeError('onHostKey requires panel.hostKeys');
      }
      hostKeyHandler = next;
      void sendReady();
      return () => {
        if (hostKeyHandler !== next) return;
        hostKeyHandler = null;
        void api.requestHide();
      };
    },
    async focusHostInput() {
      if (!invoke) throw new Error('ExpiredWindowSessionError');
      await invoke('plugin_panel_focus_host_input', { sessionEpoch: '__SESSION_EPOCH__' });
    },
    requestHide() { return requestPanelHide(); },
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
  Object.defineProperty(window, '__UIPILOT_PLUGIN_PANEL_HOST_KEY__', {
    configurable: false,
    value: async (event) => {
      if (!hostKeyHandler || !invoke) throw new Error('Host-key receiver unavailable');
      try {
        await hostKeyHandler(deepFreeze(event));
      } finally {
        await invoke('plugin_panel_host_key_ack', {
          sessionEpoch: event.sessionEpoch,
          routeSequence: event.routeSequence,
        });
      }
    },
  });
  document.addEventListener('keydown', (event) => {
    const isComposing = event.isComposing;
    const hadOpenDialog = document.querySelector('dialog[open]') !== null;
    const keyIsEscape = event.key === 'Escape';
    queueMicrotask(() => {
      if (keyIsEscape && !isComposing && !hadOpenDialog && !event.defaultPrevented) {
        void requestPanelHide();
      }
    });
  }, { capture: true });
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

fn panel_bootstrap(session_epoch: u64, host_keys: &[PanelHostKeyDeclaration]) -> String {
    PUBLIC_PANEL_BOOTSTRAP_TEMPLATE
        .replace("__SESSION_EPOCH__", &session_epoch.to_string())
        .replace(
            "__HOST_KEYS__",
            &serde_json::to_string(host_keys).expect("Host-key declarations serialize"),
        )
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

fn disarm_host_key_route(route: &mut HostKeyRouteState) {
    route.receiver_armed = false;
    route.queue.clear();
    route.pump_running = false;
    if let Some(in_flight) = route.in_flight.as_mut() {
        in_flight.phase = HostKeyDeliveryPhase::Cancelled;
    }
}

fn finish_host_key_ack_locked(
    core: &mut ControllerCore,
    ticket: &HostKeyDeliveryTicket,
    now: Instant,
    changed: &Condvar,
) -> HostKeyAckOutcome {
    let Some(session) = core
        .session
        .as_mut()
        .filter(|session| session.identity.session_epoch == ticket.session_epoch)
    else {
        return HostKeyAckOutcome::Stale;
    };
    let Some(in_flight) = session.host_key_route.in_flight.as_ref() else {
        return HostKeyAckOutcome::Stale;
    };
    if in_flight.ticket != *ticket {
        return HostKeyAckOutcome::Stale;
    }
    if in_flight.phase == HostKeyDeliveryPhase::Accomplished {
        session.host_key_route.in_flight = None;
        changed.notify_all();
        return HostKeyAckOutcome::Acknowledged;
    }
    if in_flight.phase != HostKeyDeliveryPhase::DeliveredAwaitingAck {
        return HostKeyAckOutcome::Stale;
    }
    let Some(deadline) = in_flight.ack_deadline else {
        return HostKeyAckOutcome::Stale;
    };
    if now < deadline {
        return HostKeyAckOutcome::Pending;
    }
    if let Some(in_flight) = session.host_key_route.in_flight.as_mut() {
        in_flight.phase = HostKeyDeliveryPhase::Cancelled;
    }
    disarm_host_key_route(&mut session.host_key_route);
    changed.notify_all();
    HostKeyAckOutcome::TimedOut
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
            host_keys: owner.host_keys.clone(),
        };
        core.session = Some(LiveSession {
            identity: identity.clone(),
            phase: PanelPhase::AwaitingReady,
            current_request_id: owner.request_id,
            current_submission_token: owner.submission_token,
            pending: None,
            content_focused: false,
            host_key_route: HostKeyRouteState::default(),
        });
        core.hide_ticket = None;
        core.host_input_focus = None;
        core.internal_blur_until = None;
        self.changed.notify_all();
        Some(identity)
    }

    pub(crate) fn admit_hide(
        &self,
        caller_label: &str,
        session_epoch: u64,
    ) -> Result<Option<PanelHideTicketIdentity>, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let current = core.session.as_ref().is_some_and(|session| {
            session.identity.content_label == caller_label
                && session.identity.session_epoch == session_epoch
        });
        if !current || core.hide_ticket.is_some() {
            return Ok(None);
        }
        let hide_ticket_id = core
            .next_hide_ticket_id
            .checked_add(1)
            .ok_or(PanelSettlementError::Unavailable)?;
        core.next_hide_ticket_id = hide_ticket_id;
        let identity = PanelHideTicketIdentity {
            session_epoch,
            hide_ticket_id,
        };
        core.hide_ticket = Some(PanelHideTicket {
            identity,
            phase: PanelHideTicketPhase::Admitted,
        });
        Ok(Some(identity))
    }

    pub(crate) fn observe_hide(&self, identity: PanelHideTicketIdentity) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(ticket) = core.hide_ticket.as_mut() else {
            return false;
        };
        if ticket.identity != identity || ticket.phase != PanelHideTicketPhase::Admitted {
            return false;
        }
        ticket.phase = PanelHideTicketPhase::Observed;
        true
    }

    pub(crate) fn claim_hide_commit(
        &self,
        identity: PanelHideTicketIdentity,
    ) -> Option<PanelSessionIdentity> {
        let mut core = self.core.lock().ok()?;
        let session_identity = core.session.as_ref()?.identity.clone();
        if session_identity.session_epoch != identity.session_epoch {
            return None;
        }
        let ticket = core.hide_ticket.as_mut()?;
        if ticket.identity != identity
            || !matches!(
                ticket.phase,
                PanelHideTicketPhase::Admitted | PanelHideTicketPhase::Observed
            )
        {
            return None;
        }
        ticket.phase = PanelHideTicketPhase::Committed;
        Some(session_identity)
    }

    pub(crate) fn claim_hide_fallback(
        &self,
        identity: PanelHideTicketIdentity,
        expected_phase: PanelHideTicketPhase,
    ) -> Option<PanelSessionIdentity> {
        if expected_phase == PanelHideTicketPhase::Committed {
            return None;
        }
        let mut core = self.core.lock().ok()?;
        let session_identity = core.session.as_ref()?.identity.clone();
        if session_identity.session_epoch != identity.session_epoch {
            return None;
        }
        let ticket = core.hide_ticket.as_mut()?;
        if ticket.identity != identity || ticket.phase != expected_phase {
            return None;
        }
        ticket.phase = PanelHideTicketPhase::Committed;
        Some(session_identity)
    }

    #[cfg(test)]
    fn set_next_hide_ticket_id(&self, value: u64) {
        if let Ok(mut core) = self.core.lock() {
            core.next_hide_ticket_id = value;
        }
    }

    pub(crate) fn prepare_host_input_focus(
        &self,
        caller_label: &str,
        session_epoch: u64,
        deadline: Instant,
    ) -> Result<Option<HostInputFocusIdentity>, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let current = core.session.as_ref().is_some_and(|session| {
            session.identity.content_label == caller_label
                && session.identity.session_epoch == session_epoch
        });
        if !current {
            return Ok(None);
        }
        let focus_request_id = core
            .next_focus_request_id
            .checked_add(1)
            .ok_or(PanelSettlementError::Unavailable)?;
        core.next_focus_request_id = focus_request_id;
        let identity = HostInputFocusIdentity {
            session_epoch,
            focus_request_id,
        };
        core.host_input_focus = Some(HostInputFocusTicket {
            identity,
            phase: HostInputFocusPhase::Prepared,
            confirmed_main_focus_revision: None,
            deadline,
        });
        self.changed.notify_all();
        Ok(Some(identity))
    }

    pub(crate) fn claim_host_input_focus(
        &self,
        identity: HostInputFocusIdentity,
    ) -> Result<HostInputFocusAdvance, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let session_matches = core
            .session
            .as_ref()
            .is_some_and(|session| session.identity.session_epoch == identity.session_epoch);
        let Some(ticket) = core.host_input_focus.as_mut() else {
            return Ok(HostInputFocusAdvance::Noop);
        };
        if !session_matches
            || ticket.identity != identity
            || ticket.phase != HostInputFocusPhase::Prepared
        {
            return Ok(HostInputFocusAdvance::Noop);
        }
        if Instant::now() >= ticket.deadline {
            core.host_input_focus = None;
            self.changed.notify_all();
            return Ok(HostInputFocusAdvance::Expired);
        }
        ticket.phase = HostInputFocusPhase::NativeClaimed;
        core.native_host_input_focus_claims
            .insert(identity.focus_request_id);
        Ok(HostInputFocusAdvance::Advanced)
    }

    pub(crate) fn fail_native_host_input_focus(
        &self,
        identity: HostInputFocusIdentity,
    ) -> Result<bool, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        core.native_host_input_focus_claims
            .remove(&identity.focus_request_id);
        let current = core.host_input_focus.as_ref().is_some_and(|ticket| {
            ticket.identity == identity && ticket.phase == HostInputFocusPhase::NativeClaimed
        });
        if current {
            core.host_input_focus = None;
            self.changed.notify_all();
        }
        Ok(current)
    }

    pub(crate) fn confirm_native_host_input_focus(
        &self,
        identity: HostInputFocusIdentity,
        now: Instant,
    ) -> Result<HostInputFocusAdvance, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        if !core
            .native_host_input_focus_claims
            .remove(&identity.focus_request_id)
        {
            return Ok(HostInputFocusAdvance::Noop);
        }
        let current = core
            .session
            .as_ref()
            .is_some_and(|session| session.identity.session_epoch == identity.session_epoch)
            && core.host_input_focus.as_ref().is_some_and(|ticket| {
                ticket.identity == identity && ticket.phase == HostInputFocusPhase::NativeClaimed
            });
        let revision = core
            .focus_revision
            .checked_add(1)
            .ok_or(PanelSettlementError::Unavailable)?;
        core.focus_revision = revision;
        core.main_content_focused = true;
        if let Some(session) = core.session.as_mut() {
            session.content_focused = false;
        }
        if !current {
            self.changed.notify_all();
            return Ok(HostInputFocusAdvance::Noop);
        }
        let ticket = core
            .host_input_focus
            .as_mut()
            .expect("validated host input focus ticket");
        let expired = now >= ticket.deadline;
        ticket.confirmed_main_focus_revision = Some(revision);
        if expired {
            core.host_input_focus = None;
            core.host_input_focus_settlements
                .insert(identity.focus_request_id, HostInputFocusOutcome::Failed);
            self.changed.notify_all();
            return Ok(HostInputFocusAdvance::Expired);
        }
        ticket.phase = HostInputFocusPhase::AwaitingAck;
        self.changed.notify_all();
        Ok(HostInputFocusAdvance::Advanced)
    }

    pub(crate) fn ack_host_input_focus(
        &self,
        identity: HostInputFocusIdentity,
        focused: bool,
    ) -> Result<bool, PanelSettlementError> {
        self.ack_host_input_focus_at(identity, focused, Instant::now())
    }

    fn ack_host_input_focus_at(
        &self,
        identity: HostInputFocusIdentity,
        focused: bool,
        now: Instant,
    ) -> Result<bool, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let Some(ticket) = core.host_input_focus.as_ref() else {
            return Ok(false);
        };
        if ticket.identity != identity || ticket.phase != HostInputFocusPhase::AwaitingAck {
            return Ok(false);
        }
        let session_is_current = core.session.as_ref().is_some_and(|session| {
            session.identity.session_epoch == identity.session_epoch && !session.content_focused
        });
        let accepted = focused
            && session_is_current
            && core.main_content_focused
            && ticket.confirmed_main_focus_revision == Some(core.focus_revision)
            && now < ticket.deadline;
        let outcome = if accepted {
            HostInputFocusOutcome::Focused
        } else {
            HostInputFocusOutcome::Failed
        };
        core.host_input_focus = None;
        core.host_input_focus_settlements
            .insert(identity.focus_request_id, outcome);
        self.changed.notify_all();
        Ok(true)
    }

    pub(crate) fn cancel_host_input_focus(
        &self,
        identity: HostInputFocusIdentity,
    ) -> Result<bool, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        if core
            .host_input_focus_settlements
            .remove(&identity.focus_request_id)
            .is_some()
        {
            self.changed.notify_all();
            return Ok(true);
        }
        if core
            .host_input_focus
            .as_ref()
            .is_none_or(|ticket| ticket.identity != identity)
        {
            return Ok(false);
        }
        core.host_input_focus = None;
        self.changed.notify_all();
        Ok(true)
    }

    pub(crate) fn wait_host_input_focus(
        &self,
        identity: HostInputFocusIdentity,
    ) -> Result<HostInputFocusOutcome, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        loop {
            if let Some(outcome) = core
                .host_input_focus_settlements
                .remove(&identity.focus_request_id)
            {
                return Ok(outcome);
            }
            let Some(ticket) = core.host_input_focus.as_ref() else {
                return Ok(HostInputFocusOutcome::Noop);
            };
            if ticket.identity != identity {
                return Ok(HostInputFocusOutcome::Noop);
            }
            let deadline = ticket.deadline;
            let now = Instant::now();
            if now >= deadline {
                core.host_input_focus = None;
                self.changed.notify_all();
                return Ok(HostInputFocusOutcome::Failed);
            }
            let (next, _) = self
                .changed
                .wait_timeout(core, deadline.saturating_duration_since(now))
                .map_err(|_| PanelSettlementError::Unavailable)?;
            core = next;
        }
    }

    pub(crate) fn main_content_got_focus(&self) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(revision) = core.focus_revision.checked_add(1) else {
            return false;
        };
        core.focus_revision = revision;
        core.main_content_focused = true;
        if let Some(session) = core.session.as_mut() {
            session.content_focused = false;
        }
        let live_epoch = core
            .session
            .as_ref()
            .map(|session| session.identity.session_epoch);
        if let Some(ticket) = core.host_input_focus.as_mut() {
            if Some(ticket.identity.session_epoch) == live_epoch
                && matches!(
                    ticket.phase,
                    HostInputFocusPhase::NativeClaimed | HostInputFocusPhase::AwaitingAck
                )
            {
                ticket.confirmed_main_focus_revision = Some(revision);
            }
        }
        true
    }

    pub(crate) fn main_content_lost_focus(
        &self,
        expected_transfer_blur: bool,
    ) -> Option<AppFocusLossTicket> {
        let mut core = self.core.lock().ok()?;
        let revision = core.focus_revision.checked_add(1)?;
        core.focus_revision = revision;
        core.main_content_focused = false;
        if expected_transfer_blur
            || core
                .session
                .as_ref()
                .is_some_and(|session| session.content_focused)
        {
            return None;
        }
        Some(AppFocusLossTicket {
            session_epoch: core
                .session
                .as_ref()
                .map(|session| session.identity.session_epoch),
            focus_revision: revision,
        })
    }

    pub(crate) fn content_got_focus(&self, label: &str, session_epoch: u64) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if !core.session.as_ref().is_some_and(|session| {
            session.identity.content_label == label
                && session.identity.session_epoch == session_epoch
        }) {
            return false;
        }
        let Some(revision) = core.focus_revision.checked_add(1) else {
            return false;
        };
        core.focus_revision = revision;
        core.main_content_focused = false;
        core.session
            .as_mut()
            .expect("validated panel focus session")
            .content_focused = true;
        true
    }

    pub(crate) fn content_lost_focus(
        &self,
        label: &str,
        session_epoch: u64,
    ) -> Option<AppFocusLossTicket> {
        let mut core = self.core.lock().ok()?;
        if !core.session.as_ref().is_some_and(|session| {
            session.identity.content_label == label
                && session.identity.session_epoch == session_epoch
        }) {
            return None;
        }
        let revision = core.focus_revision.checked_add(1)?;
        core.focus_revision = revision;
        core.session
            .as_mut()
            .expect("validated panel focus session")
            .content_focused = false;
        (!core.main_content_focused).then_some(AppFocusLossTicket {
            session_epoch: Some(session_epoch),
            focus_revision: revision,
        })
    }

    pub(crate) fn confirm_app_blur(&self, ticket: &AppFocusLossTicket) -> bool {
        self.core.lock().ok().is_some_and(|core| {
            let current_epoch = core
                .session
                .as_ref()
                .map(|session| session.identity.session_epoch);
            current_epoch == ticket.session_epoch
                && core.focus_revision == ticket.focus_revision
                && !core.main_content_focused
                && core
                    .session
                    .as_ref()
                    .is_none_or(|session| !session.content_focused)
        })
    }

    pub(crate) fn consume_internal_main_blur(&self, now: Instant) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if core.main_content_focused || core.session.is_some() {
            return true;
        }
        core.internal_blur_until
            .take()
            .is_some_and(|deadline| now <= deadline)
    }

    pub(crate) fn host_hidden(&self) {
        let Ok(mut core) = self.core.lock() else {
            return;
        };
        core.focus_revision = core.focus_revision.saturating_add(1);
        core.main_content_focused = false;
        core.internal_blur_until = None;
        core.session = None;
        core.hide_ticket = None;
        core.host_input_focus = None;
        self.changed.notify_all();
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

    pub(crate) fn enqueue_host_key(
        &self,
        session_epoch: u64,
        input: HostKeyEnqueueInput,
    ) -> Result<HostKeyEnqueueDecision, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let Some(session) = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)
        else {
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::Noop,
                start_pump: false,
                terminate_session: false,
            });
        };
        let route = &mut session.host_key_route;
        if session.phase != PanelPhase::Ready
            || !route.receiver_armed
            || !session.identity.host_keys.contains(&input.declaration)
            || !input.valid_chord()
        {
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::Noop,
                start_pump: false,
                terminate_session: false,
            });
        }
        if input.client_sequence < route.next_expected_client_sequence {
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::Noop,
                start_pump: false,
                terminate_session: false,
            });
        }
        if input.client_sequence > route.next_expected_client_sequence {
            disarm_host_key_route(route);
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::ProtocolViolation,
                start_pump: false,
                terminate_session: true,
            });
        }
        let Some(next_expected) = route.next_expected_client_sequence.checked_add(1) else {
            disarm_host_key_route(route);
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::ProtocolViolation,
                start_pump: false,
                terminate_session: true,
            });
        };
        route.next_expected_client_sequence = next_expected;
        if route.queue.len() >= HOST_KEY_QUEUE_CAPACITY {
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::DroppedQueueFull,
                start_pump: false,
                terminate_session: false,
            });
        }
        let Some(next_route_sequence) = route.next_route_sequence.checked_add(1) else {
            disarm_host_key_route(route);
            return Ok(HostKeyEnqueueDecision {
                outcome: HostKeyEnqueueOutcome::ProtocolViolation,
                start_pump: false,
                terminate_session: true,
            });
        };
        let route_sequence = route.next_route_sequence;
        route.next_route_sequence = next_route_sequence;
        route.queue.push_back(HostKeyDeliveryTicket {
            session_epoch,
            content_label: session.identity.content_label.clone(),
            route_sequence,
            declaration: input.declaration,
            key: input.key,
            ctrl_key: input.ctrl_key,
            meta_key: input.meta_key,
            shift_key: input.shift_key,
            alt_key: input.alt_key,
        });
        let start_pump = !route.pump_running;
        route.pump_running = true;
        Ok(HostKeyEnqueueDecision {
            outcome: HostKeyEnqueueOutcome::Enqueued { route_sequence },
            start_pump,
            terminate_session: false,
        })
    }

    pub(crate) fn claim_next_host_key(&self) -> Option<HostKeyDeliveryTicket> {
        let mut core = self.core.lock().ok()?;
        let session = core.session.as_mut()?;
        let route = &mut session.host_key_route;
        if !route.receiver_armed || route.in_flight.is_some() {
            return None;
        }
        let Some(ticket) = route.queue.pop_front() else {
            route.pump_running = false;
            return None;
        };
        route.in_flight = Some(HostKeyInFlight {
            ticket: ticket.clone(),
            phase: HostKeyDeliveryPhase::Prepared,
            ack_deadline: None,
        });
        Some(ticket)
    }

    pub(crate) fn mark_host_key_native_focused(&self, ticket: &HostKeyDeliveryTicket) -> bool {
        self.advance_host_key_phase(
            ticket,
            HostKeyDeliveryPhase::Prepared,
            HostKeyDeliveryPhase::NativeFocused,
            None,
        )
    }

    pub(crate) fn mark_host_key_delivered(
        &self,
        ticket: &HostKeyDeliveryTicket,
        ack_deadline: Instant,
    ) -> bool {
        self.advance_host_key_phase(
            ticket,
            HostKeyDeliveryPhase::NativeFocused,
            HostKeyDeliveryPhase::DeliveredAwaitingAck,
            Some(ack_deadline),
        )
    }

    fn advance_host_key_phase(
        &self,
        ticket: &HostKeyDeliveryTicket,
        expected: HostKeyDeliveryPhase,
        next: HostKeyDeliveryPhase,
        ack_deadline: Option<Instant>,
    ) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(session) = core.session.as_mut() else {
            return false;
        };
        let Some(in_flight) = session.host_key_route.in_flight.as_mut() else {
            return false;
        };
        if in_flight.ticket != *ticket || in_flight.phase != expected {
            return false;
        }
        in_flight.phase = next;
        in_flight.ack_deadline = ack_deadline;
        self.changed.notify_all();
        true
    }

    pub(crate) fn ack_host_key(
        &self,
        content_label: &str,
        session_epoch: u64,
        route_sequence: u64,
    ) -> bool {
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
        let Some(in_flight) = session.host_key_route.in_flight.as_mut() else {
            return false;
        };
        if in_flight.ticket.route_sequence != route_sequence
            || in_flight.phase != HostKeyDeliveryPhase::DeliveredAwaitingAck
        {
            return false;
        }
        in_flight.phase = HostKeyDeliveryPhase::Accomplished;
        self.changed.notify_all();
        true
    }

    #[cfg(test)]
    pub(crate) fn finish_host_key_ack(
        &self,
        ticket: &HostKeyDeliveryTicket,
        now: Instant,
    ) -> HostKeyAckOutcome {
        let Ok(mut core) = self.core.lock() else {
            return HostKeyAckOutcome::Stale;
        };
        finish_host_key_ack_locked(&mut core, ticket, now, &self.changed)
    }

    pub(crate) fn wait_host_key_ack(&self, ticket: &HostKeyDeliveryTicket) -> HostKeyAckOutcome {
        let Ok(mut core) = self.core.lock() else {
            return HostKeyAckOutcome::Stale;
        };
        loop {
            let outcome =
                finish_host_key_ack_locked(&mut core, ticket, Instant::now(), &self.changed);
            if outcome != HostKeyAckOutcome::Pending {
                return outcome;
            }
            let Some(deadline) = core
                .session
                .as_ref()
                .and_then(|session| session.host_key_route.in_flight.as_ref())
                .and_then(|in_flight| in_flight.ack_deadline)
            else {
                return HostKeyAckOutcome::Stale;
            };
            let Ok((next, _)) = self
                .changed
                .wait_timeout(core, deadline.saturating_duration_since(Instant::now()))
            else {
                return HostKeyAckOutcome::Stale;
            };
            core = next;
        }
    }

    pub(crate) fn fail_host_key_delivery(
        &self,
        ticket: &HostKeyDeliveryTicket,
    ) -> Option<PanelSessionIdentity> {
        let mut core = self.core.lock().ok()?;
        let session = core.session.as_mut()?;
        if session.identity.session_epoch != ticket.session_epoch
            || session
                .host_key_route
                .in_flight
                .as_ref()
                .is_none_or(|in_flight| in_flight.ticket != *ticket)
        {
            return None;
        }
        if let Some(in_flight) = session.host_key_route.in_flight.as_mut() {
            in_flight.phase = HostKeyDeliveryPhase::Cancelled;
        }
        disarm_host_key_route(&mut session.host_key_route);
        self.changed.notify_all();
        Some(session.identity.clone())
    }

    pub(crate) fn mark_ready(
        &self,
        content_label: &str,
        session_epoch: u64,
        host_key_receiver_registered: bool,
        host_key_registration_violation: bool,
    ) -> bool {
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
        let host_keys_required = !session.identity.host_keys.is_empty();
        if host_key_registration_violation || host_keys_required != host_key_receiver_registered {
            return false;
        }
        if session.phase != PanelPhase::AwaitingReady {
            return session.phase == PanelPhase::Ready;
        }
        session.phase = PanelPhase::Ready;
        session.host_key_route.receiver_armed = host_keys_required;
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
        core.internal_blur_until = Instant::now().checked_add(INTERNAL_BLUR_GRACE);
        core.host_input_focus = None;
        core.hide_ticket = None;
        self.changed.notify_all();
        Some(session.identity)
    }

    pub(crate) fn teardown_plugin(&self, plugin_id: &str) -> Option<PanelSessionIdentity> {
        let mut core = self.core.lock().ok()?;
        if core
            .session
            .as_ref()
            .is_none_or(|session| session.identity.plugin_id != plugin_id)
        {
            return None;
        }
        let identity = core.session.take()?.identity;
        core.internal_blur_until = Instant::now().checked_add(INTERNAL_BLUR_GRACE);
        core.host_input_focus = None;
        core.hide_ticket = None;
        self.changed.notify_all();
        Some(identity)
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
        core.internal_blur_until = Instant::now().checked_add(INTERNAL_BLUR_GRACE);
        core.host_input_focus = None;
        core.hide_ticket = None;
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

    #[cfg(test)]
    fn set_host_key_sequences(
        &self,
        session_epoch: u64,
        next_expected_client_sequence: u64,
        next_route_sequence: u64,
    ) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(session) = core
            .session
            .as_mut()
            .filter(|session| session.identity.session_epoch == session_epoch)
        else {
            return false;
        };
        session.host_key_route.next_expected_client_sequence = next_expected_client_sequence;
        session.host_key_route.next_route_sequence = next_route_sequence;
        true
    }
}

#[cfg(test)]
pub(crate) fn content_ready(
    controller: &PluginPanelController,
    label: &str,
    session_epoch: u64,
) -> bool {
    controller.mark_ready(label, session_epoch, false, false)
}

pub(crate) fn content_ready_with_host_keys(
    controller: &PluginPanelController,
    label: &str,
    session_epoch: u64,
    host_key_receiver_registered: bool,
    host_key_registration_violation: bool,
) -> bool {
    controller.mark_ready(
        label,
        session_epoch,
        host_key_receiver_registered,
        host_key_registration_violation,
    )
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

pub(crate) fn teardown_plugin(
    app: &AppHandle,
    controller: &PluginPanelController,
    plugin_id: &str,
) -> Option<PanelSessionIdentity> {
    let identity = controller.teardown_plugin(plugin_id)?;
    destroy_content(app, &identity.content_label);
    Some(identity)
}

fn panel_bounds(main: &tauri::Window) -> Result<(LogicalPosition<f64>, LogicalSize<f64>), ()> {
    let size = main.inner_size().map_err(|_| ())?;
    let scale = main.scale_factor().map_err(|_| ())?;
    let width = (size.width as f64 / scale).max(1.0);
    let height = (size.height as f64 / scale).max(1.0);
    Ok(panel_logical_bounds(width, height))
}

fn panel_logical_bounds(width: f64, height: f64) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    let left = PANEL_HORIZONTAL_INSET.min((width - 1.0).max(0.0) / 2.0);
    let top = PANEL_TOP_OFFSET.min((height - 1.0).max(0.0));
    let bottom = PANEL_BOTTOM_INSET.min((height - top - 1.0).max(0.0));
    (
        LogicalPosition::new(left, top),
        LogicalSize::new(
            (width - left * 2.0).max(1.0),
            (height - top - bottom).max(1.0),
        ),
    )
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
    if register_content_focus_events(app, &content, Arc::clone(&controller), identity).is_err() {
        destroy_content(app, &identity.content_label);
        return Err(PublicPluginManagementError::Unavailable);
    }

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
        panel_bootstrap(identity.session_epoch, &identity.host_keys),
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

#[cfg(windows)]
pub(crate) fn register_main_focus_events(
    app: &AppHandle,
    main: &WebviewWindow,
    controller: Arc<PluginPanelController>,
    lifecycle: Arc<crate::lifecycle::LifecycleCoordinator>,
) -> Result<(), ()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let got_controller = Arc::clone(&controller);
    let lost_app = app.clone();
    main.with_webview(move |platform| {
        let got = FocusChangedEventHandler::create(Box::new(move |_, _| {
            let _ = got_controller.main_content_got_focus();
            Ok(())
        }));
        let lost = FocusChangedEventHandler::create(Box::new(move |_, _| {
            let transfers =
                lost_app.state::<Arc<crate::window_transfer::MainWindowTransferCoordinator>>();
            let expected_transfer_blur = transfers.consume_expected_main_webview_blur();
            let transient_focus_suppressed = lifecycle.transient_focus_loss_suppressed();
            if let Some(ticket) = controller
                .main_content_lost_focus(expected_transfer_blur || transient_focus_suppressed)
            {
                schedule_app_blur(lost_app.clone(), Arc::clone(&controller), ticket);
            }
            Ok(())
        }));
        let mut got_token = 0;
        let mut lost_token = 0;
        let native = platform.controller();
        let result = unsafe {
            native
                .add_GotFocus(&got, &mut got_token)
                .and_then(|_| native.add_LostFocus(&lost, &mut lost_token))
        }
        .map_err(|_| ());
        let _ = sender.send(result);
    })
    .map_err(|_| ())?;
    receiver
        .recv_timeout(CONTENT_READY_TIMEOUT)
        .map_err(|_| ())?
}

#[cfg(not(windows))]
pub(crate) fn register_main_focus_events(
    _app: &AppHandle,
    _main: &WebviewWindow,
    _controller: Arc<PluginPanelController>,
    _lifecycle: Arc<crate::lifecycle::LifecycleCoordinator>,
) -> Result<(), ()> {
    Ok(())
}

#[cfg(windows)]
fn register_content_focus_events(
    app: &AppHandle,
    content: &tauri::Webview,
    controller: Arc<PluginPanelController>,
    identity: &PanelSessionIdentity,
) -> Result<(), PublicPluginManagementError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let got_controller = Arc::clone(&controller);
    let got_label = identity.content_label.clone();
    let got_epoch = identity.session_epoch;
    let lost_controller = Arc::clone(&controller);
    let lost_label = identity.content_label.clone();
    let lost_epoch = identity.session_epoch;
    let lost_app = app.clone();
    content
        .with_webview(move |platform| {
            let got = FocusChangedEventHandler::create(Box::new(move |_, _| {
                let _ = got_controller.content_got_focus(&got_label, got_epoch);
                Ok(())
            }));
            let lost = FocusChangedEventHandler::create(Box::new(move |_, _| {
                if let Some(ticket) = lost_controller.content_lost_focus(&lost_label, lost_epoch) {
                    schedule_app_blur(lost_app.clone(), Arc::clone(&lost_controller), ticket);
                }
                Ok(())
            }));
            let mut got_token = 0;
            let mut lost_token = 0;
            let native = platform.controller();
            let result = unsafe {
                native
                    .add_GotFocus(&got, &mut got_token)
                    .and_then(|_| native.add_LostFocus(&lost, &mut lost_token))
            }
            .map_err(|_| ());
            let _ = sender.send(result);
        })
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    receiver
        .recv_timeout(CONTENT_READY_TIMEOUT)
        .map_err(|_| PublicPluginManagementError::Unavailable)?
        .map_err(|_| PublicPluginManagementError::Unavailable)
}

#[cfg(not(windows))]
fn register_content_focus_events(
    _app: &AppHandle,
    _content: &tauri::Webview,
    _controller: Arc<PluginPanelController>,
    _identity: &PanelSessionIdentity,
) -> Result<(), PublicPluginManagementError> {
    Err(PublicPluginManagementError::Unavailable)
}

#[cfg(windows)]
fn schedule_app_blur(
    app: AppHandle,
    controller: Arc<PluginPanelController>,
    ticket: AppFocusLossTicket,
) {
    std::thread::spawn(move || {
        std::thread::sleep(CONTENT_BLUR_RECHECK_DELAY);
        let dispatch_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            if !controller.confirm_app_blur(&ticket) {
                return;
            }
            let Some(window) = dispatch_app.get_webview_window("main") else {
                return;
            };
            let registries = dispatch_app.state::<crate::result_registry::ResultRegistries>();
            let _ = crate::commands::clear_and_hide_reason(
                registries.main(),
                &window,
                crate::commands::HideReason::Blur,
            );
        });
    });
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

pub(crate) fn start_host_key_pump(app: AppHandle, controller: Arc<PluginPanelController>) {
    std::thread::spawn(move || {
        while let Some(ticket) = controller.claim_next_host_key() {
            let delivered = (|| {
                let content = app.get_webview(&ticket.content_label).ok_or(())?;
                content.set_focus().map_err(|_| ())?;
                if !controller.mark_host_key_native_focused(&ticket) {
                    return Err(());
                }
                let payload = serde_json::to_string(&serde_json::json!({
                    "key": ticket.key,
                    "ctrlKey": ticket.ctrl_key,
                    "metaKey": ticket.meta_key,
                    "shiftKey": ticket.shift_key,
                    "altKey": ticket.alt_key,
                    "sessionEpoch": ticket.session_epoch.to_string(),
                    "routeSequence": ticket.route_sequence.to_string(),
                }))
                .map_err(|_| ())?;
                content
                    .eval(format!(
                        "window.__UIPILOT_PLUGIN_PANEL_HOST_KEY__({payload});"
                    ))
                    .map_err(|_| ())?;
                let deadline = Instant::now().checked_add(HOST_KEY_ACK_TIMEOUT).ok_or(())?;
                controller
                    .mark_host_key_delivered(&ticket, deadline)
                    .then_some(())
                    .ok_or(())
            })();
            if delivered.is_err() {
                if controller.fail_host_key_delivery(&ticket).is_some() {
                    schedule_host_key_terminal_hide(app.clone());
                }
                return;
            }
            match controller.wait_host_key_ack(&ticket) {
                HostKeyAckOutcome::Acknowledged => {}
                HostKeyAckOutcome::TimedOut => {
                    if controller.fail_host_key_delivery(&ticket).is_some() {
                        schedule_host_key_terminal_hide(app.clone());
                    }
                    return;
                }
                HostKeyAckOutcome::Pending | HostKeyAckOutcome::Stale => return,
            }
        }
    });
}

fn schedule_host_key_terminal_hide(app: AppHandle) {
    let dispatch_app = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = dispatch_app.get_webview_window("main") else {
            return;
        };
        let registries = dispatch_app.state::<crate::result_registry::ResultRegistries>();
        let _ = crate::commands::clear_and_hide_reason(
            registries.main(),
            &window,
            crate::commands::HideReason::ExplicitReturn,
        );
    });
}

pub(crate) fn commit_panel_hide(
    app: &AppHandle,
    controller: &PluginPanelController,
    identity: PanelHideTicketIdentity,
) -> Result<bool, ()> {
    if controller.claim_hide_commit(identity).is_none() {
        return Ok(false);
    }
    schedule_committed_panel_hide(app.clone()).map(|()| true)
}

pub(crate) fn schedule_panel_hide_fallback(
    app: AppHandle,
    controller: Arc<PluginPanelController>,
    identity: PanelHideTicketIdentity,
    expected_phase: PanelHideTicketPhase,
    delay: Duration,
) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        if controller
            .claim_hide_fallback(identity, expected_phase)
            .is_some()
        {
            let _ = schedule_committed_panel_hide(app);
        }
    });
}

fn schedule_committed_panel_hide(app: AppHandle) -> Result<(), ()> {
    let dispatch_app = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = dispatch_app.get_webview_window("main") else {
            return;
        };
        let registries = dispatch_app.state::<crate::result_registry::ResultRegistries>();
        let _ = crate::commands::clear_and_hide_reason(
            registries.main(),
            &window,
            crate::commands::HideReason::ExplicitReturn,
        );
    })
    .map_err(|_| ())
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
            host_keys: Vec::new(),
        }
    }

    fn focus_deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    fn assert_focus_advanced(result: Result<HostInputFocusAdvance, PanelSettlementError>) {
        assert_eq!(result, Ok(HostInputFocusAdvance::Advanced));
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
    }

    #[test]
    fn panel_focus_loss_hides_only_without_a_new_internal_focus_owner() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.content_got_focus(&identity.content_label, identity.session_epoch,));
        let stale = controller
            .content_lost_focus(&identity.content_label, identity.session_epoch)
            .unwrap();
        assert!(controller.main_content_got_focus());
        assert!(!controller.confirm_app_blur(&stale));

        assert!(controller.content_got_focus(&identity.content_label, identity.session_epoch,));
        let current = controller
            .content_lost_focus(&identity.content_label, identity.session_epoch)
            .unwrap();
        assert!(controller.confirm_app_blur(&current));
    }

    #[test]
    fn live_panel_and_recent_teardown_consume_only_internal_main_blurs() {
        let controller = PluginPanelController::default();
        let now = Instant::now();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.consume_internal_main_blur(now));
        assert!(controller
            .teardown_session(Some(identity.session_epoch))
            .is_some());
        assert!(controller.consume_internal_main_blur(now));
        assert!(!controller.consume_internal_main_blur(now));
        assert!(!controller
            .consume_internal_main_blur(now + INTERNAL_BLUR_GRACE + Duration::from_millis(1)));
    }

    #[test]
    fn main_content_focus_suppresses_spurious_window_blur_without_a_panel() {
        let controller = PluginPanelController::default();
        let now = Instant::now();

        assert!(!controller.consume_internal_main_blur(now));
        assert!(controller.main_content_got_focus());
        assert!(controller.consume_internal_main_blur(now));
        assert!(controller.consume_internal_main_blur(now));
        let ticket = controller.main_content_lost_focus(false).unwrap();
        assert!(!controller.consume_internal_main_blur(now));
        assert!(controller.confirm_app_blur(&ticket));
    }

    #[test]
    fn main_focus_loss_with_live_panel_schedules_hide_when_panel_never_owned_focus() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.main_content_got_focus());
        let ticket = controller.main_content_lost_focus(false).unwrap();

        assert_eq!(ticket.session_epoch, Some(identity.session_epoch));
        assert!(controller.consume_internal_main_blur(Instant::now()));
        assert!(controller.confirm_app_blur(&ticket));
    }

    #[test]
    fn main_webview_blur_honors_transient_focus_suppression_before_scheduling_hide() {
        let source = include_str!("plugin_panel.rs").replace("\r\n", "\n");
        let callback = source
            .split("pub(crate) fn register_main_focus_events(")
            .nth(1)
            .and_then(|tail| tail.split("#[cfg(not(windows))]").next())
            .expect("Windows main focus registration is missing");
        let suppression = callback
            .find("lifecycle.transient_focus_loss_suppressed()")
            .expect("transient focus suppression check is missing");
        let ticket = callback
            .find("main_content_lost_focus(expected_transfer_blur || transient_focus_suppressed)")
            .expect("main content blur must receive the combined suppression decision");
        let schedule = callback
            .find("schedule_app_blur(")
            .expect("main content blur scheduling is missing");

        assert!(suppression < ticket && ticket < schedule);
    }

    #[test]
    fn panel_focus_invalidates_main_focus_loss_ticket() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.main_content_got_focus());
        let ticket = controller.main_content_lost_focus(false).unwrap();
        assert!(controller.content_got_focus(&identity.content_label, identity.session_epoch));

        assert!(!controller.confirm_app_blur(&ticket));
    }

    #[test]
    fn expected_window_transfer_blur_does_not_create_app_hide_ticket() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.main_content_got_focus());
        assert!(controller.main_content_lost_focus(true).is_none());
        assert_eq!(controller.live_identity(), Some(identity));
    }

    #[test]
    fn host_hide_invalidates_pending_panel_focus_loss() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.content_got_focus(&identity.content_label, identity.session_epoch));
        let ticket = controller
            .content_lost_focus(&identity.content_label, identity.session_epoch)
            .unwrap();
        controller.host_hidden();

        assert!(!controller.confirm_app_blur(&ticket));
        assert!(controller.live_identity().is_none());
    }

    #[test]
    fn host_input_focus_current_request_acks_only_the_confirmed_main_revision() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();
        assert!(controller.content_got_focus(&session.content_label, session.session_epoch));

        let request = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert_focus_advanced(controller.claim_host_input_focus(request));
        assert_focus_advanced(controller.confirm_native_host_input_focus(request, Instant::now()));
        assert!(controller.ack_host_input_focus(request, true).unwrap());
        assert_eq!(
            controller.wait_host_input_focus(request),
            Ok(HostInputFocusOutcome::Focused)
        );
        assert_eq!(controller.live_identity(), Some(session));
    }

    #[test]
    fn host_input_focus_latest_request_supersedes_the_previous_waiter() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();

        let first = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        let second = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();

        assert!(second.focus_request_id > first.focus_request_id);
        assert_eq!(
            controller.wait_host_input_focus(first),
            Ok(HostInputFocusOutcome::Noop)
        );
        assert_eq!(
            controller.claim_host_input_focus(first),
            Ok(HostInputFocusAdvance::Noop)
        );
        assert_focus_advanced(controller.claim_host_input_focus(second));
    }

    #[test]
    fn host_input_focus_current_timeout_fails_but_stale_timeout_is_a_noop() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();
        let current = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                Instant::now(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            controller.wait_host_input_focus(current),
            Ok(HostInputFocusOutcome::Failed)
        );

        let stale = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                Instant::now(),
            )
            .unwrap()
            .unwrap();
        let replacement = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            controller.wait_host_input_focus(stale),
            Ok(HostInputFocusOutcome::Noop)
        );
        assert_focus_advanced(controller.claim_host_input_focus(replacement));
    }

    #[test]
    fn host_input_focus_terminal_ack_survives_a_newer_prepare() {
        for (focused, lose_focus, expired, expected) in [
            (false, false, false, HostInputFocusOutcome::Failed),
            (true, true, false, HostInputFocusOutcome::Failed),
            (true, false, true, HostInputFocusOutcome::Failed),
            (true, false, false, HostInputFocusOutcome::Focused),
        ] {
            let controller = PluginPanelController::default();
            let session = controller.open_session(owner("a")).unwrap();
            assert!(controller.content_got_focus(&session.content_label, session.session_epoch));
            let deadline = focus_deadline();
            let first = controller
                .prepare_host_input_focus(&session.content_label, session.session_epoch, deadline)
                .unwrap()
                .unwrap();
            assert_focus_advanced(controller.claim_host_input_focus(first));
            assert_focus_advanced(
                controller.confirm_native_host_input_focus(first, Instant::now()),
            );
            let blur = lose_focus.then(|| controller.main_content_lost_focus(false).unwrap());
            let ack_time = if expired { deadline } else { Instant::now() };
            assert!(controller
                .ack_host_input_focus_at(first, focused, ack_time)
                .unwrap());

            let second = controller
                .prepare_host_input_focus(
                    &session.content_label,
                    session.session_epoch,
                    focus_deadline(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(controller.wait_host_input_focus(first), Ok(expected));
            assert_focus_advanced(controller.claim_host_input_focus(second));
            if let Some(blur) = blur {
                assert!(controller.confirm_app_blur(&blur));
            }
        }
    }

    #[test]
    fn host_input_focus_terminal_ack_survives_session_teardown() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();
        let request = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert_focus_advanced(controller.claim_host_input_focus(request));
        assert_focus_advanced(controller.confirm_native_host_input_focus(request, Instant::now()));
        assert!(controller.ack_host_input_focus(request, false).unwrap());
        assert!(controller
            .teardown_session(Some(session.session_epoch))
            .is_some());
        assert_eq!(
            controller.wait_host_input_focus(request),
            Ok(HostInputFocusOutcome::Failed)
        );
    }

    #[test]
    fn host_input_focus_stale_caller_and_teardown_before_claim_are_noops() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();

        assert!(controller
            .prepare_host_input_focus(
                "plugin-panel-content-forged-s0000000000000001",
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .is_none());
        assert!(controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch + 1,
                focus_deadline(),
            )
            .unwrap()
            .is_none());

        let request = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert!(controller
            .teardown_session(Some(session.session_epoch))
            .is_some());
        assert_eq!(
            controller.claim_host_input_focus(request),
            Ok(HostInputFocusAdvance::Noop)
        );
        assert_eq!(
            controller.wait_host_input_focus(request),
            Ok(HostInputFocusOutcome::Noop)
        );
    }

    #[test]
    fn host_input_focus_claim_then_teardown_makes_late_confirm_and_ack_stale() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();
        assert!(controller.main_content_got_focus());
        let blur = controller.main_content_lost_focus(false).unwrap();
        let request = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert_focus_advanced(controller.claim_host_input_focus(request));

        assert!(controller
            .teardown_session(Some(session.session_epoch))
            .is_some());
        assert_eq!(
            controller.confirm_native_host_input_focus(request, Instant::now()),
            Ok(HostInputFocusAdvance::Noop)
        );
        assert!(!controller.ack_host_input_focus(request, true).unwrap());
        assert_eq!(
            controller.wait_host_input_focus(request),
            Ok(HostInputFocusOutcome::Noop)
        );
        assert!(controller.live_identity().is_none());
        assert!(!controller.confirm_app_blur(&blur));
    }

    #[test]
    fn host_input_focus_old_ack_and_timeout_cannot_touch_replacement_session() {
        let controller = PluginPanelController::default();
        let first = controller.open_session(owner("a")).unwrap();
        let first_deadline = focus_deadline();
        let request = controller
            .prepare_host_input_focus(&first.content_label, first.session_epoch, first_deadline)
            .unwrap()
            .unwrap();
        assert_focus_advanced(controller.claim_host_input_focus(request));
        assert_focus_advanced(controller.confirm_native_host_input_focus(request, Instant::now()));
        assert!(controller
            .teardown_session(Some(first.session_epoch))
            .is_some());
        let second = controller.open_session(owner("b")).unwrap();
        let replacement = controller
            .prepare_host_input_focus(
                &second.content_label,
                second.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();

        assert!(!controller
            .ack_host_input_focus_at(request, true, first_deadline)
            .unwrap());
        assert_eq!(
            controller.wait_host_input_focus(request),
            Ok(HostInputFocusOutcome::Noop)
        );
        assert_focus_advanced(controller.claim_host_input_focus(replacement));
        assert_focus_advanced(
            controller.confirm_native_host_input_focus(replacement, Instant::now()),
        );
        assert!(controller.ack_host_input_focus(replacement, true).unwrap());
        assert_eq!(
            controller.wait_host_input_focus(replacement),
            Ok(HostInputFocusOutcome::Focused)
        );
        assert_eq!(controller.live_identity(), Some(second));
    }

    #[test]
    fn host_input_focus_true_ack_after_real_blur_cannot_cancel_hide() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();
        assert!(controller.content_got_focus(&session.content_label, session.session_epoch));
        let request = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert_focus_advanced(controller.claim_host_input_focus(request));
        assert_focus_advanced(controller.confirm_native_host_input_focus(request, Instant::now()));

        let blur = controller.main_content_lost_focus(false).unwrap();
        assert!(controller.ack_host_input_focus(request, true).unwrap());
        assert_eq!(
            controller.wait_host_input_focus(request),
            Ok(HostInputFocusOutcome::Failed)
        );
        assert!(controller.confirm_app_blur(&blur));
    }

    #[test]
    fn host_input_focus_native_failure_preserves_existing_blur_ticket() {
        let controller = PluginPanelController::default();
        let session = controller.open_session(owner("a")).unwrap();
        assert!(controller.main_content_got_focus());
        let blur = controller.main_content_lost_focus(false).unwrap();

        let request = controller
            .prepare_host_input_focus(
                &session.content_label,
                session.session_epoch,
                focus_deadline(),
            )
            .unwrap()
            .unwrap();
        assert_focus_advanced(controller.claim_host_input_focus(request));
        assert!(controller.fail_native_host_input_focus(request).unwrap());

        assert!(controller.confirm_app_blur(&blur));
    }

    #[test]
    fn plugin_mutation_teardown_only_removes_the_matching_live_session() {
        let controller = PluginPanelController::default();
        let identity = controller.open_session(owner("a")).unwrap();

        assert!(controller.teardown_plugin("com.uipilot.other").is_none());
        assert_eq!(controller.live_identity(), Some(identity.clone()));
        assert_eq!(
            controller.teardown_plugin("com.uipilot.demo-panel"),
            Some(identity)
        );
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

    fn host_key_session(controller: &PluginPanelController) -> PanelSessionIdentity {
        let mut panel_owner = owner("host-key");
        panel_owner.host_keys = vec![
            PanelHostKeyDeclaration::ArrowDown,
            PanelHostKeyDeclaration::ArrowUp,
        ];
        let identity = controller.open_session(panel_owner).unwrap();
        assert!(controller.mark_ready(
            &identity.content_label,
            identity.session_epoch,
            true,
            false,
        ));
        identity
    }

    fn host_key_input(client_sequence: u64) -> HostKeyEnqueueInput {
        HostKeyEnqueueInput {
            client_sequence,
            declaration: PanelHostKeyDeclaration::ArrowDown,
            key: PluginPanelHostKey::ArrowDown,
            ctrl_key: false,
            meta_key: false,
            shift_key: false,
            alt_key: false,
        }
    }

    #[test]
    fn host_key_ready_gate_rejects_missing_receiver_and_sticky_empty_registration() {
        let controller = PluginPanelController::default();
        let mut required_owner = owner("required");
        required_owner.host_keys = vec![PanelHostKeyDeclaration::ArrowDown];
        let required = controller.open_session(required_owner).unwrap();
        assert!(!controller.mark_ready(
            &required.content_label,
            required.session_epoch,
            false,
            false,
        ));

        let empty = controller.open_session(owner("empty")).unwrap();
        assert!(!controller.mark_ready(&empty.content_label, empty.session_epoch, false, true,));
    }

    #[test]
    fn host_key_queue_is_expected_only_serial_and_queue_full_consumes_sequences() {
        let controller = PluginPanelController::default();
        let identity = host_key_session(&controller);
        let first = controller
            .enqueue_host_key(identity.session_epoch, host_key_input(1))
            .unwrap();
        assert_eq!(
            first.outcome,
            HostKeyEnqueueOutcome::Enqueued { route_sequence: 1 }
        );
        assert!(first.start_pump);
        let first_ticket = controller.claim_next_host_key().unwrap();
        assert!(controller.claim_next_host_key().is_none());
        assert!(controller.mark_host_key_native_focused(&first_ticket));
        assert!(controller
            .mark_host_key_delivered(&first_ticket, Instant::now() + HOST_KEY_ACK_TIMEOUT,));

        for sequence in 2..=9 {
            assert!(matches!(
                controller
                    .enqueue_host_key(identity.session_epoch, host_key_input(sequence))
                    .unwrap()
                    .outcome,
                HostKeyEnqueueOutcome::Enqueued { .. }
            ));
        }
        for sequence in [10, 11] {
            assert_eq!(
                controller
                    .enqueue_host_key(identity.session_epoch, host_key_input(sequence))
                    .unwrap()
                    .outcome,
                HostKeyEnqueueOutcome::DroppedQueueFull,
            );
        }
        assert_eq!(
            controller
                .enqueue_host_key(identity.session_epoch, host_key_input(9))
                .unwrap()
                .outcome,
            HostKeyEnqueueOutcome::Noop,
        );
        assert!(controller.ack_host_key(
            &identity.content_label,
            identity.session_epoch,
            first_ticket.route_sequence,
        ));
        assert_eq!(
            controller.finish_host_key_ack(&first_ticket, Instant::now()),
            HostKeyAckOutcome::Acknowledged,
        );
        assert!(controller.claim_next_host_key().is_some());
        assert!(matches!(
            controller
                .enqueue_host_key(identity.session_epoch, host_key_input(12))
                .unwrap()
                .outcome,
            HostKeyEnqueueOutcome::Enqueued { .. }
        ));

        let reordered = controller
            .enqueue_host_key(identity.session_epoch, host_key_input(14))
            .unwrap();
        assert_eq!(reordered.outcome, HostKeyEnqueueOutcome::ProtocolViolation);
        assert!(reordered.terminate_session);
    }

    #[test]
    fn host_key_ack_timeout_disarms_without_overlapping_the_next_delivery() {
        let controller = PluginPanelController::default();
        let identity = host_key_session(&controller);
        controller
            .enqueue_host_key(identity.session_epoch, host_key_input(1))
            .unwrap();
        controller
            .enqueue_host_key(identity.session_epoch, host_key_input(2))
            .unwrap();
        let ticket = controller.claim_next_host_key().unwrap();
        assert!(controller.mark_host_key_native_focused(&ticket));
        assert!(controller.mark_host_key_delivered(&ticket, Instant::now()));
        assert_eq!(
            controller.finish_host_key_ack(&ticket, Instant::now()),
            HostKeyAckOutcome::TimedOut,
        );
        assert!(controller.claim_next_host_key().is_none());
        assert_eq!(
            controller
                .enqueue_host_key(identity.session_epoch, host_key_input(3))
                .unwrap()
                .outcome,
            HostKeyEnqueueOutcome::Noop,
        );
    }

    #[test]
    fn host_key_counters_and_stale_sessions_fail_closed_without_wrap() {
        let controller = PluginPanelController::default();
        let first = host_key_session(&controller);
        assert!(controller.set_host_key_sequences(first.session_epoch, u64::MAX, 1));
        let exhausted_client = controller
            .enqueue_host_key(first.session_epoch, host_key_input(u64::MAX))
            .unwrap();
        assert_eq!(
            exhausted_client.outcome,
            HostKeyEnqueueOutcome::ProtocolViolation
        );
        assert!(exhausted_client.terminate_session);

        let second = host_key_session(&controller);
        assert_eq!(
            controller
                .enqueue_host_key(first.session_epoch, host_key_input(1))
                .unwrap()
                .outcome,
            HostKeyEnqueueOutcome::Noop,
        );
        assert!(controller.set_host_key_sequences(second.session_epoch, 1, u64::MAX));
        let exhausted_route = controller
            .enqueue_host_key(second.session_epoch, host_key_input(1))
            .unwrap();
        assert_eq!(
            exhausted_route.outcome,
            HostKeyEnqueueOutcome::ProtocolViolation
        );
        assert!(exhausted_route.terminate_session);
    }

    #[test]
    fn panel_bootstrap_registers_one_host_key_handler_and_acks_after_settlement() {
        let bootstrap = PUBLIC_PANEL_BOOTSTRAP_TEMPLATE.replace("\r\n", "\n");
        for required in [
            "onHostKey(next)",
            "hostKeyRegistrationViolation = true",
            "__UIPILOT_PLUGIN_PANEL_HOST_KEY__",
            "await hostKeyHandler(deepFreeze(event))",
            "finally",
            "plugin_panel_host_key_ack",
        ] {
            assert!(
                bootstrap.contains(required),
                "missing bootstrap fragment: {required}"
            );
        }
    }

    #[test]
    fn panel_hide_ticket_observed_and_commit_orders_hide_exactly_once() {
        let controller = PluginPanelController::default();
        let observed_session = content_ready_session(&controller);
        let observed = controller
            .admit_hide(
                &observed_session.content_label,
                observed_session.session_epoch,
            )
            .unwrap()
            .unwrap();
        assert!(controller.observe_hide(observed));
        assert!(controller.claim_hide_commit(observed).is_some());
        assert!(controller.claim_hide_commit(observed).is_none());
        assert!(!controller.observe_hide(observed));

        let commit_first_session = content_ready_session(&controller);
        let commit_first = controller
            .admit_hide(
                &commit_first_session.content_label,
                commit_first_session.session_epoch,
            )
            .unwrap()
            .unwrap();
        assert!(controller.claim_hide_commit(commit_first).is_some());
        assert!(!controller.observe_hide(commit_first));
        assert!(controller.claim_hide_commit(commit_first).is_none());
    }

    #[test]
    fn panel_hide_fallback_is_phase_specific_and_stale_after_new_session() {
        let controller = PluginPanelController::default();
        let admitted_session = content_ready_session(&controller);
        let admitted = controller
            .admit_hide(
                &admitted_session.content_label,
                admitted_session.session_epoch,
            )
            .unwrap()
            .unwrap();
        assert!(controller
            .claim_hide_fallback(admitted, PanelHideTicketPhase::Observed)
            .is_none());
        assert!(controller
            .claim_hide_fallback(admitted, PanelHideTicketPhase::Admitted)
            .is_some());

        let observed_session = content_ready_session(&controller);
        let observed = controller
            .admit_hide(
                &observed_session.content_label,
                observed_session.session_epoch,
            )
            .unwrap()
            .unwrap();
        assert!(controller.observe_hide(observed));
        assert!(controller
            .claim_hide_fallback(observed, PanelHideTicketPhase::Admitted)
            .is_none());
        assert!(controller
            .claim_hide_fallback(observed, PanelHideTicketPhase::Observed)
            .is_some());

        let stale_session = content_ready_session(&controller);
        let stale = controller
            .admit_hide(&stale_session.content_label, stale_session.session_epoch)
            .unwrap()
            .unwrap();
        let replacement = content_ready_session(&controller);
        assert_ne!(stale.session_epoch, replacement.session_epoch);
        assert!(controller
            .claim_hide_fallback(stale, PanelHideTicketPhase::Admitted)
            .is_none());
    }

    #[test]
    fn panel_hide_ticket_exhaustion_fails_closed_without_reuse() {
        let controller = PluginPanelController::default();
        let session = content_ready_session(&controller);
        controller.set_next_hide_ticket_id(u64::MAX);
        assert_eq!(
            controller.admit_hide(&session.content_label, session.session_epoch),
            Err(PanelSettlementError::Unavailable),
        );
    }

    #[test]
    fn panel_bootstrap_hide_and_escape_follow_the_frozen_ordering() {
        let bootstrap = PUBLIC_PANEL_BOOTSTRAP_TEMPLATE.replace("\r\n", "\n");
        let resolve = bootstrap
            .find("resolve();")
            .expect("hide Promise resolve is missing");
        let observed = bootstrap
            .find("plugin_panel_request_hide_admit_observed")
            .expect("observed invoke is missing");
        let commit = bootstrap
            .find("setTimeout(() =>")
            .expect("next-macrotask commit is missing");
        assert!(resolve < observed && observed < commit);
        for required in [
            "plugin_panel_request_hide_admit",
            "plugin_panel_request_hide_commit",
            "addEventListener('keydown'",
            "queueMicrotask(() =>",
            "dialog[open]",
            "event.defaultPrevented",
            "capture: true",
        ] {
            assert!(
                bootstrap.contains(required),
                "missing hide fragment: {required}"
            );
        }
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
    fn panel_bootstrap_exposes_host_keys_focus_update_and_storage() {
        let bootstrap = panel_bootstrap(42, &[PanelHostKeyDeclaration::ArrowDown]);
        let api_body = bootstrap
            .split("const api = deepFreeze({\n")
            .nth(1)
            .and_then(|tail| {
                tail.split("\n  });\n  Object.defineProperty(window, 'uipilotPluginPanel'")
                    .next()
            })
            .expect("public panel API object is missing");
        let public_members = api_body
            .lines()
            .filter_map(|line| {
                let member = line.strip_prefix("    ")?;
                (!member.starts_with(' ') && !member.starts_with('}') && !member.is_empty())
                    .then_some(member)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            public_members,
            vec![
                "onUpdate(next) {",
                "onHostKey(next) {",
                "async focusHostInput() {",
                "requestHide() { return requestPanelHide(); },",
                "get storage() { return storageSession ? storageSession.storage : expiredStorage; },",
            ]
        );
        for required in [
            "uipilotPluginPanel",
            "onUpdate(next)",
            "onHostKey(next)",
            "async focusHostInput()",
            "plugin_panel_focus_host_input",
            "sessionEpoch: '42'",
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
        assert!(production.contains("fn panel_bootstrap("));
        assert!(production.contains("host_keys: &[PanelHostKeyDeclaration]"));
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
            "register_content_focus_events(",
            ".add_GotFocus(",
            ".add_LostFocus(",
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

    #[test]
    fn panel_bounds_match_the_fixed_launcher_result_slot() {
        let (position, size) = panel_logical_bounds(720.0, 420.0);
        assert_eq!((position.x, position.y), (12.0, 64.0));
        assert_eq!((size.width, size.height), (696.0, 320.0));

        let (position, size) = panel_logical_bounds(20.0, 20.0);
        assert!(position.x >= 0.0 && position.y >= 0.0);
        assert!(size.width >= 1.0 && size.height >= 1.0);
    }
}
