use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use super::{
    activation::{PublicPluginInstallSource, PublicPluginManager},
    manifest::{PanelHostKeyDeclaration, PublicOutputMode, PublicPermission},
    package, stage_public_package,
    webview_audio_guard::{INERT_DOCUMENT, INERT_PATH},
    PluginRuntimeError, PublicPackageError, PublicPackageSource, PublicPlatform, PublicPluginHost,
    PublicPluginResponse, PublicPluginService,
};
use crate::message_center::MessageCenterService;
use crate::plugins::{PluginCatalog, Version};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[test]
fn public_plugin_icon_protocol_rejects_query_parameters() {
    let service = PublicPluginService::default();
    let response = service.asset_response(
        "main",
        "/__uipilot_icon/installed/com.example.demo/1/icon.png",
        Some("cache=bust"),
    );
    assert_eq!(response.status(), 403);
}

#[test]
fn public_plugin_alarm_protocol_is_always_forbidden() {
    let service = PublicPluginService::default();
    for label in [
        "public-runtime-com.example.timer-g1",
        "public-plugin-content-com.example.timer",
    ] {
        let response = service.asset_response(label, "/assets/sounds/timer-alarm.wav", None);
        assert_eq!(response.status(), 403);
        assert!(response.body().is_empty());
    }
}

#[test]
fn inert_webview_document_is_host_owned_and_denies_media() {
    let service = PublicPluginService::default();
    let response = service.asset_response("untrusted-label", &format!("/{INERT_PATH}"), None);

    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), INERT_DOCUMENT.as_bytes());
    assert_eq!(
        response.headers()["content-security-policy"],
        "default-src 'none'; media-src 'none'; base-uri 'none'; form-action 'none'"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[test]
fn public_plugin_protocol_csp_denies_media() {
    assert!(super::PUBLIC_PLUGIN_CSP.contains("media-src 'none'"));
}

#[test]
fn runtime_recovery_is_single_owner_and_latest_submission_wins() {
    let root = TestRoot::new("runtime-recovery-latest");
    let (service, manager) = recovery_service(&root);
    let now = Instant::now();
    let first = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    let first_recovery = first.recovery.clone().unwrap();
    let second = service
        .schedule_command(
            manager.route("/demo second").unwrap().unwrap(),
            2,
            "/demo second".into(),
            now,
        )
        .unwrap();
    let second_recovery = second.recovery.clone().unwrap();

    assert_eq!(
        first.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(None)
    );
    let readiness_calls = AtomicU64::new(0);
    assert!(service
        .complete_runtime_recovery_with(&first_recovery, now, |_| {
            readiness_calls.fetch_add(1, Ordering::Relaxed);
            true
        })
        .unwrap()
        .is_none());
    let dispatch = service
        .complete_runtime_recovery_with(&second_recovery, now, |_| {
            panic!("a recovery waiter must not create a second Runtime")
        })
        .unwrap()
        .unwrap();
    assert_eq!(readiness_calls.load(Ordering::Relaxed), 1);
    assert_eq!(dispatch.candidate.input, "second");

    service.settle_submission(
        &second.token,
        Some(Ok(PublicPluginResponse::MainResults(Vec::new()))),
    );
    assert_eq!(
        second.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Some(Ok(PublicPluginResponse::MainResults(Vec::new()))))
    );
    let submissions = service.lock_submissions().unwrap();
    assert!(submissions.by_token.is_empty());
    assert!(submissions.token_by_request.is_empty());
    drop(submissions);
    assert!(service.lock_recoveries().unwrap().by_plugin.is_empty());
}

#[test]
fn request_scoped_abort_preserves_a_newer_unadmitted_preparation() {
    let root = TestRoot::new("request-scoped-abort");
    let (service, manager) = recovery_service(&root);
    let first = service
        .prepare_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
        )
        .unwrap();
    let second = service
        .prepare_command(
            manager.route("/demo second").unwrap().unwrap(),
            2,
            "/demo second".into(),
        )
        .unwrap();
    let first_context = first.request_context().clone();
    let first_token = first.token.clone();
    let second_token = second.token.clone();

    service
        .abort_submission_request(&first_context, &first_token, Instant::now())
        .unwrap();

    assert_eq!(
        first.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Some(Err(PluginRuntimeError::Unavailable)))
    );
    let submissions = service.lock_submissions().unwrap();
    assert!(!submissions.by_token.contains_key(&first_token));
    assert!(submissions.by_token.contains_key(&second_token));
    drop(submissions);
    service.fail_submission(&second_token);
}

