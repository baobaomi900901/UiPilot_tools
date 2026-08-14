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
    public_plugins::{PluginInvocationTheme, PublicPluginManagementError},
    settings::SettingsStore,
    window_transfer::{MainWindowSnapshot, MainWindowTransferCoordinator, TransferTarget},
};

const CONTENT_READY_TIMEOUT: Duration = Duration::from_secs(5);
const CONTENT_ACK_TIMEOUT: Duration = Duration::from_secs(5);
const PLUGIN_WINDOW_WIDTH: f64 = 520.0;
const PLUGIN_WINDOW_HEIGHT: f64 = 360.0;
const PLUGIN_SHELL_HEIGHT: f64 = 44.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginWindowOwner {
    pub(crate) ui_intent_epoch: u64,
    pub(crate) submission_token: String,
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
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
}

#[derive(Default)]
pub(crate) struct PluginWindowController {
    core: Mutex<ControllerCore>,
    changed: Condvar,
}

#[derive(Default)]
struct ControllerCore {
    windows: HashMap<String, WindowState>,
}

struct WindowState {
    generation: u64,
    pinned: bool,
    phase: PluginWindowPhase,
    owner: PluginWindowOwner,
}

impl PluginWindowController {
    pub(crate) fn submit(&self, owner: PluginWindowOwner) -> Option<PluginWindowTransaction> {
        let shell_label = plugin_shell_label(&owner.plugin_id)?;
        let content_label = plugin_content_label(&owner.plugin_id)?;
        let mut core = self.core.lock().ok()?;
        let existing = core
            .windows
            .get(&owner.plugin_id)
            .filter(|window| window.generation == owner.plugin_generation);
        let pinned = existing.is_some_and(|window| window.pinned);
        let phase = existing
            .filter(|window| window.phase != PluginWindowPhase::AwaitingReady)
            .map(|_| PluginWindowPhase::AwaitingAck)
            .unwrap_or(PluginWindowPhase::AwaitingReady);
        core.windows.insert(
            owner.plugin_id.clone(),
            WindowState {
                generation: owner.plugin_generation,
                pinned,
                phase,
                owner: owner.clone(),
            },
        );
        self.changed.notify_all();
        Some(PluginWindowTransaction {
            owner,
            shell_label,
            content_label,
            instance_number: 1,
        })
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
            .and_then(|core| core.windows.get(plugin_id).map(|window| !window.pinned))
            .unwrap_or(false)
    }

    pub(crate) fn close(&self, plugin_id: &str) -> bool {
        self.set_pinned(plugin_id, false)
    }

    pub(crate) fn remove_plugin(&self, plugin_id: &str) -> bool {
        let removed = self
            .core
            .lock()
            .ok()
            .and_then(|mut core| core.windows.remove(plugin_id))
            .is_some();
        if removed {
            self.changed.notify_all();
        }
        removed
    }
    pub(crate) fn invalidate_generation(&self, plugin_id: &str, generation: u64) -> bool {
        let Ok(mut core) = self.core.lock() else {
            return false;
        };
        if core
            .windows
            .get(plugin_id)
            .is_some_and(|window| window.generation == generation)
        {
            core.windows.remove(plugin_id);
            true
        } else {
            false
        }
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
  const deepFreeze = (value, seen = new WeakSet()) => {
    if ((typeof value !== 'object' && typeof value !== 'function') || value === null || seen.has(value)) return value;
    seen.add(value);
    for (const key of Reflect.ownKeys(value)) deepFreeze(value[key], seen);
    return Object.freeze(value);
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
  });
  Object.defineProperty(window, 'uipilotPluginWindow', { value: api, configurable: false });
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
        return false;
    };
    controller.advance(
        &owner,
        PluginWindowPhase::AwaitingReady,
        PluginWindowPhase::AwaitingAck,
    ) || controller.is_current(&owner, PluginWindowPhase::AwaitingAck)
}

