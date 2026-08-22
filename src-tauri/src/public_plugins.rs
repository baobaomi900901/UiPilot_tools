mod activation;
mod activation_bundle;
mod alarm_asset;
mod delayed_messages;
mod icon;
mod manifest;
mod package;
mod runtime;
mod scheduler;
mod secrets;
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

pub(crate) use activation::{
    parse_main_result_response, parse_window_response, PublicCommandSuggestion, PublicMainResult,
    PublicPluginInstallSource, PublicPluginInventory, PublicPluginManagementError,
    PublicPluginManager, PublicPluginMutation, PublicPluginPrepareSummary, PublicPluginRoute,
    PublicPluginWindowIdentity, PublicRuntimeCandidate, PublicWindowResponse,
};
pub(crate) use alarm_asset::{PreparedAlarmAsset, ValidatedAlarmAsset};
pub(crate) use manifest::{
    public_manifest_v1_schema, PublicActivationMode, PublicManifestV1, PublicOutputMode,
    PublicPermission, PublicPlatform,
};
pub(crate) use runtime::{
    parse_runtime_label, runtime_label, PluginApiExecution, PluginApiRequest,
    PluginCommandCompletion, PluginCommandDispatch, PluginInvocation, PluginInvocationEnvironment,
    PluginInvocationPlatform, PluginInvocationTheme, PluginRuntimeApi, PluginRuntimeError,
    PUBLIC_RUNTIME_BOOTSTRAP,
};
pub(crate) use scheduler::{
    PluginCompletionOutcome, PluginContextStatus, PluginRequestCandidate, PluginRequestContext,
    PluginRequestScheduler, PluginScheduleOutcome, PluginSubmissionOwner, ScheduledPluginRequest,
};
pub(crate) use secrets::PluginSecretStore;
pub(crate) use state::{
    EffectivePluginConfig, PluginStateError, PluginStateStore, PublicPluginFault,
};
pub(crate) use storage::PluginStorageStore;
pub(crate) use timers::{
    AudioTicket, PluginTimerService, PluginTimerStartInput, PluginTimerState, TimerError, TimerKey,
};
pub(crate) use webview_audio_guard::{
    inert_url, prepare_windows_webview, verify_windows_webview_muted, WebViewGuardAuthority,
    WebViewGuardOwner,
};
const PUBLIC_RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_PLUGIN_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; img-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PublicPluginResponse {
    MainResults(Vec<PublicMainResult>),
    Window(PublicWindowResponse),
}

pub(crate) type PublicSubmissionResult = Result<PublicPluginResponse, PluginRuntimeError>;

struct PendingPublicSubmission {
    route: PublicPluginRoute,
    sender: mpsc::Sender<Option<PublicSubmissionResult>>,
}

#[derive(Default)]
struct PublicSubmissionState {
    by_token: HashMap<String, PendingPublicSubmission>,
    token_by_request: HashMap<PluginRequestContext, String>,
}

#[derive(Default)]
pub(crate) struct PublicPluginService {
    manager: OnceLock<Arc<PublicPluginManager>>,
    submissions: Mutex<PublicSubmissionState>,
    next_submission: AtomicU64,
    startup_runtimes_started: AtomicBool,
    webview_guards: Arc<WebViewGuardAuthority>,
}

pub(crate) struct PublicSubmission {
    pub(crate) token: String,
    pub(crate) receiver: mpsc::Receiver<Option<PublicSubmissionResult>>,
    pub(crate) dispatch: Option<ScheduledPluginRequest>,
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
        let manager = Arc::new(PublicPluginManager::load(
            app_data_dir,
            PublicPluginHost::current(PublicPlatform::Windows),
            reserved_names,
            Arc::clone(&message_center),
        )?);
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

