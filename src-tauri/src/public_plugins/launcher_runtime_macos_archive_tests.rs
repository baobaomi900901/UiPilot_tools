//! Run via scripts/verify-macos-plugin-archives.mjs against its exact CLI artifacts.
use super::*;
use crate::public_plugins::manifest::PublicPermission;
use crate::result_registry::RegistryError;
use std::collections::BTreeMap;

fn fixture_path(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os(variable).expect("run the archive verification script"));
    assert!(path.is_absolute());
    path
}

struct ArchiveHost {
    root: PathBuf,
    state: Arc<ManagementState>,
    runtime: Arc<LauncherRuntime>,
    registry: ResultRegistry,
    probe: Arc<QuickJsProbe>,
}

impl ArchiveHost {
    fn new(root: PathBuf) -> Self {
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
        Self {
            root,
            state,
            runtime,
            registry,
            probe,
        }
    }

    fn install(&self, name: &str, expected_id: &str, expected_version: &str) {
        let path =
            fixture_path("UIPILOT_ARCHIVE_TEST_INPUT").join(format!("{name}.uipilot-plugin"));
        self.state
            .with(&self.root, |manager| {
                let revision = manager.list()?.revision;
                let selection = manager.reserve(SourceKind::Archive, &revision)?;
                let preview = manager
                    .selected(selection, Some(&path))?
                    .expect("selected archive");
                assert_eq!(preview.plugin.plugin_id, expected_id);
                assert_eq!(preview.plugin.version, expected_version);
                manager.commit(
                    &preview.token,
                    &preview.expected_revision,
                    preview.required_permissions,
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn save(&self, plugin: &str, values: BTreeMap<String, Value>) {
        self.state
            .with(&self.root, |manager| {
                let revision = manager.list()?.revision;
                manager.settings(plugin, &values, &revision)?;
                Ok(())
            })
            .unwrap();
    }

    fn query(&self, sequence: u64, query: &str, submit: bool) -> Result<Route, Error> {
        launch(&self.runtime, &self.root, sequence, query, submit)
    }

    fn run(&self, sequence: u64, query: &str, submit: bool, data_calls: usize) -> SearchResponse {
        let Route::Published(response) = self.query(sequence, query, submit).unwrap() else {
            panic!("expected a published result for {query}")
        };
        let report = self.probe.take_report();
        assert_eq!(
            (report.data_calls, report.replies),
            (data_calls, data_calls)
        );
        response
    }

    fn close(self) {
        drain(&self.runtime, &self.state);
    }
}

fn assert_saved_settings(response: &SearchResponse, version: &str) {
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].title, "【中文 HELLO × 3】");
    assert_eq!(
        response.items[0].subtitle.as_deref(),
        Some(format!("uppercase=true · style=boxed · build {version}").as_str())
    );
}

#[test]
#[ignore = "requires CLI archives and a signed diagnostic helper"]
fn seed_settings() {
    let path = fixture_path("UIPILOT_ARCHIVE_TEST_SETTINGS_ROOT");
    fs::create_dir(&path).expect("settings fixture must be new");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let host = ArchiveHost::new(path);
    host.install("demo-settings", ID, "1.0.0");
    host.save(
        ID,
        BTreeMap::from([
            ("greeting".into(), json!("中文 hello")),
            ("count".into(), json!(3)),
            ("uppercase".into(), json!(true)),
            ("style".into(), json!("boxed")),
        ]),
    );
    assert_saved_settings(&host.run(1, "/settings-demo", true, 4), "1.0.0");
    host.close();
}

#[test]
#[ignore = "requires CLI archives, the persisted seed and a signed diagnostic helper"]
fn settings_reopen_and_upgrade_keep_saved_values() {
    let host = ArchiveHost::new(fixture_path("UIPILOT_ARCHIVE_TEST_SETTINGS_ROOT"));
    let restored = host.run(1, "/settings-demo", true, 4);
    assert_saved_settings(&restored, "1.0.0");
    host.install("demo-settings-upgrade", ID, "1.0.1");
    assert_eq!(
        host.registry
            .resolve(&restored.request_id, &restored.items[0].result_id),
        Err(RegistryError::StaleRequest)
    );
    assert_saved_settings(&host.run(2, "/settings-demo", true, 4), "1.0.1");
    host.close();
}

#[test]
#[ignore = "requires CLI archives and a signed diagnostic helper"]
fn multi_archive_dispatches_live_and_submit_with_shared_storage() {
    let temp = Temp::new();
    let host = ArchiveHost::new(temp.0.clone());
    host.install("demo-multi", "com.uipilot.demo-multi", "1.0.0");
    host.save(
        "com.uipilot.demo-multi",
        BTreeMap::from([("prefix".into(), json!("[验收] "))]),
    );
    let Route::Published(preview) = host.query(1, "/demo-lower MiXeD", false).unwrap() else {
        panic!("expected submit hint")
    };
    assert_eq!(preview.command_hint.as_deref(), Some("Enter text"));
    assert!(host.probe.report.lock().unwrap().is_none());
    let upper = host.run(2, "/demo-upper MiXeD", false, 4);
    assert_eq!(upper.items[0].title, "[验收] MIXED");
    assert_eq!(
        upper.items[0].subtitle.as_deref(),
        Some("Shared invocation 1")
    );
    let lower = host.run(3, "/demo-lower MiXeD", true, 4);
    assert_eq!(lower.items[0].title, "[验收] mixed");
    assert_eq!(
        lower.items[0].subtitle.as_deref(),
        Some("Shared invocation 2")
    );
    assert_eq!(lower.main_result_command.unwrap().command_id, "lower");
    host.close();
}

#[test]
#[ignore = "requires CLI archives and a signed diagnostic helper"]
fn return_archive_copies_with_grant_and_rejects_after_revocation() {
    let temp = Temp::new();
    let host = ArchiveHost::new(temp.0.clone());
    let plugin = "com.uipilot.demo-return";
    host.install("demo-return", plugin, "1.0.2");
    let response = host.run(1, "/demo-return 中文", true, 0);
    let item = &response.items[0];
    let mut written = None;
    host.runtime
        .copy_result(
            &host.root,
            &response.request_id,
            &item.result_id,
            |text, admit| {
                assert!(admit());
                written = Some(text.to_owned());
                Ok(HostCopyOutcome::Accepted)
            },
        )
        .unwrap();
    assert_eq!(written.as_deref(), Some(item.title.as_str()));
    assert!(item.title.starts_with("中文 "));

    host.state
        .with(&host.root, |manager| {
            let revision = manager.list()?.revision;
            manager.permission(plugin, PublicPermission::ClipboardWrite, false, &revision)?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        host.query(2, "/demo-return 中文", true),
        Err(Error::UnsupportedAction)
    ));
    assert_eq!(host.probe.take_report().data_calls, 0);
    host.close();
}