pub(crate) fn content_ack(
    controller: &PluginWindowController,
    label: &str,
    request_id: &str,
) -> bool {
    let Some(owner) = controller.owner_for_content(label) else {
        return false;
    };
    owner.request_id == request_id
        && controller.advance(
            &owner,
            PluginWindowPhase::AwaitingAck,
            PluginWindowPhase::AwaitingFocus,
        )
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
    if app.get_webview_window(&transaction.shell_label).is_none() {
        create_window(app, Arc::clone(&controller), &transaction, window_entry)?;
    }
    if controller.is_current(&owner, PluginWindowPhase::AwaitingReady)
        && !controller.wait_for(
            &owner,
            PluginWindowPhase::AwaitingAck,
            CONTENT_READY_TIMEOUT,
        )
    {
        teardown(app, &controller, &owner.plugin_id, owner.plugin_generation);
        return Err(PublicPluginManagementError::RuntimeNotReady);
    }
    if !controller.is_current(&owner, PluginWindowPhase::AwaitingAck) {
        return Err(PublicPluginManagementError::Unavailable);
    }
    let shell = app
        .get_webview_window(&transaction.shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let theme = match update.theme {
        PluginInvocationTheme::Dark => "dark",
        PluginInvocationTheme::Light => "light",
    };
    shell
        .eval(format!(
            "document.documentElement.dataset.colorScheme={};",
            serde_json::to_string(theme).expect("static theme serializes")
        ))
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    let content = app
        .get_webview(&transaction.content_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
    let payload =
        serde_json::to_string(&update).map_err(|_| PublicPluginManagementError::Unavailable)?;
    content
        .eval(format!(
            "window.__UIPILOT_PLUGIN_WINDOW_UPDATE__({payload});"
        ))
        .map_err(|_| PublicPluginManagementError::Unavailable)?;
    if !controller.wait_for(
        &owner,
        PluginWindowPhase::AwaitingFocus,
        CONTENT_ACK_TIMEOUT,
    ) || !controller.advance(
        &owner,
        PluginWindowPhase::AwaitingFocus,
        PluginWindowPhase::AwaitingCommit,
    ) {
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
    let content = WebviewBuilder::new(
        transaction.content_label.clone(),
        WebviewUrl::CustomProtocol(content_url),
    )
    .initialization_script(PUBLIC_CONTENT_BOOTSTRAP)
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
    native
        .add_child(
            content,
            LogicalPosition::new(0.0, PLUGIN_SHELL_HEIGHT),
            LogicalSize::new(
                PLUGIN_WINDOW_WIDTH,
                PLUGIN_WINDOW_HEIGHT - PLUGIN_SHELL_HEIGHT,
            ),
        )
        .map_err(|_| PublicPluginManagementError::Unavailable)?;

    let plugin_id = transaction.owner.plugin_id.clone();
    let event_controller = Arc::clone(&controller);
    let event_shell = shell.clone();
    let event_app = app.clone();
    shell.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(false) if event_controller.should_hide_on_blur(&plugin_id) => {
            let _ = event_shell.hide();
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            event_controller.close(&plugin_id);
            let _ = event_shell.hide();
        }
        tauri::WindowEvent::Moved(position) => {
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
    let shell = app
        .get_webview_window(&shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?;
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
        if !still_current() {
            return Err(());
        }
        shell.set_focus().map_err(|_| ())?;
        if !still_current() || !shell.is_focused().map_err(|_| ())? {
            return Err(());
        }
        main.hide().map_err(|_| ())?;
        if !still_current() {
            return Err(());
        }
        Ok(())
    })();
    if native.is_err() {
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
    if !controller.advance(
        &owner,
        PluginWindowPhase::AwaitingCommit,
        PluginWindowPhase::Visible,
    ) || !transfers.commit(&lease)
    {
        let _ = shell.hide();
        return Err(PublicPluginManagementError::Unavailable);
    }
    Ok(())
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
        .get_webview_window(shell_label)
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
    if !controller.close(&owner.plugin_id) {
        return Err(PublicPluginManagementError::Unavailable);
    }
    app.get_webview_window(shell_label)
        .ok_or(PublicPluginManagementError::Unavailable)?
        .hide()
        .map_err(|_| PublicPluginManagementError::Unavailable)
}

pub(crate) fn teardown_current(
    app: &AppHandle,
    controller: &PluginWindowController,
    plugin_id: &str,
) {
    if controller.remove_plugin(plugin_id) {
        if let Some(label) = plugin_shell_label(plugin_id) {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.destroy();
            }
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
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.destroy();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn owner(token: &str, generation: u64, request: &str) -> PluginWindowOwner {
        PluginWindowOwner {
            ui_intent_epoch: generation,
            submission_token: token.into(),
            plugin_id: "com.example.demo".into(),
            plugin_generation: generation,
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
    fn pin_close_and_generation_invalidation_are_host_owned() {
        let controller = PluginWindowController::default();
        let current = owner("current", 4, "request-current");
        controller.submit(current.clone()).unwrap();
        assert!(controller.set_pinned(&current.plugin_id, true));
        assert!(!controller.should_hide_on_blur(&current.plugin_id));
        assert!(controller.close(&current.plugin_id));
        assert!(controller.should_hide_on_blur(&current.plugin_id));
        assert!(controller.invalidate_generation(&current.plugin_id, 4));
        assert!(!controller.is_current(&current, PluginWindowPhase::AwaitingReady));
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
        for required in [
            "WebviewWindowBuilder::new(",
            "WebviewUrl::App(\"index.html\".into())",
            ".visible(false)",
            ".decorations(false)",
            ".always_on_top(false)",
            "WebviewBuilder::new(",
            "WebviewUrl::CustomProtocol(content_url)",
            ".initialization_script(PUBLIC_CONTENT_BOOTSTRAP)",
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
        for forbidden in [
            "NewWindowResponse::Allow",
            "WebviewUrl::External",
            "file://",
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
    fn shell_and_content_labels_are_disjoint_and_plugin_owned() {
        let shell = plugin_shell_label("com.example.demo").unwrap();
        let content = plugin_content_label("com.example.demo").unwrap();
        assert!(shell.starts_with("plugin-shell-"));
        assert!(content.starts_with("plugin-content-"));
        assert_ne!(shell, content);
        assert!(!shell.contains("com.example.demo"));
    }
}
