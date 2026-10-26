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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginWindowCallError {
    InvalidCaller,
    ExpiredWindowSession,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowActionIdentity {
    pub(crate) owner: PluginWindowOwner,
    pub(crate) content_label: String,
    pub(crate) session_generation: u64,
    pub(crate) document_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginWindowTransaction {
    pub(crate) owner: PluginWindowOwner,
    pub(crate) shell_label: String,
    pub(crate) content_label: String,
    pub(crate) instance_number: u8,
    pub(crate) timer_session_generation: u64,
    pub(crate) document_generation: u64,
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
    next_document: u64,
    next_file_picker: u64,
    file_picker: Option<(u64, String, u64)>,
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
    document_generation: u64,
    document_revoked: bool,
    runtime_request: Option<crate::public_plugins::WindowActionRegistration>,
}

pub(crate) struct PluginWindowCallLease {
    controller: Arc<PluginWindowController>,
    owner: PluginWindowOwner,
    session_generation: u64,
}

impl PluginWindowCallLease {
    pub(crate) fn owner(&self) -> &PluginWindowOwner {
        &self.owner
    }
}

impl Drop for PluginWindowCallLease {
    fn drop(&mut self) {
        self.controller
            .finish_window_call(&self.owner.plugin_id, self.session_generation);
    }
}

// Global physical exclusion is independent of replaceable session state and async task lifetime.
static FILE_PICKER_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct PhysicalPickerPermit;
impl PhysicalPickerPermit {
    fn acquire() -> Result<Self, crate::public_plugins::file_open::FileOpenError> {
        FILE_PICKER_BUSY
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map(|_| Self)
            .map_err(|_| crate::public_plugins::file_open::FileOpenError::FileOpenBusyError)
    }
}
impl Drop for PhysicalPickerPermit {
    fn drop(&mut self) {
        FILE_PICKER_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}
pub(crate) struct FileOpenGuard {
    pub(crate) identity: WindowActionIdentity,
    controller: Arc<PluginWindowController>,
    token: u64,
    _physical: PhysicalPickerPermit,
}
impl Drop for FileOpenGuard {
    fn drop(&mut self) {
        let mut core = self
            .controller
            .core
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if core
            .file_picker
            .as_ref()
            .is_some_and(|(token, _, _)| *token == self.token)
        {
            core.file_picker = None;
        }
    }
}
impl PluginWindowController {
    fn file_identity_current(core: &ControllerCore, identity: &WindowActionIdentity) -> bool {
        plugin_content_label(&identity.owner.plugin_id).as_deref()
            == Some(identity.content_label.as_str())
            && identity.session_generation != 0
            && identity.document_generation != 0
            && core
                .windows
                .get(&identity.owner.plugin_id)
                .is_some_and(|w| {
                    w.owner == identity.owner
                        && w.timer_session.generation == identity.session_generation
                        && w.document_generation == identity.document_generation
                        && !w.document_revoked
                        && w.phase == PluginWindowPhase::Visible
                        && w.timer_session.phase == TimerSessionPhase::Active
                })
    }
    pub(crate) fn admit_file_open<T>(
        self: &Arc<Self>,
        label: &str,
        session: u64,
        document: u64,
        authorize: impl FnOnce(
            &WindowActionIdentity,
        ) -> Result<T, crate::public_plugins::file_open::FileOpenError>,
    ) -> Result<(FileOpenGuard, T), crate::public_plugins::file_open::FileOpenError> {
        use crate::public_plugins::file_open::FileOpenError as E;
        let plugin = plugin_id_from_content_label(label).ok_or(E::ExpiredWindowSessionError)?;
        let mut core = self.core.lock().map_err(|_| E::FileOpenFailedError)?;
        let owner = core
            .windows
            .get(&plugin)
            .ok_or(E::ExpiredWindowSessionError)?
            .owner
            .clone();
        let identity = WindowActionIdentity {
            owner,
            content_label: label.into(),
            session_generation: session,
            document_generation: document,
        };
        if !Self::file_identity_current(&core, &identity) {
            return Err(E::ExpiredWindowSessionError);
        }
        let captured = authorize(&identity)?;
        let permit = PhysicalPickerPermit::acquire()?;
        core.next_file_picker = core
            .next_file_picker
            .checked_add(1)
            .ok_or(E::FileOpenFailedError)?;
        let token = core.next_file_picker;
        core.file_picker = Some((token, plugin, document));
        Ok((
            FileOpenGuard {
                identity,
                controller: self.clone(),
                token,
                _physical: permit,
            },
            captured,
        ))
    }
    pub(crate) fn check_file_open<T>(
        &self,
        identity: &WindowActionIdentity,
        authorize: impl FnOnce() -> Result<T, crate::public_plugins::file_open::FileOpenError>,
    ) -> Result<T, crate::public_plugins::file_open::FileOpenError> {
        use crate::public_plugins::file_open::FileOpenError as E;
        let core = self.core.lock().map_err(|_| E::FileOpenFailedError)?;
        if !Self::file_identity_current(&core, identity) {
            return Err(E::ExpiredWindowSessionError);
        }
        authorize()
    }
    // This short lease covers only focus effects. It never spans native selection or reading.
    fn begin_file_focus(
        self: &Arc<Self>,
        identity: &WindowActionIdentity,
        authorize: impl FnOnce() -> Result<(), crate::public_plugins::file_open::FileOpenError>,
    ) -> Result<PluginWindowCallLease, crate::public_plugins::file_open::FileOpenError> {
        use crate::public_plugins::file_open::FileOpenError as E;
        let mut core = self.core.lock().map_err(|_| E::FileOpenFailedError)?;
        if !Self::file_identity_current(&core, identity) {
            return Err(E::ExpiredWindowSessionError);
        }
        authorize()?;
        let w = core
            .windows
            .get_mut(&identity.owner.plugin_id)
            .ok_or(E::ExpiredWindowSessionError)?;
        w.timer_session.in_flight = w
            .timer_session
            .in_flight
            .checked_add(1)
            .ok_or(E::FileOpenFailedError)?;
        Ok(PluginWindowCallLease {
            controller: self.clone(),
            owner: identity.owner.clone(),
            session_generation: identity.session_generation,
        })
    }
}

impl PluginWindowController {
    pub(crate) fn submit(&self, owner: PluginWindowOwner) -> Option<PluginWindowTransaction> {
        let shell_label = plugin_shell_label(&owner.plugin_id)?;
        let content_label = plugin_content_label(&owner.plugin_id)?;
        let mut core = self.core.lock().ok()?;
        if let Some(window) = core.windows.get_mut(&owner.plugin_id) {
            window.runtime_request.take();
            window.timer_session.phase = TimerSessionPhase::Closing;
        }
        while core
            .windows
            .get(&owner.plugin_id)
            .is_some_and(|window| window.timer_session.in_flight > 0)
        {
            core = self.changed.wait(core).ok()?;
        }
        let existing = core.windows.get(&owner.plugin_id).filter(|window| {
            window.generation == owner.plugin_generation && !window.document_revoked
        });
        // AwaitingReady is rebuilt below, so its old native Destroyed hook must own a different document.
        let document_generation = existing
            .filter(|window| window.phase != PluginWindowPhase::AwaitingReady)
            .map(|window| window.document_generation);
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
        let document_generation = match document_generation {
            Some(document) => document,
            None => {
                core.next_document = core.next_document.checked_add(1)?;
                core.next_document
            }
        };
        core.windows.insert(
            owner.plugin_id.clone(),
            WindowState {
                generation: owner.plugin_generation,
                pinned,
                focused: false,
                phase,
                owner: owner.clone(),
                document_generation,
                document_revoked: false,
                runtime_request: None,
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
            document_generation,
        })
    }

    /// A document may be reused for many submits. Hooks bind document identity, never the first session.
    pub(crate) fn revoke_document(&self, plugin_id: &str, document_generation: u64) -> bool {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let Some(window) = core
            .windows
            .get_mut(plugin_id)
            .filter(|w| w.document_generation == document_generation)
        else {
            return false;
        };
        window.runtime_request.take();
        window.document_revoked = true;
        window.timer_session.phase = TimerSessionPhase::Revoked;
        window.focused = false;
        self.changed.notify_all();
        true
    }

    pub(crate) fn admit_window_action(
        self: &Arc<Self>,
        service: &Arc<PublicPluginService>,
        label: &str,
        session_generation: u64,
        document_generation: u64,
        request: crate::public_plugins::window_action::WindowActionRequest,
    ) -> Result<
        (
            crate::public_plugins::ScheduledPluginRequest,
            std::sync::mpsc::Receiver<
                Result<Value, crate::public_plugins::window_action::WindowActionError>,
            >,
        ),
        crate::public_plugins::window_action::WindowActionError,
    > {
        use crate::public_plugins::window_action::WindowActionError as E;
        let plugin_id = plugin_id_from_content_label(label).ok_or(E::ExpiredWindowSessionError)?;
        let mut core = self
            .core
            .lock()
            .map_err(|_| E::WindowRequestUnavailableError)?;
        let window = core
            .windows
            .get_mut(&plugin_id)
            .ok_or(E::ExpiredWindowSessionError)?;
        if window.timer_session.generation != session_generation
            || window.document_generation != document_generation
            || matches!(
                window.timer_session.phase,
                TimerSessionPhase::Closing | TimerSessionPhase::Revoked
            )
        {
            return Err(E::ExpiredWindowSessionError);
        }
        if window.phase != PluginWindowPhase::Visible
            || window.timer_session.phase != TimerSessionPhase::Active
            || window.runtime_request.as_ref().is_some_and(|r| r.active())
        {
            return Err(E::WindowRequestBusyError);
        }
        window.runtime_request.take();
        let identity = WindowActionIdentity {
            owner: window.owner.clone(),
            content_label: label.into(),
            session_generation,
            document_generation,
        };
        let (dispatch, registration, receiver) =
            service.prepare_window_action(self, &identity, request)?;
        window.runtime_request = Some(registration);
        Ok((dispatch, receiver))
    }

    pub(crate) fn finish_window_action(
        &self,
        identity: &WindowActionIdentity,
        context: &crate::public_plugins::PluginRequestContext,
        claim: impl FnOnce() -> Result<(), crate::public_plugins::content_action::ContentActionError>,
    ) -> Result<(), crate::public_plugins::content_action::ContentActionError> {
        use crate::public_plugins::content_action::ContentActionError as E;
        let mut core = self.core.lock().map_err(|_| E::Unavailable)?;
        let window = core
            .windows
            .get_mut(&identity.owner.plugin_id)
            .filter(|w| {
                w.owner == identity.owner
                    && w.timer_session.generation == identity.session_generation
                    && w.document_generation == identity.document_generation
                    && w.timer_session.phase == TimerSessionPhase::Active
                    && w.phase == PluginWindowPhase::Visible
            })
            .ok_or(E::Expired)?;
        if !window
            .runtime_request
            .as_ref()
            .is_some_and(|r| r.matches(context) && r.active())
        {
            return Err(E::Expired);
        }
        let result = claim();
        if let Some(registration) = window.runtime_request.take() {
            registration.disarm();
        }
        result
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

    pub(crate) fn begin_window_call(
        self: &Arc<Self>,
        content_label: &str,
        session_generation: u64,
        mutable: bool,
    ) -> Result<PluginWindowCallLease, PluginWindowCallError> {
        let plugin_id = plugin_id_from_content_label(content_label)
            .ok_or(PluginWindowCallError::InvalidCaller)?;
        let mut core = self
            .core
            .lock()
            .map_err(|_| PluginWindowCallError::Unavailable)?;
        let window = core
            .windows
            .get_mut(&plugin_id)
            .ok_or(PluginWindowCallError::ExpiredWindowSession)?;
        let allowed = window.timer_session.generation == session_generation
            && match window.timer_session.phase {
                TimerSessionPhase::Prepared => !mutable,
                TimerSessionPhase::Active => true,
                TimerSessionPhase::Closing | TimerSessionPhase::Revoked => false,
            };
        if !allowed {
            return Err(PluginWindowCallError::ExpiredWindowSession);
        }
        window.timer_session.in_flight = window
            .timer_session
            .in_flight
            .checked_add(1)
            .ok_or(PluginWindowCallError::Unavailable)?;
        Ok(PluginWindowCallLease {
            controller: Arc::clone(self),
            owner: window.owner.clone(),
            session_generation,
        })
    }

    // Native callbacks own a document, which can be reused by a later submit. Check the current
    // session under the core lock and reserve a short lease across native hide + final revocation.
    // Submit/uninstall already wait for these short leases; Runtime actions never use this count.
    fn begin_document_hide(
        self: &Arc<Self>,
        plugin_id: &str,
        document_generation: u64,
        only_if_blurred: bool,
    ) -> Option<PluginWindowCallLease> {
        self.begin_document_hide_checked(plugin_id, document_generation, only_if_blurred, None)
    }

    fn begin_document_hide_checked(
        self: &Arc<Self>,
        plugin_id: &str,
        document_generation: u64,
        only_if_blurred: bool,
        expected: Option<&WindowActionIdentity>,
    ) -> Option<PluginWindowCallLease> {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        if expected.is_some_and(|identity| !Self::file_identity_current(&core, identity)) {
            return None;
        }
        if only_if_blurred
            && core
                .file_picker
                .as_ref()
                .is_some_and(|(_, plugin, document)| {
                    plugin == plugin_id && *document == document_generation
                })
        {
            return None;
        }
        let window = core.windows.get_mut(plugin_id)?;
        if window.document_generation != document_generation
            || window.document_revoked
            || window.timer_session.phase == TimerSessionPhase::Closing
            || (only_if_blurred
                && (window.focused
                    || window.pinned
                    || window.phase != PluginWindowPhase::Visible
                    || window.timer_session.phase != TimerSessionPhase::Active))
        {
            return None;
        }
        let owner = window.owner.clone();
        let session_generation = window.timer_session.generation;
        window.runtime_request.take();
        window.timer_session.phase = TimerSessionPhase::Closing;
        let matches = |window: &WindowState| {
            window.owner == owner
                && window.document_generation == document_generation
                && window.timer_session.generation == session_generation
        };
        while core
            .windows
            .get(plugin_id)
            .is_some_and(|window| matches(window) && window.timer_session.in_flight > 0)
        {
            core = self.changed.wait(core).ok()?;
        }
        let window = core.windows.get_mut(plugin_id).filter(|window| {
            matches(window)
                && !window.document_revoked
                && window.timer_session.phase == TimerSessionPhase::Closing
        })?;
        window.timer_session.in_flight = 1;
        Some(PluginWindowCallLease {
            controller: Arc::clone(self),
            owner,
            session_generation,
        })
    }

    pub(crate) fn begin_timer_session_close(&self, plugin_id: &str) -> bool {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        window.runtime_request.take();
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
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        if window.timer_session.phase != TimerSessionPhase::Closing {
            return false;
        }
        window.runtime_request.take();
        window.timer_session.phase = TimerSessionPhase::Revoked;
        if hidden {
            window.focused = false;
        }
        self.changed.notify_all();
        true
    }

    pub(crate) fn revoke_timer_session(&self, plugin_id: &str, session_generation: u64) -> bool {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return false;
        };
        if window.timer_session.generation != session_generation {
            return false;
        }
        window.runtime_request.take();
        window.timer_session.phase = TimerSessionPhase::Revoked;
        window.focused = false;
        self.changed.notify_all();
        true
    }

    fn finish_window_call(&self, plugin_id: &str, session_generation: u64) {
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

    fn observe_document_focus(
        &self,
        plugin_id: &str,
        document_generation: u64,
        focused: bool,
    ) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        let Some(window) = core.windows.get_mut(plugin_id).filter(|window| {
            window.document_generation == document_generation && !window.document_revoked
        }) else {
            return false;
        };
        window.focused = focused;
        self.changed.notify_all();
        true
    }

    #[cfg(test)]
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

    #[cfg(test)]
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
                    window.phase == PluginWindowPhase::Visible
                        && !window.pinned
                        && !window.focused
                        && !core.file_picker.as_ref().is_some_and(|(_, plugin, doc)| {
                            plugin == plugin_id && *doc == window.document_generation
                        })
                })
            })
            .unwrap_or(false)
    }

    pub(crate) fn close(&self, plugin_id: &str) -> bool {
        self.set_pinned(plugin_id, false)
    }

    pub(crate) fn remove_plugin(&self, plugin_id: &str) -> bool {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(window) = core.windows.get_mut(plugin_id) {
            window.runtime_request.take();
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
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let Some(window) = core.windows.get_mut(plugin_id) else {
            return true;
        };
        window.runtime_request.take();
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

    pub(crate) fn active_plugin_ids(&self) -> Vec<String> {
        self.core
            .lock()
            .map(|core| core.windows.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn invalidate_generation(&self, plugin_id: &str, generation: u64) -> bool {
        self.invalidate_generation_with(plugin_id, generation, || ())
            .is_some()
    }

    // `capture` may only clone a native handle from the Tauri manager; it must not execute native
    // operations. Generation/owner/document checks and capture share this lock. Effects run later.
    fn invalidate_generation_with<T>(
        &self,
        plugin_id: &str,
        generation: u64,
        capture: impl FnOnce() -> T,
    ) -> Option<T> {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let window = core
            .windows
            .get_mut(plugin_id)
            .filter(|window| window.generation == generation)?;
        let owner = window.owner.clone();
        let document_generation = window.document_generation;
        let session_generation = window.timer_session.generation;
        window.runtime_request.take();
        window.timer_session.phase = TimerSessionPhase::Closing;
        let matches = |window: &WindowState| {
            window.generation == generation
                && window.owner == owner
                && window.document_generation == document_generation
                && window.timer_session.generation == session_generation
        };
        while core
            .windows
            .get(plugin_id)
            .is_some_and(|window| matches(window) && window.timer_session.in_flight > 0)
        {
            core = self.changed.wait(core).ok()?;
        }
        // Condvar waiting releases the lock; never capture or remove a replacement on wake.
        if !core.windows.get(plugin_id).is_some_and(matches) {
            return None;
        }
        let native_handle = capture();
        core.windows.remove(plugin_id);
        self.changed.notify_all();
        Some(native_handle)
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
  __PANEL_JSON__
  const windowActionsEnabled = __WINDOW_ACTIONS__;
  const filesOpenEnabled = typeof __FILES_OPEN__ !== 'undefined' && __FILES_OPEN__;
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
  const expiredStorage = deepFreeze({
    get: async () => { throw new Error('ExpiredWindowSessionError'); },
    set: async () => { throw new Error('ExpiredWindowSessionError'); },
    remove: async () => { throw new Error('ExpiredWindowSessionError'); },
  });
  const createTimerSession = (sessionGeneration, documentGeneration) => {
    const session = { sessionGeneration, documentGeneration, updating: false, requestBusy: false, active: true, state: null, stateHandler: null, readToken: 0 };
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
    session.storage = deepFreeze({
      async get(key) {
        return call('plugin_window_storage_get', { key });
      },
      async set(key, value) {
        await call('plugin_window_storage_set', { key, value: deepFreeze(value) });
      },
      async remove(key) {
        await call('plugin_window_storage_remove', { key });
      },
    });
    if (windowActionsEnabled) session.request = async function(input) {
      if (!session.active || timerSession !== session) throw panelJson.error('ExpiredWindowSessionError');
      if (session.updating || session.requestBusy) throw panelJson.error('WindowRequestBusyError');
      let request;
      try {
        if (arguments.length !== 1) throw new Error();
        request = panelJson.snapshot(input, ['action', 'payload']);
        if (!panelJson.validAction(request.action)) throw new Error();
      } catch (_) { throw panelJson.error('InvalidWindowRequestError'); }
      if (!invoke) throw panelJson.error('WindowRequestUnavailableError');
      session.requestBusy = true;
      try {
        const response = await invoke('plugin_window_request', { sessionGeneration, documentGeneration, request });
        if (!session.active || timerSession !== session) throw 'ExpiredWindowSessionError';
        try { return panelJson.reply(response); }
        catch (_) { throw 'InvalidWindowResponseError'; }
      } catch (error) {
        throw panelJson.error(panelJson.windowErrorName(error));
      } finally { session.requestBusy = false; }
    };
    if (filesOpenEnabled) session.files = deepFreeze({
      async openText() {
        if (arguments.length !== 0) throw new TypeError('files.openText takes no arguments');
        const current = () => session.active && timerSession === session;
        const error = name => Object.assign(new Error(name), { name });
        if (!current()) throw error('ExpiredWindowSessionError');
        if (!session.fileReady || session.updating || session.fileBusy) throw error('FileOpenBusyError');
        session.fileBusy = true;
        try {
          const result = await invoke('plugin_window_open_text', { sessionGeneration, documentGeneration });
          if (!current()) throw 'ExpiredWindowSessionError';
          if (result === null) return null;
          if (!result || typeof result !== 'object' || Array.isArray(result)
            || Object.keys(result).sort().join(',') !== 'byteLength,name,text'
            || typeof result.name !== 'string' || !result.name || /[\\/:\x00-\x1f]/.test(result.name)
            || typeof result.text !== 'string' || result.text.includes('\0')
            || !Number.isInteger(result.byteLength) || result.byteLength < 0 || result.byteLength > 8192) throw 'FileOpenFailedError';
          for (const character of result.text) {
            const code = character.charCodeAt(0);
            if (character.length === 1 && code >= 0xd800 && code <= 0xdfff) throw 'FileOpenFailedError';
          }
          const size = new TextEncoder().encode(result.text).length;
          if (result.byteLength !== size && result.byteLength !== size + 3) throw 'FileOpenFailedError';
          return deepFreeze({ name: result.name, text: result.text, byteLength: result.byteLength });
        } catch (failure) {
          const names = ['FileOpenBusyError','ExpiredWindowSessionError','FileOpenPermissionError','FileOpenUnsupportedError','FileOpenTooLargeError','FileOpenEncodingError','FileOpenFailedError'];
          throw error(!current() ? 'ExpiredWindowSessionError' : names.includes(failure) ? failure : 'FileOpenFailedError');
        } finally { session.fileBusy = false; }
      },
    });
    return session;
  };
  const sendReady = async () => {
    if (!invoke || !handler || readySent) return;
    readySent = true;
    await invoke('plugin_window_content_ready');
  };
  const withRequest = (shape) => {
    if (filesOpenEnabled) Object.defineProperty(shape, 'files', { enumerable: true, get: () => timerSession ? timerSession.files : undefined });
    if (windowActionsEnabled) Object.defineProperty(shape, 'request', { enumerable: true, get: () => timerSession ? timerSession.request : undefined });
    return shape;
  };
  const api = deepFreeze(withRequest({
    onUpdate(next) {
      if (handler || typeof next !== 'function') throw new TypeError('one onUpdate handler required');
      handler = next;
      void sendReady();
      return () => { if (handler === next) handler = null; };
    },
    get timer() { return timerSession ? timerSession.facade : expiredTimer; },
    get storage() { return timerSession ? timerSession.storage : expiredStorage; },
    close: async () => {
      if (!invoke) throw new Error('Unavailable');
      await invoke('plugin_window_content_close');
    },
  }));
  Object.defineProperty(window, 'uipilotPluginWindow', { value: api, configurable: false });
  Object.defineProperty(window, '__UIPILOT_PLUGIN_TIMER_PREPARE__', {
    configurable: false,
    value: ({ sessionGeneration, documentGeneration }) => {
      if (!canonicalU64(sessionGeneration) || sessionGeneration === '0') throw new TypeError('invalid session generation');
      if (!canonicalU64(documentGeneration) || documentGeneration === '0') throw new TypeError('invalid document generation');
      if (timerSession) timerSession.active = false;
      timerSession = createTimerSession(sessionGeneration, documentGeneration);
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
      const session = timerSession;
      if (session) { session.updating = true; session.fileReady = false; }
      try {
        await handler(deepFreeze(update));
        await invoke('plugin_window_content_ack', { requestId: update.requestId });
        if (session && session.active && timerSession === session) session.fileReady = true;
      } finally { if (session) session.updating = false; }
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

fn public_content_bootstrap(runtime_actions: bool, files_open: bool) -> String {
    PUBLIC_CONTENT_BOOTSTRAP
        .replace(
            "__PANEL_JSON__",
            crate::public_plugins::panel_action::JSON_BOOTSTRAP,
        )
        .replace("__FILES_OPEN__", if files_open { "true" } else { "false" })
        .replace(
            "__WINDOW_ACTIONS__",
            if runtime_actions { "true" } else { "false" },
        )
}

/// Admission captures the original shell. The permit moves into the blocking closure, so
/// dropping the async waiter cannot admit another native dialog while this worker still runs.
pub(crate) async fn open_text(
    app: AppHandle,
    controller: Arc<PluginWindowController>,
    service: Arc<PublicPluginService>,
    label: String,
    session: u64,
    document: u64,
) -> Result<
    Option<crate::public_plugins::file_open::OpenedText>,
    crate::public_plugins::file_open::FileOpenError,
> {
    use crate::public_plugins::file_open::{self, FileOpenError as E};
    use tauri_plugin_dialog::DialogExt;
    let manager = service
        .manager()
        .map_err(|_| E::FileOpenFailedError)?
        .clone();
    let (guard, shell) = controller.admit_file_open(&label, session, document, |identity| {
        manager.authorize_file_open(&identity.owner)?;
        app.get_window(
            &plugin_shell_label(&identity.owner.plugin_id).ok_or(E::ExpiredWindowSessionError)?,
        )
        .ok_or(E::ExpiredWindowSessionError)
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        let identity = guard.identity.clone();
        let focus_failed = std::cell::Cell::new(false);
        let result = file_open::select_and_read(
            || {
                controller
                    .check_file_open(&identity, || manager.authorize_file_open(&identity.owner))
            },
            || {
                app.dialog()
                    .file()
                    .set_parent(&shell)
                    .set_title("选择文本文件（最多 8 KiB）")
                    .add_filter("文本文件", &["json", "txt", "md", "csv"])
                    .add_filter("所有文件", &["*"])
                    .blocking_pick_file()
                    .map(|selected| {
                        selected
                            .into_path()
                            .map_err(|_| E::FileOpenUnsupportedError)
                    })
                    .transpose()
            },
            file_open::read_selected,
            || {
                let _focus = controller
                    .begin_file_focus(&identity, || manager.authorize_file_open(&identity.owner))?;
                if shell.set_focus().is_err() {
                    focus_failed.set(true);
                    return Err(E::FileOpenFailedError);
                }
                Ok(())
            },
        );
        // Guard remains through focus. Cleanup never looks up a newer shell by its label.
        drop(guard);
        if focus_failed.get() {
            if let Some(_hide) = controller.begin_document_hide_checked(
                &identity.owner.plugin_id,
                identity.document_generation,
                true,
                Some(&identity),
            ) {
                if shell.hide().is_ok() {
                    controller.finish_timer_session_hide(&identity.owner.plugin_id, true);
                    controller.close(&identity.owner.plugin_id);
                } else {
                    controller
                        .revoke_document(&identity.owner.plugin_id, identity.document_generation);
                    let _ = shell.destroy();
                }
            }
        }
        result
    })
    .await
    .map_err(|_| E::FileOpenFailedError)?
}

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

pub(crate) fn content_close(
    app: &AppHandle,
    controller: &PluginWindowController,
    content_label: &str,
) -> Result<(), PublicPluginManagementError> {
    let owner = controller
        .owner_for_content(content_label)
        .ok_or(PublicPluginManagementError::InvalidCaller)?;
    let shell_label =
        plugin_shell_label(&owner.plugin_id).ok_or(PublicPluginManagementError::Unavailable)?;
    close(app, controller, &shell_label)
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
    if !shell_exists || controller.is_current(&owner, PluginWindowPhase::AwaitingReady) {
        if shell_exists {
            destroy_current(app, &owner.plugin_id);
        }
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
        "documentGeneration": transaction.document_generation.to_string(),
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
// WebView2 maps custom protocols to an http subdomain. Compare only the exact host-owned document.
struct WindowDocumentNavigation {
    destination: tauri::Url,
    started: std::sync::atomic::AtomicBool,
}
impl WindowDocumentNavigation {
    fn same_document(url: &tauri::Url, expected: &tauri::Url) -> bool {
        let origin = (url.scheme() == "uipilot-public-plugin"
            && url.host_str() == Some("localhost"))
            || (url.scheme() == "http"
                && url.host_str() == Some("uipilot-public-plugin.localhost"));
        origin
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == expected.path()
            && url.query() == expected.query()
            && url.fragment() == expected.fragment()
    }
    fn allows(&self, url: &tauri::Url) -> bool {
        use std::sync::atomic::Ordering;
        if !self.started.load(Ordering::Acquire)
            && inert_url().is_ok_and(|inert| Self::same_document(url, &inert))
        {
            return true;
        }
        Self::same_document(url, &self.destination) && !self.started.swap(true, Ordering::AcqRel)
    }
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
    .title("UiPilot_tools Plugin")
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
    let navigation_controller = Arc::clone(&controller);
    let navigation_plugin = transaction.owner.plugin_id.clone();
    let document_generation = transaction.document_generation;
    let navigation_shell = shell.clone();
    let navigation = WindowDocumentNavigation {
        destination: content_url.clone(),
        started: std::sync::atomic::AtomicBool::new(false),
    };
    let content = WebviewBuilder::new(
        transaction.content_label.clone(),
        WebviewUrl::CustomProtocol(inert_url),
    )
    .on_navigation(move |url| {
        if navigation.allows(url) {
            return true;
        }
        if navigation_controller.revoke_document(&navigation_plugin, document_generation) {
            let _ = navigation_shell.destroy();
        }
        false
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
        public_content_bootstrap(
            app.state::<Arc<PublicPluginService>>()
                .manager()?
                .window_actions_enabled(&WindowActionIdentity {
                    owner: transaction.owner.clone(),
                    content_label: transaction.content_label.clone(),
                    session_generation: transaction.timer_session_generation,
                    document_generation: transaction.document_generation,
                }),
            app.state::<Arc<PublicPluginService>>()
                .manager()?
                .files_open_enabled(&transaction.owner),
        ),
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
        tauri::WindowEvent::Destroyed => {
            event_controller.revoke_document(&plugin_id, document_generation);
        }
        tauri::WindowEvent::Focused(true) => {
            let observed = event_controller.observe_document_focus(&plugin_id, document_generation, true);
            if !observed { return; }
            eprintln!(
                "[DEBUG-plugin-window-focus] focus-event plugin_id={} focused=true observed={observed} hide_admitted={}",
                plugin_id,
                event_controller.should_hide_on_blur(&plugin_id)
            );
        }
        tauri::WindowEvent::Focused(false) => {
            let observed = event_controller.observe_document_focus(&plugin_id, document_generation, false);
            if !observed { return; }
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
                    let result = hide_document(
                        &blur_controller, &blur_plugin_id, document_generation, true,
                        || blur_shell.hide().is_ok(), || { let _ = blur_shell.destroy(); },
                    );
                    eprintln!("[DEBUG-plugin-window-focus] auto-hide plugin_id={} result={result:?}", blur_plugin_id);
                });
            });
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            let lifecycle = event_app.state::<Arc<lifecycle::LifecycleCoordinator>>();
            if lifecycle.should_prevent_close() {
                api.prevent_close();
                let _ = hide_document(
                    &event_controller, &plugin_id, document_generation, false,
                    || event_shell.hide().is_ok(), || { let _ = event_shell.destroy(); },
                );
            }
        }
        tauri::WindowEvent::Moved(position) if event_controller.owns_plugin(&plugin_id) => {
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

// Callers supply the shell handle captured when this document's native callbacks were created.
// The short lease prevents replacement between admission and effect, without holding core on UI calls.
fn hide_document(
    controller: &Arc<PluginWindowController>,
    plugin_id: &str,
    document_generation: u64,
    only_if_blurred: bool,
    hide: impl FnOnce() -> bool,
    destroy: impl FnOnce(),
) -> Result<bool, PublicPluginManagementError> {
    let Some(_native_effect) =
        controller.begin_document_hide(plugin_id, document_generation, only_if_blurred)
    else {
        return Ok(false);
    };
    if hide() {
        if !controller.finish_timer_session_hide(plugin_id, true) || !controller.close(plugin_id) {
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok(true)
    } else {
        controller.revoke_document(plugin_id, document_generation);
        destroy();
        Err(PublicPluginManagementError::Unavailable)
    }
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

pub(crate) fn destroy_all_for_exit(app: &AppHandle, controller: &PluginWindowController) {
    for plugin_id in controller.active_plugin_ids() {
        let _ = controller.close_for_uninstall(&plugin_id);
        destroy_current(app, &plugin_id);
    }
}
pub(crate) fn teardown(
    app: &AppHandle,
    controller: &PluginWindowController,
    plugin_id: &str,
    generation: u64,
) {
    let Some(label) = plugin_shell_label(plugin_id) else {
        return;
    };
    // Lookup is inside the identity lock; native destruction uses this captured handle outside it.
    if let Some(Some(window)) =
        controller.invalidate_generation_with(plugin_id, generation, || app.get_window(&label))
    {
        let _ = window.destroy();
    }
}
#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread};

    use super::*;
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
    fn file_open_physical_guard_survives_submit_and_close_without_leasing_runtime() {
        use crate::public_plugins::file_open::FileOpenError as E;
        let c = Arc::new(PluginWindowController::default());
        let a = owner("file-open", 1, "a");
        let t = c.submit(a.clone()).unwrap();
        assert!(c.advance(
            &a,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible
        ));
        assert!(c.activate_timer_session(&a));
        for (label, session, doc) in [
            ("main", t.timer_session_generation, t.document_generation),
            (t.content_label.as_str(), 0, t.document_generation),
            (
                t.content_label.as_str(),
                t.timer_session_generation,
                t.document_generation + 1,
            ),
        ] {
            assert!(matches!(
                c.admit_file_open(label, session, doc, |_| Ok(())),
                Err(E::ExpiredWindowSessionError)
            ));
        }
        assert!(matches!(
            c.admit_file_open(
                &t.content_label,
                t.timer_session_generation,
                t.document_generation,
                |_| Err::<(), _>(E::FileOpenPermissionError)
            ),
            Err(E::FileOpenPermissionError)
        ));
        let (guard, ()) = c
            .admit_file_open(
                &t.content_label,
                t.timer_session_generation,
                t.document_generation,
                |_| Ok(()),
            )
            .unwrap();
        let original_identity = guard.identity.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let picker = thread::spawn(move || {
            let held = guard;
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(held);
        });
        started_rx.recv().unwrap();
        assert!(
            c.core.try_lock().is_ok(),
            "blocked physical picker never holds controller mutex"
        );
        assert!(!c.should_hide_on_blur(&a.plugin_id));
        assert!(c
            .begin_document_hide(&a.plugin_id, t.document_generation, true)
            .is_none());
        assert_eq!(
            c.core.lock().unwrap().windows[&a.plugin_id]
                .timer_session
                .in_flight,
            0
        );
        let other = Arc::new(PluginWindowController::default());
        let other_t = other.submit(a.clone()).unwrap();
        other.advance(
            &a,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible,
        );
        other.activate_timer_session(&a);
        assert!(matches!(
            other.admit_file_open(
                &other_t.content_label,
                other_t.timer_session_generation,
                other_t.document_generation,
                |_| Ok(())
            ),
            Err(E::FileOpenBusyError)
        ));
        let b = owner("replacement", 1, "b");
        let next = c.submit(b.clone()).unwrap();
        assert_eq!(next.document_generation, t.document_generation);
        assert!(matches!(
            c.check_file_open(&original_identity, || Ok(())),
            Err(E::ExpiredWindowSessionError)
        ));
        c.advance(
            &b,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::Visible,
        );
        c.activate_timer_session(&b);
        assert!(!c.should_hide_on_blur(&b.plugin_id));
        assert!(c
            .begin_document_hide(&b.plugin_id, t.document_generation, true)
            .is_none());
        assert!(c.begin_timer_session_close(&b.plugin_id));
        assert!(c.finish_timer_session_hide(&b.plugin_id, true));
        assert!(c.revoke_document(&b.plugin_id, t.document_generation));
        release_tx.send(()).unwrap();
        picker.join().unwrap();
        assert_eq!(
            c.core.lock().unwrap().windows[&b.plugin_id]
                .timer_session
                .phase,
            TimerSessionPhase::Revoked
        );
        let (other_guard, ()) = other
            .admit_file_open(
                &other_t.content_label,
                other_t.timer_session_generation,
                other_t.document_generation,
                |_| Ok(()),
            )
            .unwrap();
        assert!(!other.should_hide_on_blur(&a.plugin_id));
        drop(other_guard);
        assert!(other.should_hide_on_blur(&a.plugin_id));
    }

    #[test]
    fn window_action_native_navigation_gate_allows_only_initial_exact_document() {
        for origin in [
            "uipilot-public-plugin://localhost",
            "http://uipilot-public-plugin.localhost",
        ] {
            let navigation = WindowDocumentNavigation {
                destination: tauri::Url::parse(
                    "uipilot-public-plugin://localhost/dist/window.html",
                )
                .unwrap(),
                started: std::sync::atomic::AtomicBool::new(false),
            };
            assert!(navigation
                .allows(&tauri::Url::parse(&format!("{origin}/__uipilot_inert.html")).unwrap()));
            assert!(navigation
                .allows(&tauri::Url::parse(&format!("{origin}/dist/window.html")).unwrap()));
            assert!(
                !navigation
                    .allows(&tauri::Url::parse(&format!("{origin}/dist/window.html")).unwrap()),
                "reload invalidates the document"
            );
            for bad in [
                "https://example.test/dist/window.html",
                "http://localhost/dist/window.html",
                "http://uipilot-public-plugin.localhost/dist/other.html",
            ] {
                assert!(!navigation.allows(&tauri::Url::parse(bad).unwrap()));
            }
        }
        let source: String = include_str!("plugin_window.rs")
            .split("mod tests {")
            .next()
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(source.contains(".on_navigation(move|url|{ifnavigation.allows(url){returntrue;}"));
        assert!(source.contains(
            "navigation_controller.revoke_document(&navigation_plugin,document_generation)"
        ));
        assert!(source.contains("tauri::WindowEvent::Destroyed=>{event_controller.revoke_document(&plugin_id,document_generation)"));
    }

    #[test]
    fn window_action_rebuilding_unready_content_rotates_document_and_ignores_old_destroy() {
        let controller = PluginWindowController::default();
        let first = owner("unready-a", 1, "request-a");
        let initial = controller.submit(first.clone()).unwrap();
        let next = owner("unready-b", 1, "request-b");
        let replacement = controller.submit(next.clone()).unwrap();
        assert_ne!(initial.document_generation, replacement.document_generation);
        assert!(!controller.revoke_document(&first.plugin_id, initial.document_generation));
        assert!(controller.advance(
            &next,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible
        ));
        assert!(controller.activate_timer_session(&next));
    }

    #[test]
    fn window_action_review_stale_native_focus_does_not_mutate_replacement() {
        let controller = PluginWindowController::default();
        let initial = controller.submit(owner("old-native", 1, "old")).unwrap();
        let next = owner("new-native", 2, "new");
        let replacement = controller.submit(next.clone()).unwrap();
        assert!(controller.advance(
            &next,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible
        ));
        assert!(controller.activate_timer_session(&next));
        assert!(controller.observe_document_focus(
            &next.plugin_id,
            replacement.document_generation,
            true
        ));
        assert!(!controller.observe_document_focus(
            &next.plugin_id,
            initial.document_generation,
            false
        ));
        assert!(controller.has_focus(&next.plugin_id));
        assert!(controller.is_current(&next, PluginWindowPhase::Visible));
    }

    #[test]
    fn window_action_review_cleanup_captures_old_native_handle_before_replacement() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let controller = PluginWindowController::default();
        let old = owner("cleanup-old", 1, "old");
        controller.submit(old.clone()).unwrap();
        let old_native = Arc::new(AtomicUsize::new(0));
        let new_native = Arc::new(AtomicUsize::new(0));
        let native_by_label = Mutex::new(old_native.clone());
        let captured = controller
            .invalidate_generation_with(&old.plugin_id, 1, || {
                assert!(
                    controller.core.try_lock().is_err(),
                    "identity validation and handle capture share one lock"
                );
                native_by_label.lock().unwrap().clone()
            })
            .unwrap();
        assert!(
            controller.core.try_lock().is_ok(),
            "native effects run with the core lock released"
        );
        // Retirement -> replacement -> native effect is a deterministic interleaving.
        let replacement = owner("cleanup-new", 2, "new");
        controller.submit(replacement.clone()).unwrap();
        *native_by_label.lock().unwrap() = new_native.clone();
        captured.fetch_add(1, Ordering::SeqCst);
        assert_eq!(old_native.load(Ordering::SeqCst), 1);
        assert_eq!(new_native.load(Ordering::SeqCst), 0);
        assert!(controller.is_current(&replacement, PluginWindowPhase::AwaitingReady));
        assert!(controller
            .invalidate_generation_with(&replacement.plugin_id, 1, || panic!(
                "stale cleanup must not capture replacement"
            ))
            .is_none());
    }

    #[test]
    fn window_action_review_blur_and_close_ignore_replaced_native_document() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let controller = Arc::new(PluginWindowController::default());
        let old = controller.submit(owner("old-event", 1, "old")).unwrap();
        let next = owner("replacement-event", 2, "new");
        let replacement = controller.submit(next.clone()).unwrap();
        assert!(controller.advance(
            &next,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible
        ));
        assert!(controller.activate_timer_session(&next));
        let native_calls = AtomicUsize::new(0);
        // This is the delayed callback's actual execution point, after replacement won.
        assert!(!hide_document(
            &controller,
            &next.plugin_id,
            old.document_generation,
            true,
            || {
                native_calls.fetch_add(1, Ordering::SeqCst);
                true
            },
            || {
                native_calls.fetch_add(1, Ordering::SeqCst);
            }
        )
        .unwrap());
        assert!(controller.set_pinned(&next.plugin_id, true));
        assert!(!hide_document(
            &controller,
            &next.plugin_id,
            old.document_generation,
            false,
            || {
                native_calls.fetch_add(1, Ordering::SeqCst);
                true
            },
            || {
                native_calls.fetch_add(1, Ordering::SeqCst);
            }
        )
        .unwrap());
        assert_eq!(native_calls.load(Ordering::SeqCst), 0);
        let core = controller.core.lock().unwrap();
        let window = &core.windows[&next.plugin_id];
        assert!(window.pinned);
        assert_eq!(window.timer_session.phase, TimerSessionPhase::Active);
        assert_eq!(window.document_generation, replacement.document_generation);
        assert_eq!(window.owner, next);
    }

    #[test]
    fn window_action_review_blur_reused_document_owns_latest_session_and_native_lease() {
        let controller = Arc::new(PluginWindowController::default());
        let first = owner("reuse-old", 1, "old");
        let initial = controller.submit(first.clone()).unwrap();
        assert!(controller.advance(
            &first,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible
        ));
        assert!(controller.activate_timer_session(&first));
        let latest = owner("reuse-new", 1, "new");
        let reused = controller.submit(latest.clone()).unwrap();
        assert_eq!(initial.document_generation, reused.document_generation);
        assert!(controller.advance(
            &latest,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::Visible
        ));
        assert!(controller.activate_timer_session(&latest));
        assert!(controller.observe_document_focus(
            &latest.plugin_id,
            initial.document_generation,
            false
        ));
        assert!(hide_document(
            &controller,
            &latest.plugin_id,
            initial.document_generation,
            true,
            || {
                {
                    let core = controller
                        .core
                        .try_lock()
                        .expect("native effect must not hold core lock");
                    let window = &core.windows[&latest.plugin_id];
                    assert_eq!(window.owner, latest);
                    assert_eq!(
                        window.timer_session.in_flight, 1,
                        "short native-effect lease blocks submit through final revocation"
                    );
                    assert_eq!(window.timer_session.phase, TimerSessionPhase::Closing);
                }
                // hide may synchronously emit native callbacks. They must reject instead of waiting
                // on their own native-effect lease on the UI thread.
                assert!(controller
                    .begin_document_hide(&latest.plugin_id, initial.document_generation, false)
                    .is_none());
                true
            },
            || panic!("successful native hide must not destroy")
        )
        .unwrap());
        let core = controller.core.lock().unwrap();
        let window = &core.windows[&latest.plugin_id];
        assert_eq!(window.owner, latest);
        assert_eq!(window.timer_session.phase, TimerSessionPhase::Revoked);
        assert_eq!(window.timer_session.in_flight, 0);
    }

    #[test]
    fn window_action_review_blur_failure_revokes_document_without_reentrant_wait() {
        let controller = Arc::new(PluginWindowController::default());
        let first = owner("failure", 1, "old");
        let initial = controller.submit(first.clone()).unwrap();
        assert!(controller.advance(
            &first,
            PluginWindowPhase::AwaitingReady,
            PluginWindowPhase::Visible
        ));
        assert!(controller.activate_timer_session(&first));
        assert!(hide_document(
            &controller,
            &first.plugin_id,
            initial.document_generation,
            true,
            || false,
            || {
                assert!(controller.core.try_lock().is_ok());
                assert!(controller.revoke_document(&first.plugin_id, initial.document_generation));
                assert!(controller
                    .begin_document_hide(&first.plugin_id, initial.document_generation, false)
                    .is_none());
            }
        )
        .is_err());
        let replacement = controller
            .submit(owner("failure-reopened", 1, "new"))
            .unwrap();
        assert_ne!(replacement.document_generation, initial.document_generation);
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
    fn window_session_is_read_only_until_visible_and_old_generations_expire() {
        let controller = Arc::new(PluginWindowController::default());
        let first = owner("timer-a", 3, "request-a");
        let transaction = controller.submit(first.clone()).unwrap();
        assert_eq!(transaction.timer_session_generation, 1);

        let read = controller
            .begin_window_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                false,
            )
            .unwrap();
        assert_eq!(read.owner(), &first);
        drop(read);
        assert!(matches!(
            controller.begin_window_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                true,
            ),
            Err(PluginWindowCallError::ExpiredWindowSession)
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
                .begin_window_call(
                    &transaction.content_label,
                    transaction.timer_session_generation,
                    true,
                )
                .unwrap(),
        );

        assert!(controller.begin_timer_session_close(&first.plugin_id));
        assert!(matches!(
            controller.begin_window_call(
                &transaction.content_label,
                transaction.timer_session_generation,
                false,
            ),
            Err(PluginWindowCallError::ExpiredWindowSession)
        ));
        assert!(controller.finish_timer_session_hide(&first.plugin_id, true));

        let second = owner("timer-b", 3, "request-b");
        let next = controller.submit(second).unwrap();
        assert!(next.timer_session_generation > transaction.timer_session_generation);
        assert!(matches!(
            controller.begin_window_call(
                &next.content_label,
                transaction.timer_session_generation,
                false,
            ),
            Err(PluginWindowCallError::ExpiredWindowSession)
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
            .begin_window_call(
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
            .begin_window_call(
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
    fn bootstrap_owns_session_bound_timer_and_storage_facades_without_general_events() {
        for fragment in [
            "get timer()",
            "get storage()",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
            "plugin_window_storage_get",
            "plugin_window_storage_set",
            "plugin_window_storage_remove",
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
        assert!(PUBLIC_CONTENT_BOOTSTRAP.contains("session.storage = deepFreeze({"));
        assert!(PUBLIC_CONTENT_BOOTSTRAP.contains("const api = deepFreeze(withRequest({"));
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
            .split("\npub(crate) fn close(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn teardown_current(").next())
            .expect("plugin window close function is missing");
        assert!(close.contains("app: &AppHandle"));
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
            "plugin_window_content_close",
            "close: async () =>",
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
            "public_content_bootstrap(",
            ".on_navigation(",
            ".on_new_window(|_, _| NewWindowResponse::Deny)",
            ".on_download(|_, _| false)",
            ".add_child(",
            "should_prevent_close()",
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
        let handler: String = source
            .split("shell.on_window_event(move |event|")
            .nth(1)
            .and_then(|tail| tail.split("    Ok(())").next())
            .expect("native event handler is missing")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(handler.contains(
            "event_controller.observe_document_focus(&plugin_id,document_generation,true)"
        ));
        assert!(handler.contains(
            "event_controller.observe_document_focus(&plugin_id,document_generation,false)"
        ));
        let deferred = handler
            .find("std::thread::spawn")
            .expect("blur must leave the event callback");
        let wait = handler
            .find("std::thread::sleep(PLUGIN_BLUR_RECHECK_DELAY)")
            .expect("transient child focus must settle");
        let hide = handler
            .find("hide_document(")
            .expect("document-bound native hide is missing");
        assert!(deferred < wait && wait < hide);
        assert!(handler.contains("&blur_controller,&blur_plugin_id,document_generation,true"));
        assert!(handler.contains("&event_controller,&plugin_id,document_generation,false"));
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