#[test]
fn request_scoped_abort_promotes_and_binds_a_newer_waiting_submission() {
    let root = TestRoot::new("request-scoped-abort-waiting");
    let (service, manager) = recovery_service(&root);
    let now = Instant::now();
    let first = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    let first_recovery = first.recovery.clone().unwrap();
    let first_dispatch = service
        .complete_runtime_recovery_with(&first_recovery, now, |_| true)
        .unwrap()
        .unwrap();
    let first_context = first_dispatch.context;
    let first_token = first.token.clone();
    let second = service
        .schedule_command(
            manager.route("/demo second").unwrap().unwrap(),
            2,
            "/demo second".into(),
            now + Duration::from_secs(1),
        )
        .unwrap();
    let second_token = second.token.clone();
    assert!(second.dispatch.is_none());
    assert_eq!(
        first.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(None)
    );

    let promoted = service
        .abort_submission_request(&first_context, &first_token, now + Duration::from_secs(2))
        .unwrap()
        .expect("the waiting request must be promoted");

    assert_eq!(promoted.candidate.owner.submission_token, second_token);
    assert_eq!(
        manager.scheduler().context_status(&promoted.context),
        super::PluginContextStatus::Current
    );
    service
        .abort_submission_request(
            &promoted.context,
            &second_token,
            now + Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(
        second.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Some(Err(PluginRuntimeError::Unavailable)))
    );
}

#[test]
fn runtime_recovery_failure_settles_every_waiter_and_clears_indexes() {
    let root = TestRoot::new("runtime-recovery-failure");
    let (service, manager) = recovery_service(&root);
    let now = Instant::now();
    let first = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    let first_recovery = first.recovery.clone().unwrap();
    let second = service
        .schedule_command(
            manager.route("/demo second").unwrap().unwrap(),
            2,
            "/demo second".into(),
            now,
        )
        .unwrap();

    assert_eq!(
        first.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(None)
    );
    assert!(service
        .complete_runtime_recovery_with(&first_recovery, now, |_| false)
        .unwrap()
        .is_none());
    assert_eq!(
        second.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(None)
    );
    let submissions = service.lock_submissions().unwrap();
    assert!(submissions.by_token.is_empty());
    assert!(submissions.token_by_request.is_empty());
    drop(submissions);
    assert!(service.lock_recoveries().unwrap().by_plugin.is_empty());
}

#[test]
fn joining_existing_runtime_recovery_does_not_allocate_another_attempt() {
    let root = TestRoot::new("runtime-recovery-join-exhausted");
    let (service, manager) = recovery_service(&root);
    let now = Instant::now();
    let first = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    service.next_recovery.store(u64::MAX, Ordering::Release);

    let second = service
        .schedule_command(
            manager.route("/demo second").unwrap().unwrap(),
            2,
            "/demo second".into(),
            now,
        )
        .expect("joining the existing attempt must not allocate a new recovery ID");

    assert_eq!(
        first.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(None)
    );
    service.settle_submission(&second.token, None);
}

