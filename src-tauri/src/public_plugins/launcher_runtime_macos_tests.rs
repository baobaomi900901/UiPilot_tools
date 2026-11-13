use super::*;

#[path = "launcher_runtime_macos_process_tests.rs"]
mod process;
#[test]
fn exact_route_does_not_match_prefix_builtins_or_empty_commands() {
    assert_eq!(split_query("/demo  中文  😀 "), Some(("demo", "中文  😀")));
    for query in [
        "demo",
        "/",
        "/find test",
        "/settings",
        "/quicklinks",
        "/web-search",
    ] {
        assert!(split_query(query).is_none(), "{query}");
    }
}
use super::super::{
    management_macos::SourceKind,
    runtime_data_macos::{DataCall, DataOperation, DataRuntimeError},
};
use crate::result_registry::{ResultAction, ResultRegistries};
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::mpsc, thread};
static NEXT: AtomicU64 = AtomicU64::new(0);
const ID: &str = "com.uipilot.demo-settings";
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "uipilot-launcher-runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(path.join("package")).unwrap();
        fs::write(
            path.join("package/plugin.json"),
            include_bytes!(
                "../../../examples/public-plugins/com.uipilot.demo-settings/package/plugin.json"
            ),
        )
        .unwrap();
        fs::write(
            path.join("package/runtime.js"),
            include_bytes!(
                "../../../examples/public-plugins/com.uipilot.demo-settings/package/runtime.js"
            ),
        )
        .unwrap();
        Self(path)
    }
    fn edit(&self, change: impl FnOnce(&mut Value)) {
        let path = self.0.join("package/plugin.json");
        let mut value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut value);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn install(&self, state: &ManagementState) {
        state
            .with(&self.0, |m| {
                let revision = m.list()?.revision;
                let selection = m.reserve(SourceKind::Directory, &revision)?;
                let p = m
                    .selected(selection, Some(&self.0.join("package")))?
                    .unwrap();
                m.commit(&p.token, &p.expected_revision, p.required_permissions)?;
                Ok(())
            })
            .unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Fake<F>(F);
impl<F: Fn(RuntimeInput, Cancellation) -> Result<TerminalResult, Error> + Send + Sync>
    RuntimeTransport for Fake<F>
{
    fn run(&self, input: RuntimeInput, cancel: Cancellation) -> Result<TerminalResult, Error> {
        (self.0)(input, cancel)
    }
}
fn registry() -> ResultRegistry {
    ResultRegistries::default().main().clone()
}
fn completion(input: &RuntimeInput, title: &str) -> TerminalResult {
    TerminalResult {
        raw: serde_json::to_vec(&json!({
            "requestId": input.invocation.request_id,
            "results": [{"id": "private-row", "title": title}]
        }))
        .unwrap(),
    }
}
fn call(
    binding: &RuntimeDataBinding,
    id: u64,
    operation: DataOperation,
) -> Result<Value, DataRuntimeError> {
    binding
        .call(DataCall {
            context: binding.context().clone(),
            call_id: id,
            request: operation,
        })
        .and_then(|reply| reply.result)
}
fn launch(
    runtime: &LauncherRuntime,
    root: &Path,
    sequence: u64,
    query: &str,
    submit: bool,
) -> Result<Route, Error> {
    runtime.route(
        root,
        1,
        sequence,
        query,
        InvocationTheme::Dark,
        submit,
        None,
    )
}
fn start(runtime: &LauncherRuntime, registry: &ResultRegistry) {
    registry.on_show("1".into());
    runtime.begin_invocation(1);
}
fn drain(runtime: &LauncherRuntime, state: &ManagementState) {
    let owner = runtime
        .lane
        .lock()
        .unwrap()
        .active
        .as_ref()
        .map(|r| r.identity.clone());
    runtime.invalidate_current();
    if let Some(owner) = owner {
        state.close_runtime(&owner).unwrap();
    }
}

#[test]
fn closed_factory_and_available_noncommands_do_not_load_inventory_or_consume_app_sequence() {
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    let registry = registry();
    let runtime = LauncherRuntime::closed(state.clone(), registry.clone());
    start(&runtime, &registry);
    assert!(!runtime.available());
    assert!(matches!(
        launch(&runtime, &temp.0, 1, "/settings-demo", true),
        Ok(Route::NotHandled)
    ));
    assert!(!temp.0.join("plugin-management-v1").exists());
    assert!(registry
        .begin_query(QueryDomain::Application, "1", 1)
        .is_some());
    drop(runtime);
    let fake = Arc::new(Fake(|_: RuntimeInput, _: Cancellation| {
        panic!("must not run")
    }));
    let available =
        LauncherRuntime::with_transport(state, registry.clone(), fake, Duration::from_secs(2));
    registry.on_show("2".into());
    available.begin_invocation(2);
    for (seq, query) in [(1, "plain app"), (2, "/find x"), (3, "/web-search x")] {
        assert!(matches!(
            available.route(&temp.0, 2, seq, query, InvocationTheme::Light, true, None),
            Ok(Route::NotHandled)
        ));
        assert!(registry
            .begin_query(QueryDomain::Application, "2", seq)
            .is_some());
    }
    for (seq, input) in [(4, "x".repeat(64 * 1024 + 1)), (5, "x\0y".into())] {
        assert!(matches!(
            available.route(
                &temp.0,
                2,
                seq,
                &format!("/settings-demo {input}"),
                InvocationTheme::Light,
                true,
                None,
            ),
            Ok(Route::NotHandled)
        ));
        assert!(registry
            .begin_query(QueryDomain::Application, "2", seq)
            .is_some());
    }
    assert!(split_query(&format!("/demo {}", "x".repeat(64 * 1024))).is_some());
    assert_eq!(split_query("/notes x"), Some(("notes", "x")));
    assert!(!temp.0.join("plugin-management-v1").exists());
}
#[test]
fn persisted_install_routes_without_opening_management_and_publishes_real_information_rows() {
    let temp = Temp::new();
    {
        let state = ManagementState::default();
        temp.install(&state);
        state
            .with(&temp.0, |m| {
                let r = m.list()?.revision;
                m.settings(
                    ID,
                    &std::collections::BTreeMap::from([(
                        "greeting".into(),
                        json!("saved 中文 😀"),
                    )]),
                    &r,
                )?;
                Ok(())
            })
            .unwrap();
    }
    fs::remove_file(temp.0.join("package/plugin.json")).unwrap();
    fs::remove_file(temp.0.join("package/runtime.js")).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captures = seen.clone();
    let fake = Arc::new(Fake(move |input: RuntimeInput, _: Cancellation| {
        assert!(input
            .package
            .resource("runtime.js")
            .unwrap()
            .starts_with(b"const BUILD"));
        assert!(input.invocation.command_id.is_none());
        assert_eq!(input.invocation.input, "中文  😀");
        assert_eq!(input.invocation.context.theme, InvocationTheme::Dark);
        assert!(chrono::DateTime::parse_from_rfc3339(&input.invocation.context.invoked_at).is_ok());
        assert!(Instant::now() < input.deadline);
        let value = call(
            &input.binding,
            1,
            DataOperation::SettingGet {
                key: "greeting".into(),
            },
        )
        .unwrap();
        call(
            &input.binding,
            2,
            DataOperation::StorageSet {
                key: "sample".into(),
                value: value.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            call(
                &input.binding,
                3,
                DataOperation::StorageGet {
                    key: "sample".into()
                }
            )
            .unwrap(),
            value
        );
        captures.lock().unwrap().push(input.binding.clone());
        Ok(completion(&input, value.as_str().unwrap()))
    }));
    let state = Arc::new(ManagementState::default());
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        fake,
        Duration::from_secs(2),
    );
    start(&runtime, &registry);
    let response = match launch(&runtime, &temp.0, 1, "/settings-demo  中文  😀 ", true).unwrap()
    {
        Route::Published(r) => r,
        _ => panic!("real publication expected"),
    };
    assert_eq!(response.items[0].title, "saved 中文 😀");
    assert!(response.replace_local_results);
    assert!(!response.items[0].has_default_action);
    assert!(registry
        .resolve(&response.request_id, &response.items[0].result_id)
        .is_err());
    assert!(call(
        &seen.lock().unwrap()[0],
        4,
        DataOperation::StorageGet {
            key: "sample".into()
        }
    )
    .is_err());
    drain(&runtime, &state);
    seen.lock().unwrap().clear();
}
#[test]
fn submit_input_required_disabled_exact_case_and_multi_command_contracts() {
    let temp = Temp::new();
    temp.edit(|m| {
        m["command"]["inputRequired"] = json!(true);
        m["command"]["inputPlaceholder"] = json!("请输入文本");
    });
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captures = seen.clone();
    let fake = Arc::new(Fake(move |i: RuntimeInput, _: Cancellation| {
        captures.lock().unwrap().push(i.invocation.clone());
        Ok(completion(&i, "ok"))
    }));
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        fake,
        Duration::from_secs(2),
    );
    start(&runtime, &registry);
    for (seq, query, submit) in [(1, "/settings-demo", true), (2, "/settings-demo x", false)] {
        let Route::Published(response) = launch(&runtime, &temp.0, seq, query, submit).unwrap()
        else {
            panic!("expected the input hint")
        };
        assert_eq!(response.command_hint.as_deref(), Some("请输入文本"));
        assert!(response.items.is_empty());
    }
    for (seq, q, submit) in [(3, "/settings-dem x", true), (4, "/SETTINGS-DEMO x", true)] {
        assert!(matches!(
            launch(&runtime, &temp.0, seq, q, submit),
            Ok(Route::NotHandled)
        ));
    }
    assert!(seen.lock().unwrap().is_empty());
    assert!(matches!(
        launch(&runtime, &temp.0, 5, "/settings-demo x", true),
        Ok(Route::Published(_))
    ));
    state
        .with(&temp.0, |m| {
            let r = m.list()?.revision;
            m.enabled(ID, false, &r)?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        launch(&runtime, &temp.0, 6, "/settings-demo x", true),
        Ok(Route::NotHandled)
    ));
    temp.edit(|m| {
        m["minimumHostVersion"] = json!("0.5.0");
        let mut c = m.as_object_mut().unwrap().remove("command").unwrap();
        c["id"] = json!("read");
        c["activationMode"] = json!("live");
        m["commands"] = json!([c]);
    });
    temp.install(&state);
    state
        .with(&temp.0, |m| {
            let r = m.list()?.revision;
            m.enabled(ID, true, &r)?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        launch(&runtime, &temp.0, 7, "/settings-demo live", false),
        Ok(Route::Published(_))
    ));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].command_id.as_deref(), Some("read"));
    drain(&runtime, &state);
}

