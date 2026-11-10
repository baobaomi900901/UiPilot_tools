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
        let mut modules = std::collections::BTreeMap::new();
        let mut source_bytes = 0;
        for (path, bytes) in input
            .package
            .resources()
            .filter(|(path, _)| path.ends_with(".js"))
        {
            source_bytes += bytes.len();
            if modules.len() >= 32 || source_bytes > 2048 {
                return Err(Error::Unavailable);
            }
            let source = std::str::from_utf8(bytes).map_err(|_| Error::Unavailable)?;
            modules.insert(format!("pkg/{path}"), source);
        }
        let entry = format!("pkg/{}", input.package.manifest().runtime.entry);
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
                    "invocation":input.invocation,"graph":{"entry":entry,"modules":modules}}),
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

fn nested_entry(temp: &Temp, source: &str) -> PathBuf {
    let package = temp.0.join("package");
    fs::create_dir(package.join("entry")).unwrap();
    fs::remove_file(package.join("runtime.js")).unwrap();
    fs::write(package.join("entry/run.js"), source).unwrap();
    temp.edit(|manifest| manifest["runtime"]["entry"] = json!("entry/run.js"));
    package
}

#[test]
#[ignore = "requires the separately built diagnostic QuickJS helper"]
fn module_imports_use_the_installed_snapshot_and_real_data_service() {
    let temp = Temp::new();
    let package = nested_entry(
        &temp,
        r#"
        import { next, count } from './reexport.js';
        const state = await import('../state.js');
        export async function onCommand(inv, api) {
            if (!Object.isFrozen(inv) || !Object.isFrozen(inv.context)) throw Error('mutable input');
            next();
            const greeting = await api.settings.get('greeting');
            return {requestId: inv.requestId, results: [
                {id: 'modules', title: greeting + ':' + count + ':' + state.count}
            ]};
        }
    "#,
    );
    fs::write(
        package.join("entry/reexport.js"),
        "export { next, count } from '../state.js';",
    )
    .unwrap();
    fs::write(
        package.join("state.js"),
        "export let count = 1; export function next() { count++; }",
    )
    .unwrap();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    fs::remove_dir_all(package).unwrap();
    let registry = registry();
    let probe = QuickJsProbe::new(None);
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        probe.clone(),
        Duration::from_secs(5),
    );
    start(&runtime, &registry);
    let Route::Published(response) = launch(&runtime, &temp.0, 1, "/settings-demo", true).unwrap()
    else {
        panic!("expected the installed module graph to publish")
    };
    assert_eq!(response.items[0].title, "你好:2:2");
    let report = probe.take_report();
    assert_eq!((report.data_calls, report.replies), (1, 1));
    drain(&runtime, &state);
}

#[test]
#[ignore = "requires the separately built diagnostic QuickJS helper"]
fn missing_and_outside_modules_fail_before_data_calls_or_publication() {
    for import in [
        "import './missing.js';",
        "await import('../../outside.js');",
    ] {
        let temp = Temp::new();
        nested_entry(
            &temp,
            &format!(
                r#"
            {import}
            export async function onCommand(inv, api) {{
                await api.settings.get('greeting');
                return {{requestId: inv.requestId, results: [{{id: 'unexpected', title: 'unexpected'}}]}};
            }}
        "#
            ),
        );
        fs::write(temp.0.join("outside.js"), "export const outside = true;").unwrap();
        let state = Arc::new(ManagementState::default());
        temp.install(&state);
        let registry = registry();
        let probe = QuickJsProbe::new(None);
        let runtime = LauncherRuntime::with_transport(
            state.clone(),
            registry.clone(),
            probe.clone(),
            Duration::from_secs(5),
        );
        start(&runtime, &registry);
        assert!(
            matches!(
                launch(&runtime, &temp.0, 1, "/settings-demo", true),
                Err(Error::Unavailable)
            ),
            "{import}"
        );
        let report = probe.take_report();
        assert_eq!((report.data_calls, report.replies), (0, 0));
        drain(&runtime, &state);
    }
}