#[test]
fn stale_runtime_recovery_completion_cannot_open_scheduler_admission() {
    let root = TestRoot::new("runtime-recovery-stale");
    let (service, manager) = recovery_service(&root);
    let now = Instant::now();
    let submission = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    let mut stale = submission.recovery.clone().unwrap();
    stale.attempt_id = stale.attempt_id.checked_add(1).unwrap();
    let readiness_calls = AtomicU64::new(0);

    assert!(service
        .complete_runtime_recovery_with(&stale, now, |_| {
            readiness_calls.fetch_add(1, Ordering::Relaxed);
            true
        })
        .unwrap()
        .is_none());

    assert_eq!(readiness_calls.load(Ordering::Relaxed), 0);
    assert!(
        manager
            .route("/demo still-recovering")
            .unwrap()
            .unwrap()
            .runtime_recovery_needed
    );
    service.settle_submission(&submission.token, None);
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-public-package-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn package(&self) -> PathBuf {
        let path = self.0.join("source");
        fs::create_dir(&path).unwrap();
        path
    }

    fn staging(&self) -> PathBuf {
        self.0.join("staging")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn recovery_service(root: &TestRoot) -> (Arc<PublicPluginService>, Arc<PublicPluginManager>) {
    let source = root.package();
    write_package(&source, &manifest("mainResult"));
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            host(),
            ["find".into(), "math".into()],
            Arc::new(MessageCenterService::load(&app_data)),
        )
        .unwrap(),
    );
    let now = Instant::now();
    let prepared = manager
        .prepare(
            "main",
            PublicPluginInstallSource::DevelopmentDirectory { path: source },
            now,
        )
        .unwrap();
    manager
        .commit_with_readiness("main", &prepared.token, BTreeSet::new(), now, |_| true)
        .unwrap();
    let transaction = manager
        .begin_uninstall("com.uipilot.demo", false)
        .unwrap()
        .unwrap();
    manager.abort_uninstall_before_commit(transaction).unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(Arc::clone(&manager)).is_ok());
    (service, manager)
}

#[test]
fn accepts_archive_and_directory_packages() {
    for (name, archive, mode) in [
        ("directory-window", false, "window"),
        ("directory-panel", false, "panel"),
        ("archive-main", true, "mainResult"),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &manifest(mode));
        let source = if archive {
            let path = root.0.join("candidate.uipilot-plugin");
            archive_directory(&source, &path);
            PublicPackageSource::Archive(path)
        } else {
            PublicPackageSource::DevelopmentDirectory(source)
        };

        let prepared = stage_public_package(source, &root.staging(), &host()).unwrap();
        assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo");
        assert_eq!(prepared.digest.len(), 64);
        assert_eq!(prepared.resources["plugin.json"].mime, "application/json");
        assert_eq!(
            prepared.resources["dist/runtime.js"].mime,
            "text/javascript"
        );
        assert_eq!(prepared.revalidate(), Ok(()));
        if archive {
            let runtime = prepared.package_root.join("dist/runtime.js");
            package::make_file_writable(&runtime);
            fs::write(&runtime, "tampered").unwrap();
            assert_eq!(
                prepared.revalidate(),
                Err(PublicPackageError::InvalidPackage)
            );
        }
        assert_eq!(
            prepared.manifest.command.output_mode,
            match mode {
                "window" => PublicOutputMode::Window,
                "panel" => PublicOutputMode::Panel,
                _ => PublicOutputMode::MainResult,
            }
        );
        let transaction = prepared.transaction_root().to_path_buf();
        assert!(prepared.package_root.starts_with(&transaction));
        drop(prepared);
        assert!(!transaction.exists());
    }
}

#[test]
fn accepts_timer_alarm_in_directory_and_archive_without_exposing_it_publicly() {
    for (name, archive) in [("timer-directory", false), ("timer-archive", true)] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &timer_manifest());
        let alarm = source.join("assets/sounds/timer-alarm.wav");
        fs::create_dir_all(alarm.parent().unwrap()).unwrap();
        fs::write(&alarm, valid_alarm_wav()).unwrap();
        let source = if archive {
            let path = root.0.join("candidate.uipilot-plugin");
            archive_directory(&source, &path);
            PublicPackageSource::Archive(path)
        } else {
            PublicPackageSource::DevelopmentDirectory(source)
        };

        let prepared = stage_public_package(source, &root.staging(), &host()).unwrap();

        assert_eq!(prepared.revalidate(), Ok(()));
        assert!(!prepared
            .resources
            .contains_key("assets/sounds/timer-alarm.wav"));
    }
}