    pub(crate) fn shutdown(&self) {
        if let Some(manager) = self.manager.get() {
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
        let outcome = self
            .manager()?
            .scheduler()
            .enqueue(
                PluginRequestCandidate {
                    plugin_id: route.plugin_id.clone(),
                    plugin_generation: route.generation,
                    activation_id: route.activation_id,
                    activation_mode: route.activation_mode,
                    input: route.input.clone(),
                    owner: PluginSubmissionOwner {
                        ui_intent_epoch,
                        control_value,
                        submission_token: token.clone(),
                    },
                },
                now,
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let dispatch = match outcome {
            PluginScheduleOutcome::Dispatched(request) => {
                self.bind_request(&token, &request.context)?;
                Some(request)
            }
            PluginScheduleOutcome::Waiting {
                expired,
                replaced_submission_token,
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
        })
    }

    pub(crate) fn dispatch(
        self: &Arc<Self>,
        app: &AppHandle,
        request: &ScheduledPluginRequest,
        theme: PluginInvocationTheme,
        invoked_at: String,
    ) -> Result<(), PublicPluginManagementError> {
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
                    invocation: PluginInvocation {
                        api_version: 1,
                        request_id: request.context.request_id.clone(),
                        input: request.candidate.input.clone(),
                        context: PluginInvocationEnvironment {
                            platform: PluginInvocationPlatform::Windows,
                            theme,
                            invoked_at,
                        },
                    },
                },
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let service = Arc::clone(self);
        let app = app.clone();
        let deadline = request.deadline;
        thread::spawn(move || {
            thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let _ = service.expire_runtime_timeouts(&app, Instant::now());
        });
        Ok(())
    }

    pub(crate) fn complete_submission(
        &self,
        app: &AppHandle,
        completion: &PluginCommandCompletion,
        outcome: PluginCompletionOutcome,
        now: Instant,
    ) -> Result<Option<ScheduledPluginRequest>, PluginRuntimeError> {
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
                                parse_main_result_response(&completion.context, value)
                            })
                            .and_then(|results| {
                                if results.iter().any(|result| result.copy_text.is_some())
                                    && !self
                                        .manager()
                                        .map_err(|_| PluginRuntimeError::Unavailable)?
                                        .can_copy_text(
                                            &pending.route.plugin_id,
                                            pending.route.generation,
                                        )
                                {
                                    Err(PluginRuntimeError::InvalidOperation)
                                } else {
                                    Ok(PublicPluginResponse::MainResults(results))
                                }
                            }),
                        PublicOutputMode::Window => value
                            .and_then(|value| parse_window_response(&completion.context, value))
                            .map(PublicPluginResponse::Window),
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
                    self.settle_submission(&next.candidate.owner.submission_token, None);
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
                    crate::plugin_window::teardown_current(
                        app,
                        controller.inner().as_ref(),
                        &completion.context.plugin_id,
                    );
                }
                return Ok(None);
            }
        }
        if let Some(next) = outcome.next.as_ref() {
            self.bind_request(&next.candidate.owner.submission_token, &next.context)
                .map_err(|_| PluginRuntimeError::Unavailable)?;
        }
        Ok(outcome.next)
    }

    fn expire_runtime_timeouts(
        self: &Arc<Self>,
        app: &AppHandle,
        now: Instant,
    ) -> Result<(), PublicPluginManagementError> {
        let replacements = self
            .manager()?
            .scheduler()
            .expire_timeouts(now)
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        for replacement in replacements {
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
                crate::plugin_window::teardown_current(
                    app,
                    controller.inner().as_ref(),
                    &replacement.plugin_id,
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
                self.settle_plugin_submissions(&replacement.plugin_id);
                return Err(error);
            }
            let next = self
                .manager()?
                .scheduler()
                .runtime_replaced(
                    &replacement.plugin_id,
                    replacement.new_generation,
                    candidate.activation_id,
                    now,
                )
                .map_err(|_| PublicPluginManagementError::Unavailable)?;
            if let Some(next) = next {
                self.bind_request(&next.candidate.owner.submission_token, &next.context)?;
                let settings = app.state::<crate::settings::SettingsStore>();
                let theme = crate::commands::invocation_theme(app, settings.snapshot().theme);
                let invoked_at = crate::commands::invoked_at_rfc3339();
                if let Err(error) = self.dispatch(app, &next, theme, invoked_at) {
                    self.fail_submission(&next.candidate.owner.submission_token);
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

    fn settle_plugin_submissions(&self, plugin_id: &str) {
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
                let _ = pending
                    .sender
                    .send(Some(Err(PluginRuntimeError::Unavailable)));
            }
        }
    }
    pub(crate) fn fail_submission(&self, token: &str) {
        self.settle_submission(token, Some(Err(PluginRuntimeError::Unavailable)));
    }

    fn bind_request(
        &self,
        token: &str,
        context: &PluginRequestContext,
    ) -> Result<(), PublicPluginManagementError> {
        let mut submissions = self.lock_submissions()?;
        if !submissions.by_token.contains_key(token) {
            return Err(PublicPluginManagementError::Unavailable);
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
        let token = self
            .lock_submissions()
            .ok()
            .and_then(|mut submissions| submissions.token_by_request.remove(context));
        if let Some(token) = token {
            self.settle_submission(&token, result);
        }
    }
    fn settle_submission(&self, token: &str, result: Option<PublicSubmissionResult>) {
        let pending = self
            .lock_submissions()
            .ok()
            .and_then(|mut submissions| submissions.by_token.remove(token));
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
            manager.asset(label, path).or_else(|| {
                crate::plugin_window::plugin_id_from_content_label(label)
                    .and_then(|plugin_id| manager.window_asset(&plugin_id, path))
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
            PUBLIC_RUNTIME_BOOTSTRAP,
            target_url,
            on_unmuted,
            PUBLIC_RUNTIME_READY_TIMEOUT,
        )
        .is_err()
        {
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
            let _ = window.destroy();
            Err(PublicPluginManagementError::RuntimeNotReady)
        }
    }

    pub(crate) fn destroy_runtime(app: &AppHandle, label: Option<&str>) {
        if let Some(window) = label.and_then(|label| app.get_webview_window(label)) {
            let _ = window.destroy();
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginDataScope {
    plugin_id: String,
}

impl PluginDataScope {
    pub(crate) fn new(plugin_id: &str) -> Result<Self, PublicPackageError> {
        manifest::valid_plugin_id(plugin_id)
            .then(|| Self {
                plugin_id: plugin_id.into(),
            })
            .ok_or(PublicPackageError::InvalidPackage)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidPluginScope;

fn authorize_plugin_scope(
    scope: &PluginDataScope,
    plugin_id: &str,
) -> Result<(), InvalidPluginScope> {
    (scope.plugin_id == plugin_id)
        .then_some(())
        .ok_or(InvalidPluginScope)
}

fn valid_json_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => true,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(f64::is_finite),
        serde_json::Value::Array(values) => values.iter().all(valid_json_value),
        serde_json::Value::Object(values) => values.iter().all(|(key, value)| {
            !matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                && valid_json_value(value)
        }),
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PublicPackageSource {
    Archive(PathBuf),
    DevelopmentDirectory(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PublicPluginHost {
    pub(crate) platform: PublicPlatform,
    pub(crate) version: [u32; 3],
    pub(crate) api_version: u32,
}

impl PublicPluginHost {
    pub(crate) const fn current(platform: PublicPlatform) -> Self {
        Self {
            platform,
            version: [0, 2, 0],
            api_version: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublicPackageError {
    InvalidPackage,
    IncompatiblePlatform,
    IncompatibleApi,
    UnsupportedPermission,
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
