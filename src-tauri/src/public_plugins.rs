pub(crate) mod host_services;
mod activation;
mod activation_bundle;
mod alarm_asset;
pub(crate) mod content_action;
mod data_call_gate;
mod data_scope;
mod settings_values;
mod storage_document;
#[allow(dead_code)]
mod call_session;
mod request_contract;
mod delayed_messages;
pub(crate) mod diagnostics;
pub(crate) mod file_open;
mod icon;
mod manifest;
pub(crate) mod window_action;
// Task 4 wires the verified transport behind PluginHttpsBroker.
#[allow(dead_code)]
mod network;
mod owner_cleanup;
mod package;
pub(crate) mod panel_action;
pub(crate) mod result_action;
mod runtime;
mod scheduler;
mod secrets;
mod sensitive_bytes;
mod state;
mod storage;
mod timers;
mod webview_audio_guard;

#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{
    http::Response,
    webview::{NewWindowResponse, WebviewWindow},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::message_center::MessageCenterService;
use crate::native_attention::AttentionRoutePort;
use crate::settings::SettingsStore;
use owner_cleanup::retry_pending_owner_cleanup;

pub(crate) use activation::{
    parse_main_result_response, parse_panel_response, parse_window_response,
    PublicCommandSuggestion, PublicMainResult, PublicPanelResponse, PublicPluginInstallSource,
    PublicPluginInventory, PublicPluginManagementError, PublicPluginManager, PublicPluginMutation,
    PublicPluginPrepareSummary, PublicPluginRefreshTarget, PublicPluginRoute,
    PublicPluginWindowIdentity, PublicRuntimeCandidate, PublicWindowResponse, WindowStorageError,
};
#[cfg(test)]
pub(crate) use alarm_asset::AlarmAssetIdentity;
pub(crate) use alarm_asset::{PreparedAlarmAsset, ValidatedAlarmAsset};
pub(crate) use manifest::{
    public_manifest_v1_schema, valid_plugin_id, PanelHostKeyDeclaration, PanelHostKeyFocus,
    PublicActivationMode, PublicManifestV1, PublicOutputMode, PublicPermission, PublicPlatform, PublicPluginHost, PublicPackageError,
};
pub(crate) use network::{PluginNetworkErrorCode, PluginNetworkResponse};
#[cfg(test)]
pub(crate) use network::{PluginNetworkRequest, PluginNetworkRequestMethod};
pub(crate) use runtime::{
    parse_runtime_label, runtime_label, validate_plugin_network_caller, PluginApiExecution,
    PluginApiRequest, PluginCommandCompletion, PluginCommandDispatch, PluginInvocation,
    PluginInvocationEnvironment, PluginInvocationPlatform, PluginInvocationTheme,
    PluginNetworkCommandError, PluginNetworkCommandInput, PluginRuntimeApi, PluginRuntimeError,
};
pub(crate) use scheduler::{
    PluginCompletionOutcome, PluginContextStatus, PluginRequestCandidate, PluginRequestContext,
    PluginRequestOwner, PluginRequestScheduler, PluginScheduleOutcome, PluginSubmissionOwner,
    ReservedPluginRequest, ScheduledPluginRequest,
};
pub(crate) use secrets::PluginSecretStore;
pub(crate) use state::{
    EffectivePluginConfig, PluginStateError, PluginStateStore, PublicPluginFault,
};
pub(crate) use storage::PluginStorageStore;
pub(crate) use timers::{
    AudioTicket, PluginTimerService, PluginTimerStartInput, PluginTimerState, TimerAudioCompletion,
    TimerError, TimerKey,
};
pub(crate) use webview_audio_guard::{
    inert_url, prepare_windows_webview, verify_windows_webview_muted, WebViewGuardAuthority,
    WebViewGuardOwner,
};
const PUBLIC_RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_PLUGIN_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; img-src data:; media-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PublicPluginResponse {
    MainResults(Vec<PublicMainResult>),
    ServiceMainResults {
        results: Vec<PublicMainResult>,
        credential_revision: u64,
        publication: result_action::ResultActionLease,
    },
    MainActionResults {
        route: PublicPluginRoute,
        results: Vec<result_action::PublicActionResult>,
    },
    Window(PublicWindowResponse),
    Panel(PublicPanelResponse),
}

#[cfg(test)]
pub(crate) fn with_settings_service_for_test(
    operation: impl FnOnce(&PublicPluginService, &PublicPluginManager),
) {
    tests::with_settings_service_for_test(operation);
}

#[cfg(test)]
pub(crate) fn with_running_result_action_for_test(
    legacy: Arc<crate::plugins::PluginManager>,
    operation: impl FnOnce(
        &PublicPluginService,
        &PublicPluginManager,
        &crate::result_registry::ResultRegistry,
        &ScheduledPluginRequest,
        &mpsc::Receiver<result_action::ResultActionOutcome>,
    ),
) {
    tests::with_running_result_action_for_test(legacy, operation);
}

pub(crate) type PublicSubmissionResult = Result<PublicPluginResponse, PluginRuntimeError>;

enum ContentActionIdentity {
    Panel(
        crate::plugin_panel::PanelSessionIdentity,
        std::sync::Weak<crate::plugin_panel::PluginPanelController>,
    ),
    Window(
        crate::plugin_window::WindowActionIdentity,
        std::sync::Weak<crate::plugin_window::PluginWindowController>,
    ),
}
impl ContentActionIdentity {
    fn runtime_owner(&self) -> (String, u64, u64) {
        match self {
            Self::Panel(i, _) => (i.plugin_id.clone(), i.generation, i.activation_id),
            Self::Window(i, _) => (
                i.owner.plugin_id.clone(),
                i.owner.plugin_generation,
                i.owner.activation_id,
            ),
        }
    }
}
enum ContentActionSender {
    Panel(mpsc::Sender<Result<serde_json::Value, panel_action::PanelActionError>>),
    Window(mpsc::Sender<Result<serde_json::Value, window_action::WindowActionError>>),
}
struct PendingContentAction {
    identity: ContentActionIdentity,
    active: Arc<AtomicBool>,
    sender: ContentActionSender,
}
pub(crate) type WindowActionRegistration = ContentActionRegistration;
pub(crate) type PanelActionRegistration = ContentActionRegistration;

pub(crate) struct ContentActionRegistration {
    context: PluginRequestContext,
    service: std::sync::Weak<PublicPluginService>,
    active: Arc<AtomicBool>,
}
impl ContentActionRegistration {
    pub(crate) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub(crate) fn matches(&self, context: &PluginRequestContext) -> bool {
        &self.context == context
    }
    pub(crate) fn disarm(&self) {
        self.active.store(false, Ordering::Release);
    }
}
impl Drop for ContentActionRegistration {
    fn drop(&mut self) {
        if self.active.swap(false, Ordering::AcqRel) {
            if let Some(service) = self.service.upgrade() {
                service.retire_panel_action(&self.context);
            }
        }
    }
}
struct PendingResultAction {
    context: PluginRequestContext,
    origin: result_action::ResultActionOrigin,
    registry: crate::result_registry::ResultRegistry,
    legacy: Arc<crate::plugins::PluginManager>,
    reserved_ids: u64,
    sender: mpsc::Sender<result_action::ResultActionOutcome>,
}
struct PendingPublicSubmission {
    route: PublicPluginRoute,
    sender: mpsc::Sender<Option<PublicSubmissionResult>>,
}