#[test]
fn accepts_supported_alarm_pcm_boundaries() {
    for (name, wav) in [
        ("mono-24-padding", alarm_wav(1, 1, 44_100, 24)),
        ("stereo-48k-24", alarm_wav(100, 2, 48_000, 24)),
        (
            "max-duration-mono-24",
            alarm_wav(44_100 * 15, 1, 44_100, 24),
        ),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &timer_manifest());
        write_alarm(&source, &wav);
        let prepared = stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source),
            &root.staging(),
            &host(),
        )
        .unwrap();
        assert!(prepared.alarm.is_some());
        assert_eq!(prepared.revalidate(), Ok(()));
    }
}

#[test]
fn timer_permission_and_fixed_alarm_must_be_declared_together() {
    let missing = TestRoot::new("timer-alarm-missing");
    let source = missing.package();
    write_package(&source, &timer_manifest());
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(source),
        &missing,
        PublicPackageError::InvalidPackage,
    );

    let unexpected = TestRoot::new("timer-alarm-unexpected");
    let source = unexpected.package();
    write_package(&source, &manifest("window"));
    write_alarm(&source, &valid_alarm_wav());
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(source),
        &unexpected,
        PublicPackageError::InvalidPackage,
    );
}

#[test]
fn rejects_malformed_or_unsupported_timer_alarm_wav() {
    let mut bad_riff = valid_alarm_wav();
    bad_riff[0] = b'X';
    let mut trailing = valid_alarm_wav();
    trailing.push(0);
    let mut non_pcm = valid_alarm_wav();
    non_pcm[20..22].copy_from_slice(&3_u16.to_le_bytes());
    let mut channels = valid_alarm_wav();
    channels[22..24].copy_from_slice(&3_u16.to_le_bytes());
    let mut sample_rate = valid_alarm_wav();
    sample_rate[24..28].copy_from_slice(&22_050_u32.to_le_bytes());
    let mut byte_rate = valid_alarm_wav();
    byte_rate[28..32].copy_from_slice(&1_u32.to_le_bytes());
    let mut block_align = valid_alarm_wav();
    block_align[32..34].copy_from_slice(&1_u16.to_le_bytes());
    let mut bits = valid_alarm_wav();
    bits[34..36].copy_from_slice(&8_u16.to_le_bytes());
    let mut unknown_chunk = valid_alarm_wav();
    unknown_chunk[36..40].copy_from_slice(b"JUNK");
    let mut extended_fmt = valid_alarm_wav();
    extended_fmt[16..20].copy_from_slice(&18_u32.to_le_bytes());
    let mut mismatched_data_length = valid_alarm_wav();
    mismatched_data_length[40..44].copy_from_slice(&199_u32.to_le_bytes());
    let mut even_padding = valid_alarm_wav();
    even_padding.push(0);
    let riff_size = u32::try_from(even_padding.len() - 8).unwrap();
    even_padding[4..8].copy_from_slice(&riff_size.to_le_bytes());
    let mut bad_padding = alarm_wav(1, 1, 44_100, 24);
    *bad_padding.last_mut().unwrap() = 1;
    let mut missing_padding = alarm_wav(1, 1, 44_100, 24);
    missing_padding.pop();
    let zero_frames = alarm_wav(0, 1, 44_100, 16);
    let too_long = alarm_wav(44_100 * 15 + 1, 1, 44_100, 16);

    for (name, wav) in [
        ("bad-riff", bad_riff),
        ("trailing", trailing),
        ("non-pcm", non_pcm),
        ("channels", channels),
        ("sample-rate", sample_rate),
        ("byte-rate", byte_rate),
        ("block-align", block_align),
        ("bits", bits),
        ("unknown-chunk", unknown_chunk),
        ("extended-fmt", extended_fmt),
        ("mismatched-data-length", mismatched_data_length),
        ("even-padding", even_padding),
        ("bad-padding", bad_padding),
        ("missing-padding", missing_padding),
        ("zero-frames", zero_frames),
        ("too-long", too_long),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &timer_manifest());
        write_alarm(&source, &wav);
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }
}

