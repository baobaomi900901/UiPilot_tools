use crate::{
    command_hotkeys::CommandEntry,
    lifecycle::{LifecycleCoordinator, ShowTarget},
    message_center::{
        MessageCenterService, MessagePostGuardEffect, MessagePublishOutcome, MessagePublishRequest,
        MessagePublisher, MessageSummary,
    },
    public_plugins::PublicPluginService,
    settings::SettingsStore,
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
const PLUGIN: &str = "com.uipilot.translate";
const COMMAND: &str = "default";
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionTranslation {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationSelectionSettings {
    pub(crate) hotkey: Option<String>,
    pub(crate) status: &'static str,
}
fn selection_chord_conflicts(settings: &crate::settings::Settings, chord: &str) -> bool {
    let mut other = settings.clone();
    other.translation_selection_hotkey = None;
    crate::command_hotkeys::launcher_conflicts(&other, chord)
        || crate::hotkey::HotkeyKind::parse(&other.hotkey).is_ok_and(|h| h.canonical() == chord)
}
fn capture_if_authorized(
    mut authorized: impl FnMut() -> bool,
    capture: impl FnOnce() -> Result<String, crate::selection_capture::SelectionCaptureError>,
) -> Option<Result<String, crate::selection_capture::SelectionCaptureError>> {
    if !authorized() {
        return None;
    }
    let result = capture();
    authorized().then_some(result)
}
fn deliver_if_authorized(authorized: impl FnOnce() -> bool, deliver: impl FnOnce()) {
    if authorized() {
        deliver();
    }
}
fn recovery_failure_effect(
    publisher: &impl MessagePublisher,
    result: &Result<String, crate::selection_capture::SelectionCaptureError>,
) -> Option<MessagePostGuardEffect> {
    let error = result.as_ref().err().copied()?;
    if !error.recovery_failure() {
        return None;
    }
    let outcome = publisher.commit_publish(MessagePublishRequest {
        plugin_id: "uipilot.host.selection".into(),
        plugin_name_snapshot: "划词翻译".into(),
        // The failure text is static. Captured text, clipboard formats, target
        // identity and plugin-controlled content never enter this host record.
        content: error.message().into(),
    });
    match outcome {
        MessagePublishOutcome::Published(message) => {
            Some(MessagePostGuardEffect::Ready(MessageSummary {
                revision: message.revision,
                unread_count: message.unread_count,
            }))
        }
        _ => None,
    }
}
impl LifecycleCoordinator {
    pub(crate) fn cancel_selection_capture(&self) {
        self.selection_cancel
            .lock()
            .unwrap()
            .store(true, Ordering::Release);
    }
    fn selection_authority(&self, app: &AppHandle) -> Option<u64> {
        app.state::<Arc<PublicPluginService>>()
            .manager()
            .ok()?
            .selection_translation_authority()
    }
    pub(crate) fn selection_entry_current(&self, app: &AppHandle, entry: &CommandEntry) -> bool {
        entry.plugin_id == PLUGIN
            && entry.command_id == COMMAND
            && self.selection_hotkeys.matches(entry)
            && app
                .state::<SettingsStore>()
                .snapshot()
                .translation_selection_hotkey
                .as_deref()
                .and_then(|s| crate::hotkey::command_chord(s).ok())
                .map(|s| s.to_string())
                .as_ref()
                == Some(&entry.hotkey)
            && self.selection_authority(app).is_some()
    }
    pub(crate) fn translation_selection_settings(
        &self,
        app: &AppHandle,
    ) -> TranslationSelectionSettings {
        let hotkey = app
            .state::<SettingsStore>()
            .snapshot()
            .translation_selection_hotkey;
        let canonical = hotkey
            .as_deref()
            .and_then(|s| crate::hotkey::command_chord(s).ok())
            .map(|s| s.to_string());
        let status = self.selection_hotkeys.status(
            PLUGIN,
            COMMAND,
            canonical.as_deref(),
            self.selection_authority(app).is_some(),
        );
        TranslationSelectionSettings { hotkey, status }
    }
    pub(crate) fn reconcile_selection_hotkey(self: &Arc<Self>, app: &AppHandle) -> Result<(), ()> {
        let _critical = self.reserve_critical().map_err(|_| ())?;
        let _operation = self.lock_hotkey_operation()?;
        self.cancel_selection_capture();
        let saved = app
            .state::<SettingsStore>()
            .snapshot()
            .translation_selection_hotkey;
        let enabled = self.selection_authority(app).is_some();
        let replacement = saved
            .as_deref()
            .and_then(|s| crate::hotkey::command_chord(s).ok());
        if self
            .selection_hotkeys
            .entries()
            .iter()
            .any(|b| b.confirmed && Some(b.shortcut) == replacement)
            && enabled
            && !replacement.is_some_and(|s| {
                selection_chord_conflicts(&app.state::<SettingsStore>().snapshot(), &s.to_string())
            })
        {
            return Ok(());
        }
        let conflicts = replacement.is_some_and(|s| {
            selection_chord_conflicts(&app.state::<SettingsStore>().snapshot(), &s.to_string())
        });
        let enabled = enabled && !conflicts;
        let native = app.global_shortcut();
        self.selection_hotkeys.edit(
            PLUGIN,
            COMMAND,
            replacement,
            enabled,
            &mut |s| native.register(s).map_err(|_| ()),
            &mut |s| native.unregister(s).map_err(|_| ()),
            || Ok(()),
        )
    }
    pub(crate) fn edit_selection_hotkey(
        self: &Arc<Self>,
        app: &AppHandle,
        hotkey: Option<String>,
    ) -> Result<TranslationSelectionSettings, ()> {
        let replacement = hotkey
            .as_deref()
            .map(crate::hotkey::command_chord)
            .transpose()
            .map_err(|_| ())?;
        let _critical = self.reserve_critical().map_err(|_| ())?;
        let _operation = self.lock_hotkey_operation()?;
        let store = app.state::<SettingsStore>();
        let mut snapshot = store.snapshot();
        snapshot.translation_selection_hotkey = None;
        if replacement.is_some_and(|s| {
            crate::command_hotkeys::launcher_conflicts(&snapshot, &s.to_string())
                || crate::hotkey::HotkeyKind::parse(&snapshot.hotkey)
                    .is_ok_and(|h| h.canonical() == s.to_string())
        }) {
            return Err(());
        }
        self.cancel_selection_capture();
        let enabled = self.selection_authority(app).is_some();
        let native = app.global_shortcut();
        self.selection_hotkeys.edit(
            PLUGIN,
            COMMAND,
            replacement,
            enabled,
            &mut |s| native.register(s).map_err(|_| ()),
            &mut |s| native.unregister(s).map_err(|_| ()),
            || {
                store
                    .set_translation_selection_hotkey(replacement.map(|s| s.to_string()))
                    .map_err(|_| ())
            },
        )?;
        Ok(self.translation_selection_settings(app))
    }
    pub(crate) fn dispatch_selection_hotkey(
        self: &Arc<Self>,
        app: &AppHandle,
        shortcut: &Shortcut,
        target: Result<
            crate::selection_capture::CaptureTarget,
            crate::selection_capture::SelectionCaptureError,
        >,
    ) -> bool {
        let Some(entry) = self.selection_hotkeys.candidate(shortcut.id()) else {
            return false;
        };
        if !self.selection_entry_current(app, &entry) {
            return true;
        }
        let Some(authority) = self.selection_authority(app) else {
            return true;
        };
        let Ok(history) = app
            .state::<Arc<PublicPluginService>>()
            .manager()
            .map(|manager| manager.clipboard_history_for_selection())
        else {
            return true;
        };
        self.cancel_selection_capture();
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.selection_cancel.lock().unwrap() = Arc::clone(&cancelled);
        let original_target = target.as_ref().ok().copied();
        let app = app.clone();
        let messages = Arc::clone(app.state::<Arc<MessageCenterService>>().inner());
        let recovery_app = app.clone();
        let recovery_reporter: crate::selection_capture::RecoveryReporter =
            Arc::new(move |error| {
                // This callback outlives a timed-out receiver and carries only an
                // error enum. Ready updates the unread state without native attention.
                let effect = recovery_failure_effect(messages.as_ref(), &Err(error));
                messages.dispatch_post_guard(&recovery_app, effect);
            });
        let coordinator = Arc::clone(self);
        std::thread::spawn(move || {
            let Some(result) = capture_if_authorized(
                || {
                    !cancelled.load(Ordering::Acquire)
                        && coordinator.selection_entry_current(&app, &entry)
                        && coordinator.selection_authority(&app) == Some(authority)
                },
                || {
                    target.and_then(|t| {
                        crate::selection_capture::capture_selection_for(
                            t,
                            Arc::clone(&cancelled),
                            Arc::clone(&history),
                            Arc::clone(&recovery_reporter),
                        )
                    })
                },
            ) else {
                return;
            };
            let dispatcher = app.clone();
            let _ = dispatcher.run_on_main_thread(move || {
                deliver_if_authorized(
                    || {
                        !cancelled.load(Ordering::Acquire)
                            && coordinator.selection_entry_current(&app, &entry)
                            && coordinator.selection_authority(&app) == Some(authority)
                            && original_target.is_none_or(|target| {
                                crate::selection_capture::target_is_current(target)
                            })
                    },
                    || {
                        static NEXT: std::sync::atomic::AtomicU64 =
                            std::sync::atomic::AtomicU64::new(1);
                        let id = format!("selection-{}", NEXT.fetch_add(1, Ordering::Relaxed));
                        let (text, error) = match result {
                            Ok(text) => (Some(text), None),
                            Err(e) => (None, Some(e.message().to_string())),
                        };
                        let _ = coordinator.show_main_selection_entry(
                            &app,
                            ShowTarget::Launcher,
                            Some(entry.clone()),
                            Some(SelectionTranslation { id, text, error }),
                            None,
                        );
                    },
                );
            });
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct RecoveryPublisher {
        requests: std::sync::Mutex<Vec<MessagePublishRequest>>,
        fail: bool,
    }
    impl MessagePublisher for RecoveryPublisher {
        fn is_available(&self) -> bool {
            !self.fail
        }
        fn commit_publish(&self, request: MessagePublishRequest) -> MessagePublishOutcome {
            if self.fail {
                return MessagePublishOutcome::Unavailable;
            }
            self.requests.lock().unwrap().push(request.clone());
            MessagePublishOutcome::Published(crate::message_center::MessagePublished {
                id: "diagnostic".into(),
                plugin_id: request.plugin_id,
                plugin_name_snapshot: request.plugin_name_snapshot,
                created_at: "now".into(),
                content: request.content,
                revision: "4".into(),
                unread_count: 2,
            })
        }
    }
    #[test]
    fn recovery_failure_is_persisted_quietly_before_revoked_capture_is_discarded() {
        use crate::selection_capture::SelectionCaptureError;
        use std::cell::{Cell, RefCell};
        for error in [
            SelectionCaptureError::ClipboardRestore,
            SelectionCaptureError::HistoryCleanup,
        ] {
            let active = Cell::new(true);
            let publisher = RecoveryPublisher::default();
            let effect = RefCell::new(None);
            let result = capture_if_authorized(
                || active.get(),
                || {
                    active.set(false);
                    let result = Err(error);
                    *effect.borrow_mut() = recovery_failure_effect(&publisher, &result);
                    result
                },
            );
            assert!(result.is_none());
            let records = publisher.requests.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].plugin_id, "uipilot.host.selection");
            assert_eq!(records[0].plugin_name_snapshot, "划词翻译");
            assert_eq!(records[0].content, error.message());
            assert_eq!(
                *effect.borrow(),
                Some(MessagePostGuardEffect::Ready(MessageSummary {
                    revision: "4".into(),
                    unread_count: 2,
                }))
            );
            let delivered = Cell::new(false);
            deliver_if_authorized(|| false, || delivered.set(true));
            assert!(!delivered.get());
        }
    }
    #[test]
    fn ordinary_capture_results_never_publish_a_diagnostic_or_private_text() {
        use crate::selection_capture::SelectionCaptureError;
        let publisher = RecoveryPublisher::default();
        for result in [
            Ok("private selected text".into()),
            Err(SelectionCaptureError::Cancelled),
            Err(SelectionCaptureError::Unsupported),
            Err(SelectionCaptureError::ClipboardBackup),
        ] {
            assert_eq!(recovery_failure_effect(&publisher, &result), None);
        }
        assert!(publisher.requests.lock().unwrap().is_empty());
        let unavailable = RecoveryPublisher {
            fail: true,
            ..RecoveryPublisher::default()
        };
        assert_eq!(
            recovery_failure_effect(&unavailable, &Err(SelectionCaptureError::ClipboardRestore)),
            None
        );
    }
    #[test]
    fn preexisting_binding_wins_over_default_selection_chord_at_restart() {
        let mut settings = crate::settings::Settings::default();
        settings.public_plugin_command_hotkeys.insert(
            "com.example.prior".into(),
            std::collections::BTreeMap::from([("default".into(), "Alt+A".into())]),
        );
        let chord = crate::hotkey::command_chord("Alt+A").unwrap().to_string();
        assert!(selection_chord_conflicts(&settings, &chord));
        settings.public_plugin_command_hotkeys.clear();
        assert!(!selection_chord_conflicts(&settings, &chord));
    }

    #[test]
    fn queued_delivery_rechecks_authority_after_capture_before_show() {
        use std::cell::Cell;
        for scenario in ["revoke", "disable", "cancel", "newer action"] {
            let current = Cell::new(true);
            let delivered = Cell::new(false);
            let captured =
                capture_if_authorized(|| current.get(), || Ok("selected".into())).unwrap();
            assert!(captured.is_ok());
            // The worker has queued main-thread completion. A newer operation wins.
            current.set(false);
            deliver_if_authorized(|| current.get(), || delivered.set(true));
            assert!(!delivered.get(), "{scenario}");
        }
        let delivered = Cell::new(false);
        deliver_if_authorized(|| true, || delivered.set(true));
        assert!(delivered.get());
    }

    #[test]
    fn unauthorized_capture_never_calls_native_reader() {
        let result = capture_if_authorized(|| false, || panic!("unauthorized native access"));
        assert!(result.is_none());
    }
    #[test]
    fn capture_completed_after_revoke_disable_or_cancel_is_discarded() {
        use std::cell::Cell;
        for _scenario in ["revoke", "disable", "cancel", "newer action"] {
            let active = Cell::new(true);
            let result = capture_if_authorized(
                || active.get(),
                || {
                    active.set(false);
                    Ok("private".into())
                },
            );
            assert!(result.is_none());
        }
    }
    #[test]
    fn authorized_capture_keeps_its_result() {
        assert_eq!(
            capture_if_authorized(|| true, || Ok("selected".into())),
            Some(Ok("selected".into()))
        );
    }

    #[test]
    fn selection_payload_keeps_text_and_specific_failure_separate() {
        let success = SelectionTranslation {
            id: "one".into(),
            text: Some("hello".into()),
            error: None,
        };
        let value = serde_json::to_value(success).unwrap();
        assert_eq!(value["text"], "hello");
        assert!(value.get("error").is_none());
        let failure = SelectionTranslation {
            id: "two".into(),
            text: None,
            error: Some("未选中文字".into()),
        };
        let value = serde_json::to_value(failure).unwrap();
        assert!(value.get("text").is_none());
        assert_eq!(value["error"], "未选中文字");
    }
    #[test]
    fn selection_preference_reserves_chord_against_all_other_shortcuts() {
        let settings = crate::settings::Settings::default();
        let chord = crate::hotkey::command_chord("Alt+A").unwrap().to_string();
        assert!(crate::command_hotkeys::launcher_conflicts(
            &settings, &chord
        ));
        assert!(crate::command_hotkeys::conflicts(
            &settings, PLUGIN, COMMAND, &chord
        ));
        for feature in crate::settings::ManagedBuiltinFeature::ALL {
            assert!(crate::screenshot::hotkey::builtin_conflicts(
                &settings, feature, &chord
            ));
        }
        let mut cleared = settings;
        cleared.translation_selection_hotkey = None;
        assert!(!crate::command_hotkeys::launcher_conflicts(
            &cleared, &chord
        ));
    }
    #[test]
    fn old_settings_default_to_alt_a_and_explicit_null_remains_cleared() {
        let mut old = serde_json::json!({"hotkey":"Shift+Space","autostart":false});
        let settings: crate::settings::Settings = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(
            settings.translation_selection_hotkey.as_deref(),
            Some("Alt+A")
        );
        old["translationSelectionHotkey"] = serde_json::Value::Null;
        let settings: crate::settings::Settings = serde_json::from_value(old).unwrap();
        assert_eq!(settings.translation_selection_hotkey, None);
        assert!(serde_json::to_value(settings).unwrap()["translationSelectionHotkey"].is_null());
    }
}