#[derive(Default)]
struct PublicSubmissionState {
    by_token: HashMap<String, PendingPublicSubmission>,
    token_by_request: HashMap<PluginRequestContext, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct PublicRuntimeRecovery {
    attempt_id: u64,
    submission_token: String,
    candidate: PublicRuntimeCandidate,
    owner: bool,
}

struct RuntimeRecoveryAttempt {
    attempt_id: u64,
    candidate: PublicRuntimeCandidate,
    request: ReservedPluginRequest,
    current_submission_token: String,
    finalizing: bool,
    settled: Option<bool>,
}

#[derive(Default)]
struct RuntimeRecoveryState {
    by_plugin: HashMap<String, RuntimeRecoveryAttempt>,
}

pub(crate) struct ManagementOutcome<T = PublicPluginMutation> {
    pub(crate) result: Result<T, PublicPluginManagementError>,
    // Request/result invalidation occurred, even if result is Err. A successful
    // favorite write changes stored UI metadata without setting this flag.
    pub(crate) changed: bool,
}

#[derive(Default)]
pub(crate) struct PublicPluginService {
    manager: OnceLock<Arc<PublicPluginManager>>,
    submissions: Mutex<PublicSubmissionState>,
    content_actions: Mutex<HashMap<PluginRequestContext, PendingContentAction>>,
    result_action: Mutex<Option<PendingResultAction>>,
    recoveries: Mutex<RuntimeRecoveryState>,
    recovery_changed: Condvar,
    next_submission: AtomicU64,
    next_recovery: AtomicU64,
    startup_runtimes_started: AtomicBool,
    webview_guards: Arc<WebViewGuardAuthority>,
}

pub(crate) struct PublicSubmission {
    pub(crate) token: String,
    pub(crate) receiver: mpsc::Receiver<Option<PublicSubmissionResult>>,
    pub(crate) dispatch: Option<ScheduledPluginRequest>,
    pub(crate) recovery: Option<PublicRuntimeRecovery>,
}

pub(crate) struct PreparedPublicSubmission {
    pub(crate) token: String,
    pub(crate) receiver: mpsc::Receiver<Option<PublicSubmissionResult>>,
    route: PublicPluginRoute,
    request: ReservedPluginRequest,
}

impl PreparedPublicSubmission {
    pub(crate) fn request_context(&self) -> &PluginRequestContext {
        &self.request.context
    }
}
impl PublicPluginService {
    pub(crate) fn initialize(
        &self,
        app: &AppHandle,
        app_data_dir: &Path,
        reserved_names: impl IntoIterator<Item = String>,
        message_center: Arc<MessageCenterService>,
        attention_route: Arc<dyn AttentionRoutePort>,
    ) -> Result<Arc<PublicPluginManager>, PublicPluginManagementError> {
        let settings = app.state::<SettingsStore>();
        let _ = retry_pending_owner_cleanup(app_data_dir, settings.inner());
        let manager = Arc::new(PublicPluginManager::load(
            app_data_dir,
            PublicPluginHost::current(PublicPlatform::Windows),
            reserved_names,
            Arc::clone(&message_center),
        )?);
        let changed_app = app.clone();
        manager.attach_command_hotkeys(
            settings.inner().clone(),
            Arc::new(move || {
                let app = changed_app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let coordinator = Arc::clone(
                        app.state::<Arc<crate::lifecycle::LifecycleCoordinator>>()
                            .inner(),
                    );
                    let _ = coordinator.reconcile_command_hotkeys(&app);
                });
            }),
        )?;
        message_center
            .start_native_attention(manager.timer_service(), attention_route)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        manager.start_delayed_messages(app)?;
        if let Err(error) = manager.start_timers(app) {
            manager.shutdown_delayed_messages();
            return Err(error);
        }
        if self.manager.set(Arc::clone(&manager)).is_err() {
            manager.shutdown_delayed_messages();
            manager.shutdown_timers();
            return Err(PublicPluginManagementError::Unavailable);
        }
        Ok(manager)
    }

    pub(crate) fn manager(&self) -> Result<&Arc<PublicPluginManager>, PublicPluginManagementError> {
        self.manager
            .get()
            .ok_or(PublicPluginManagementError::Unavailable)
    }

    pub(crate) fn manage_with_effects<T>(
        &self,
        operation: impl FnOnce(
            &PublicPluginManager,
            &mut activation::ManagementEffects,
        ) -> Result<T, PublicPluginManagementError>,
    ) -> ManagementOutcome<T> {
        let mut effects = activation::ManagementEffects::default();
        let result = self
            .manager()
            .and_then(|manager| operation(manager, &mut effects));
        self.settle_management_effects(&effects);
        ManagementOutcome {
            result,
            changed: effects.changed,
        }
    }

    pub(crate) fn rename_command(
        &self,
        plugin_id: &str,
        command_id: &str,
        name_override: Option<&str>,
    ) -> ManagementOutcome {
        self.manage_with_effects(|manager, effects| {
            manager.rename_command_with_effects(plugin_id, command_id, name_override, effects)
        })
    }

    pub(crate) fn set_command_favorite(
        &self,
        plugin_id: &str,
        command_id: &str,
        favorite: bool,
    ) -> ManagementOutcome {
        self.manage_with_effects(|manager, effects| {
            manager.set_command_favorite_with_effects(plugin_id, command_id, favorite, effects)
        })
    }

    pub(crate) fn save_settings(
        &self,
        plugin_id: &str,
        settings: &BTreeMap<String, serde_json::Value>,
        secrets: &BTreeMap<String, Option<String>>,
    ) -> ManagementOutcome {
        let mut effects = activation::ManagementEffects::default();
        let result = (|| {
            let manager = self.manager()?;
            let initial = manager.validate_settings_save(plugin_id, settings, secrets)?;
            for (key, value) in secrets {
                manager.save_secret_with_effects(plugin_id, key, value.as_deref(), &mut effects)?;
            }
            if settings.is_empty() {
                return Ok(initial);
            }
            manager.save_settings_with_effects(plugin_id, settings, &mut effects)
        })();
        self.settle_management_effects(&effects);
        ManagementOutcome {
            result,
            changed: effects.changed,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_network_access(
        &self,
        plugin_id: &str,
        granted: bool,
        expected_inventory_revision: Option<&str>,
    ) -> Result<PublicPluginMutation, PublicPluginManagementError> {
        self.set_network_access_outcome(plugin_id, granted, expected_inventory_revision)
            .result
    }

    pub(crate) fn set_permission_access_outcome(
        &self,
        plugin_id: &str,
        permission: PublicPermission,
        granted: bool,
        expected_inventory_revision: &str,
    ) -> ManagementOutcome {
        self.manage_with_effects(|manager, effects| {
            manager.set_permission_access_with_effects(
                plugin_id,
                permission,
                granted,
                expected_inventory_revision,
                effects,
            )
        })
    }

    pub(crate) fn set_network_access_outcome(
        &self,
        plugin_id: &str,
        granted: bool,
        expected_inventory_revision: Option<&str>,
    ) -> ManagementOutcome {
        let mut effects = activation::ManagementEffects::default();
        let result = self.manager().and_then(|manager| {
            manager.set_network_access_with_effects(
                plugin_id,
                granted,
                expected_inventory_revision,
                &mut effects,
            )
        });
        self.settle_management_effects(&effects);
        ManagementOutcome {
            result,
            changed: effects.changed,
        }
    }

    fn settle_management_effects(&self, effects: &activation::ManagementEffects) {
        // Waiting submissions may never have been bound to a request. Settle the
        // exact token as well as its context, never a plugin-wide moving target.
        for (context, token) in &effects.retired {
            self.settle_result_action(
                context,
                result_action::ResultActionErrorCode::ActionCancelled,
            );
            self.settle_panel_action(
                context,
                Err(panel_action::PanelActionError::ExpiredPanelSession),
                false,
            );
            self.settle_request(context, None);
            if let Some(token) = token {
                self.settle_submission(token, None);
            }
        }
    }

    pub(crate) fn shutdown(&self) {
        let result_context = self
            .result_action
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|p| p.context.clone()));
        if let Some(context) = result_context {
            if let Ok(manager) = self.manager() {
                manager.retire_panel_action(&context);
            }
            self.settle_result_action(
                &context,
                result_action::ResultActionErrorCode::ActionCancelled,
            );
        }
        let contexts = self
            .content_actions
            .lock()
            .ok()
            .map(|map| map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for context in contexts {
            self.retire_panel_action(&context);
        }
        if let Some(manager) = self.manager.get() {
            manager.shutdown_network();
            manager.shutdown_clipboard_history();
            manager.shutdown_delayed_messages();
            manager.shutdown_timers();
        }
    }

    pub(crate) fn start_enabled_runtimes(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> Result<(), PublicPluginManagementError> {
        let manager = Arc::clone(self.manager()?);
        let candidates = manager.runtime_candidates()?;
        if self
            .startup_runtimes_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }
        let app = app.clone();
        let service = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            for candidate in candidates {
                let ready = service.create_runtime(&app, &candidate).is_ok();
                if !ready {
                    let _ = manager.mark_runtime_unavailable(
                        &candidate.plugin_id,
                        candidate.generation,
                        candidate.activation_id,
                    );
                }
            }
        });
        Ok(())
    }

    pub(crate) fn schedule_command(
        &self,
        route: PublicPluginRoute,
        ui_intent_epoch: u64,
        control_value: String,
        now: Instant,
    ) -> Result<PublicSubmission, PublicPluginManagementError> {
        let prepared = self.prepare_command(route, ui_intent_epoch, control_value)?;
        self.admit_prepared_command(prepared, now)
    }

    pub(crate) fn prepare_command(
        &self,
        route: PublicPluginRoute,
        ui_intent_epoch: u64,
        control_value: String,
    ) -> Result<PreparedPublicSubmission, PublicPluginManagementError> {
        let number = self
            .next_submission
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let token = format!("public-submission-{number:016x}");
        let (sender, receiver) = mpsc::channel();
        self.lock_submissions()?.by_token.insert(
            token.clone(),
            PendingPublicSubmission {
                route: route.clone(),
                sender,
            },
        );
        let request = self.manager()?.scheduler().reserve(PluginRequestCandidate {
            command_id: route.command_id.clone(),
            plugin_id: route.plugin_id.clone(),
            plugin_generation: route.generation,
            activation_id: route.activation_id,
            admission_epoch: route.admission_epoch,
            activation_mode: route.activation_mode,
            input: route.input.clone(),
            owner: crate::public_plugins::PluginRequestOwner::Command(PluginSubmissionOwner {
                ui_intent_epoch,
                control_value,
                submission_token: token.clone(),
            }),
        });
        let request = match request {
            Ok(request) => request,
            Err(_) => {
                self.settle_submission(&token, None);
                return Err(PublicPluginManagementError::Unavailable);
            }
        };
        Ok(PreparedPublicSubmission {
            token,
            receiver,
            route,
            request,
        })
    }

    pub(crate) fn admit_prepared_command(
        &self,
        prepared: PreparedPublicSubmission,
        now: Instant,
    ) -> Result<PublicSubmission, PublicPluginManagementError> {
        self.admit_prepared_command_with(prepared, now, || {})
    }

    fn admit_prepared_command_with(
        &self,
        prepared: PreparedPublicSubmission,
        now: Instant,
        after_enqueue: impl FnOnce(),
    ) -> Result<PublicSubmission, PublicPluginManagementError> {
        let PreparedPublicSubmission {
            token,
            receiver,
            route,
            request,
        } = prepared;
        if route.runtime_recovery_needed {
            let candidate = match self.manager()?.runtime_recovery_candidate(
                &route.plugin_id,
                route.generation,
                route.activation_id,
                route.admission_epoch,
            )? {
                Some(candidate) => candidate,
                None => {
                    self.settle_submission(&token, None);
                    return Err(PublicPluginManagementError::Unavailable);
                }
            };
            let recovery = match self.register_runtime_recovery(candidate, request, &token) {
                Ok(recovery) => recovery,
                Err(error) => {
                    self.settle_submission(&token, None);
                    return Err(error);
                }
            };
            return Ok(PublicSubmission {
                token,
                receiver,
                dispatch: None,
                recovery: Some(recovery),
            });
        }
        let outcome = match self.manager()?.enqueue_reserved(request, now) {
            Ok(outcome) => outcome,
            Err(_) => {
                self.settle_submission(&token, None);
                return Err(PublicPluginManagementError::Unavailable);
            }
        };
        after_enqueue();
        let result_context = self.result_action.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|p| {
                    p.context.plugin_id == route.plugin_id
                        && p.context.plugin_generation == route.generation
                })
                .map(|p| {
                    p.origin.lease.revoke();
                    p.context.clone()
                })
        });
        if let Some(context) = result_context {
            self.settle_result_action(
                &context,
                result_action::ResultActionErrorCode::ActionCancelled,
            );
        }

        let contexts = self
            .content_actions
            .lock()
            .ok()
            .map(|map| {
                map.keys()
                    .filter(|c| {
                        c.plugin_id == route.plugin_id && c.plugin_generation == route.generation
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for context in contexts {
            self.settle_panel_action(
                &context,
                Err(panel_action::PanelActionError::PanelRequestCancelledError),
                false,
            );
        }
        let dispatch = match outcome {
            PluginScheduleOutcome::Dispatched(request) => {
                self.bind_unsent_dispatch(request, now)?
            }
            PluginScheduleOutcome::Waiting {
                expired,
                replaced_submission_token,
                ..
            } => {
                self.settle_request(&expired, None);
                if let Some(replaced) = replaced_submission_token {
                    self.settle_submission(&replaced, None);
                }
                None
            }
        };
        Ok(PublicSubmission {
            token,
            receiver,
            dispatch,
            recovery: None,
        })
    }

    fn register_runtime_recovery(
        &self,
        candidate: PublicRuntimeCandidate,
        request: ReservedPluginRequest,
        submission_token: &str,
    ) -> Result<PublicRuntimeRecovery, PublicPluginManagementError> {
        let mut recoveries = self.lock_recoveries()?;
        if let Some(attempt) = recoveries
            .by_plugin
            .get_mut(&candidate.plugin_id)
            .filter(|attempt| attempt.candidate == candidate)
        {
            let attempt_id = attempt.attempt_id;
            let replaced = std::mem::replace(
                &mut attempt.current_submission_token,
                submission_token.into(),
            );
            attempt.request = request;
            drop(recoveries);
            if replaced != submission_token {
                self.settle_submission(&replaced, None);
            }
            return Ok(PublicRuntimeRecovery {
                attempt_id,
                submission_token: submission_token.into(),
                candidate,
                owner: false,
            });
        }
        let attempt_id = self
            .next_recovery
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .checked_add(1)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        let replaced = recoveries
            .by_plugin
            .remove(&candidate.plugin_id)
            .map(|attempt| attempt.current_submission_token);
        recoveries.by_plugin.insert(
            candidate.plugin_id.clone(),
            RuntimeRecoveryAttempt {
                attempt_id,
                candidate: candidate.clone(),
                request,
                current_submission_token: submission_token.into(),
                finalizing: false,
                settled: None,
            },
        );
        drop(recoveries);
        if let Some(replaced) = replaced.filter(|replaced| replaced != submission_token) {
            self.settle_submission(&replaced, None);
        }
        Ok(PublicRuntimeRecovery {
            attempt_id,
            submission_token: submission_token.to_owned(),
            candidate,
            owner: true,
        })
    }

    pub(crate) fn complete_runtime_recovery_with<F>(
        &self,
        recovery: &PublicRuntimeRecovery,
        now: Instant,
        readiness: F,
    ) -> Result<Option<ScheduledPluginRequest>, PublicPluginManagementError>
    where
        F: FnOnce(&PublicRuntimeCandidate) -> bool,
    {
        if recovery.owner {
            {
                let mut recoveries = self.lock_recoveries()?;
                let Some(attempt) = recoveries
                    .by_plugin
                    .get_mut(&recovery.candidate.plugin_id)
                    .filter(|attempt| {
                        attempt.attempt_id == recovery.attempt_id
                            && !attempt.finalizing
                            && attempt.settled.is_none()
                    })
                else {
                    return Ok(None);
                };
                attempt.finalizing = true;
            }
            let runtime_ready = readiness(&recovery.candidate);
            let accepted = runtime_ready
                && self
                    .manager()?
                    .finish_runtime_recovery(&recovery.candidate)?;
            if !accepted
                && self
                    .manager()?
                    .runtime_recovery_candidate(
                        &recovery.candidate.plugin_id,
                        recovery.candidate.generation,
                        recovery.candidate.activation_id,
                        recovery.candidate.admission_epoch,
                    )?
                    .is_some()
            {
                let _ = self.manager()?.mark_runtime_unavailable(
                    &recovery.candidate.plugin_id,
                    recovery.candidate.generation,
                    recovery.candidate.activation_id,
                );
            }
            let mut recoveries = self.lock_recoveries()?;
            let Some(attempt) = recoveries
                .by_plugin
                .get_mut(&recovery.candidate.plugin_id)
                .filter(|attempt| attempt.attempt_id == recovery.attempt_id)
            else {
                return Ok(None);
            };
            attempt.finalizing = false;
            attempt.settled = Some(accepted);
            self.recovery_changed.notify_all();
        }

        let mut recoveries = self.lock_recoveries()?;
        while recoveries
            .by_plugin
            .get(&recovery.candidate.plugin_id)
            .is_some_and(|attempt| {
                attempt.attempt_id == recovery.attempt_id && attempt.settled.is_none()
            })
        {
            recoveries = self
                .recovery_changed
                .wait(recoveries)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
        }
        let Some(attempt) = recoveries
            .by_plugin
            .get(&recovery.candidate.plugin_id)
            .filter(|attempt| attempt.attempt_id == recovery.attempt_id)
        else {
            return Ok(None);
        };
        if attempt.settled != Some(true) {
            recoveries.by_plugin.remove(&recovery.candidate.plugin_id);
            drop(recoveries);
            self.settle_plugin_submissions_with(&recovery.candidate.plugin_id, None);
            return Ok(None);
        }
        if attempt.current_submission_token != recovery.submission_token {
            return Ok(None);
        }
        let request = attempt.request.clone();
        recoveries.by_plugin.remove(&recovery.candidate.plugin_id);
        drop(recoveries);
        let dispatch = self
            .manager()?
            .enqueue_reserved(request, now)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let PluginScheduleOutcome::Dispatched(dispatch) = dispatch else {
            self.settle_submission(&recovery.submission_token, None);
            return Ok(None);
        };
        self.bind_unsent_dispatch(dispatch, now)
    }

    pub(crate) fn recover_runtime(
        &self,
        app: &AppHandle,
        recovery: &PublicRuntimeRecovery,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PublicPluginManagementError> {
        let ready = if recovery.owner {
            self.create_runtime(app, &recovery.candidate).is_ok()
        } else {
            false
        };
        self.complete_runtime_recovery_with(recovery, now, |_| ready)
    }

    pub(crate) fn cleanup_retired_result_actions(
        &self,
        registry: &crate::result_registry::ResultRegistry,
    ) {
        if let Some(context) = registry.take_retired_result_action() {
            if let Ok(manager) = self.manager() {
                manager.retire_panel_action(&context);
            }
            self.settle_result_action(
                &context,
                result_action::ResultActionErrorCode::ActionCancelled,
            );
        }
    }
    pub(crate) fn prepare_result_action(
        self: &Arc<Self>,
        app: &AppHandle,
        legacy: Arc<crate::plugins::PluginManager>,
        registry: crate::result_registry::ResultRegistry,
        origin: result_action::ResultActionOrigin,
    ) -> Result<
        (
            ScheduledPluginRequest,
            mpsc::Receiver<result_action::ResultActionOutcome>,
        ),
        result_action::ResultActionErrorCode,
    > {
        let service = Arc::clone(self);
        let watchdog_app = app.clone();
        self.prepare_result_action_with_observer(legacy, registry, origin, move || {
            result_action::prepare_deadline_observer(move |_| {
                let _ = service.expire_runtime_timeouts(&watchdog_app, Instant::now());
            })
        })
    }
    fn prepare_result_action_with_observer(
        self: &Arc<Self>,
        legacy: Arc<crate::plugins::PluginManager>,
        registry: crate::result_registry::ResultRegistry,
        origin: result_action::ResultActionOrigin,
        prepare_observer: impl FnOnce() -> Result<
            mpsc::Sender<Instant>,
            result_action::ResultActionErrorCode,
        >,
    ) -> Result<
        (
            ScheduledPluginRequest,
            mpsc::Receiver<result_action::ResultActionOutcome>,
        ),
        result_action::ResultActionErrorCode,
    > {
        use result_action::ResultActionErrorCode as E;
        let mut pending = self
            .result_action
            .lock()
            .map_err(|_| E::ActionUnavailable)?;
        if let Some(active) = pending.as_ref() {
            return Err(if active.origin.lease == origin.lease {
                E::ActionInProgress
            } else {
                E::ActionBusy
            });
        }
        let manager = self.manager().map_err(|_| E::ActionUnavailable)?;
        let reserved = manager.reserve_result_action(&origin)?;
        let reserved_ids = registry
            .reserve_result_action_ids()
            .ok_or(E::ActionUnavailable)?;
        let (sender, receiver) = mpsc::channel();
        // Fallible observer creation occurs before the source claim or physical slot.
        let arm = prepare_observer()?;
        let request = legacy
            .with_public_result_admission(|| {
                manager.admit_result_action(&registry, &origin, reserved, Instant::now())
            })
            .ok_or(E::ActionUnavailable)??;
        *pending = Some(PendingResultAction {
            context: request.context.clone(),
            origin,
            registry,
            legacy,
            reserved_ids,
            sender,
        });
        let _ = arm.send(request.deadline);
        Ok((request, receiver))
    }
    pub(crate) fn settle_result_action(
        &self,
        context: &PluginRequestContext,
        code: result_action::ResultActionErrorCode,
    ) -> bool {
        let pending = self.result_action.lock().ok().and_then(|mut slot| {
            if slot.as_ref().is_some_and(|p| p.context == *context) {
                slot.take()
            } else {
                None
            }
        });
        let Some(pending) = pending else {
            return false;
        };
        pending.registry.consume_result_action(&pending.origin);
        if let Ok(manager) = self.manager() {
            manager.discard_panel_completion(context);
        }
        let _ = pending
            .sender
            .send(result_action::ResultActionOutcome::failed(code, true));
        true
    }
    fn complete_result_action(
        &self,
        completion: &PluginCommandCompletion,
        accepted: bool,
    ) -> Option<(result_action::ResultActionOrigin, bool, bool)> {
        use result_action::{ResultActionErrorCode as E, ResultActionOutcome as O};
        let pending = self.result_action.lock().ok().and_then(|mut slot| {
            if slot
                .as_ref()
                .is_some_and(|p| p.context == completion.context)
            {
                slot.take()
            } else {
                None
            }
        })?;
        let result = (|| {
            if !accepted || !pending.origin.lease.live() {
                return Err(E::ActionCancelled);
            }
            if completion.failed {
                return Err(E::ActionFailed);
            }
            let rows = result_action::parse(
                &completion.context,
                completion
                    .response
                    .clone()
                    .ok_or(E::ActionInvalidResponse)?,
            )
            .map_err(|_| E::ActionInvalidResponse)?;
            let requirements = result_action::ResultActionRequirements::rows(&rows);
            let prepared = pending
                .registry
                .prepare_result_actions_reserved(
                    pending.origin.route.clone(),
                    rows,
                    pending.reserved_ids,
                )
                .ok_or(E::ActionUnavailable)?;
            pending
                .legacy
                .with_public_result_admission(|| {
                    self.manager()
                        .map_err(|_| E::ActionUnavailable)?
                        .finish_result_action(
                            &pending.registry,
                            &pending.origin,
                            &completion.context,
                            prepared,
                            requirements,
                        )
                })
                .ok_or(E::ActionUnavailable)?
        })();
        let success = result.is_ok();
        let fault = matches!(result, Err(E::ActionFailed | E::ActionInvalidResponse));
        let outcome = match result {
            Ok(response) => O::ResultsUpdated { response },
            Err(code) => {
                pending.registry.consume_result_action(&pending.origin);
                O::failed(code, true)
            }
        };
        if let Ok(manager) = self.manager() {
            manager.discard_panel_completion(&completion.context);
        }
        // Replacement retired the old lease under registry lock; no callback carries that lock.
        self.cleanup_retired_result_actions(&pending.registry);
        let _ = pending.sender.send(outcome);
        Some((pending.origin, success, fault))
    }

    pub(crate) fn prepare_panel_action(
        self: &Arc<Self>,
        controller: &Arc<crate::plugin_panel::PluginPanelController>,
        identity: &crate::plugin_panel::PanelSessionIdentity,
        request: panel_action::PanelActionRequest,
    ) -> Result<
        (
            ScheduledPluginRequest,
            PanelActionRegistration,
            mpsc::Receiver<Result<serde_json::Value, panel_action::PanelActionError>>,
        ),
        panel_action::PanelActionError,
    > {
        use panel_action::PanelActionError as E;
        let mut pending = self
            .content_actions
            .lock()
            .map_err(|_| E::PanelRequestUnavailableError)?;
        let request = self
            .manager()
            .map_err(|_| E::PanelRequestUnavailableError)?
            .admit_panel_action(identity, request, Instant::now())?;
        let (sender, receiver) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        pending.insert(
            request.context.clone(),
            PendingContentAction {
                identity: ContentActionIdentity::Panel(
                    identity.clone(),
                    Arc::downgrade(controller),
                ),
                active: active.clone(),
                sender: ContentActionSender::Panel(sender),
            },
        );
        let registration = PanelActionRegistration {
            context: request.context.clone(),
            service: Arc::downgrade(self),
            active,
        };
        Ok((request, registration, receiver))
    }

    pub(crate) fn prepare_window_action(
        self: &Arc<Self>,
        controller: &Arc<crate::plugin_window::PluginWindowController>,
        identity: &crate::plugin_window::WindowActionIdentity,
        request: window_action::WindowActionRequest,
    ) -> Result<
        (
            ScheduledPluginRequest,
            WindowActionRegistration,
            mpsc::Receiver<Result<serde_json::Value, window_action::WindowActionError>>,
        ),
        window_action::WindowActionError,
    > {
        use window_action::WindowActionError as E;
        let mut pending = self
            .content_actions
            .lock()
            .map_err(|_| E::WindowRequestUnavailableError)?;
        let request = self
            .manager()
            .map_err(|_| E::WindowRequestUnavailableError)?
            .admit_window_action(identity, request, Instant::now())?;
        let (sender, receiver) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        pending.insert(
            request.context.clone(),
            PendingContentAction {
                identity: ContentActionIdentity::Window(
                    identity.clone(),
                    Arc::downgrade(controller),
                ),
                active: active.clone(),
                sender: ContentActionSender::Window(sender),
            },
        );
        let registration = PanelActionRegistration {
            context: request.context.clone(),
            service: Arc::downgrade(self),
            active,
        };
        Ok((request, registration, receiver))
    }

    pub(crate) fn retire_panel_action(&self, context: &PluginRequestContext) {
        if let Ok(manager) = self.manager() {
            manager.retire_panel_action(context);
        }
        self.settle_panel_action(
            context,
            Err(panel_action::PanelActionError::ExpiredPanelSession),
            false,
        );
    }

    pub(crate) fn settle_panel_action(
        &self,
        context: &PluginRequestContext,
        result: Result<serde_json::Value, panel_action::PanelActionError>,
        recheck: bool,
    ) -> bool {
        self.settle_panel_action_with(context, result, recheck, || {})
    }

    fn settle_panel_action_with(
        &self,
        context: &PluginRequestContext,
        result: Result<serde_json::Value, panel_action::PanelActionError>,
        recheck: bool,
        after_take: impl FnOnce(),
    ) -> bool {
        self.settle_content_action_with(context, result.map_err(Into::into), recheck, after_take)
    }
    pub(crate) fn settle_window_action(
        &self,
        context: &PluginRequestContext,
        result: Result<serde_json::Value, content_action::ContentActionError>,
        recheck: bool,
    ) -> bool {
        self.settle_content_action_with(context, result, recheck, || {})
    }
    fn settle_content_action_with(
        &self,
        context: &PluginRequestContext,
        result: Result<serde_json::Value, content_action::ContentActionError>,
        recheck: bool,
        after_take: impl FnOnce(),
    ) -> bool {
        use content_action::ContentActionError as E;
        // Never carry this lock into a controller (admission takes controller first).
        let pending = self
            .content_actions
            .lock()
            .ok()
            .and_then(|mut map| map.remove(context));
        let Some(pending) = pending else {
            if recheck {
                if let Ok(manager) = self.manager() {
                    manager.discard_panel_completion(context);
                }
            }
            return false;
        };
        after_take();
        let terminal = matches!(result, Err(E::Timeout | E::Unavailable));
        let ownership = if !recheck {
            Ok(())
        } else {
            match &pending.identity {
                ContentActionIdentity::Panel(identity, controller) => {
                    controller.upgrade().ok_or(E::Expired).and_then(|c| {
                        c.finish_panel_action(identity, context, || {
                            if terminal {
                                return Ok(());
                            }
                            self.manager()
                                .map_err(|_| {
                                    panel_action::PanelActionError::PanelRequestUnavailableError
                                })?
                                .claim_panel_completion(identity, context)
                        })
                        .map_err(Into::into)
                    })
                }
                ContentActionIdentity::Window(identity, controller) => {
                    controller.upgrade().ok_or(E::Expired).and_then(|c| {
                        c.finish_window_action(identity, context, || {
                            if terminal {
                                return Ok(());
                            }
                            self.manager()
                                .map_err(|_| E::Unavailable)?
                                .claim_window_completion(identity, context)
                        })
                    })
                }
            }
        };
        let result = ownership.and(result);
        pending.active.store(false, Ordering::Release);
        if let Ok(manager) = self.manager() {
            manager.discard_panel_completion(context);
        }
        match pending.sender {
            ContentActionSender::Panel(sender) => {
                let _ = sender.send(result.map_err(Into::into));
            }
            ContentActionSender::Window(sender) => {
                let _ = sender.send(result.map_err(Into::into));
            }
        }
        true
    }

    pub(crate) fn dispatch(
        self: &Arc<Self>,
        app: &AppHandle,
        request: &ScheduledPluginRequest,
        theme: PluginInvocationTheme,
        invoked_at: String,
    ) -> Result<(), PublicPluginManagementError> {
        if !request.candidate.owner.live() {
            return Err(PublicPluginManagementError::Unavailable);
        }
        if !matches!(request.candidate.owner, PluginRequestOwner::ResultAction(_)) {
            let service = Arc::clone(self);
            let watchdog_app = app.clone();
            let deadline = request.deadline;
            thread::spawn(move || {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                let _ = service.expire_runtime_timeouts(&watchdog_app, Instant::now());
            });
        }
        if self.manager()?.scheduler().context_status(&request.context)
            != PluginContextStatus::Current
        {
            return Err(PublicPluginManagementError::Unavailable);
        }
        let environment = PluginInvocationEnvironment {
            platform: PluginInvocationPlatform::Windows,
            theme,
            invoked_at,
        };
        let (kind, invocation) = match &request.candidate.owner {
            PluginRequestOwner::Command(_) => (
                "command",
                serde_json::to_value(PluginInvocation {
                    command_id: self.manager()?.invocation_command_id(request)?,
                    api_version: 1,
                    request_id: request.context.request_id.clone(),
                    input: request.candidate.input.clone(),
                    context: environment,
                })
                .map_err(|_| PublicPluginManagementError::Unavailable)?,
            ),
            PluginRequestOwner::ResultAction(origin) => {
                let result_action::PublicResultAction::Runtime { id, payload, .. } = &origin.action
                else {
                    return Err(PublicPluginManagementError::Unavailable);
                };
                (
                    "resultAction",
                    serde_json::json!({"apiVersion":1,"requestId":request.context.request_id,"commandId":origin.route.command_id,"resultId":origin.business_result_id,"actionId":id,"payload":payload,"input":origin.route.input,"context":environment}),
                )
            }
            PluginRequestOwner::WindowAction {
                request: action, ..
            } => (
                "windowAction",
                serde_json::json!({"apiVersion": 1, "requestId": request.context.request_id, "action": action.action,
                    "payload": action.payload, "context": environment}),
            ),
            PluginRequestOwner::PanelAction {
                request: action, ..
            } => (
                "panelAction",
                serde_json::json!({
                    "apiVersion": 1, "requestId": request.context.request_id, "action": action.action,
                    "payload": action.payload, "context": environment,
                }),
            ),
        };
        let label = runtime_label(
            &request.context.plugin_id,
            request.context.plugin_generation,
        )
        .ok_or(PublicPluginManagementError::Unavailable)?;
        let window = app
            .get_webview_window(&label)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        window
            .emit(
                "uipilot-public-plugin-command",
                PluginCommandDispatch {
                    context: request.context.clone(),
                    kind: kind.into(),
                    invocation,
                },
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?;

        Ok(())
    }

    pub(crate) fn complete_submission(
        &self,
        app: &AppHandle,
        completion: &PluginCommandCompletion,
        outcome: PluginCompletionOutcome,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PluginRuntimeError> {
        if let Some((origin, success, fault)) =
            self.complete_result_action(completion, outcome.accepted)
        {
            if outcome.accepted && (success || fault) {
                let disabled = self
                    .manager()
                    .map_err(|_| PluginRuntimeError::Unavailable)?
                    .record_runtime_result(
                        &origin.route.plugin_id,
                        origin.route.generation,
                        origin.route.activation_id,
                        success,
                        now,
                    )
                    .map_err(|_| PluginRuntimeError::Unavailable)?;
                if disabled {
                    self.settle_plugin_submissions(&origin.route.plugin_id);
                    Self::destroy_runtime(app, Some(&origin.route.runtime_label));
                    return Ok(None);
                }
            }
            return outcome
                .next
                .map(|next| self.bind_unsent_dispatch(next, now))
                .transpose()
                .map(Option::flatten)
                .map_err(|_| PluginRuntimeError::Unavailable);
        }
        use panel_action::PanelActionError as E;
        let panel_result = if !outcome.accepted {
            Err(E::ExpiredPanelSession)
        } else if completion.failed {
            Err(E::PanelRequestFailedError)
        } else {
            completion
                .response
                .clone()
                .ok_or(E::InvalidPanelResponseError)
                .and_then(|v| panel_action::response_data(&completion.context.request_id, v))
        };
        let panel_owner = self.content_actions.lock().ok().and_then(|map| {
            map.get(&completion.context)
                .map(|p| p.identity.runtime_owner())
        });
        let panel_success = panel_result.is_ok();
        // Credential validation must cover the actual content sender, not only
        // physical Runtime completion: mutation can win between those two steps.
        let settled = if let Some((revision, _)) = &outcome.credential_publication {
            app.state::<crate::credentials::CredentialStore>()
                .with_revision(*revision, || {
                    Ok(self.settle_panel_action(&completion.context, panel_result, true))
                })
                .unwrap_or_else(|_| {
                    self.settle_panel_action(&completion.context, Err(E::ExpiredPanelSession), true)
                })
        } else {
            self.settle_panel_action(&completion.context, panel_result, true)
        };
        if settled {
            if let Some((plugin_id, generation, activation_id)) =
                panel_owner.filter(|_| outcome.accepted)
            {
                let disabled = self
                    .manager()
                    .map_err(|_| PluginRuntimeError::Unavailable)?
                    .record_runtime_result(
                        &plugin_id,
                        generation,
                        activation_id,
                        panel_success,
                        now,
                    )
                    .map_err(|_| PluginRuntimeError::Unavailable)?;
                if disabled {
                    self.settle_panel_generation(&plugin_id, generation);
                    self.settle_plugin_submissions(&plugin_id);
                    Self::destroy_runtime(app, runtime_label(&plugin_id, generation).as_deref());
                    if let Some(controller) =
                        app.try_state::<Arc<crate::plugin_panel::PluginPanelController>>()
                    {
                        if let Some(identity) =
                            controller.teardown_generation(&plugin_id, generation)
                        {
                            crate::plugin_panel::destroy_content(app, &identity.content_label);
                        }
                    }
                    if let Some(controller) =
                        app.try_state::<Arc<crate::plugin_window::PluginWindowController>>()
                    {
                        crate::plugin_window::teardown(
                            app,
                            controller.inner(),
                            &plugin_id,
                            generation,
                        );
                    }
                    return Ok(None);
                }
            }
            return outcome
                .next
                .map(|next| self.bind_unsent_dispatch(next, now))
                .transpose()
                .map(Option::flatten)
                .map_err(|_| PluginRuntimeError::Unavailable);
        }
        self.complete_submission_with(completion, outcome, now, || {
            if let Some(controller) =
                app.try_state::<Arc<crate::plugin_panel::PluginPanelController>>()
            {
                if let Some(identity) = controller.teardown_generation(
                    &completion.context.plugin_id,
                    completion.context.plugin_generation,
                ) {
                    crate::plugin_panel::destroy_content(app, &identity.content_label);
                }
            }
            Self::destroy_runtime(
                app,
                runtime_label(
                    &completion.context.plugin_id,
                    completion.context.plugin_generation,
                )
                .as_deref(),
            );
            if let Some(controller) =
                app.try_state::<Arc<crate::plugin_window::PluginWindowController>>()
            {
                crate::plugin_window::teardown(
                    app,
                    controller.inner().as_ref(),
                    &completion.context.plugin_id,
                    completion.context.plugin_generation,
                );
            }
        })
    }

    fn complete_submission_with<F>(
        &self,
        completion: &PluginCommandCompletion,
        outcome: PluginCompletionOutcome,
        now: Instant,
        on_disabled: F,
    ) -> Result<Option<ScheduledPluginRequest>, PluginRuntimeError>
    where
        F: FnOnce(),
    {
        let mut runtime_success = None;
        let token = self
            .lock_submissions()
            .map_err(|_| PluginRuntimeError::Unavailable)?
            .token_by_request
            .remove(&completion.context);
        if let Some(token) = token {
            let pending = self
                .lock_submissions()
                .map_err(|_| PluginRuntimeError::Unavailable)?
                .by_token
                .remove(&token);
            if let Some(pending) = pending {
                let result = if !outcome.accepted {
                    None
                } else if completion.failed {
                    Some(Err(PluginRuntimeError::InvalidOperation))
                } else {
                    let value = completion
                        .response
                        .clone()
                        .ok_or(PluginRuntimeError::InvalidOperation);
                    let result = match pending.route.output_mode {
                        PublicOutputMode::MainResult => value
                            .and_then(|value| {
                                if self.manager().map_err(|_|PluginRuntimeError::Unavailable)?.result_actions_enabled(&pending.route){
                                    return result_action::parse(&completion.context,value).map(|results|PublicPluginResponse::MainActionResults{route:pending.route.clone(),results});
                                }
                                parse_main_result_response(&completion.context, value).map(PublicPluginResponse::MainResults)
                            })
                            .and_then(|results| {
                                if matches!(&results,PublicPluginResponse::MainResults(rows) if rows.iter().any(|result|result.copy_text.is_some()))
                                    && !self
                                        .manager()
                                        .map_err(|_| PluginRuntimeError::Unavailable)?
                                        .can_copy_text(
                                            &pending.route.plugin_id,
                                            &pending.route.command_id,
                                            pending.route.generation,
                                        )
                                {
                                    Err(PluginRuntimeError::InvalidOperation)
                                } else {
                                    Ok(results)
                                }
                            }),
                        PublicOutputMode::Window => value
                            .and_then(|value| parse_window_response(&completion.context, value))
                            .map(PublicPluginResponse::Window),
                        PublicOutputMode::Panel => value
                            .and_then(|value| parse_panel_response(&completion.context, value))
                            .map(PublicPluginResponse::Panel),
                    };
                    Some(result)
                };
                runtime_success = result.as_ref().map(|result| {
                    (
                        result.is_ok(),
                        pending.route.generation,
                        pending.route.activation_id,
                    )
                });
                let result = result.map(|result| {
                    result.map(|response| match (response, outcome.credential_publication) {
                        (PublicPluginResponse::MainResults(results), Some((credential_revision, publication))) => {
                            PublicPluginResponse::ServiceMainResults { results, credential_revision, publication }
                        }
                        (response, _) => response,
                    })
                });
                let _ = pending.sender.send(result);
            }
        }
        if let Some((success, generation, activation_id)) = runtime_success {
            let disabled = self
                .manager()
                .map_err(|_| PluginRuntimeError::Unavailable)?
                .record_runtime_result(
                    &completion.context.plugin_id,
                    generation,
                    activation_id,
                    success,
                    now,
                )
                .map_err(|_| PluginRuntimeError::Unavailable)?;
            if disabled {
                if let Some(next) = outcome.next.as_ref() {
                    self.settle_submission(
                        &next
                            .candidate
                            .owner
                            .command()
                            .expect("waiting request is a command")
                            .submission_token,
                        None,
                    );
                }
                on_disabled();
                return Ok(None);
            }
        }
        outcome
            .next
            .map(|next| self.bind_unsent_dispatch(next, now))
            .transpose()
            .map(Option::flatten)
            .map_err(|_| PluginRuntimeError::Unavailable)
    }

    fn expire_runtime_timeouts(
        self: &Arc<Self>,
        app: &AppHandle,
        now: Instant,
    ) -> Result<(), PublicPluginManagementError> {
        let replacements = self
            .manager()?
            .expire_network_timeouts(now)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        for replacement in replacements {
            self.settle_result_action(
                &replacement.expired,
                result_action::ResultActionErrorCode::ActionTimedOut,
            );
            self.settle_panel_action(
                &replacement.expired,
                Err(panel_action::PanelActionError::PanelRequestTimeoutError),
                true,
            );
            if let Some(controller) =
                app.try_state::<Arc<crate::plugin_panel::PluginPanelController>>()
            {
                if let Some(identity) = controller
                    .teardown_generation(&replacement.plugin_id, replacement.previous_generation)
                {
                    crate::plugin_panel::destroy_content(app, &identity.content_label);
                }
            }
            self.settle_request(
                &replacement.expired,
                Some(Err(PluginRuntimeError::Unavailable)),
            );
            Self::destroy_runtime(
                app,
                runtime_label(&replacement.plugin_id, replacement.previous_generation).as_deref(),
            );
            if let Some(controller) =
                app.try_state::<Arc<crate::plugin_window::PluginWindowController>>()
            {
                crate::plugin_window::teardown(
                    app,
                    controller.inner().as_ref(),
                    &replacement.plugin_id,
                    replacement.previous_generation,
                );
            }
            if replacement.counts_as_fault
                && self.manager()?.record_runtime_result(
                    &replacement.plugin_id,
                    replacement.previous_generation,
                    replacement.previous_activation_id,
                    false,
                    now,
                )?
            {
                self.settle_panel_generation(
                    &replacement.plugin_id,
                    replacement.previous_generation,
                );
                self.settle_plugin_submissions(&replacement.plugin_id);
                continue;
            }
            let candidate = match self.manager()?.replace_runtime_generation(
                &replacement.plugin_id,
                replacement.previous_generation,
                replacement.new_generation,
            ) {
                Ok(candidate) => candidate,
                Err(error) => {
                    let _ = self.manager()?.mark_runtime_unavailable(
                        &replacement.plugin_id,
                        replacement.previous_generation,
                        replacement.previous_activation_id,
                    );
                    self.settle_panel_generation(
                        &replacement.plugin_id,
                        replacement.previous_generation,
                    );
                    self.settle_plugin_submissions(&replacement.plugin_id);
                    return Err(error);
                }
            };
            if let Err(error) = self.create_runtime(app, &candidate) {
                let _ = self.manager()?.mark_runtime_unavailable(
                    &replacement.plugin_id,
                    candidate.generation,
                    candidate.activation_id,
                );
                self.settle_panel_generation(
                    &replacement.plugin_id,
                    replacement.previous_generation,
                );
                self.settle_plugin_submissions(&replacement.plugin_id);
                return Err(error);
            }
            let next = self
                .manager()?
                .finish_runtime_replacement(
                    &replacement.plugin_id,
                    replacement.new_generation,
                    candidate.activation_id,
                    candidate.admission_epoch,
                    now,
                )
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            if let Some(next) = next {
                let Some(next) = self.bind_unsent_dispatch(next, now)? else {
                    continue;
                };
                let settings = app.state::<crate::settings::SettingsStore>();
                let theme = crate::commands::invocation_theme(app, settings.snapshot().theme);
                let invoked_at = crate::commands::invoked_at_rfc3339();
                if let Err(error) = self.dispatch(app, &next, theme, invoked_at) {
                    self.fail_submission(
                        &next
                            .candidate
                            .owner
                            .command()
                            .expect("waiting request is a command")
                            .submission_token,
                    );
                    let _ = self.manager()?.mark_runtime_unavailable(
                        &replacement.plugin_id,
                        candidate.generation,
                        candidate.activation_id,
                    );
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn settle_panel_generation(&self, plugin_id: &str, generation: u64) {
        let result_context = self.result_action.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|p| {
                    p.context.plugin_id == plugin_id && p.context.plugin_generation == generation
                })
                .map(|p| p.context.clone())
        });
        if let Some(context) = result_context {
            self.settle_result_action(
                &context,
                result_action::ResultActionErrorCode::ActionCancelled,
            );
        }
        let contexts = self
            .content_actions
            .lock()
            .ok()
            .map(|map| {
                map.keys()
                    .filter(|c| c.plugin_id == plugin_id && c.plugin_generation == generation)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for context in contexts {
            self.settle_panel_action(
                &context,
                Err(panel_action::PanelActionError::ExpiredPanelSession),
                false,
            );
        }
    }

    fn settle_plugin_submissions(&self, plugin_id: &str) {
        self.settle_plugin_submissions_with(plugin_id, Some(Err(PluginRuntimeError::Unavailable)));
    }

    fn settle_plugin_submissions_with(
        &self,
        plugin_id: &str,
        result: Option<PublicSubmissionResult>,
    ) {
        let pending = self.lock_submissions().ok().map(|mut submissions| {
            let tokens = submissions
                .by_token
                .iter()
                .filter(|(_, pending)| pending.route.plugin_id == plugin_id)
                .map(|(token, _)| token.clone())
                .collect::<Vec<_>>();
            submissions
                .token_by_request
                .retain(|context, _| context.plugin_id != plugin_id);
            tokens
                .into_iter()
                .filter_map(|token| submissions.by_token.remove(&token))
                .collect::<Vec<_>>()
        });
        if let Some(pending) = pending {
            for pending in pending {
                let _ = pending.sender.send(result.clone());
            }
        }
    }
    pub(crate) fn fail_submission(&self, token: &str) {
        self.settle_submission(token, Some(Err(PluginRuntimeError::Unavailable)));
    }

    pub(crate) fn abort_submission_request(
        &self,
        context: &PluginRequestContext,
        submission_token: &str,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PublicPluginManagementError> {
        let mut next = self
            .manager()?
            .cancel_scheduled_request(context, now)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        self.settle_submission(submission_token, Some(Err(PluginRuntimeError::Unavailable)));
        while let Some(candidate) = next.as_ref() {
            if self
                .bind_request(
                    &candidate
                        .candidate
                        .owner
                        .command()
                        .expect("waiting request is a command")
                        .submission_token,
                    &candidate.context,
                )
                .is_ok()
            {
                break;
            }
            let token = candidate
                .candidate
                .owner
                .command()
                .expect("waiting request is a command")
                .submission_token
                .clone();
            let context = candidate.context.clone();
            self.settle_submission(&token, Some(Err(PluginRuntimeError::Unavailable)));
            next = self
                .manager()?
                .cancel_scheduled_request(&context, now)
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
        }
        Ok(next)
    }

    fn bind_unsent_dispatch(
        &self,
        request: ScheduledPluginRequest,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PublicPluginManagementError> {
        let token = request
            .candidate
            .owner
            .token()
            .ok_or(PublicPluginManagementError::Unavailable)?
            .to_owned();
        if self.bind_request(&token, &request.context).is_ok() {
            return Ok(Some(request));
        }
        self.abort_submission_request(&request.context, &token, now)
    }
    fn bind_request(
        &self,
        token: &str,
        context: &PluginRequestContext,
    ) -> Result<(), PublicPluginManagementError> {
        let mut submissions = self.lock_submissions()?;
        let pending = submissions
            .by_token
            .get_mut(token)
            .ok_or(PublicPluginManagementError::Unavailable)?;
        if pending.route.result_publication.is_some() {
            pending.route =
                self.manager()?
                    .bind_result_command_route(&pending.route, context, token)?;
        }
        submissions
            .token_by_request
            .insert(context.clone(), token.to_owned());
        Ok(())
    }

    fn settle_request(
        &self,
        context: &PluginRequestContext,
        result: Option<PublicSubmissionResult>,
    ) {
        self.settle_panel_action(
            context,
            Err(panel_action::PanelActionError::PanelRequestCancelledError),
            false,
        );
        let token = self
            .lock_submissions()
            .ok()
            .and_then(|mut submissions| submissions.token_by_request.remove(context));
        if let Some(token) = token {
            self.settle_submission(&token, result);
        }
    }
    fn settle_submission(&self, token: &str, result: Option<PublicSubmissionResult>) {
        let pending = self.lock_submissions().ok().and_then(|mut submissions| {
            submissions
                .token_by_request
                .retain(|_, mapped_token| mapped_token != token);
            submissions.by_token.remove(token)
        });
        if let Some(pending) = pending {
            let _ = pending.sender.send(result);
        }
    }

    fn lock_submissions(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, PublicSubmissionState>, PublicPluginManagementError> {
        self.submissions
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }

    fn lock_recoveries(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, RuntimeRecoveryState>, PublicPluginManagementError> {
        self.recoveries
            .lock()
            .map_err(|_| PublicPluginManagementError::Unavailable)
    }
    pub(crate) fn asset_response(
        &self,
        label: &str,
        path: &str,
        query: Option<&str>,
    ) -> Response<Vec<u8>> {
        if path.trim_start_matches('/') == webview_audio_guard::INERT_PATH {
            return Response::builder()
                .status(200)
                .header("content-type", "text/html; charset=utf-8")
                .header("x-content-type-options", "nosniff")
                .header(
                    "content-security-policy",
                    "default-src 'none'; media-src 'none'; base-uri 'none'; form-action 'none'",
                )
                .body(webview_audio_guard::INERT_DOCUMENT.as_bytes().to_vec())
                .unwrap();
        }
        if path.trim_start_matches('/') == alarm_asset::ALARM_PATH {
            return Response::builder().status(403).body(Vec::new()).unwrap();
        }
        if query.is_some() && icon::is_icon_request(path) {
            return Response::builder().status(403).body(Vec::new()).unwrap();
        }
        let manager = self.manager().ok();
        if let Some(asset) =
            manager.and_then(|manager| manager.icon_asset(label, path, Instant::now()))
        {
            return Response::builder()
                .status(200)
                .header("content-type", icon::ICON_MIME)
                .header("x-content-type-options", "nosniff")
                .header("cache-control", asset.cache_control)
                .body(asset.bytes)
                .unwrap();
        }
        if icon::is_icon_request(path) {
            return Response::builder().status(403).body(Vec::new()).unwrap();
        }
        let Some(asset) = manager.and_then(|manager| {
            manager
                .asset(label, path)
                .or_else(|| {
                    crate::plugin_window::plugin_id_from_content_label(label)
                        .and_then(|plugin_id| manager.window_asset(&plugin_id, path))
                })
                .or_else(|| {
                    crate::plugin_panel::plugin_id_from_panel_content_label(label)
                        .and_then(|plugin_id| manager.panel_asset(&plugin_id, path))
                })
        }) else {
            return Response::builder().status(403).body(Vec::new()).unwrap();
        };
        Response::builder()
            .status(200)
            .header("content-type", asset.mime)
            .header("x-content-type-options", "nosniff")
            .header("content-security-policy", PUBLIC_PLUGIN_CSP)
            .body(asset.bytes)
            .unwrap()
    }

    pub(crate) fn create_runtime(
        &self,
        app: &AppHandle,
        candidate: &PublicRuntimeCandidate,
    ) -> Result<WebviewWindow, PublicPluginManagementError> {
        let target_url =
            tauri::Url::parse("uipilot-public-plugin://localhost/__uipilot_runtime.html")
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let inert_url = inert_url().map_err(|_| PublicPluginManagementError::RuntimeNotReady)?;
        let ready = Arc::new((Mutex::new(None), Condvar::new()));
        let title_ready = Arc::clone(&ready);
        let window = WebviewWindowBuilder::new(
            app,
            candidate.label.clone(),
            WebviewUrl::CustomProtocol(inert_url),
        )
        .visible(false)
        .focusable(false)
        .skip_taskbar(true)
        .incognito(true)
        .on_navigation(public_runtime_navigation_allowed)
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_document_title_changed(move |_, title| {
            let settled = match title.as_str() {
                "uipilot-public-plugin-ready" => Some(true),
                "uipilot-public-plugin-failed" => Some(false),
                _ => None,
            };
            if let Some(settled) = settled {
                if let Ok(mut state) = title_ready.0.lock() {
                    *state = Some(settled);
                    title_ready.1.notify_all();
                }
            }
        })
        .build()
        .map_err(|_| PublicPluginManagementError::RuntimeNotReady)?;
        let unmute_app = app.clone();
        let on_unmuted = Arc::new(move |owner| {
            let WebViewGuardOwner::Runtime {
                label,
                plugin_id,
                generation,
                activation_id,
            } = owner
            else {
                return;
            };
            if let Some(service) = unmute_app.try_state::<Arc<PublicPluginService>>() {
                if let Ok(manager) = service.manager() {
                    let _ = manager.mark_runtime_unavailable(&plugin_id, generation, activation_id);
                }
                service.settle_panel_generation(&plugin_id, generation);
                if let Some(controller) =
                    unmute_app.try_state::<Arc<crate::plugin_panel::PluginPanelController>>()
                {
                    if let Some(identity) = controller.teardown_generation(&plugin_id, generation) {
                        crate::plugin_panel::destroy_content(&unmute_app, &identity.content_label);
                    }
                }
                service.settle_plugin_submissions(&plugin_id);
            }
            PublicPluginService::destroy_runtime(&unmute_app, Some(&label));
        });
        if prepare_windows_webview(
            window.as_ref(),
            Arc::clone(&self.webview_guards),
            WebViewGuardOwner::Runtime {
                label: candidate.label.clone(),
                plugin_id: candidate.plugin_id.clone(),
                generation: candidate.generation,
                activation_id: candidate.activation_id,
            },
            runtime::public_runtime_bootstrap_with_result_actions(
                candidate.network_https_declared,
                candidate.panel_actions,
                candidate.window_actions,
                candidate.result_actions,
            ),
            target_url,
            on_unmuted,
            PUBLIC_RUNTIME_READY_TIMEOUT,
        )
        .is_err()
        {
            if let Ok(manager) = self.manager() {
                manager.invalidate_runtime_generation(&candidate.plugin_id, candidate.generation);
            }
            let _ = window.destroy();
            return Err(PublicPluginManagementError::RuntimeNotReady);
        }
        let settled = ready
            .1
            .wait_timeout_while(
                ready
                    .0
                    .lock()
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
                PUBLIC_RUNTIME_READY_TIMEOUT,
                |state| state.is_none(),
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .0;
        if *settled == Some(true) {
            Ok(window)
        } else {
            if let Ok(manager) = self.manager() {
                manager.invalidate_runtime_generation(&candidate.plugin_id, candidate.generation);
            }
            let _ = window.destroy();
            Err(PublicPluginManagementError::RuntimeNotReady)
        }
    }

    pub(crate) fn destroy_runtime(app: &AppHandle, label: Option<&str>) {
        if let Some(label) = label {
            if let Some(service) = app.try_state::<Arc<PublicPluginService>>() {
                service.invalidate_runtime_label(label);
            }
        }
        if let Some(window) = label.and_then(|label| app.get_webview_window(label)) {
            let _ = window.destroy();
        }
    }

    fn invalidate_runtime_label(&self, label: &str) {
        let Some(identity) = parse_runtime_label(label) else {
            return;
        };
        if let Ok(manager) = self.manager() {
            manager.invalidate_runtime_generation(&identity.plugin_id, identity.generation);
        }
    }

    pub(crate) fn webview_guards(&self) -> Arc<WebViewGuardAuthority> {
        Arc::clone(&self.webview_guards)
    }
}

fn public_runtime_navigation_allowed(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "uipilot-public-plugin" | "http")
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host.eq_ignore_ascii_case("uipilot-public-plugin.localhost")
        })
        && url.port().is_none()
}

#[allow(unused_imports)]
pub(crate) use data_scope::{PluginDataScope, InvalidPluginScope};
use data_scope::{authorize_plugin_scope, valid_json_value};

#[derive(Clone, Debug)]
pub(crate) enum PublicPackageSource {
    Archive(PathBuf),
    DevelopmentDirectory(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicResource {
    pub(crate) mime: &'static str,
    pub(crate) length: u64,
    pub(crate) sha256: String,
}

#[derive(Debug)]
pub(crate) struct PreparedPublicPlugin {
    transaction_root: Option<PathBuf>,
    pub(crate) package_root: PathBuf,
    pub(crate) manifest: PublicManifestV1,
    pub(crate) digest: String,
    pub(crate) resources: BTreeMap<String, PublicResource>,
    package_resources: BTreeMap<String, PublicResource>,
    pub(crate) alarm: Option<PreparedAlarmAsset>,
}

impl PreparedPublicPlugin {
    fn new(
        transaction_root: PathBuf,
        package_root: PathBuf,
        manifest: PublicManifestV1,
        digest: String,
        resources: BTreeMap<String, PublicResource>,
        package_resources: BTreeMap<String, PublicResource>,
        alarm: Option<PreparedAlarmAsset>,
    ) -> Self {
        Self {
            transaction_root: Some(transaction_root),
            package_root,
            manifest,
            digest,
            resources,
            package_resources,
            alarm,
        }
    }

    #[cfg(test)]
    pub(crate) fn transaction_root(&self) -> &Path {
        self.transaction_root
            .as_deref()
            .expect("prepared transaction root missing")
    }

    pub(crate) fn revalidate(&self) -> Result<(), PublicPackageError> {
        package::revalidate_snapshot(&self.package_root, &self.digest, &self.package_resources)?;
        if let Some(alarm) = &self.alarm {
            alarm.revalidate_at(&self.package_root)?;
        }
        Ok(())
    }

    pub(crate) fn persist(mut self, destination: &Path) -> Result<bool, PublicPackageError> {
        self.revalidate()?;
        if destination.exists() {
            package::revalidate_snapshot(destination, &self.digest, &self.package_resources)?;
            if let Some(alarm) = &self.alarm {
                alarm.revalidate_at(destination)?;
            }
            return Ok(false);
        }
        let parent = destination
            .parent()
            .ok_or(PublicPackageError::InvalidPackage)?;
        std::fs::create_dir_all(parent).map_err(|_| PublicPackageError::InvalidPackage)?;
        let transaction_root = self
            .transaction_root
            .take()
            .expect("prepared transaction root missing");
        if std::fs::rename(&self.package_root, destination).is_err() {
            package::remove_transaction(transaction_root);
            return Err(PublicPackageError::InvalidPackage);
        }
        package::remove_transaction(transaction_root);
        Ok(true)
    }
}

impl Drop for PreparedPublicPlugin {
    fn drop(&mut self) {
        if let Some(path) = self.transaction_root.take() {
            package::remove_transaction(path);
        }
    }
}

pub(crate) fn stage_public_package(
    source: PublicPackageSource,
    staging_root: &Path,
    host: &PublicPluginHost,
) -> Result<PreparedPublicPlugin, PublicPackageError> {
    package::stage(source, staging_root, host)
}