#[test]
fn source_alarm_hardlink_is_copied_but_staged_multilink_is_rejected() {
    let root = TestRoot::new("timer-alarm-hardlink");
    let source = root.package();
    write_package(&source, &timer_manifest());
    let original = root.0.join("original.wav");
    fs::write(&original, valid_alarm_wav()).unwrap();
    let alarm = source.join("assets/sounds/timer-alarm.wav");
    fs::create_dir_all(alarm.parent().unwrap()).unwrap();
    fs::hard_link(&original, &alarm).unwrap();

    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();
    fs::write(&original, alarm_wav(101, 1, 44_100, 16)).unwrap();
    assert_eq!(prepared.revalidate(), Ok(()));

    let staged_alarm = prepared.package_root.join("assets/sounds/timer-alarm.wav");
    fs::hard_link(staged_alarm, root.0.join("staged-alarm-link.wav")).unwrap();
    assert_eq!(
        prepared.revalidate(),
        Err(PublicPackageError::InvalidPackage)
    );
}

#[test]
fn pomodoro_reference_package_is_installable() {
    let root = TestRoot::new("pomodoro-reference");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/public-plugins/com.uipilot.pomodoro/package");
    let directory = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source.clone()),
        &root.staging(),
        &host(),
    )
    .unwrap();
    let archive_path = root.0.join("pomodoro.uipilot-plugin");
    archive_directory(&source, &archive_path);
    let archive = stage_public_package(
        PublicPackageSource::Archive(archive_path),
        &root.staging(),
        &host(),
    )
    .unwrap();

    assert_eq!(directory.manifest.plugin_id, "com.uipilot.pomodoro");
    assert_eq!(
        directory.manifest.permissions,
        vec![
            PublicPermission::UiWindow,
            PublicPermission::NotificationsPublish,
            PublicPermission::TimerControl,
        ]
    );
    let directory_alarm = directory.alarm.as_ref().unwrap();
    let archive_alarm = archive.alarm.as_ref().unwrap();
    assert_eq!(directory.digest, archive.digest);
    assert_eq!(
        directory_alarm.resource_sha256,
        archive_alarm.resource_sha256
    );
    assert_eq!(directory_alarm.bytes, archive_alarm.bytes);
    assert_eq!(directory.revalidate(), Ok(()));
    assert_eq!(archive.revalidate(), Ok(()));
}