#[test]
fn ordinary_query_cancels_old_run_without_consuming_app_sequence_and_late_result_cannot_overwrite()
{
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    let fake = Arc::new(Fake(move |input: RuntimeInput, cancel: Cancellation| {
        entered_tx.send((input.binding.clone(), cancel)).unwrap();
        release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Ok(completion(&input, "obsolete"))
    }));
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        fake,
        Duration::from_secs(3),
    );
    start(&runtime, &registry);
    let rt = runtime.clone();
    let root = temp.0.clone();
    let old = thread::spawn(move || launch(&rt, &root, 1, "/settings-demo", true));
    let (binding, cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        launch(&runtime, &temp.0, 2, "application", false),
        Ok(Route::NotHandled)
    ));
    assert!(cancel.is_cancelled());
    assert!(call(&binding, 1, DataOperation::StorageGet { key: "x".into() }).is_err());
    let query = registry
        .begin_query(QueryDomain::Application, "1", 2)
        .unwrap();
    let action = ResultAction::CopyBuiltInText {
        text: "fresh".into(),
    };
    let (set, rows) = registry
        .publish_if_latest(
            query,
            vec![("fresh", action.clone())],
            || true,
            |id, rows| (id, rows),
        )
        .unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(old.join().unwrap(), Ok(Route::Stale)));
    assert_eq!(registry.resolve(&set, &rows[0].0), Ok(action));
    drain(&runtime, &state);
}
#[test]
fn lower_sequence_and_old_invocation_do_not_cancel_new_active_request() {
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    let fake = Arc::new(Fake(move |input: RuntimeInput, cancel: Cancellation| {
        entered_tx.send((input.binding.clone(), cancel)).unwrap();
        release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Ok(completion(&input, "current"))
    }));
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        fake,
        Duration::from_secs(3),
    );
    start(&runtime, &registry);
    let rt = runtime.clone();
    let root = temp.0.clone();
    let first = thread::spawn(move || launch(&rt, &root, 1, "/settings-demo", true));
    let (_, first_cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let rt = runtime.clone();
    let root = temp.0.clone();
    let second = thread::spawn(move || launch(&rt, &root, 2, "/settings-demo", true));
    assert!(first_cancel.wait(Duration::from_secs(2)));
    release_tx.send(()).unwrap();
    assert!(matches!(first.join().unwrap(), Ok(Route::Stale)));
    let (binding, cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        launch(&runtime, &temp.0, 1, "app", false),
        Ok(Route::Stale)
    ));
    assert!(matches!(
        runtime.route(&temp.0, 0, 99, "app", InvocationTheme::Light, false, None),
        Ok(Route::Stale)
    ));
    assert!(!cancel.is_cancelled());
    assert_eq!(
        call(
            &binding,
            1,
            DataOperation::SettingGet {
                key: "greeting".into()
            }
        )
        .unwrap(),
        json!("你好")
    );
    release_tx.send(()).unwrap();
    assert!(matches!(second.join().unwrap(), Ok(Route::Published(_))));
    drain(&runtime, &state);
}
#[test]
fn management_settings_disable_permission_and_upgrade_cancel_before_drain_and_new_owner_recovers() {
    for change in ["settings", "disable", "permission", "upgrade"] {
        let temp = Temp::new();
        temp.edit(|m| m["permissions"] = json!(["clipboard.write"]));
        let state = Arc::new(ManagementState::default());
        temp.install(&state);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        let count = AtomicU64::new(0);
        let fake = Arc::new(Fake(move |input: RuntimeInput, cancel: Cancellation| {
            let first = count.fetch_add(1, Ordering::Relaxed) == 0;
            if first {
                entered_tx.send((input.binding.clone(), cancel)).unwrap();
                release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            }
            let value = call(
                &input.binding,
                1,
                DataOperation::SettingGet {
                    key: "greeting".into(),
                },
            );
            if first {
                assert!(value.is_err());
                return Ok(completion(&input, "old"));
            }
            Ok(completion(&input, value.unwrap().as_str().unwrap()))
        }));
        let registry = registry();
        let runtime = LauncherRuntime::with_transport(
            state.clone(),
            registry.clone(),
            fake,
            Duration::from_secs(3),
        );
        start(&runtime, &registry);
        let rt = runtime.clone();
        let root = temp.0.clone();
        let run = thread::spawn(move || launch(&rt, &root, 1, "/settings-demo", true));
        let (old_binding, cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if change == "upgrade" {
            temp.edit(|m| m["version"] = json!("1.0.1"));
        }
        let manager = state.clone();
        let root = temp.0.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let mutation = thread::spawn(move || {
            manager
                .with(&root, |m| {
                    let revision = m.list()?.revision;
                    match change {
                        "settings" => {
                            m.settings(
                                ID,
                                &std::collections::BTreeMap::from([(
                                    "greeting".into(),
                                    json!("changed"),
                                )]),
                                &revision,
                            )?;
                        }
                        "disable" => {
                            m.enabled(ID, false, &revision)?;
                        }
                        "permission" => {
                            m.permission(
                                ID,
                                super::super::manifest::PublicPermission::ClipboardWrite,
                                false,
                                &revision,
                            )?;
                        }
                        _ => {
                            let selected = m.reserve(SourceKind::Directory, &revision)?;
                            let p = m.selected(selected, Some(&root.join("package")))?.unwrap();
                            m.commit(&p.token, &p.expected_revision, p.required_permissions)?;
                        }
                    }
                    Ok(())
                })
                .unwrap();
            done_tx.send(()).unwrap();
        });
        assert!(cancel.wait(Duration::from_secs(2)));
        assert!(done_rx.try_recv().is_err());
        assert!(call(
            &old_binding,
            1,
            DataOperation::StorageGet { key: "x".into() }
        )
        .is_err());
        release_tx.send(()).unwrap();
        assert!(matches!(run.join().unwrap(), Ok(Route::Stale)));
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        mutation.join().unwrap();
        if change == "disable" {
            state
                .with(&temp.0, |m| {
                    let r = m.list()?.revision;
                    m.enabled(ID, true, &r)?;
                    Ok(())
                })
                .unwrap();
        }
        let response = match launch(&runtime, &temp.0, 2, "/settings-demo", true).unwrap() {
            Route::Published(r) => r,
            _ => panic!("new request expected"),
        };
        assert_eq!(
            response.items[0].title,
            if change == "settings" {
                "changed"
            } else {
                "你好"
            }
        );
        assert!(call(
            &old_binding,
            2,
            DataOperation::StorageGet { key: "x".into() }
        )
        .is_err());
        drain(&runtime, &state);
    }
}
#[test]
fn hide_and_management_reset_revoke_immediately_without_waiting_for_transport() {
    for hide in [false, true] {
        let temp = Temp::new();
        let state = Arc::new(ManagementState::default());
        temp.install(&state);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        let count = AtomicU64::new(0);
        let fake = Arc::new(Fake(move |input: RuntimeInput, cancel: Cancellation| {
            if count.fetch_add(1, Ordering::Relaxed) == 0 {
                entered_tx.send((input.binding.clone(), cancel)).unwrap();
                release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            }
            Ok(completion(&input, "fresh"))
        }));
        let registry = registry();
        let runtime = LauncherRuntime::with_transport(
            state.clone(),
            registry.clone(),
            fake,
            Duration::from_secs(3),
        );
        start(&runtime, &registry);
        let rt = runtime.clone();
        let root = temp.0.clone();
        let run = thread::spawn(move || launch(&rt, &root, 1, "/settings-demo", true));
        let (binding, cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if hide {
            registry.hide_and_clear();
            runtime.invalidate_current();
        } else {
            state.reset();
        }
        assert!(cancel.is_cancelled());
        assert!(call(&binding, 1, DataOperation::StorageGet { key: "x".into() }).is_err());
        release_tx.send(()).unwrap();
        assert!(matches!(run.join().unwrap(), Ok(Route::Stale)));
        if hide {
            registry.on_show("2".into());
            runtime.begin_invocation(2);
        }
        assert!(matches!(
            runtime.route(
                &temp.0,
                if hide { 2 } else { 1 },
                2,
                "/settings-demo",
                InvocationTheme::Light,
                true,
                None,
            ),
            Ok(Route::Published(_))
        ));
        drain(&runtime, &state);
    }
}
#[test]
fn timeout_failed_terminal_malformed_result_and_copy_action_never_publish() {
    for case in ["timeout", "failed", "malformed", "copy", "panic"] {
        let temp = Temp::new();
        let state = Arc::new(ManagementState::default());
        temp.install(&state);
        let fake = Arc::new(Fake(
            move |input: RuntimeInput, cancel: Cancellation| match case {
                "timeout" => {
                    assert!(cancel.wait(Duration::from_secs(2)));
                    Ok(completion(&input, "late"))
                }
                "failed" => Err(Error::Unavailable),
                "panic" => panic!("fixed transport panic"),
                "malformed" => Ok(TerminalResult {
                    raw: b"{}".to_vec(),
                }),
                _ => Ok(TerminalResult {
                    raw: serde_json::to_vec(&json!({
                        "requestId": input.invocation.request_id,
                        "results": [{
                            "id": "x", "title": "x",
                            "defaultAction": {"type": "copyText", "text": "must not copy"}
                        }]
                    }))
                    .unwrap(),
                }),
            },
        ));
        let registry = registry();
        let runtime = LauncherRuntime::with_transport(
            state.clone(),
            registry.clone(),
            fake,
            Duration::from_millis(40),
        );
        start(&runtime, &registry);
        assert!(
            !matches!(
                launch(&runtime, &temp.0, 1, "/settings-demo", true),
                Ok(Route::Published(_))
            ),
            "{case}"
        );
        assert!(registry
            .begin_query(QueryDomain::Application, "1", 2)
            .is_some());
        drain(&runtime, &state);
    }
}

#[test]
fn revoked_copy_permission_keeps_plugin_enabled_and_regrant_allows_fresh_copy() {
    use super::super::manifest::PublicPermission;
    let temp = Temp::new();
    temp.edit(|m| m["permissions"] = json!(["clipboard.write"]));
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let set_grant = |granted| {
        state.with(&temp.0, |manager| {
            let revision = manager.list()?.revision;
            manager.permission(ID, PublicPermission::ClipboardWrite, granted, &revision)?;
            Ok(())
        }).unwrap();
    };
    let fake = Arc::new(Fake(|input: RuntimeInput, _: Cancellation| Ok(TerminalResult {
        raw: serde_json::to_vec(&json!({
            "requestId":input.invocation.request_id,
            "results":[{"id":"copy", "title":"recovered",
                "defaultAction":{"type":"copyText", "text":"recovered"}}]
        })).unwrap(),
    })));
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(state.clone(), registry.clone(), fake, Duration::from_secs(5));
    start(&runtime, &registry);
    set_grant(false);
    for sequence in 1..=6 {
        let error = launch(&runtime, &temp.0, sequence, "/settings-demo", true).unwrap_err();
        assert_eq!(error.code(), "pluginPermissionDenied");
        state.with(&temp.0, |manager| {
            let snapshot = manager.list()?;
            let plugin = snapshot.plugins.iter().find(|p| p.plugin_id == ID).unwrap();
            assert!(plugin.enabled);
            assert!(plugin.permissions.iter().all(|p| !p.granted));
            Ok(())
        }).unwrap();
    }
    set_grant(true);
    let Route::Published(response) = launch(&runtime, &temp.0, 7, "/settings-demo", true).unwrap()
    else { panic!("fresh query after regrant must publish") };
    assert!(response.items[0].has_default_action);
    let mut writes = 0;
    runtime.copy_result(&temp.0, &response.request_id, &response.items[0].result_id, |text, admit| {
        assert_eq!(text, "recovered");
        assert!(admit());
        writes += 1;
        assert!(!admit());
        Ok(HostCopyOutcome::Accepted)
    }).unwrap();
    assert_eq!(writes, 1);
    drain(&runtime, &state);
}

#[test]
fn permission_regrant_preserves_explicit_disable_across_reload() {
    use super::super::manifest::PublicPermission;
    let temp = Temp::new();
    temp.edit(|m| m["permissions"] = json!(["clipboard.write"]));
    let state = ManagementState::default();
    temp.install(&state);
    state.with(&temp.0, |manager| {
        let revision = manager.list()?.revision;
        let snapshot = manager.permission(ID, PublicPermission::ClipboardWrite, false, &revision)?;
        let snapshot = manager.enabled(ID, false, &snapshot.revision)?;
        let snapshot = manager.permission(ID, PublicPermission::ClipboardWrite, true, &snapshot.revision)?;
        assert!(!snapshot.plugins[0].enabled);
        assert!(snapshot.plugins[0].permissions[0].granted);
        Ok(())
    }).unwrap();
    drop(state);
    ManagementState::default().with(&temp.0, |manager| {
        let snapshot = manager.list()?;
        assert!(!snapshot.plugins[0].enabled);
        assert!(snapshot.plugins[0].permissions[0].granted);
        Ok(())
    }).unwrap();
}
#[test]
fn stale_query_waiting_before_open_cannot_close_a_newly_attached_request() {
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    let fake = Arc::new(Fake(move |input: RuntimeInput, cancel: Cancellation| {
        entered_tx.send((input.binding.clone(), cancel)).unwrap();
        release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Ok(completion(&input, "new"))
    }));
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        fake,
        Duration::from_secs(3),
    );
    start(&runtime, &registry);
    let (paused_tx, paused_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    *runtime.before_open.lock().unwrap() = Some(Box::new(move || {
        paused_tx.send(()).unwrap();
        resume_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    let rt = runtime.clone();
    let root = temp.0.clone();
    let old = thread::spawn(move || launch(&rt, &root, 1, "/settings-demo", true));
    paused_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let rt = runtime.clone();
    let root = temp.0.clone();
    let new = thread::spawn(move || launch(&rt, &root, 2, "/settings-demo", true));
    let (binding, cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    resume_tx.send(()).unwrap();
    assert!(matches!(old.join().unwrap(), Ok(Route::Stale)));
    assert!(!cancel.is_cancelled());
    assert!(call(
        &binding,
        1,
        DataOperation::SettingGet {
            key: "greeting".into()
        }
    )
    .is_ok());
    release_tx.send(()).unwrap();
    assert!(matches!(new.join().unwrap(), Ok(Route::Published(_))));
    drain(&runtime, &state);
}
#[test]
fn delayed_reset_notification_only_retires_its_captured_management_epoch() {
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    let count = AtomicU64::new(0);
    let fake = Arc::new(Fake(move |input: RuntimeInput, cancel: Cancellation| {
        if count.fetch_add(1, Ordering::Relaxed) > 0 {
            entered_tx.send((input.binding.clone(), cancel)).unwrap();
            release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
        Ok(completion(&input, "new epoch"))
    }));
    let registry = registry();
    let runtime = LauncherRuntime::with_transport(
        state.clone(),
        registry.clone(),
        fake,
        Duration::from_secs(3),
    );
    start(&runtime, &registry);
    assert!(matches!(
        launch(&runtime, &temp.0, 1, "/settings-demo", true),
        Ok(Route::Published(_))
    ));
    let (paused_tx, paused_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    *state.before_reset_hook.lock().unwrap() = Some(Box::new(move || {
        paused_tx.send(()).unwrap();
        resume_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    let reset_state = state.clone();
    let reset = thread::spawn(move || reset_state.reset());
    paused_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let rt = runtime.clone();
    let root = temp.0.clone();
    let new = thread::spawn(move || launch(&rt, &root, 2, "/settings-demo", true));
    let (binding, cancel) = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    resume_tx.send(()).unwrap();
    reset.join().unwrap();
    assert!(!cancel.is_cancelled());
    assert!(call(
        &binding,
        1,
        DataOperation::SettingGet {
            key: "greeting".into()
        }
    )
    .is_ok());
    release_tx.send(()).unwrap();
    assert!(matches!(new.join().unwrap(), Ok(Route::Published(_))));
    drain(&runtime, &state);
}

#[test]
fn renamed_favorite_catalog_and_dispatch_keep_the_original_command_identity() {
    let temp = Temp::new();
    let state = Arc::new(ManagementState::default());
    temp.install(&state);
    state.with(&temp.0, |m| {
        let revision = m.list()?.revision;
        m.command_preference(ID, "default", super::super::management_macos::CommandPreference {
            name_override: Some("mac-settings".into()), favorite: true,
        }, &revision)?;
        Ok(())
    }).unwrap();
    let registry = registry();
    let fake = Arc::new(Fake(|input: RuntimeInput, _: Cancellation| {
        assert_eq!(input.package.manifest().plugin_id, ID);
        assert!(input.invocation.command_id.is_none());
        Ok(completion(&input, "renamed result"))
    }));
    let runtime = LauncherRuntime::with_transport(state.clone(), registry.clone(), fake, Duration::from_secs(5));
    start(&runtime, &registry);
    let Route::Published(catalog) = launch(&runtime, &temp.0, 1, "/mac", false).unwrap() else { panic!("catalog missing") };
    assert_eq!(catalog.items.len(), 1);
    assert_eq!(catalog.items[0].title, "/mac-settings");
    assert_eq!(catalog.items[0].favorite, crate::model::ResultFavorite::public_plugin(ID.into(), "default".into(), true));
    assert!(matches!(launch(&runtime, &temp.0, 2, "/settings-demo", true), Ok(Route::NotHandled)));
    let Route::Published(result) = launch(&runtime, &temp.0, 3, "/mac-settings", true).unwrap() else { panic!("dispatch missing") };
    assert_eq!(result.items[0].title, "renamed result");
    drain(&runtime, &state);
}
