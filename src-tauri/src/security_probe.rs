use std::{
    cmp::min,
    sync::mpsc::{self, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tauri::{webview::PageLoadEvent, App, WebviewUrl, WebviewWindowBuilder};

const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const CALLBACK_TIMEOUT: Duration = Duration::from_millis(250);
const RETRY_DELAY: Duration = Duration::from_millis(50);

const READ_OBSERVATION_SCRIPT: &str = r#"
(() => {
  try {
    return {
      documentReadyState: document.readyState,
      probeState: window.__UIPILOT_SECURITY_PROBE_STATE__ ?? null,
      hash: window.location.hash,
      evalError: null
    };
  } catch (_) {
    return {
      documentReadyState: 'loading',
      probeState: null,
      hash: '',
      evalError: 'script-failed'
    };
  }
})()
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DocumentReadyState {
    Loading,
    Interactive,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ProbeState {
    InvokePending,
    InvokeSucceeded,
    AclDenied,
    InvokeOtherError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum EvalError {
    ScriptFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeObservation {
    document_ready_state: DocumentReadyState,
    probe_state: Option<ProbeState>,
    hash: String,
    eval_error: Option<EvalError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LegacyUrlResult {
    Matched,
    Missed,
    QueryFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntermediateState {
    ScriptNotStarted,
    InvokePending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeOutcome {
    LegacyUrlChannelMatched,
    PageLoadTimeout,
    ProbeScriptNotStarted,
    InvokeTimeout,
    InvokeUnexpectedlyAllowed,
    InvokeOtherError,
    LegacyUrlChannelMiss,
    LegacyUrlQueryFailed,
    EvalDispatchFailed,
    EvalCallbackTimeout,
    InvalidEvalObservation,
    EvalScriptFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeDecision {
    Retry(IntermediateState),
    Exit(ProbeOutcome),
}

#[derive(Clone, Copy)]
struct ProbeTiming {
    total_timeout: Duration,
    callback_timeout: Duration,
    retry_delay: Duration,
}

#[tauri::command]
pub(crate) fn load_settings() -> &'static str {
    "unexpectedly allowed"
}

pub(crate) fn setup(app: &mut App) -> tauri::Result<()> {
    let (page_load_sender, page_load_receiver) = mpsc::sync_channel(1);
    let page_load_guard = page_load_sender.clone();
    let probe = WebviewWindowBuilder::new(
        app,
        "security-probe",
        WebviewUrl::App("security-probe.html".into()),
    )
    .title("UiPilot security probe")
    .visible(false)
    .on_page_load(move |window, payload| {
        if window.label() == "security-probe" && payload.event() == PageLoadEvent::Finished {
            let _ = page_load_sender.try_send(());
        }
    })
    .build()?;

    thread::spawn(move || {
        let _page_load_guard = page_load_guard;
        let deadline = Instant::now() + TOTAL_TIMEOUT;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let page_loaded =
            !remaining.is_zero() && page_load_receiver.recv_timeout(remaining).is_ok();

        let outcome = if page_loaded {
            let timing = ProbeTiming {
                total_timeout: deadline.saturating_duration_since(Instant::now()),
                callback_timeout: CALLBACK_TIMEOUT,
                retry_delay: RETRY_DELAY,
            };
            let eval_probe = probe.clone();
            let legacy_probe = probe;

            run_eval_loop_with(
                timing,
                move |sender| {
                    eval_probe
                        .eval_with_callback(READ_OBSERVATION_SCRIPT, move |raw| {
                            let _ = sender.try_send(raw);
                        })
                        .map_err(|_| ())
                },
                move |observation| {
                    let legacy = if is_strict_acl(observation) {
                        Some(match legacy_probe.url() {
                            Ok(url) if url.fragment() == Some("acl-denied") => {
                                LegacyUrlResult::Matched
                            }
                            Ok(_) => LegacyUrlResult::Missed,
                            Err(_) => LegacyUrlResult::QueryFailed,
                        })
                    } else {
                        None
                    };
                    classify_diagnostic(true, observation, legacy)
                },
            )
        } else {
            ProbeOutcome::PageLoadTimeout
        };

        std::process::exit(exit_code(outcome));
    });

    Ok(())
}

fn parse_observation(raw: &str) -> Result<ProbeObservation, ProbeOutcome> {
    serde_json::from_str(raw).map_err(|_| ProbeOutcome::InvalidEvalObservation)
}

fn classify_diagnostic(
    page_load_finished: bool,
    observation: &ProbeObservation,
    legacy_url_result: Option<LegacyUrlResult>,
) -> ProbeDecision {
    if !page_load_finished {
        return ProbeDecision::Exit(ProbeOutcome::PageLoadTimeout);
    }
    if observation.eval_error == Some(EvalError::ScriptFailed) {
        return ProbeDecision::Exit(ProbeOutcome::EvalScriptFailed);
    }

    match observation.probe_state {
        None => ProbeDecision::Retry(IntermediateState::ScriptNotStarted),
        Some(ProbeState::InvokePending) => ProbeDecision::Retry(IntermediateState::InvokePending),
        Some(ProbeState::InvokeSucceeded) => {
            ProbeDecision::Exit(ProbeOutcome::InvokeUnexpectedlyAllowed)
        }
        Some(ProbeState::InvokeOtherError) => ProbeDecision::Exit(ProbeOutcome::InvokeOtherError),
        Some(ProbeState::AclDenied) if is_strict_acl(observation) => match legacy_url_result {
            Some(LegacyUrlResult::Matched) => {
                ProbeDecision::Exit(ProbeOutcome::LegacyUrlChannelMatched)
            }
            Some(LegacyUrlResult::Missed) => {
                ProbeDecision::Exit(ProbeOutcome::LegacyUrlChannelMiss)
            }
            Some(LegacyUrlResult::QueryFailed) => {
                ProbeDecision::Exit(ProbeOutcome::LegacyUrlQueryFailed)
            }
            None => ProbeDecision::Exit(ProbeOutcome::InvalidEvalObservation),
        },
        Some(ProbeState::AclDenied) => ProbeDecision::Exit(ProbeOutcome::InvalidEvalObservation),
    }
}

fn is_strict_acl(observation: &ProbeObservation) -> bool {
    matches!(
        observation.document_ready_state,
        DocumentReadyState::Interactive | DocumentReadyState::Complete
    ) && observation.probe_state == Some(ProbeState::AclDenied)
        && observation.hash == "#acl-denied"
        && observation.eval_error.is_none()
}

fn exit_code(outcome: ProbeOutcome) -> i32 {
    match outcome {
        ProbeOutcome::LegacyUrlChannelMatched => 73,
        ProbeOutcome::PageLoadTimeout => 74,
        ProbeOutcome::ProbeScriptNotStarted => 75,
        ProbeOutcome::InvokeTimeout => 76,
        ProbeOutcome::InvokeUnexpectedlyAllowed => 77,
        ProbeOutcome::InvokeOtherError => 78,
        ProbeOutcome::LegacyUrlChannelMiss => 79,
        ProbeOutcome::LegacyUrlQueryFailed => 80,
        ProbeOutcome::EvalDispatchFailed => 81,
        ProbeOutcome::EvalCallbackTimeout => 82,
        ProbeOutcome::InvalidEvalObservation => 83,
        ProbeOutcome::EvalScriptFailed => 84,
    }
}

fn run_eval_loop_with<D, C>(timing: ProbeTiming, mut dispatch: D, mut classify: C) -> ProbeOutcome
where
    D: FnMut(SyncSender<String>) -> Result<(), ()>,
    C: FnMut(&ProbeObservation) -> ProbeDecision,
{
    let deadline = Instant::now() + timing.total_timeout;
    let mut last_intermediate = IntermediateState::ScriptNotStarted;

    loop {
        if deadline.saturating_duration_since(Instant::now()).is_zero() {
            return intermediate_outcome(last_intermediate);
        }

        let (sender, receiver) = mpsc::sync_channel(1);
        if dispatch(sender).is_err() {
            return ProbeOutcome::EvalDispatchFailed;
        }

        let post_dispatch_remaining = deadline.saturating_duration_since(Instant::now());
        if post_dispatch_remaining.is_zero() {
            return intermediate_outcome(last_intermediate);
        }

        let full_callback_budget = post_dispatch_remaining >= timing.callback_timeout;
        let callback_wait = min(timing.callback_timeout, post_dispatch_remaining);
        let raw = match receiver.recv_timeout(callback_wait) {
            Ok(raw) => raw,
            Err(RecvTimeoutError::Timeout) if full_callback_budget => {
                return ProbeOutcome::EvalCallbackTimeout;
            }
            Err(RecvTimeoutError::Timeout) => {
                return intermediate_outcome(last_intermediate);
            }
            Err(RecvTimeoutError::Disconnected) => {
                return ProbeOutcome::EvalCallbackTimeout;
            }
        };

        if deadline.saturating_duration_since(Instant::now()).is_zero() {
            return intermediate_outcome(last_intermediate);
        }

        let observation = match parse_observation(&raw) {
            Ok(observation) => observation,
            Err(outcome) => return outcome,
        };

        match classify(&observation) {
            ProbeDecision::Exit(outcome) => {
                if deadline.saturating_duration_since(Instant::now()).is_zero() {
                    return intermediate_outcome(last_intermediate);
                }
                return outcome;
            }
            ProbeDecision::Retry(intermediate) => {
                last_intermediate = intermediate;
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return intermediate_outcome(last_intermediate);
        }
        let retry_wait = min(timing.retry_delay, remaining);
        thread::sleep(retry_wait);
        if retry_wait < timing.retry_delay {
            return intermediate_outcome(last_intermediate);
        }
    }
}

fn intermediate_outcome(state: IntermediateState) -> ProbeOutcome {
    match state {
        IntermediateState::ScriptNotStarted => ProbeOutcome::ProbeScriptNotStarted,
        IntermediateState::InvokePending => ProbeOutcome::InvokeTimeout,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    const EXACT_ACL_JSON: &str = r##"{
        "documentReadyState":"complete",
        "probeState":"acl-denied",
        "hash":"#acl-denied",
        "evalError":null
    }"##;
    const PENDING_JSON: &str = r##"{
        "documentReadyState":"complete",
        "probeState":"invoke-pending",
        "hash":"",
        "evalError":null
    }"##;
    const SCRIPT_FAILED_JSON: &str = r##"{
        "documentReadyState":"loading",
        "probeState":null,
        "hash":"",
        "evalError":"script-failed"
    }"##;

    #[test]
    fn diagnostic_exact_acl_and_legacy_match_is_73() {
        assert_eq!(
            exit_code(classify_diagnostic_exact(LegacyUrlResult::Matched)),
            73
        );
    }

    #[test]
    fn diagnostic_exact_acl_and_legacy_miss_is_79() {
        assert_eq!(
            exit_code(classify_diagnostic_exact(LegacyUrlResult::Missed)),
            79
        );
    }

    #[test]
    fn diagnostic_exact_acl_and_legacy_error_is_80() {
        assert_eq!(
            exit_code(classify_diagnostic_exact(LegacyUrlResult::QueryFailed)),
            80
        );
    }

    #[test]
    fn parser_rejects_null_strings_arrays_invalid_json_and_unknown_fields() {
        for raw in [
            "null",
            "\"acl-denied\"",
            "[]",
            "{",
            r##"{"documentReadyState":"complete","probeState":"acl-denied","hash":"#acl-denied","evalError":null,"extra":true}"##,
        ] {
            assert_eq!(
                parse_observation(raw),
                Err(ProbeOutcome::InvalidEvalObservation)
            );
        }
    }

    #[test]
    fn parser_rejects_missing_fields_and_unknown_enum_values() {
        for raw in [
            r##"{"probeState":"acl-denied","hash":"#acl-denied","evalError":null}"##,
            r##"{"documentReadyState":"ready","probeState":"acl-denied","hash":"#acl-denied","evalError":null}"##,
            r##"{"documentReadyState":"complete","probeState":"other","hash":"#acl-denied","evalError":null}"##,
            r##"{"documentReadyState":"complete","probeState":"acl-denied","hash":"#acl-denied","evalError":"unknown"}"##,
        ] {
            assert_eq!(
                parse_observation(raw),
                Err(ProbeOutcome::InvalidEvalObservation)
            );
        }
    }

    #[test]
    fn diagnostic_terminal_and_retry_matrix_is_exact() {
        let mut observation = exact_acl_observation();
        assert_eq!(
            classify_diagnostic(false, &observation, Some(LegacyUrlResult::Matched)),
            ProbeDecision::Exit(ProbeOutcome::PageLoadTimeout)
        );

        observation.probe_state = None;
        observation.hash.clear();
        assert_eq!(
            classify_diagnostic(true, &observation, None),
            ProbeDecision::Retry(IntermediateState::ScriptNotStarted)
        );

        observation.probe_state = Some(ProbeState::InvokePending);
        assert_eq!(
            classify_diagnostic(true, &observation, None),
            ProbeDecision::Retry(IntermediateState::InvokePending)
        );

        observation.probe_state = Some(ProbeState::InvokeSucceeded);
        assert_eq!(
            classify_diagnostic(true, &observation, None),
            ProbeDecision::Exit(ProbeOutcome::InvokeUnexpectedlyAllowed)
        );

        observation.probe_state = Some(ProbeState::InvokeOtherError);
        assert_eq!(
            classify_diagnostic(true, &observation, None),
            ProbeDecision::Exit(ProbeOutcome::InvokeOtherError)
        );
    }

    #[test]
    fn diagnostic_never_accepts_non_strict_acl_observations() {
        let exact = exact_acl_observation();
        let mut invalid = Vec::new();

        let mut loading = exact.clone();
        loading.document_ready_state = DocumentReadyState::Loading;
        invalid.push(loading);

        for hash in [
            "",
            "#ACL-DENIED",
            " #acl-denied",
            "%23acl-denied",
            "#acl-denied-extra",
        ] {
            let mut observation = exact.clone();
            observation.hash = hash.into();
            invalid.push(observation);
        }

        for state in [
            None,
            Some(ProbeState::InvokePending),
            Some(ProbeState::InvokeSucceeded),
            Some(ProbeState::InvokeOtherError),
        ] {
            let mut observation = exact.clone();
            observation.probe_state = state;
            invalid.push(observation);
        }

        let mut script_failed = exact;
        script_failed.eval_error = Some(EvalError::ScriptFailed);
        invalid.push(script_failed);

        for observation in invalid {
            let decision = classify_diagnostic(true, &observation, Some(LegacyUrlResult::Matched));
            assert_ne!(
                decision,
                ProbeDecision::Exit(ProbeOutcome::LegacyUrlChannelMatched)
            );
        }
    }

    #[test]
    fn callback_timeout_is_terminal_and_dispatches_once() {
        let dispatches = AtomicUsize::new(0);
        let outcome = run_eval_loop_with(
            test_timing(),
            |sender| {
                dispatches.fetch_add(1, Ordering::SeqCst);
                hold_sender(sender, Duration::from_millis(50));
                Ok(())
            },
            |observation| classify_diagnostic(true, observation, None),
        );

        assert_eq!(outcome, ProbeOutcome::EvalCallbackTimeout);
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callback_wait_is_capped_by_remaining_deadline_and_dispatches_once() {
        let dispatches = AtomicUsize::new(0);
        let started = Instant::now();
        let outcome = run_eval_loop_with(
            ProbeTiming {
                total_timeout: Duration::from_millis(20),
                callback_timeout: Duration::from_millis(250),
                retry_delay: Duration::from_millis(1),
            },
            |sender| {
                dispatches.fetch_add(1, Ordering::SeqCst);
                hold_sender(sender, Duration::from_millis(100));
                Ok(())
            },
            |observation| classify_diagnostic(true, observation, None),
        );

        assert_eq!(outcome, ProbeOutcome::ProbeScriptNotStarted);
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn retry_wait_is_capped_by_remaining_deadline_without_redispatch() {
        let dispatches = AtomicUsize::new(0);
        let started = Instant::now();
        let outcome = run_eval_loop_with(
            ProbeTiming {
                total_timeout: Duration::from_millis(100),
                callback_timeout: Duration::from_millis(20),
                retry_delay: Duration::from_millis(250),
            },
            |sender| {
                dispatches.fetch_add(1, Ordering::SeqCst);
                sender.try_send(PENDING_JSON.into()).unwrap();
                Ok(())
            },
            |observation| classify_diagnostic(true, observation, None),
        );

        assert_eq!(outcome, ProbeOutcome::InvokeTimeout);
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        assert!(started.elapsed() < Duration::from_millis(180));
    }

    #[test]
    fn callback_budget_is_recomputed_after_dispatch_returns() {
        let dispatches = AtomicUsize::new(0);
        let started = Instant::now();
        let outcome = run_eval_loop_with(
            ProbeTiming {
                total_timeout: Duration::from_millis(200),
                callback_timeout: Duration::from_millis(250),
                retry_delay: Duration::from_millis(1),
            },
            |sender| {
                dispatches.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(150));
                hold_sender(sender, Duration::from_millis(500));
                Ok(())
            },
            |observation| classify_diagnostic(true, observation, None),
        );

        assert_eq!(outcome, ProbeOutcome::ProbeScriptNotStarted);
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        assert!(started.elapsed() < Duration::from_millis(300));
    }

    #[test]
    fn dispatch_error_invalid_payload_and_script_error_are_fixed() {
        let dispatch_error = run_eval_loop_with(
            test_timing(),
            |_| Err(()),
            |_| panic!("dispatch failure must not classify"),
        );
        assert_eq!(dispatch_error, ProbeOutcome::EvalDispatchFailed);

        let invalid = run_with_payload("null", None);
        assert_eq!(invalid, ProbeOutcome::InvalidEvalObservation);

        let script_failed = run_with_payload(SCRIPT_FAILED_JSON, None);
        assert_eq!(script_failed, ProbeOutcome::EvalScriptFailed);
    }

    #[test]
    fn a_late_callback_after_terminal_timeout_cannot_change_the_outcome() {
        let attempted = Arc::new(AtomicBool::new(false));
        let callback_attempted = Arc::clone(&attempted);
        let outcome = run_eval_loop_with(
            test_timing(),
            move |sender| {
                let callback_attempted = Arc::clone(&callback_attempted);
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(20));
                    let _ = sender.try_send(EXACT_ACL_JSON.into());
                    callback_attempted.store(true, Ordering::SeqCst);
                });
                Ok(())
            },
            |observation| classify_diagnostic(true, observation, None),
        );

        assert_eq!(outcome, ProbeOutcome::EvalCallbackTimeout);
        thread::sleep(Duration::from_millis(40));
        assert!(attempted.load(Ordering::SeqCst));
        assert_eq!(outcome, ProbeOutcome::EvalCallbackTimeout);
    }

    fn exact_acl_observation() -> ProbeObservation {
        parse_observation(EXACT_ACL_JSON).unwrap()
    }

    fn classify_diagnostic_exact(legacy: LegacyUrlResult) -> ProbeOutcome {
        match classify_diagnostic(true, &exact_acl_observation(), Some(legacy)) {
            ProbeDecision::Exit(outcome) => outcome,
            ProbeDecision::Retry(state) => {
                panic!("exact ACL unexpectedly retried: {state:?}")
            }
        }
    }

    fn test_timing() -> ProbeTiming {
        ProbeTiming {
            total_timeout: Duration::from_millis(100),
            callback_timeout: Duration::from_millis(1),
            retry_delay: Duration::from_millis(1),
        }
    }

    fn hold_sender(sender: std::sync::mpsc::SyncSender<String>, duration: Duration) {
        thread::spawn(move || {
            thread::sleep(duration);
            drop(sender);
        });
    }

    fn run_with_payload(payload: &'static str, legacy: Option<LegacyUrlResult>) -> ProbeOutcome {
        run_eval_loop_with(
            test_timing(),
            move |sender| {
                sender.try_send(payload.into()).unwrap();
                Ok(())
            },
            move |observation| classify_diagnostic(true, observation, legacy.clone()),
        )
    }
}
