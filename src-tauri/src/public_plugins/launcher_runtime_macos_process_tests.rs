//! Opt-in process tests. The helper path is only read by the test binary.
use super::*;
use crate::public_plugins::{
    call_session::ReplyPermit, request_contract::PluginRequestContext,
    storage_document::validate_mac_json,
};

#[path = "../../../spikes/macos-runtime-bridge-probe/src/pipe.rs"]
mod pipe;
use pipe::{Event, Frame, Pipe};

struct Report {
    pid: u32,
    data_calls: usize,
    replies: usize,
    reaped: bool,
    cancelled_before_deadline: bool,
}

struct QuickJsProbe {
    helper: PathBuf,
    waiting_for_reply: Option<mpsc::Sender<()>>,
    report: Mutex<Option<Report>>,
}

impl QuickJsProbe {
    fn new(waiting_for_reply: Option<mpsc::Sender<()>>) -> Arc<Self> {
        let path = PathBuf::from(
            std::env::var_os("UIPILOT_LAUNCHER_TEST_HELPER")
                .expect("set UIPILOT_LAUNCHER_TEST_HELPER to the freshly built probe helper"),
        );
        assert!(path.is_absolute() && path.is_file());
        Arc::new(Self {
            helper: path,
            waiting_for_reply,
            report: Mutex::new(None),
        })
    }

    fn take_report(&self) -> Report {
        let report = self.report.lock().unwrap().take().expect("helper ran");
        assert!(report.reaped, "helper {} was not reaped", report.pid);
        eprintln!(
            "launcher helper pid={} calls={} replies={} reaped={}",
            report.pid, report.data_calls, report.replies, report.reaped
        );
        report
    }
}

impl RuntimeTransport for QuickJsProbe {
    fn run(&self, input: RuntimeInput, cancel: Cancellation) -> Result<TerminalResult, Error> {
        let source = input
            .package
            .resource(&input.package.manifest().runtime.entry)
            .filter(|bytes| bytes.len() <= 2048)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or(Error::Unavailable)?;
        let context = input.binding.context();
        let cancelled = cancel.clone();
        let mut pipe = Pipe::new(&self.helper, input.deadline, move || {
            cancelled.is_cancelled()
        })
        .map_err(|_| Error::Unavailable)?;
        let mut report = Report {
            pid: pipe.pid(),
            data_calls: 0,
            replies: 0,
            reaped: false,
            cancelled_before_deadline: false,
        };
        let result = (|| {
            pipe.write(
                json!({"mode":"launcherDemo","context":context,
                    "invocation":input.invocation,"source":source}),
                None,
            )?;
            let mut stderr_eof = false;
            loop {
                match pipe.event()? {
                    Event::Frame(Frame::DataCall { request }) => {
                        let id = request.call_id;
                        let reply = input.binding.call(request).map_err(|_| ())?;
                        report.data_calls += 1;
                        if let Some(waiting) = &self.waiting_for_reply {
                            // The real child is blocked on stdin. Keep the actual reply
                            // permit alive until management has closed admission.
                            waiting.send(()).map_err(|_| ())?;
                            report.cancelled_before_deadline = cancel
                                .wait(input.deadline.saturating_duration_since(Instant::now()))
                                && Instant::now() < input.deadline;
                            return Err(());
                        }
                        let body = match reply.result {
                            Ok(value) => json!({"ok":true,"value":value}),
                            Err(error) => json!({"ok":false,"error":error}),
                        };
                        pipe.write(
                            json!({"kind":"reply","context":context,"callId":id,"value":body}),
                            Some(reply.permit),
                        )?;
                        report.replies += 1;
                    }
                    Event::Frame(Frame::Complete {
                        context: actual,
                        response,
                    }) if actual == *context && response.request_id == context.request_id => {
                        pipe.terminal(stderr_eof)?;
                        return Ok(TerminalResult {
                            raw: response.data.into_bytes(),
                        });
                    }
                    Event::StderrEof if !stderr_eof => stderr_eof = true,
                    _ => return Err(()),
                }
            }
        })();
        report.reaped = pipe.stop();
        let reaped = report.reaped;
        *self.report.lock().unwrap() = Some(report);
        if !reaped {
            return Err(Error::Unavailable);
        }
        result.map_err(|_| Error::Unavailable)
    }
}

#[test]
#[ignore = "requires the separately built diagnostic QuickJS helper"]
fn installed_javascript_reads_saved_settings_and_publishes_through_launcher() {
    let temp = Temp::new();
    {
        let state = ManagementState::default();
        temp.install(&state);
        state
            .with(&temp.0, |m| {
                let revision = m.list()?.revision;
                m.settings(
                    ID,
                    &std::collections::BTreeMap::from([
                        ("greeting".into(), json!("中文 hello")),
                        ("count".into(), json!(3)),
                        ("uppercase".into(), json!(true)),
                        ("style".into(), json!("boxed")),
                    ]),
                    &revision,
                )?;
                Ok(())
            })
            .unwrap();
    }
    fs::remove_dir_all(temp.0.join("package")).unwrap();
    let state = Arc::new(ManagementState::default());
    let registry = registry();
    let probe = QuickJsProbe::new(None);
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        probe.clone(),
        Duration::from_secs(5),
    );
    start(&runtime, &registry);
    assert!(matches!(
        launch(&runtime, &temp.0, 1, "/settings-demo", false),
        Ok(Route::NotHandled)
    ));
    let Route::Published(response) =
        launch(&runtime, &temp.0, 2, "/settings-demo 中文", true).unwrap()
    else {
        panic!("expected a published JS result")
    };
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].title, "【中文 HELLO × 3】");
    assert_eq!(
        response.items[0].subtitle.as_deref(),
        Some("uppercase=true · style=boxed · build 1.0.0")
    );
    assert!(!response.items[0].has_default_action);
    assert_eq!(
        registry.resolve(&response.request_id, &response.items[0].result_id),
        Err(crate::result_registry::RegistryError::UnknownResult)
    );
    let report = probe.take_report();
    assert_eq!((report.data_calls, report.replies), (4, 4));
    drain(&runtime, &state);
}

#[test]
#[ignore = "requires the separately built diagnostic QuickJS helper"]
fn disabling_plugin_reaps_child_waiting_for_reply_before_acknowledgement() {
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let (waiting, pending) = mpsc::channel();
    let registry = registry();
    let probe = QuickJsProbe::new(Some(waiting));
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        probe.clone(),
        Duration::from_secs(5),
    );
    start(&runtime, &registry);
    let worker = {
        let runtime = runtime.clone();
        let root = temp.0.clone();
        thread::spawn(move || launch(&runtime, &root, 1, "/settings-demo", true))
    };
    pending
        .recv_timeout(Duration::from_secs(3))
        .expect("child waiting for its first reply");
    state
        .with(&temp.0, |m| {
            let revision = m.list()?.revision;
            m.enabled(ID, false, &revision)?;
            Ok(())
        })
        .unwrap();
    let report = probe.take_report();
    assert_eq!((report.data_calls, report.replies), (1, 0));
    assert!(
        report.cancelled_before_deadline,
        "disable must not rely on timeout"
    );
    assert!(matches!(worker.join().unwrap(), Ok(Route::Stale)));
    assert!(matches!(
        launch(&runtime, &temp.0, 2, "/settings-demo", true),
        Ok(Route::NotHandled)
    ));
    drain(&runtime, &state);
}