#[test]
#[ignore = "requires the separately built diagnostic QuickJS helper"]
fn demo_return_copy_checks_grant_at_publication_and_at_single_use_admission() {
    use crate::public_plugins::manifest::PublicPermission;
    const COPY_PLUGIN: &str = "com.uipilot.demo-return";
    for case in ["denied", "revoked", "copied"] {
        let temp = Temp::new();
        let package = temp.0.join("package");
        fs::remove_file(package.join("runtime.js")).unwrap();
        fs::create_dir(package.join("dist")).unwrap();
        fs::write(
            package.join("plugin.json"),
            include_bytes!(
                "../../../examples/public-plugins/com.uipilot.demo-return/package/plugin.json"
            ),
        )
        .unwrap();
        fs::write(
            package.join("dist/runtime.js"),
            include_bytes!(
                "../../../examples/public-plugins/com.uipilot.demo-return/package/dist/runtime.js"
            ),
        )
        .unwrap();
        let state = Arc::new(ManagementState::default());
        temp.install(&state);
        let revoke = || {
            state
                .with(&temp.0, |manager| {
                    let revision = manager.list()?.revision;
                    manager.permission(
                        COPY_PLUGIN,
                        PublicPermission::ClipboardWrite,
                        false,
                        &revision,
                    )?;
                    Ok(())
                })
                .unwrap()
        };
        if case == "denied" {
            revoke();
        }
        let registry = registry();
        let probe = QuickJsProbe::new(None);
        let runtime = LauncherRuntime::with_transport(
            state.clone(),
            registry.clone(),
            probe.clone(),
            Duration::from_secs(5),
        );
        start(&runtime, &registry);
        let route = launch(&runtime, &temp.0, 1, "/demo-return 中文", true);
        let report = probe.take_report();
        assert_eq!(report.data_calls, 0);
        if case == "denied" {
            assert!(matches!(route, Err(Error::UnsupportedAction)));
        } else {
            let Route::Published(response) = route.unwrap() else {
                panic!("expected copy result")
            };
            let item = &response.items[0];
            assert!(item.has_default_action);
            let mut writes = 0;
            let result = runtime.copy_result(
                &temp.0,
                &response.request_id,
                &item.result_id,
                |text, admit| {
                    assert!(text.starts_with("中文 "));
                    assert_eq!(text, item.title);
                    if case == "revoked" {
                        revoke();
                    }
                    if !admit() {
                        return Err(HostCopyError::PermissionDenied);
                    }
                    writes += 1;
                    assert!(!admit());
                    Ok(HostCopyOutcome::Accepted)
                },
            );
            if case == "revoked" {
                assert!(matches!(result, Err(HostCopyError::PermissionDenied)));
                assert_eq!(writes, 0);
            } else {
                let owner = result.unwrap();
                assert_eq!(writes, 1);
                assert!(registry
                    .begin_query(QueryDomain::Application, "1", 2)
                    .is_some());
                assert!(!registry.finish_main_action(&owner));
            }
            assert!(matches!(
                runtime.copy_result(&temp.0, &response.request_id, &item.result_id, |_, _| {
                    panic!("retired ticket must not enter the driver")
                }),
                Err(HostCopyError::PermissionDenied)
            ));
        }
        drain(&runtime, &state);
    }
}

#[test]
#[ignore = "requires the separately built diagnostic QuickJS helper"]
fn installed_command_suggestion_previews_without_running_then_dispatches_its_identity() {
    let temp = Temp::new();
    temp.edit(|manifest| {
        manifest["command"]["defaultName"] = json!("find-settings");
        manifest["command"]["inputRequired"] = json!(true);
        manifest["command"]["inputPlaceholder"] = json!("请输入内容");
    });
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
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
        launch(&runtime, &temp.0, 1, "/fi", false),
        Ok(Route::NotHandled)
    ));
    let Route::Published(suggestions) = launch(&runtime, &temp.0, 2, "/find-set", false).unwrap()
    else {
        panic!("expected a command candidate")
    };
    assert_eq!(suggestions.items.len(), 1);
    let LauncherResultActivation::MainResultActivation {
        plugin_id,
        command_id,
        command_label,
        ..
    } = &suggestions.items[0].activation
    else {
        panic!("expected a stable command identity")
    };
    assert_eq!(command_label, "find-settings");
    let origin = |phase, command_id| {
        Some(CompletionOrigin {
            phase,
            plugin_id: plugin_id.clone(),
            command_id: Some(command_id),
        })
    };
    let Route::Published(preview) = runtime
        .route(
            &temp.0,
            1,
            3,
            "/find-settings",
            InvocationTheme::Dark,
            false,
            origin(CompletionPhase::Preview, command_id.clone()),
        )
        .unwrap()
    else {
        panic!("expected preview")
    };
    assert_eq!(preview.command_hint.as_deref(), Some("请输入内容"));
    assert!(preview.items.is_empty());
    assert!(matches!(
        runtime.route(
            &temp.0,
            1,
            4,
            "/find-settings 中文",
            InvocationTheme::Dark,
            true,
            origin(CompletionPhase::Commit, "different-command".into()),
        ),
        Ok(Route::Stale)
    ));
    assert!(matches!(
        runtime.route(
            &temp.0,
            1,
            5,
            "/find-settings 中文",
            InvocationTheme::Dark,
            true,
            origin(CompletionPhase::Preview, command_id.clone()),
        ),
        Ok(Route::Stale)
    ));
    assert!(probe.report.lock().unwrap().is_none());
    let Route::Published(result) = runtime
        .route(
            &temp.0,
            1,
            6,
            "/find-settings 中文",
            InvocationTheme::Dark,
            true,
            origin(CompletionPhase::Commit, command_id.clone()),
        )
        .unwrap()
    else {
        panic!("expected committed command result")
    };
    let command = result.main_result_command.unwrap();
    assert_eq!(command.plugin_id, *plugin_id);
    assert_eq!(command.command_id, *command_id);
    assert_eq!(command.argument, "中文");
    assert_eq!(result.items[0].title, "你好 × 2");
    assert_eq!(probe.take_report().data_calls, 4);
    drain(&runtime, &state);
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
