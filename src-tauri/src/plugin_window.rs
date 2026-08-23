use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use serde::Serialize;
use serde_json::Value;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::{
    lifecycle,
    public_plugins::{
        inert_url, prepare_windows_webview, verify_windows_webview_muted, PluginInvocationTheme,
        PluginTimerState, PublicPluginManagementError, PublicPluginService, TimerKey,
        WebViewGuardOwner,
    },
    settings::SettingsStore,
    window_transfer::{MainWindowSnapshot, MainWindowTransferCoordinator, TransferTarget},
};

const CONTENT_READY_TIMEOUT: Duration = Duration::from_secs(5);
const CONTENT_ACK_TIMEOUT: Duration = Duration::from_secs(5);
const PLUGIN_FOCUS_CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);
const PLUGIN_BLUR_RECHECK_DELAY: Duration = Duration::from_millis(50);
const PLUGIN_WINDOW_WIDTH: f64 = 520.0;
const PLUGIN_WINDOW_HEIGHT: f64 = 360.0;
const PLUGIN_SHELL_HEIGHT: f64 = 44.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginWindowOwner {
    pub(crate) ui_intent_epoch: u64,
    pub(crate) submission_token: String,
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) admission_epoch: u64,
    pub(crate) request_id: String,
    pub(crate) control_value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginWindowPhase {
    AwaitingReady,
    AwaitingAck,
    AwaitingFocus,
    AwaitingCommit,
    Visible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginWindowTransaction {
    pub(crate) owner: PluginWindowOwner,
    pub(crate) shell_label: String,
    pub(crate) content_label: String,
    pub(crate) instance_number: u8,
    pub(crate) timer_session_generation: u64,
}

#[derive(Default)]
pub(crate) struct PluginWindowController {
    core: Mutex<ControllerCore>,
    changed: Condvar,
}

#[derive(Default)]
struct ControllerCore {
    windows: HashMap<String, WindowState>,
    timer_session_generations: HashMap<String, u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TimerSessionPhase {
    Prepared,
    Active,
    Closing,
    Revoked,
}

struct TimerSessionState {
    generation: u64,
    phase: TimerSessionPhase,
    in_flight: usize,
}

struct WindowState {
    generation: u64,
    pinned: bool,
    focused: bool,
    phase: PluginWindowPhase,
    owner: PluginWindowOwner,
    timer_session: TimerSessionState,
}

pub(crate) struct TimerCallLease {
    controller: Arc<PluginWindowController>,
    owner: PluginWindowOwner,
    session_generation: u64,
}

impl TimerCallLease {
    pub(crate) fn owner(&self) -> &PluginWindowOwner {
        &self.owner
    }
}

impl Drop for TimerCallLease {
    fn drop(&mut self) {
        self.controller
            .finish_timer_call(&self.owner.plugin_id, self.session_generation);
    }
}

impl PluginWindowController {
    pub(crate) fn submit(&self, owner: PluginWindowOwner) -> Option<PluginWindowTransaction> {
        let shell_label = plugin_shell_label(&owner.plugin_id)?;
        let content_label = plugin_content_label(&owner.plugin_id)?;
        let mut core = self.core.lock().ok()?;
        if let Some(window) = core.windows.get_mut(&owner.plugin_id) {
            window.timer_session.phase = TimerSessionPhase::Closing;
        }
        while core
            .windows
            .get(&owner.plugin_id)
            .is_some_and(|window| window.timer_session.in_flight > 0)
        {
            core = self.changed.wait(core).ok()?;
        }
        let existing = core
            .windows
            .get(&owner.plugin_id)
            .filter(|window| window.generation == owner.plugin_generation);
        let pinned = existing.is_some_and(|window| window.pinned);
        let phase = existing
            .filter(|window| window.phase != PluginWindowPhase::AwaitingReady)
            .map(|_| PluginWindowPhase::AwaitingAck)
            .unwrap_or(PluginWindowPhase::AwaitingReady);
        let timer_session_generation = core
            .timer_session_generations
            .get(&owner.plugin_id)
            .copied()
            .unwrap_or(0)
            .checked_add(1)?;
        core.timer_session_generations
            .insert(owner.plugin_id.clone(), timer_session_generation);
        core.windows.insert(
            owner.plugin_id.clone(),
            WindowState {
                generation: owner.plugin_generation,
                pinned,
                focused: false,
                phase,
                owner: owner.clone(),
                timer_session: TimerSessionState {
                    generation: timer_session_generation,
                    phase: TimerSessionPhase::Prepared,
                    in_flight: 0,
                },
            },
        );
        self.changed.notify_all();
        Some(PluginWindowTransaction {
            owner,
            shell_label,
            content_label,
            instance_number: 1,
            timer_session_generation,
        })
    }

    fn active_timer_session(&self, key: &TimerKey) -> Option<(String, u64)> {
        let core = self.core.lock().ok()?;
        let window = core.windows.get(&key.plugin_id)?;
        (window.owner.plugin_generation == key.plugin_generation
            && window.owner.activation_id == key.activation_id
            && window.phase == PluginWindowPhase::Visible
            && window.timer_session.phase == TimerSessionPhase::Active)
            .then(|| {
                (
                    plugin_content_label(&key.plugin_id)
                        .expect("validated plugin id has a content label"),
                    window.timer_session.generation,
                )
            })
    }

    pub(crate) fn activate_timer_session(&self, owner: &PluginWindowOwner) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(&owner.plugin_id) else {
            return false;
        };
        if window.owner != *owner
            || window.phase != PluginWindowPhase::Visible
            || window.timer_session.phase != TimerSessionPhase::Prepared
        {
            return false;
        }
        window.timer_session.phase = TimerSessionPhase::Active;
        self.changed.notify_all();
        true
    }

    pub(crate) fn begin_timer_call(
        self: &Arc<Self>,
        content_label: &str,
        session_generation: u64,
        mutable: bool,
    ) -> Result<TimerCallLease, crate::public_plugins::TimerError> {
        let plugin_id = plugin_id_from_content_label(content_label)
            .ok_or(crate::public_plugins::TimerError::InvalidCaller)?;
        let mut core = self
            .core
            .lock()
            .map_err(|_| crate::public_plugins::TimerError::TimerUnavailable)?;
        let window = core
            .windows
            .get_mut(&plugin_id)
            .ok_or(crate::public_plugins::TimerError::ExpiredWindowSessionError)?;
        let allowed = window.timer_session.generation == session_generation
            && match window.timer_session.phase {
                TimerSessionPhase::Prepared => !mutable,
                TimerSessionPhase::Active => true,
                TimerSessionPhase::Closing | TimerSessionPhase::Revoked => false,
            };
        if !allowed {
            return Err(crate::public_plugins::TimerError::ExpiredWindowSessionError);
        }
        window.timer_session.in_flight = window
            .timer_session
            .in_flight
            .checked_add(1)
            .ok_or(crate::public_plugins::TimerError::TimerUnavailable)?;
        Ok(TimerCallLease {
            controller: Arc::clone(self),
            owner: window.owner.clone(),
            session_generation,
        })
    }

    pub(crate) fn begin_timer_session_close(&self, plugin_id: &str) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        window.timer_session.phase = TimerSessionPhase::Closing;
        while core
            .windows
            .get(plugin_id)
            .is_some_and(|window| window.timer_session.in_flight > 0)
        {
            let Ok(next) = self.changed.wait(core) else {
                return false;
            };
            core = next;
        }
        true
    }

    pub(crate) fn finish_timer_session_hide(&self, plugin_id: &str, hidden: bool) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        if window.timer_session.phase != TimerSessionPhase::Closing {
            return false;
        }
        window.timer_session.phase = TimerSessionPhase::Revoked;
        if hidden {
            window.focused = false;
        }
        self.changed.notify_all();
        true
    }

    pub(crate) fn revoke_timer_session(&self, plugin_id: &str, session_generation: u64) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        if window.timer_session.generation != session_generation {
            return false;
        }
        window.timer_session.phase = TimerSessionPhase::Revoked;
        window.focused = false;
        self.changed.notify_all();
        true
    }

    fn finish_timer_call(&self, plugin_id: &str, session_generation: u64) {
        let Ok(mut core) = self.core.lock() else {
            return;
        };
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return;
        };
        if window.timer_session.generation == session_generation
            && window.timer_session.in_flight > 0
        {
            window.timer_session.in_flight -= 1;
            self.changed.notify_all();
        }
    }

    pub(crate) fn advance(
        &self,
        owner: &PluginWindowOwner,
        expected: PluginWindowPhase,
        next: PluginWindowPhase,
    ) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(&owner.plugin_id) else {
            return false;
        };
        if window.owner != *owner || window.phase != expected {
            return false;
        }
        window.phase = next;
        self.changed.notify_all();
        true
    }

    pub(crate) fn is_current(&self, owner: &PluginWindowOwner, phase: PluginWindowPhase) -> bool {
        self.core
            .lock()
            .ok()
            .and_then(|core| {
                core.windows
                    .get(&owner.plugin_id)
                    .map(|window| window.owner == *owner && window.phase == phase)
            })
            .unwrap_or(false)
    }

    pub(crate) fn begin_focus_confirmation(&self, owner: &PluginWindowOwner) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(&owner.plugin_id) else {
            return false;
        };
        if window.owner != *owner || window.phase != PluginWindowPhase::AwaitingCommit {
            return false;
        }
        window.focused = false;
        true
    }

    pub(crate) fn observe_focus(&self, plugin_id: &str, focused: bool) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        window.focused = focused;
        self.changed.notify_all();
        true
    }

    pub(crate) fn has_focus(&self, plugin_id: &str) -> bool {
        self.core
            .lock()
            .ok()
            .and_then(|core| core.windows.get(plugin_id).map(|window| window.focused))
            .unwrap_or(false)
    }

    pub(crate) fn wait_for_focus(&self, owner: &PluginWindowOwner, timeout: Duration) -> bool {
        let Ok(core) = self.core.lock() else {
            return false;
        };
        let Ok((core, _)) = self.changed.wait_timeout_while(core, timeout, |core| {
            core.windows.get(&owner.plugin_id).is_some_and(|window| {
                window.owner == *owner
                    && window.phase == PluginWindowPhase::AwaitingCommit
                    && !window.focused
            })
        }) else {
            return false;
        };
        core.windows.get(&owner.plugin_id).is_some_and(|window| {
            window.owner == *owner
                && window.phase == PluginWindowPhase::AwaitingCommit
                && window.focused
        })
    }

    pub(crate) fn set_pinned(&self, plugin_id: &str, pinned: bool) -> bool {
        self.core
            .lock()
            .ok()
            .and_then(|mut core| {
                core.windows
                    .get_mut(plugin_id)
                    .map(|window| window.pinned = pinned)
            })
            .is_some()
    }

    pub(crate) fn should_hide_on_blur(&self, plugin_id: &str) -> bool {
        self.core
            .lock()
            .ok()
            .and_then(|core| {
                core.windows.get(plugin_id).map(|window| {
                    window.phase == PluginWindowPhase::Visible && !window.pinned && !window.focused
                })
            })
            .unwrap_or(false)
    }

    pub(crate) fn close(&self, plugin_id: &str) -> bool {
        self.set_pinned(plugin_id, false)
    }

    pub(crate) fn remove_plugin(&self, plugin_id: &str) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if let Some(window) = core.windows.get_mut(plugin_id) {
            window.timer_session.phase = TimerSessionPhase::Closing;
        }
        while core
            .windows
            .get(plugin_id)
            .is_some_and(|window| window.timer_session.in_flight > 0)
        {
            let Ok(next) = self.changed.wait(core) else {
                return false;
            };
            core = next;
        }
        let removed = core.windows.remove(plugin_id).is_some();
        drop(core);
        if removed {
            self.changed.notify_all();
        }
        removed
    }

    pub(crate) fn close_for_uninstall(&self, plugin_id: &str) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return true;
        };
        window.timer_session.phase = TimerSessionPhase::Closing;
        while core
            .windows
            .get(plugin_id)
            .is_some_and(|window| window.timer_session.in_flight > 0)
        {
            let Ok(next) = self.changed.wait(core) else {
                return false;
            };
            core = next;
        }
        core.windows.remove(plugin_id);
        drop(core);
        self.changed.notify_all();
        true
    }

    pub(crate) fn owns_plugin(&self, plugin_id: &str) -> bool {
        self.core
            .lock()
            .is_ok_and(|core| core.windows.contains_key(plugin_id))
    }
    pub(crate) fn invalidate_generation(&self, plugin_id: &str, generation: u64) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if core
            .windows
            .get(plugin_id)
            .is_none_or(|window| window.generation != generation)
        {
            return false;
        }
        if let Some(window) = core.windows.get_mut(plugin_id) {
            window.timer_session.phase = TimerSessionPhase::Closing;
        }
        while core.windows.get(plugin_id).is_some_and(|window| {
            window.generation == generation && window.timer_session.in_flight > 0
        }) {
            let Ok(next) = self.changed.wait(core) else {
                return false;
            };
            core = next;
        }
        let removed = core
            .windows
            .get(plugin_id)
            .is_some_and(|window| window.generation == generation);
        if removed {
            core.windows.remove(plugin_id);
            self.changed.notify_all();
        }
        removed
    }

    pub(crate) fn wait_for(
        &self,
        owner: &PluginWindowOwner,
        phase: PluginWindowPhase,
        timeout: Duration,
    ) -> bool {
        let Ok(core) = self.core.lock() else {
            return false;
        };
        let Ok((core, _)) = self.changed.wait_timeout_while(core, timeout, |core| {
            core.windows
                .get(&owner.plugin_id)
                .is_some_and(|window| window.owner == *owner && window.phase != phase)
        }) else {
            return false;
        };
        core.windows
            .get(&owner.plugin_id)
            .is_some_and(|window| window.owner == *owner && window.phase == phase)
    }

    pub(crate) fn owner_for_content(&self, label: &str) -> Option<PluginWindowOwner> {
        let plugin_id = plugin_id_from_content_label(label)?;
        self.core
            .lock()
            .ok()?
            .windows
            .get(&plugin_id)
            .map(|window| window.owner.clone())
    }

    pub(crate) fn owner_for_shell(&self, label: &str) -> Option<PluginWindowOwner> {
        let plugin_id = plugin_id_from_shell_label(label)?;
        self.core
            .lock()
            .ok()?
            .windows
            .get(&plugin_id)
            .map(|window| window.owner.clone())
    }

    pub(crate) fn owner_for_token(&self, token: &str) -> Option<PluginWindowOwner> {
        self.core
            .lock()
            .ok()?
            .windows
            .values()
            .find(|window| {
                window.owner.submission_token == token
                    && window.phase == PluginWindowPhase::AwaitingCommit
            })
            .map(|window| window.owner.clone())
    }
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