#[test]
fn rejects_resource_and_archive_path_variants() {
    for path in ["dist/icon.png", "dist/runtime.min.js", "dist/data.json"] {
        let root = TestRoot::new(path.replace(['/', '.'], "-").as_str());
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        let extra = path
            .split('/')
            .fold(source.clone(), |root, part| root.join(part));
        fs::create_dir_all(extra.parent().unwrap()).unwrap();
        fs::write(extra, "x").unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    for (name, entries) in [
        (
            "traversal",
            vec![
                ("../escape.js", b"x".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
        (
            "case-collision",
            vec![
                ("dist/runtime.js", b"x".as_slice()),
                ("dist/Runtime.js", b"y".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
        (
            "parent-case-collision",
            vec![
                ("Dist/a.js", b"x".as_slice()),
                ("dist/b.js", b"y".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
    ] {
        let root = TestRoot::new(name);
        let archive = root.0.join("candidate.uipilot-plugin");
        write_archive(&archive, &entries);
        assert_rejected(
            PublicPackageSource::Archive(archive),
            &root,
            PublicPackageError::InvalidPackage,
        );
        assert!(!root.0.join("escape.js").exists());
    }
}

#[test]
fn rejects_incompatible_or_malformed_and_preserves_legacy_loader() {
    for (name, field, replacement, expected) in [
        (
            "platform",
            "supportedPlatforms",
            json!(["macos"]),
            PublicPackageError::IncompatiblePlatform,
        ),
        (
            "api",
            "apiVersion",
            json!(2),
            PublicPackageError::IncompatibleApi,
        ),
        (
            "permission",
            "permissions",
            json!(["network.https"]),
            PublicPackageError::InvalidPackage,
        ),
        (
            "settings",
            "settings",
            json!([{"key":"limit","type":"number","label":"Limit","min":10,"max":5}]),
            PublicPackageError::InvalidPackage,
        ),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        let mut candidate = manifest("mainResult");
        candidate[field] = replacement;
        write_package(&source, &candidate);
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            expected,
        );
    }

    let root = TestRoot::new("legacy");
    let legacy = root.0.join("internal.sample");
    fs::create_dir(&legacy).unwrap();
    fs::write(
        legacy.join("plugin.json"),
        r#"{"manifest":1,"id":"internal.sample","version":"1.0.0","minHostVersion":"0.2.0","runtime":"index.html","feature":{"id":"calculate","trigger":"/sample"},"permissions":["clipboard.writeText"]}"#,
    )
    .unwrap();
    fs::write(legacy.join("index.html"), "").unwrap();
    assert_eq!(
        PluginCatalog::load(&root.0, Version::new(0, 2, 0))
            .unwrap()
            .entry_count_for_test(),
        1
    );
}

#[test]
fn notifications_publish_permission_is_available_only_on_windows() {
    for (label, platform, expected) in [
        ("notifications-windows", PublicPlatform::Windows, Ok(())),
        (
            "notifications-macos",
            PublicPlatform::Macos,
            Err(PublicPackageError::UnsupportedPermission),
        ),
    ] {
        let root = TestRoot::new(label);
        let source = root.package();
        let mut candidate = manifest("mainResult");
        candidate["supportedPlatforms"] = json!(["windows", "macos"]);
        candidate["permissions"] = json!(["notifications.publish"]);
        write_package(&source, &candidate);
        let result = stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source),
            &root.staging(),
            &PublicPluginHost::current(platform),
        )
        .map(|_| ());
        assert_eq!(result, expected);
    }
}

#[test]
fn plugin_network_manifest_package_stages_only_for_windows_host_0_3_2() {
    let root = TestRoot::new("network-manifest-package");
    let source = root.package();
    let mut candidate = manifest("mainResult");
    candidate["minimumHostVersion"] = json!("0.3.2");
    candidate["permissions"] = json!(["network.https"]);
    candidate["network"] = json!({ "httpsHosts": ["api.example.com"] });
    write_package(&source, &candidate);

    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &PublicPluginHost::current(PublicPlatform::Windows),
    )
    .unwrap();
    assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo");
}

fn host() -> PublicPluginHost {
    PublicPluginHost::current(PublicPlatform::Windows)
}

fn manifest(mode: &str) -> Value {
    let window = mode == "window";
    let panel = mode == "panel";
    let mut value = json!({
        "schemaVersion":1,
        "pluginId":"com.uipilot.demo",
        "version":"1.0.0",
        "apiVersion":1,
        "minimumHostVersion": if panel { "0.3.0" } else { "0.2.0" },
        "name":"Demo",
        "supportedPlatforms":["windows"],
        "command":{
            "defaultName":"demo",
            "activationMode":if window || panel { "submit" } else { "live" },
            "outputMode":mode,
            "inputRequired":false
        },
        "runtime":{"entry":"dist/runtime.js"},
        "permissions": if window {
            json!(["ui.window"])
        } else if panel {
            json!(["ui.panel"])
        } else {
            json!([])
        }
    });
    if window {
        value["window"] = json!({"entry":"dist/window.html"});
    }
    if panel {
        value["panel"] = json!({"entry":"dist/panel.html"});
    }
    value
}

fn timer_manifest() -> Value {
    let mut value = manifest("window");
    value["permissions"] = json!(["ui.window", "notifications.publish", "timer.control"]);
    value
}

fn valid_alarm_wav() -> Vec<u8> {
    alarm_wav(100, 1, 44_100, 16)
}

fn alarm_wav(frames: u32, channels: u16, sample_rate: u32, bits_per_sample: u16) -> Vec<u8> {
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data = vec![0_u8; usize::from(block_align) * usize::try_from(frames).unwrap()];
    let padding = data.len() % 2;
    let riff_size = 36_u32 + u32::try_from(data.len() + padding).unwrap();
    let mut wav = Vec::with_capacity(44 + data.len() + padding);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    wav.extend_from_slice(&data);
    if padding == 1 {
        wav.push(0);
    }
    wav
}

fn write_alarm(root: &Path, bytes: &[u8]) {
    let alarm = root.join("assets/sounds/timer-alarm.wav");
    fs::create_dir_all(alarm.parent().unwrap()).unwrap();
    fs::write(alarm, bytes).unwrap();
}

fn write_package(root: &Path, manifest: &Value) {
    fs::create_dir(root.join("dist")).unwrap();
    fs::write(
        root.join("plugin.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("dist/runtime.js"),
        "export function onCommand() {}",
    )
    .unwrap();
    if manifest["command"]["outputMode"] == "window" {
        fs::write(root.join("dist/window.html"), "<!doctype html>").unwrap();
    }
    if manifest["command"]["outputMode"] == "panel" {
        fs::write(root.join("dist/panel.html"), "<!doctype html>").unwrap();
    }
}

fn assert_rejected(source: PublicPackageSource, root: &TestRoot, expected: PublicPackageError) {
    assert_eq!(
        stage_public_package(source, &root.staging(), &host()).unwrap_err(),
        expected
    );
    assert!(!root.staging().exists() || fs::read_dir(root.staging()).unwrap().next().is_none());
}

fn archive_directory(source: &Path, destination: &Path) {
    let mut entries = Vec::new();
    collect_files(source, source, &mut entries);
    let borrowed = entries
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    write_archive(destination, &borrowed);
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(root, &path, output);
        } else {
            output.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(path).unwrap(),
            ));
        }
    }
}

fn write_archive(destination: &Path, entries: &[(&str, &[u8])]) {
    let mut archive = ZipWriter::new(File::create(destination).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (path, bytes) in entries {
        archive.start_file(*path, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn png_with_dimensions(source: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut png = source.to_vec();
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

fn png_with_animation_control(source: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(20);
    chunk.extend_from_slice(&8_u32.to_be_bytes());
    chunk.extend_from_slice(b"acTL");
    chunk.extend_from_slice(&1_u32.to_be_bytes());
    chunk.extend_from_slice(&0_u32.to_be_bytes());
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut png = Vec::with_capacity(source.len() + chunk.len());
    png.extend_from_slice(&source[..33]);
    png.extend_from_slice(&chunk);
    png.extend_from_slice(&source[33..]);
    png
}

#[test]
fn public_plugin_icon_validation_is_fixed_bounded_and_atomic() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let valid_icon =
        fs::read(workspace.join("examples/public-plugins/com.uipilot.demo-win/package/icon.png"))
            .unwrap();

    let root = TestRoot::new("valid-icon");
    let source = root.package();
    write_package(&source, &manifest("mainResult"));
    fs::write(source.join("icon.png"), &valid_icon).unwrap();
    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();
    assert_eq!(prepared.resources["icon.png"].mime, "image/png");
    assert_eq!(
        prepared.resources["icon.png"].length,
        valid_icon.len() as u64
    );
    assert_eq!(
        fs::read(prepared.package_root.join("icon.png")).unwrap(),
        valid_icon
    );

    let invalid_icons = [
        ("corrupt", b"not a png".to_vec()),
        ("wrong-size", png_with_dimensions(&valid_icon, 64, 128)),
        ("animated", png_with_animation_control(&valid_icon)),
        ("oversized", {
            let mut bytes = valid_icon.clone();
            bytes.resize(128 * 1024 + 1, 0);
            bytes
        }),
    ];
    for (name, bytes) in invalid_icons {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        fs::write(source.join("icon.png"), bytes).unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    for path in ["Icon.png", "assets/icon.png", "other.png"] {
        let root = TestRoot::new(path.replace(['/', '.'], "-").as_str());
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        let icon = path
            .split('/')
            .fold(source.clone(), |parent, component| parent.join(component));
        fs::create_dir_all(icon.parent().unwrap()).unwrap();
        fs::write(icon, &valid_icon).unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    let root = TestRoot::new("extra-icon");
    let source = root.package();
    write_package(&source, &manifest("mainResult"));
    fs::write(source.join("icon.png"), &valid_icon).unwrap();
    fs::write(source.join("other.png"), &valid_icon).unwrap();
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(source),
        &root,
        PublicPackageError::InvalidPackage,
    );
}

#[test]
fn repository_demo_examples_stage_as_independently_removable_public_plugins() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cases = [
        (
            "com.uipilot.demo-win",
            "demo-win",
            "打开演示子窗口",
            PublicOutputMode::Window,
            "1.0.4",
            vec![
                PublicPermission::UiWindow,
                PublicPermission::NotificationsPublish,
            ],
            true,
            6,
        ),
        (
            "com.uipilot.demo-return",
            "demo-return",
            "返回示例文本到主界面",
            PublicOutputMode::MainResult,
            "1.0.2",
            vec![PublicPermission::ClipboardWrite],
            false,
            3,
        ),
    ];

    for (
        plugin_id,
        command,
        summary,
        output_mode,
        version,
        permissions,
        has_window,
        resource_count,
    ) in cases
    {
        let root = TestRoot::new(command);
        let source = workspace.join(format!("examples/public-plugins/{plugin_id}/package"));
        let prepared = stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source),
            &root.staging(),
            &host(),
        )
        .unwrap();
        assert_eq!(prepared.manifest.plugin_id, plugin_id);
        assert_eq!(prepared.manifest.command.default_name, command);
        assert_eq!(prepared.manifest.command.output_mode, output_mode);
        let serialized = serde_json::to_value(&prepared.manifest).unwrap();
        assert_eq!(serialized["version"], version);
        assert_eq!(serialized["command"]["summary"], summary);
        assert_eq!(serialized["command"]["inputPlaceholder"], "请输入信息回车");
        assert_eq!(prepared.manifest.permissions, permissions);
        assert_eq!(prepared.manifest.window.is_some(), has_window);
        assert_eq!(prepared.resources.len(), resource_count);
    }

    for production in [
        include_str!("../commands.rs"),
        include_str!("../public_plugins.rs"),
        include_str!("../../../src/launcher-core.ts"),
    ] {
        for command in ["/demo-win", "/demo-return"] {
            assert!(!production.contains(command));
        }
    }
}

#[test]
fn repository_demo_panel_stages_with_the_panel_only_contract() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let root = TestRoot::new("demo-panel-reference");
    let source = workspace.join("examples/public-plugins/com.uipilot.demo-panel/package");
    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();

    assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo-panel");
    assert_eq!(prepared.manifest.command.default_name, "demo-panel");
    assert_eq!(
        prepared.manifest.command.output_mode,
        PublicOutputMode::Panel
    );
    assert_eq!(
        prepared.manifest.permissions,
        vec![PublicPermission::UiPanel]
    );
    assert_eq!(
        prepared.manifest.panel.as_ref().unwrap().host_keys,
        vec![
            PanelHostKeyDeclaration::ArrowDown,
            PanelHostKeyDeclaration::ArrowUp,
            PanelHostKeyDeclaration::PrimaryN,
            PanelHostKeyDeclaration::Tab,
            PanelHostKeyDeclaration::ShiftTab,
        ]
    );
    assert!(prepared.manifest.window.is_none());
    assert_eq!(
        prepared
            .resources
            .keys()
            .filter(|path| path.as_str() != "icon.png")
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "dist/panel.css",
            "dist/panel.html",
            "dist/panel.js",
            "dist/runtime.js",
            "plugin.json",
        ]
    );
}

#[cfg(windows)]
#[test]
fn demo_packaging_script_writes_both_installable_archives() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for (plugin_id, resource_count) in [("com.uipilot.demo-win", 6), ("com.uipilot.demo-return", 3)]
    {
        let root = TestRoot::new(plugin_id);
        let output = root.0.join(format!("{plugin_id}.uipilot-plugin"));
        let status = std::process::Command::new("powershell.exe")
            .current_dir(workspace)
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "scripts/package-demo-plugin.ps1",
                "-PluginId",
                plugin_id,
                "-OutputPath",
            ])
            .arg(&output)
            .status()
            .unwrap();
        assert!(status.success());
        let prepared = stage_public_package(
            PublicPackageSource::Archive(output),
            &root.staging(),
            &host(),
        )
        .unwrap();
        assert_eq!(prepared.manifest.plugin_id, plugin_id);
        assert_eq!(prepared.resources.len(), resource_count);
    }
}