pub(crate) fn plugin_id_from_content_label(label: &str) -> Option<String> {
    decode_label_component(label.strip_prefix("plugin-content-")?)
}

pub(crate) fn plugin_id_from_shell_label(label: &str) -> Option<String> {
    decode_label_component(label.strip_prefix("plugin-shell-")?)
}
pub(crate) fn plugin_shell_label(plugin_id: &str) -> Option<String> {
    label_component(plugin_id).map(|value| format!("plugin-shell-{value}"))
}

pub(crate) fn plugin_content_label(plugin_id: &str) -> Option<String> {
    label_component(plugin_id).map(|value| format!("plugin-content-{value}"))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginWindowUpdate {
    pub(crate) request_id: String,
    pub(crate) input: String,
    pub(crate) platform: &'static str,
    pub(crate) theme: PluginInvocationTheme,
    pub(crate) invoked_at: String,
    pub(crate) instance_number: u8,
    pub(crate) data: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginWindowPrepared {
    pub(crate) transfer_token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginWindowPinState {
    pub(crate) pinned: bool,
}

pub(crate) const PUBLIC_CONTENT_BOOTSTRAP: &str = r#"
(() => {
  'use strict';
  let handler = null;
  let invoke = null;
  let readySent = false;
  let timerSession = null;
  const U64_MAX = '18446744073709551615';
  const deepFreeze = (value, seen = new WeakSet()) => {
    if ((typeof value !== 'object' && typeof value !== 'function') || value === null || seen.has(value)) return value;
    seen.add(value);
    for (const key of Reflect.ownKeys(value)) deepFreeze(value[key], seen);
    return Object.freeze(value);
  };
  const compareU64Decimal = (left, right) => left.length === right.length
    ? (left === right ? 0 : left < right ? -1 : 1)
    : left.length < right.length ? -1 : 1;
  const canonicalU64 = (value) => typeof value === 'string'
    && /^(0|[1-9][0-9]*)$/.test(value)
    && compareU64Decimal(value, U64_MAX) <= 0;
  const normalizeTimerState = (value) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new TypeError('invalid timer state');
    const keys = Object.keys(value).sort().join(',');
    if (keys !== 'durationMs,phase,remainingMs,timerRevision') throw new TypeError('invalid timer state');
    if (!canonicalU64(value.timerRevision) || !['idle','running','paused','fired'].includes(value.phase)) {
      throw new TypeError('invalid timer state');
    }
    const safe = (item) => item === null || (Number.isSafeInteger(item) && item >= 0);
    if (!safe(value.durationMs) || !safe(value.remainingMs)) throw new TypeError('invalid timer state');
    return deepFreeze({
      timerRevision: value.timerRevision,
      phase: value.phase,
      durationMs: value.durationMs,
      remainingMs: value.remainingMs,
    });
  };
  const acceptTimerState = (session, value, allowEqualRunning, notify) => {
    if (!session.active || timerSession !== session) throw new Error('ExpiredWindowSessionError');
    const next = normalizeTimerState(value);
    const current = session.state;
    const order = current ? compareU64Decimal(next.timerRevision, current.timerRevision) : 1;
    const equalRefresh = order === 0 && allowEqualRunning
      && current.phase === 'running' && next.phase === 'running'
      && current.durationMs === next.durationMs;
    if (order > 0 || equalRefresh) {
      session.state = next;
      if (notify && session.stateHandler) {
        try { session.stateHandler(next); } catch (_) {}
      }
    }
    return session.state || next;
  };
  const expiredTimer = deepFreeze({
    getState: async () => { throw new Error('ExpiredWindowSessionError'); },
    start: async () => { throw new Error('ExpiredWindowSessionError'); },
    stop: async () => { throw new Error('ExpiredWindowSessionError'); },
    reset: async () => { throw new Error('ExpiredWindowSessionError'); },
    onStateChanged: () => { throw new Error('ExpiredWindowSessionError'); },
  });
  const createTimerSession = (sessionGeneration) => {
    const session = { sessionGeneration, active: true, state: null, stateHandler: null, readToken: 0 };
    const call = async (command, args = {}) => {
      if (!session.active || timerSession !== session || !invoke) throw new Error('ExpiredWindowSessionError');
      return invoke(command, { sessionGeneration, ...args });
    };
    session.facade = deepFreeze({
      async getState() {
        const token = ++session.readToken;
        const state = await call('plugin_window_timer_get_state');
        const latest = token === session.readToken;
        return acceptTimerState(session, state, latest, false);
      },
      async start(input) {
        const state = await call('plugin_window_timer_start', { input: input === undefined ? null : deepFreeze(input) });
        return acceptTimerState(session, state, false, true);
      },
      async stop() {
        const state = await call('plugin_window_timer_stop');
        return acceptTimerState(session, state, false, true);
      },
      async reset() {
        const state = await call('plugin_window_timer_reset');
        return acceptTimerState(session, state, false, true);
      },
      onStateChanged(next) {
        if (session.stateHandler || typeof next !== 'function') throw new TypeError('one onStateChanged handler required');
        session.stateHandler = next;
        let subscribed = true;
        return () => {
          if (!subscribed) return;
          subscribed = false;
          if (session.stateHandler === next) session.stateHandler = null;
        };
      },
    });
    return session;
  };
  const sendReady = async () => {
    if (!invoke || !handler || readySent) return;
    readySent = true;
    await invoke('plugin_window_content_ready');
  };
  const api = deepFreeze({
    onUpdate(next) {
      if (handler || typeof next !== 'function') throw new TypeError('one onUpdate handler required');
      handler = next;
      void sendReady();
      return () => { if (handler === next) handler = null; };
    },
    get timer() { return timerSession ? timerSession.facade : expiredTimer; },
  });
  Object.defineProperty(window, 'uipilotPluginWindow', { value: api, configurable: false });
  Object.defineProperty(window, '__UIPILOT_PLUGIN_TIMER_PREPARE__', {
    configurable: false,
    value: ({ sessionGeneration }) => {
      if (!canonicalU64(sessionGeneration) || sessionGeneration === '0') throw new TypeError('invalid session generation');
      if (timerSession) timerSession.active = false;
      timerSession = createTimerSession(sessionGeneration);
    },
  });
  Object.defineProperty(window, '__UIPILOT_PLUGIN_TIMER_STATE__', {
    configurable: false,
    value: ({ sessionGeneration, state }) => {
      if (!timerSession || timerSession.sessionGeneration !== sessionGeneration) return;
      acceptTimerState(timerSession, state, false, true);
    },
  });
  Object.defineProperty(window, '__UIPILOT_PLUGIN_WINDOW_UPDATE__', {
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
      await invoke('plugin_window_content_ack', { requestId: update.requestId });
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

pub(crate) fn content_ready(controller: &PluginWindowController, label: &str) -> bool {
    let Some(owner) = controller.owner_for_content(label) else {
        eprintln!(
            "[DEBUG-plugin-window-focus] content-ready label={} accepted=false reason=unknown-owner",
            label
        );
        return false;
    };
    let accepted = controller.advance(
        &owner,
        PluginWindowPhase::AwaitingReady,
        PluginWindowPhase::AwaitingAck,
    ) || controller.is_current(&owner, PluginWindowPhase::AwaitingAck);
    eprintln!(
        "[DEBUG-plugin-window-focus] content-ready plugin_id={} accepted={accepted}",
        owner.plugin_id
    );
    accepted
}

pub(crate) fn content_ack(
    controller: &PluginWindowController,
    label: &str,
    request_id: &str,
) -> bool {
    let Some(owner) = controller.owner_for_content(label) else {
        eprintln!(
            "[DEBUG-plugin-window-focus] content-ack label={} accepted=false reason=unknown-owner",
            label
        );
        return false;
    };
    let request_matches = owner.request_id == request_id;
    let accepted = request_matches
        && controller.advance(
            &owner,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::AwaitingFocus,
        );
    eprintln!(
        "[DEBUG-plugin-window-focus] content-ack plugin_id={} request_matches={request_matches} accepted={accepted}",
        owner.plugin_id
    );
    accepted
}

pub(crate) fn prepare(
    app: &AppHandle,
    controller: Arc<PluginWindowController>,
    owner: PluginWindowOwner,
    update: PluginWindowUpdate,
    window_entry: &str,
) -> Result<PluginWindowPrepared, PublicPluginManagementError> {
    let transaction = controller
        .submit(owner.clone())
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let shell_exists = app.get_window(&transaction.shell_label).is_some();
    eprintln!(
        "[DEBUG-plugin-window-focus] prepare-start plugin_id={} shell_exists={shell_exists}",
        owner.plugin_id
    );
    if !shell_exists {
        let created = create_window(app, Arc::clone(&controller), &transaction, window_entry);
        eprintln!(
            "[DEBUG-plugin-window-focus] create-window-result plugin_id={} ok={}",
            owner.plugin_id,
            created.is_ok()
        );
        created?;
    } else {
        let content = app
            .get_webview(&transaction.content_label)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if verify_windows_webview_muted(&content, CONTENT_READY_TIMEOUT).is_err() {
            let _ = controller
                .revoke_timer_session(&owner.plugin_id, transaction.timer_session_generation);
            if let Some(shell) = app.get_window(&transaction.shell_label) {
                let _ = shell.destroy();
            }
            return Err(PublicPluginManagementError::Unavailable);
        }
        app.state::<Arc<PublicPluginService>>()
            .webview_guards()
            .rebind_current(WebViewGuardOwner::Content {
                label: transaction.content_label.clone(),
                plugin_id: owner.plugin_id.clone(),
                session_generation: transaction.timer_session_generation,
            })
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
    }

    let awaiting_ready = controller.is_current(&owner, PluginWindowPhase::AwaitingReady);
    let ready = !awaiting_ready
        || controller.wait_for(
            &owner,
            PluginWindowPhase::AwaitingAck,
            CONTENT_READY_TIMEOUT,
        );
    eprintln!(
        "[DEBUG-plugin-window-focus] ready-wait plugin_id={} awaiting_ready={awaiting_ready} ready={ready}",
        owner.plugin_id
    );
    if !ready {
        teardown(app, &controller, &owner.plugin_id, owner.plugin_generation);
        return Err(PublicPluginManagementError::RuntimeNotReady);
    }

    let awaiting_ack = controller.is_current(&owner, PluginWindowPhase::AwaitingAck);
    eprintln!(
        "[DEBUG-plugin-window-focus] ready-state plugin_id={} awaiting_ack={awaiting_ack}",
        owner.plugin_id
    );
    if !awaiting_ack {
        return Err(PublicPluginManagementError::Unavailable);
    }

    eprintln!(
        "[DEBUG-plugin-window-focus] shell-lookup-start plugin_id={}",
        owner.plugin_id
    );
    let shell = app
        .get_webview(&transaction.shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    eprintln!(
        "[DEBUG-plugin-window-focus] shell-lookup-found plugin_id={}",
        owner.plugin_id
    );
    let theme = match update.theme {
        PluginInvocationTheme::Dark => "dark",
        PluginInvocationTheme::Light => "light",
    };
    eprintln!(
        "[DEBUG-plugin-window-focus] theme-eval-start plugin_id={}",
        owner.plugin_id
    );
    let theme_eval = shell.eval(format!(
        "document.documentElement.dataset.colorScheme={};",
        serde_json::to_string(theme).expect("static theme serializes")
    ));
    eprintln!(
        "[DEBUG-plugin-window-focus] theme-eval plugin_id={} ok={}",
        owner.plugin_id,
        theme_eval.is_ok()
    );
    theme_eval.map_err(|_| PublicPluginManagementError::Unavailable)?;

    let content = app.get_webview(&transaction.content_label);
    eprintln!(
        "[DEBUG-plugin-window-focus] content-lookup plugin_id={} found={}",
        owner.plugin_id,
        content.is_some()
    );
    let content = content.ok_or(PublicPluginManagementError::Unavailable)?;
    let session_generation = transaction.timer_session_generation.to_string();
    let session_payload = serde_json::to_string(&serde_json::json!({
        "sessionGeneration": session_generation,
    }))
    .map_err(|_| PublicPluginManagementError::Unavailable)?;
    content
        .eval(format!(
            "window.__UIPILOT_PLUGIN_TIMER_PREPARE__({session_payload});"
        ))
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let payload =
        serde_json::to_string(&update).map_err(|_| PublicPluginManagementError::Unavailable)?;
    let update_eval = content.eval(format!(
        "window.__UIPILOT_PLUGIN_WINDOW_UPDATE__({payload});"
    ));
    eprintln!(
        "[DEBUG-plugin-window-focus] update-eval plugin_id={} ok={}",
        owner.plugin_id,
        update_eval.is_ok()
    );
    update_eval.map_err(|_| PublicPluginManagementError::Unavailable)?;

    let acked = controller.wait_for(
        &owner,
        PluginWindowPhase::AwaitingFocus,
        CONTENT_ACK_TIMEOUT,
    );
    let advanced = acked
        && controller.advance(
            &owner,
            PluginWindowPhase::AwaitingFocus,
            PluginWindowPhase::AwaitingCommit,
        );
    eprintln!(
        "[DEBUG-plugin-window-focus] ack-wait plugin_id={} acked={acked} advanced={advanced}",
        owner.plugin_id
    );
    if !advanced {
        return Err(PublicPluginManagementError::Unavailable);
    }

    Ok(PluginWindowPrepared {
        transfer_token: owner.submission_token,
    })
}
fn create_window(
    app: &AppHandle,
    controller: Arc<PluginWindowController>,
    transaction: &PluginWindowTransaction,
    window_entry: &str,
) -> Result<(), PublicPluginManagementError> {
    let shell = WebviewWindowBuilder::new(
        app,
        transaction.shell_label.clone(),
        WebviewUrl::App("index.html".into()),
    )
    .title("UiPilot Plugin")
    .inner_size(PLUGIN_WINDOW_WIDTH, PLUGIN_WINDOW_HEIGHT)
    .visible(false)
    .decorations(false)
    .resizable(false)
    .always_on_top(false)
    .build()
    .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let saved_position = app
        .state::<SettingsStore>()
        .plugin_window_position(&transaction.owner.plugin_id);
    let _ = lifecycle::place_main_window(&shell, saved_position);
    let content_url = tauri::Url::parse(&format!(
        "uipilot-public-plugin://localhost/{}",
        window_entry.trim_start_matches('/')
    ))
    .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let inert_url = inert_url().map_err(|_| PublicPluginManagementError::Unavailable)?;
    let content = WebviewBuilder::new(
        transaction.content_label.clone(),
        WebviewUrl::CustomProtocol(inert_url),
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
    let native = app
        .get_window(&transaction.shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let content = native
        .add_child(
            content,
            LogicalPosition::new(0.0, PLUGIN_SHELL_HEIGHT),
            LogicalSize::new(
                PLUGIN_WINDOW_WIDTH,
                PLUGIN_WINDOW_HEIGHT - PLUGIN_SHELL_HEIGHT,
            ),
        )
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let guard_controller = Arc::clone(&controller);
    let guard_shell = shell.clone();
    let on_unmuted = Arc::new(move |owner| {
        let WebViewGuardOwner::Content {
            plugin_id,
            session_generation,
            ..
        } = owner
        else {
            return;
        };
        if guard_controller.revoke_timer_session(&plugin_id, session_generation) {
            let _ = guard_shell.destroy();
        }
    });
    if prepare_windows_webview(
        &content,
        app.state::<Arc<PublicPluginService>>().webview_guards(),
        WebViewGuardOwner::Content {
            label: transaction.content_label.clone(),
            plugin_id: transaction.owner.plugin_id.clone(),
            session_generation: transaction.timer_session_generation,
        },
        PUBLIC_CONTENT_BOOTSTRAP,
        content_url,
        on_unmuted,
        CONTENT_READY_TIMEOUT,
    )
    .is_err()
    {
        let _ = controller.revoke_timer_session(
            &transaction.owner.plugin_id,
            transaction.timer_session_generation,
        );
        let _ = shell.destroy();
        return Err(PublicPluginManagementError::RuntimeNotReady);
    }

    let plugin_id = transaction.owner.plugin_id.clone();
    let event_controller = Arc::clone(&controller);
    let event_shell = shell.clone();
    let event_app = app.clone();
    shell.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(true) => {
            let observed = event_controller.observe_focus(&plugin_id, true);
            eprintln!(
                "[DEBUG-plugin-window-focus] focus-event plugin_id={} focused=true observed={observed} hide_admitted={}",
                plugin_id,
                event_controller.should_hide_on_blur(&plugin_id)
            );
        }
        tauri::WindowEvent::Focused(false) => {
            let observed = event_controller.observe_focus(&plugin_id, false);
            eprintln!(
                "[DEBUG-plugin-window-focus] focus-event plugin_id={} focused=false observed={observed} hide_admitted={}",
                plugin_id,
                event_controller.should_hide_on_blur(&plugin_id)
            );
            let blur_controller = Arc::clone(&event_controller);
            let blur_shell = event_shell.clone();
            let blur_plugin_id = plugin_id.clone();
            let blur_app = event_app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(PLUGIN_BLUR_RECHECK_DELAY);
                let _ = blur_app.run_on_main_thread(move || {
                    let focused = blur_controller.has_focus(&blur_plugin_id);
                    let hide_admitted = blur_controller.should_hide_on_blur(&blur_plugin_id);
                    eprintln!(
                        "[DEBUG-plugin-window-focus] blur-recheck plugin_id={} focused={focused} hide_admitted={hide_admitted}",
                        blur_plugin_id
                    );
                    if !focused && hide_admitted {
                        let result = hide_and_revoke(
                            blur_controller.as_ref(),
                            &blur_plugin_id,
                            || blur_shell.hide().is_ok(),
                            || {
                                let _ = blur_shell.destroy();
                            },
                        );
                        eprintln!(
                            "[DEBUG-plugin-window-focus] auto-hide plugin_id={} result={result:?}",
                            blur_plugin_id
                        );
                    }
                });
            });
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if hide_and_revoke(
                event_controller.as_ref(),
                &plugin_id,
                || event_shell.hide().is_ok(),
                || {
                    let _ = event_shell.destroy();
                },
            )
            .is_ok()
            {
                event_controller.close(&plugin_id);
            }
        }
        tauri::WindowEvent::Moved(position) => {
            if event_controller.owns_plugin(&plugin_id) {
                let _ = event_app
                    .state::<SettingsStore>()
                    .set_plugin_window_position(
                        &plugin_id,
                        crate::settings::WindowPosition {
                            x: position.x,
                            y: position.y,
                        },
                    );
            }
        }
        _ => {}
    });
    Ok(())
}

pub(crate) fn commit(
    app: &AppHandle,
    controller: &PluginWindowController,
    transfers: &MainWindowTransferCoordinator,
    token: &str,
) -> Result<(), PublicPluginManagementError> {
    let owner = controller
        .owner_for_token(token)
        .ok_or(PublicPluginManagementError::InvalidToken)?;
    let main = app
        .get_webview_window("main")
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let shell_label =
        plugin_shell_label(&owner.plugin_id).ok_or(PublicPluginManagementError::Unavailable)?;
    let content_label =
        plugin_content_label(&owner.plugin_id).ok_or(PublicPluginManagementError::Unavailable)?;
    let shell = app
        .get_window(&shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let content = app
        .get_webview(&content_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    eprintln!(
        "[DEBUG-plugin-window-focus] commit-start plugin_id={}",
        owner.plugin_id
    );
    let snapshot = MainWindowSnapshot {
        visible: main
            .is_visible()
            .map_err(|_| PublicPluginManagementError::Unavailable)?,
        focused: main
            .is_focused()
            .map_err(|_| PublicPluginManagementError::Unavailable)?,
        always_on_top: main
            .is_always_on_top()
            .map_err(|_| PublicPluginManagementError::Unavailable)?,
    };
    let target = TransferTarget::Plugin {
        plugin_id: owner.plugin_id.clone(),
        submission_token: owner.submission_token.clone(),
    };
    let lease = transfers
        .begin(target, snapshot)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    if !controller.is_current(&owner, PluginWindowPhase::AwaitingCommit) {
        let _ = transfers.rollback(&lease);
        return Err(PublicPluginManagementError::InvalidToken);
    }
    let still_current = || {
        transfers.is_current(&lease)
            && controller.is_current(&owner, PluginWindowPhase::AwaitingCommit)
    };
    let native = (|| -> Result<(), ()> {
        main.set_always_on_top(false).map_err(|_| ())?;
        if !still_current() {
            return Err(());
        }
        shell.show().map_err(|_| ())?;
        eprintln!(
            "[DEBUG-plugin-window-focus] shell-show-ok plugin_id={}",
            owner.plugin_id
        );
        if !still_current() {
            return Err(());
        }
        if !controller.begin_focus_confirmation(&owner) {
            return Err(());
        }
        shell.set_focus().map_err(|_| ())?;
        content.set_focus().map_err(|_| ())?;
        eprintln!(
            "[DEBUG-plugin-window-focus] content-focus-request-ok plugin_id={}",
            owner.plugin_id
        );
        let shell_focused = controller.wait_for_focus(&owner, PLUGIN_FOCUS_CONFIRM_TIMEOUT);
        eprintln!(
            "[DEBUG-plugin-window-focus] shell-focus-check plugin_id={} focused={shell_focused}",
            owner.plugin_id
        );
        if !shell_focused || !still_current() {
            return Err(());
        }
        main.hide().map_err(|_| ())?;
        eprintln!(
            "[DEBUG-plugin-window-focus] main-hide-ok plugin_id={}",
            owner.plugin_id
        );
        if !still_current() {
            return Err(());
        }
        Ok(())
    })();
    if native.is_err() {
        eprintln!(
            "[DEBUG-plugin-window-focus] commit-native-failed plugin_id={}",
            owner.plugin_id
        );
        if let Some(snapshot) = transfers.rollback(&lease) {
            let _ = main.set_always_on_top(snapshot.always_on_top);
            let _ = if snapshot.visible {
                main.show()
            } else {
                main.hide()
            };
            if snapshot.focused {
                let _ = main.set_focus();
            }
        }
        let _ = shell.hide();
        return Err(PublicPluginManagementError::Unavailable);
    }
    let advanced = controller.advance(
        &owner,
        PluginWindowPhase::AwaitingCommit,
        PluginWindowPhase::Visible,
    );
    let committed = advanced && transfers.commit(&lease);
    eprintln!(
        "[DEBUG-plugin-window-focus] commit-finish plugin_id={} advanced={advanced} committed={committed}",
        owner.plugin_id
    );
    if !advanced || !committed {
        let _ = shell.hide();
        return Err(PublicPluginManagementError::Unavailable);
    }
    if !controller.activate_timer_session(&owner) {
        let _ = hide_and_revoke(
            controller,
            &owner.plugin_id,
            || shell.hide().is_ok(),
            || {
                let _ = shell.destroy();
            },
        );
        return Err(PublicPluginManagementError::Unavailable);
    }
    if let Ok(manager) = app.state::<Arc<PublicPluginService>>().manager() {
        if let Ok(state) = manager.window_timer_get_state(
            &owner.plugin_id,
            owner.plugin_generation,
            owner.activation_id,
        ) {
            let key = TimerKey::new(
                &owner.plugin_id,
                owner.plugin_generation,
                owner.activation_id,
            )
            .ok_or(PublicPluginManagementError::Unavailable)?;
            publish_timer_state(app, controller, &key, &state);
        }
    }
    Ok(())
}

fn hide_and_revoke(
    controller: &PluginWindowController,
    plugin_id: &str,
    hide: impl FnOnce() -> bool,
    destroy: impl FnOnce(),
) -> Result<(), PublicPluginManagementError> {
    if !controller.begin_timer_session_close(plugin_id) {
        return Err(PublicPluginManagementError::Unavailable);
    }
    match hide() {
        true => {
            if !controller.finish_timer_session_hide(plugin_id, true) {
                return Err(PublicPluginManagementError::Unavailable);
            }
            Ok(())
        }
        false => {
            let _ = controller.finish_timer_session_hide(plugin_id, false);
            let _ = controller.remove_plugin(plugin_id);
            destroy();
            Err(PublicPluginManagementError::Unavailable)
        }
    }
}

pub(crate) fn publish_timer_state(
    app: &AppHandle,
    controller: &PluginWindowController,
    key: &TimerKey,
    state: &PluginTimerState,
) {
    let Some((content_label, session_generation)) = controller.active_timer_session(key) else {
        return;
    };
    let Some(content) = app.get_webview(&content_label) else {
        return;
    };
    let Ok(payload) = serde_json::to_string(&serde_json::json!({
        "sessionGeneration": session_generation.to_string(),
        "state": state,
    })) else {
        return;
    };
    let _ = content.eval(format!("window.__UIPILOT_PLUGIN_TIMER_STATE__({payload});"));
}

pub(crate) fn set_pinned(
    app: &AppHandle,
    controller: &PluginWindowController,
    shell_label: &str,
    pinned: bool,
) -> Result<PluginWindowPinState, PublicPluginManagementError> {
    let owner = controller
        .owner_for_shell(shell_label)
        .ok_or(PublicPluginManagementError::InvalidCaller)?;
    if !controller.set_pinned(&owner.plugin_id, pinned) {
        return Err(PublicPluginManagementError::Unavailable);
    }
    let shell = app
        .get_window(shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    shell
        .set_always_on_top(false)
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    Ok(PluginWindowPinState { pinned })
}

pub(crate) fn close(
    app: &AppHandle,
    controller: &PluginWindowController,
    shell_label: &str,
) -> Result<(), PublicPluginManagementError> {
    let owner = controller
        .owner_for_shell(shell_label)
        .ok_or(PublicPluginManagementError::InvalidCaller)?;
    let shell = app
        .get_window(shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    hide_and_revoke(
        controller,
        &owner.plugin_id,
        || shell.hide().is_ok(),
        || {
            let _ = shell.destroy();
        },
    )?;
    if !controller.close(&owner.plugin_id) {
        return Err(PublicPluginManagementError::Unavailable);
    }
    Ok(())
}

pub(crate) fn teardown_current(
    app: &AppHandle,
    controller: &PluginWindowController,
    plugin_id: &str,
) {
    if controller.remove_plugin(plugin_id) {
        destroy_current(app, plugin_id);
    }
}

pub(crate) fn destroy_current(app: &AppHandle, plugin_id: &str) {
    if let Some(label) = plugin_shell_label(plugin_id) {
        if let Some(window) = app.get_window(&label) {
            let _ = window.destroy();
        }
    }
}
pub(crate) fn teardown(
    app: &AppHandle,
    controller: &PluginWindowController,
    plugin_id: &str,
    generation: u64,
) {
    if controller.invalidate_generation(plugin_id, generation) {
        if let Some(label) = plugin_shell_label(plugin_id) {
            if let Some(window) = app.get_window(&label) {
                let _ = window.destroy();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread};

    use super::*;
    use crate::public_plugins::TimerError;

    fn owner(token: &str, generation: u64, request: &str) -> PluginWindowOwner {
        PluginWindowOwner {
            ui_intent_epoch: generation,
            submission_token: token.into(),
            plugin_id: "com.example.demo".into(),
            plugin_generation: generation,
            activation_id: generation,
            admission_epoch: generation,
            request_id: request.into(),
            control_value: format!("/demo {token}"),
        }
    }

    #[test]
    fn singleton_reuse_keeps_instance_one_and_stale_ready_or_ack_cannot_advance() {
        let controller = PluginWindowController::default();
        let first = owner("a", 3, "request-a");
        let first_window = controller.submit(first.clone()).unwrap();
        assert_eq!(first_window.instance_number, 1);
        assert!(controller.advance(
            &first,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::AwaitingAck
        ));

        let second = owner("b", 3, "request-b");
        let second_window = controller.submit(second.clone()).unwrap();
        assert_eq!(first_window.shell_label, second_window.shell_label);
        assert_eq!(second_window.instance_number, 1);
        assert!(!controller.advance(
            &first,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::AwaitingFocus
        ));
        assert!(controller.advance(
            &second,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::AwaitingFocus
        ));
    }

    #[test]
    fn timer_session_is_read_only_until_visible_and_old_generations_expire() {
        let controller = Arc::new(PluginWindowController::default());
        let first = owner("timer-a", 3, "request-a");
        let transaction = controller.submit(first.clone()).unwrap();
        assert_eq!(transaction.timer_session_generation, 1);

        let read = controller
            .begin_timer_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                false,
            )
            .unwrap();
        assert_eq!(read.owner(), &first);
        drop(read);
        assert!(matches!(
            controller.begin_timer_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                true,
            ),
            Err(TimerError::ExpiredWindowSessionError)
        ));

        for (expected, next) in [
            (
                PluginWindowPhase::AwaitingReady,
                PluginWindowPhase::AwaitingAck,
            ),
            (
                PluginWindowPhase::AwaitingAck,
                PluginWindowPhase::AwaitingFocus,
            ),
            (
                PluginWindowPhase::AwaitingFocus,
                PluginWindowPhase::AwaitingCommit,
            ),
            (
                PluginWindowPhase::AwaitingCommit,
                PluginWindowPhase::Visible,
            ),
        ] {
            assert!(controller.advance(&first, expected, next));
        }
        assert!(controller.activate_timer_session(&first));
        let stale_reinstall_key = TimerKey::new(
            &first.plugin_id,
            first.plugin_generation,
            first.activation_id + 1,
        )
        .unwrap();
        assert!(controller
            .active_timer_session(&stale_reinstall_key)
            .is_none());
        drop(
            controller
                .begin_timer_call(
                    &transaction.content_label,
                    transaction.timer_session_generation,
                    true,
                )
                .unwrap(),
        );

        assert!(controller.begin_timer_session_close(&first.plugin_id));
        assert!(matches!(
            controller.begin_timer_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                false,
            ),
            Err(TimerError::ExpiredWindowSessionError)
        ));
        assert!(controller.finish_timer_session_hide(&first.plugin_id, true));

        let second = owner("timer-b", 3, "request-b");
        let next = controller.submit(second).unwrap();
        assert!(next.timer_session_generation > transaction.timer_session_generation);
        assert!(matches!(
            controller.begin_timer_call(
                &next.content_label,
                transaction.timer_session_generation,
                false,
            ),
            Err(TimerError::ExpiredWindowSessionError)
        ));
    }

    #[test]
    fn hiding_waits_for_an_admitted_timer_call_to_finish() {
        let controller = Arc::new(PluginWindowController::default());
        let current = owner("timer-barrier", 4, "request-barrier");
        let transaction = controller.submit(current.clone()).unwrap();
        for (expected, next) in [
            (
                PluginWindowPhase::AwaitingReady,
                PluginWindowPhase::AwaitingAck,
            ),
            (
                PluginWindowPhase::AwaitingAck,
                PluginWindowPhase::AwaitingFocus,
            ),
            (
                PluginWindowPhase::AwaitingFocus,
                PluginWindowPhase::AwaitingCommit,
            ),
            (
                PluginWindowPhase::AwaitingCommit,
                PluginWindowPhase::Visible,
            ),
        ] {
            assert!(controller.advance(&current, expected, next));
        }
        assert!(controller.activate_timer_session(&current));
        let lease = controller
            .begin_timer_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                true,
            )
            .unwrap();
        let closing = Arc::clone(&controller);
        let plugin_id = current.plugin_id.clone();
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let result = closing.begin_timer_session_close(&plugin_id);
            let _ = sender.send(result);
        });

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        drop(lease);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(true));
        thread.join().unwrap();
    }

    #[test]
    fn uninstall_close_waits_for_call_lease_and_removes_window_owner() {
        let controller = Arc::new(PluginWindowController::default());
        let current = owner("uninstall-barrier", 4, "request-uninstall");
        let transaction = controller.submit(current.clone()).unwrap();
        let lease = controller
            .begin_timer_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                false,
            )
            .unwrap();
        let closing = Arc::clone(&controller);
        let plugin_id = current.plugin_id.clone();
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let result = closing.close_for_uninstall(&plugin_id);
            let _ = sender.send(result);
        });

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        drop(lease);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(true));
        thread.join().unwrap();
        assert!(!controller.owns_plugin(&current.plugin_id));
        assert!(controller.close_for_uninstall(&current.plugin_id));
    }

    #[test]
    fn mute_guard_revokes_only_the_exact_content_session() {
        let controller = PluginWindowController::default();
        let first = owner("mute-guard-first", 2, "request-first");
        let first_transaction = controller.submit(first.clone()).unwrap();
        assert!(controller.advance(
            &first,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::AwaitingAck
        ));
        assert!(controller
            .revoke_timer_session(&first.plugin_id, first_transaction.timer_session_generation));

        let second = owner("mute-guard-second", 2, "request-second");
        let second_transaction = controller.submit(second.clone()).unwrap();
        assert!(!controller.revoke_timer_session(
            &second.plugin_id,
            first_transaction.timer_session_generation
        ));
        assert!(controller.is_current(&second, PluginWindowPhase::AwaitingAck));
        assert!(controller.revoke_timer_session(
            &second.plugin_id,
            second_transaction.timer_session_generation
        ));
    }

    #[test]
    fn bootstrap_owns_a_session_bound_timer_facade_without_general_events() {
        for fragment in [
            "get timer()",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
            "__UIPILOT_PLUGIN_TIMER_PREPARE__",
            "__UIPILOT_PLUGIN_TIMER_STATE__",
            "sessionGeneration",
            "compareU64Decimal",
        ] {
            assert!(
                PUBLIC_CONTENT_BOOTSTRAP.contains(fragment),
                "missing {fragment}"
            );
        }
        assert!(!PUBLIC_CONTENT_BOOTSTRAP.contains("listen("));
        assert!(PUBLIC_CONTENT_BOOTSTRAP
            .contains("Reflect.deleteProperty(window, '__TAURI_INTERNALS__')"));
    }

    #[test]
    fn blur_hiding_is_disallowed_until_the_window_is_visible() {
        let controller = PluginWindowController::default();
        let current = owner("blur", 4, "request-blur");
        controller.submit(current.clone()).unwrap();

        for (expected, next) in [
            (
                PluginWindowPhase::AwaitingReady,
                PluginWindowPhase::AwaitingAck,
            ),
            (
                PluginWindowPhase::AwaitingAck,
                PluginWindowPhase::AwaitingFocus,
            ),
            (
                PluginWindowPhase::AwaitingFocus,
                PluginWindowPhase::AwaitingCommit,
            ),
            (
                PluginWindowPhase::AwaitingCommit,
                PluginWindowPhase::Visible,
            ),
        ] {
            assert!(!controller.should_hide_on_blur(&current.plugin_id));
            assert!(controller.advance(&current, expected, next));
        }

        assert!(controller.should_hide_on_blur(&current.plugin_id));
    }

    #[test]
    fn focus_event_confirms_current_commit() {
        let controller = PluginWindowController::default();
        let current = owner("focus", 4, "request-focus");
        controller.submit(current.clone()).unwrap();
        for (expected, next) in [
            (
                PluginWindowPhase::AwaitingReady,
                PluginWindowPhase::AwaitingAck,
            ),
            (
                PluginWindowPhase::AwaitingAck,
                PluginWindowPhase::AwaitingFocus,
            ),
            (
                PluginWindowPhase::AwaitingFocus,
                PluginWindowPhase::AwaitingCommit,
            ),
        ] {
            assert!(controller.advance(&current, expected, next));
        }
        assert!(controller.begin_focus_confirmation(&current));
        assert!(!controller.wait_for_focus(&current, Duration::ZERO));
        assert!(controller.observe_focus(&current.plugin_id, true));
        assert!(controller.wait_for_focus(&current, Duration::ZERO));
    }

    #[test]
    fn refocus_cancels_blur_hiding() {
        let controller = PluginWindowController::default();
        let current = owner("refocus", 4, "request-refocus");
        controller.submit(current.clone()).unwrap();
        for (expected, next) in [
            (
                PluginWindowPhase::AwaitingReady,
                PluginWindowPhase::AwaitingAck,
            ),
            (
                PluginWindowPhase::AwaitingAck,
                PluginWindowPhase::AwaitingFocus,
            ),
            (
                PluginWindowPhase::AwaitingFocus,
                PluginWindowPhase::AwaitingCommit,
            ),
            (
                PluginWindowPhase::AwaitingCommit,
                PluginWindowPhase::Visible,
            ),
        ] {
            assert!(controller.advance(&current, expected, next));
        }
        assert!(controller.observe_focus(&current.plugin_id, false));
        assert!(controller.should_hide_on_blur(&current.plugin_id));
        assert!(controller.observe_focus(&current.plugin_id, true));
        assert!(controller.has_focus(&current.plugin_id));
        assert!(!controller.should_hide_on_blur(&current.plugin_id));
    }

    #[test]
    fn pin_close_and_generation_invalidation_are_host_owned() {
        let controller = PluginWindowController::default();
        let current = owner("current", 4, "request-current");
        controller.submit(current.clone()).unwrap();
        assert!(controller.advance(
            &current,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::AwaitingAck
        ));
        assert!(controller.advance(
            &current,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::AwaitingFocus
        ));
        assert!(controller.advance(
            &current,
            PluginWindowPhase::AwaitingFocus,
            PluginWindowPhase::AwaitingCommit
        ));
        assert!(controller.advance(
            &current,
            PluginWindowPhase::AwaitingCommit,
            PluginWindowPhase::Visible
        ));
        assert!(controller.set_pinned(&current.plugin_id, true));
        assert!(!controller.should_hide_on_blur(&current.plugin_id));
        assert!(controller.close(&current.plugin_id));
        assert!(controller.should_hide_on_blur(&current.plugin_id));
        assert!(controller.invalidate_generation(&current.plugin_id, 4));
        assert!(!controller.is_current(&current, PluginWindowPhase::AwaitingReady));
    }

    #[test]
    fn explicit_close_resets_pin_only_after_native_hide_succeeds() {
        let source = include_str!("plugin_window.rs").replace(
            "
", "
",
        );
        let close = source
            .split(
                "pub(crate) fn close(
    app: &AppHandle,",
            )
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn teardown_current(").next())
            .expect("plugin window close function is missing");
        let hide = close
            .find("hide_and_revoke(")
            .expect("session-aware native hide is missing");
        let reset = close
            .find("controller.close(&owner.plugin_id)")
            .expect("pin reset is missing");
        assert!(hide < reset);
    }

    #[test]
    fn content_ready_and_ack_are_bound_to_exact_label_and_current_request() {
        let controller = PluginWindowController::default();
        let current = owner("content", 5, "request-content");
        let transaction = controller.submit(current.clone()).unwrap();
        assert!(!content_ready(&controller, "plugin-content-forged"));
        assert!(content_ready(&controller, &transaction.content_label));
        assert!(content_ready(&controller, &transaction.content_label));
        assert!(!content_ack(
            &controller,
            &transaction.content_label,
            "wrong-request"
        ));
        assert!(content_ack(
            &controller,
            &transaction.content_label,
            "request-content"
        ));
        let replacement = owner("replacement", 5, "request-replacement");
        controller.submit(replacement).unwrap();
        assert!(!content_ack(
            &controller,
            &transaction.content_label,
            "request-content"
        ));
    }

    #[test]
    fn content_bootstrap_exposes_only_one_way_update_and_deletes_tauri_internals() {
        for required in [
            "uipilotPluginWindow",
            "onUpdate(next)",
            "plugin_window_content_ready",
            "plugin_window_content_ack",
            "Reflect.deleteProperty(window, '__TAURI_INTERNALS__')",
            "--uipilot-color-${name}",
            "background",
        ] {
            assert!(PUBLIC_CONTENT_BOOTSTRAP.contains(required));
        }
        for forbidden in ["storageGet", "clipboard", "shell", "fetch(", "WebSocket"] {
            assert!(!PUBLIC_CONTENT_BOOTSTRAP.contains(forbidden));
        }
    }
    #[test]
    fn host_window_uses_separate_locked_down_shell_and_content_webviews() {
        let source = include_str!("plugin_window.rs").replace("\r\n", "\n");
        let production = source
            .split("fn create_window(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn commit(").next())
            .expect("plugin window production source markers are missing");
        let preparation = source
            .split("pub(crate) fn prepare(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn commit(").next())
            .expect("plugin window preparation source markers are missing");
        for required in [
            "WebviewWindowBuilder::new(",
            "WebviewUrl::App(\"index.html\".into())",
            ".visible(false)",
            ".decorations(false)",
            ".always_on_top(false)",
            "WebviewBuilder::new(",
            "WebviewUrl::CustomProtocol(inert_url)",
            "prepare_windows_webview(",
            "PUBLIC_CONTENT_BOOTSTRAP",
            ".on_navigation(",
            ".on_new_window(|_, _| NewWindowResponse::Deny)",
            ".on_download(|_, _| false)",
            ".add_child(",
        ] {
            assert!(
                production.contains(required),
                "missing isolation fragment: {required}"
            );
        }
        assert!(preparation.contains("verify_windows_webview_muted("));
        for forbidden in [
            "NewWindowResponse::Allow",
            "WebviewUrl::External",
            "file://",
            ".initialization_script(PUBLIC_CONTENT_BOOTSTRAP)",
            ".always_on_top(true)",
            "Command::new",
        ] {
            assert!(
                !production.contains(forbidden),
                "forbidden window fragment: {forbidden}"
            );
        }
    }

    #[test]
    fn multi_webview_plugin_shell_avoids_webview_window_lookup() {
        let source = include_str!("plugin_window.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("plugin window test module marker is missing");

        assert_eq!(production.matches("get_webview_window(").count(), 1);
        assert!(production.contains(".get_webview_window(\"main\")"));
        assert!(production.contains(".get_webview(&transaction.shell_label)"));
    }

    #[test]
    fn plugin_window_commit_waits_for_focus_event_before_hiding_main() {
        let commands = include_str!("commands.rs").replace("\r\n", "\n");
        assert!(commands
            .contains("#[tauri::command]\npub(crate) async fn commit_plugin_window_transfer("));
        let command = commands
            .split("pub(crate) async fn commit_plugin_window_transfer(")
            .nth(1)
            .and_then(|tail| tail.split("\n#[tauri::command]").next())
            .expect("async plugin window commit command is missing");
        assert!(command.contains("tauri::async_runtime::spawn_blocking"));

        let source = include_str!("plugin_window.rs").replace("\r\n", "\n");
        let commit = source
            .split("pub(crate) fn commit(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn set_pinned(").next())
            .expect("plugin window commit source markers are missing");
        let reset = commit
            .find("controller.begin_focus_confirmation(&owner)")
            .expect("focus confirmation reset is missing");
        let request = commit
            .find("shell.set_focus()")
            .expect("native focus request is missing");
        let content_request = commit
            .find("content.set_focus()")
            .expect("content webview focus request is missing");
        let evidence = commit
            .find("controller.wait_for_focus(")
            .expect("focus event wait is missing");
        let hide = commit.find("main.hide()").expect("main hide is missing");
        assert!(reset < request && request < content_request && content_request < evidence);
        assert!(evidence < hide);
        assert!(!commit.contains("shell.is_focused()"));
    }

    #[test]
    fn blur_event_rechecks_observed_focus_before_hiding() {
        let source = include_str!("plugin_window.rs").replace("\r\n", "\n");
        let handler = source
            .split("shell.on_window_event(move |event|")
            .nth(1)
            .and_then(|tail| tail.split("    Ok(())").next())
            .expect("plugin window event handler source markers are missing");
        assert!(handler.contains("event_controller.observe_focus(&plugin_id, true)"));
        assert!(handler.contains("event_controller.observe_focus(&plugin_id, false)"));
        let deferred = handler
            .find("std::thread::spawn")
            .expect("blur handling must leave the event callback");
        let wait = handler
            .find("std::thread::sleep(PLUGIN_BLUR_RECHECK_DELAY)")
            .expect("blur handling must allow transient child focus to settle");
        let focus_recheck = handler
            .find("blur_controller.has_focus(&blur_plugin_id)")
            .expect("blur handling must recheck observed focus");
        let hide = handler
            .find("hide_and_revoke(")
            .expect("blur handling must retain unpinned auto-hide");

        assert!(deferred < wait && wait < focus_recheck && focus_recheck < hide);
        assert!(!handler.contains("blur_shell.is_focused()"));
    }

    #[test]
    fn shell_and_content_labels_are_disjoint_and_plugin_owned() {
        let shell = plugin_shell_label("com.example.demo").unwrap();
        let content = plugin_content_label("com.example.demo").unwrap();
        assert!(shell.starts_with("plugin-shell-"));
        assert!(content.starts_with("plugin-content-"));
        assert_ne!(shell, content);
        assert!(!shell.contains("com.example.demo"));
    }
}
