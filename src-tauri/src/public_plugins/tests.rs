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
    manifest::{PanelHostKeyDeclaration, PublicActivationMode, PublicOutputMode, PublicPermission},
    package, stage_public_package,
    webview_audio_guard::{INERT_DOCUMENT, INERT_PATH},
    PluginCommandCompletion, PluginRuntimeError, PublicPackageError, PublicPackageSource,
    PublicPlatform, PublicPluginHost, PublicPluginResponse, PublicPluginService,
};
use crate::message_center::MessageCenterService;
use crate::plugins::{PluginCatalog, Version};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn settings_service(root: &TestRoot) -> (Arc<PublicPluginService>, Arc<PublicPluginManager>) {
    let source = root.package();
    let mut value = manifest("mainResult");
    value["minimumHostVersion"] = json!("0.3.4");
    value["settings"] = json!([
        {"key":"flag", "label":"Flag", "type":"boolean", "default":false},
        {"key":"first", "label":"First", "type":"secret"},
        {"key":"second", "label":"Second", "type":"secret"}
    ]);
    value["permissions"] = json!(["network.https"]);
    value["network"] = json!({"httpsHosts":["api.example.com"]});
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost::current(PublicPlatform::Windows),
            [],
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
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([PublicPermission::NetworkHttps]),
            now,
            |_| true,
        )
        .unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    (service, manager)
}

pub(super) fn with_settings_service_for_test(
    operation: impl FnOnce(&PublicPluginService, &PublicPluginManager),
) {
    let root = TestRoot::new("settings-command");
    let (service, manager) = settings_service(&root);
    operation(&service, &manager);
}

#[test]
fn credential_completion_preserves_publication_guard_through_submission_receiver() {
    let root = TestRoot::new("credential-completion-envelope");
    let (service, manager) = settings_service(&root);
    let now = Instant::now();
    let submission = service
        .schedule_command(
            manager.route("/demo hello").unwrap().unwrap(),
            1,
            "/demo hello".into(),
            now,
        )
        .unwrap();
    let dispatch = submission.dispatch.as_ref().unwrap();
    let publication = manager
        .scheduler()
        .capture_result_publication(
            &dispatch.context.plugin_id,
            dispatch.context.plugin_generation,
            dispatch.candidate.activation_id,
            dispatch.candidate.admission_epoch,
        )
        .unwrap();
    let completion = PluginCommandCompletion {
        context: dispatch.context.clone(),
        failed: false,
        response: Some(json!({
            "requestId": dispatch.context.request_id,
            "results": [{"id": "translation", "title": "translated"}]
        })),
    };
    let mut outcome = manager
        .scheduler()
        .complete(&completion.context, now)
        .unwrap();
    assert!(outcome.accepted);
    // Native service completion adds this metadata; it must survive the async receiver.
    outcome.credential_publication = Some((42, publication.clone()));
    service
        .complete_submission_with(&completion, outcome, now, || panic!("must not fault"))
        .unwrap();
    let Some(Ok(PublicPluginResponse::ServiceMainResults {
        results,
        credential_revision,
        publication: received_publication,
    })) = submission
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("service completion must retain its native publication guard")
    };
    assert_eq!(results[0].title, "translated");
    assert_eq!(credential_revision, 42);
    assert_eq!(received_publication, publication);
    assert!(manager
        .scheduler()
        .with_result_publication(&dispatch.context.plugin_id, &received_publication, || true)
        .unwrap());

    // Revocation may happen after completion and receipt, before registry publication.
    service
        .set_network_access("com.uipilot.demo", false, None)
        .unwrap();
    assert!(manager
        .scheduler()
        .with_result_publication(
            &dispatch.context.plugin_id,
            &received_publication,
            || panic!("revoked service result must never reach the registry"),
        )
        .is_err());
}

#[test]
fn image_legacy_service_submission_binds_before_dispatch_and_keeps_copy_result() {
    let root = TestRoot::new("image-legacy-service-order");
    let source = root.package();
    let mut value = manifest("mainResult");
    value["minimumHostVersion"] = json!("0.20.0");
    value["command"]["activationMode"] = json!("submit");
    value["command"]["inputCapabilities"] = json!(["image.ocr"]);
    value["permissions"] = json!(["clipboard.write"]);
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost::current(PublicPlatform::Windows),
            [],
            Arc::new(MessageCenterService::load(&app_data)),
        )
        .unwrap(),
    );
    let now = Instant::now();
    let installation = manager
        .prepare(
            "main",
            PublicPluginInstallSource::DevelopmentDirectory { path: source },
            now,
        )
        .unwrap();
    manager
        .commit_with_readiness(
            "main",
            &installation.token,
            BTreeSet::from([PublicPermission::ClipboardWrite]),
            now,
            |_| true,
        )
        .unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    manager.launcher_images.begin_import(1).unwrap();
    let image = manager
        .launcher_images
        .finish_import(
            1,
            crate::launcher_image::normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap(),
        )
        .unwrap();
    manager
        .launcher_images
        .admit_intent(
            &image.id,
            2,
            true,
            || Some(()),
            |contexts| manager.retire_image_contexts(contexts),
        )
        .unwrap()
        .unwrap();
    let route = manager
        .image_route("com.uipilot.demo", "default", "")
        .unwrap();
    let publication = route.result_publication.clone().unwrap();
    assert!(!manager.result_actions_enabled(&route));
    let prepared = service.prepare_command(route, 2, String::new()).unwrap();
    let context = prepared.request_context().clone();
    service
        .bind_prepared_image(&prepared, &image.id, 2)
        .unwrap();
    let submission = service.admit_prepared_command(prepared, now).unwrap();
    let dispatch = submission
        .dispatch
        .as_ref()
        .expect("a legacy OCR image command must dispatch");
    assert_eq!(dispatch.context, context);
    assert_eq!(manager.invocation_command_id(dispatch).unwrap(), None);
    assert!(manager.launcher_images.bound(&context, None).is_ok());
    let completion = PluginCommandCompletion {
        context: context.clone(),
        failed: false,
        response: Some(json!({
            "requestId":context.request_id,
            "results":[{"id":"ocr", "title":"first", "detail":"first\nsecond", "defaultAction":{"type":"copyText", "text":"first\nsecond"}}]
        })),
    };
    let mut outcome = manager.scheduler().complete(&context, now).unwrap();
    assert!(outcome.accepted);
    outcome.credential_publication = Some((42, publication.clone()));
    service
        .complete_submission_with(&completion, outcome, now, || {
            panic!("valid legacy OCR must not fault")
        })
        .unwrap();
    let Some(Ok(PublicPluginResponse::ServiceMainResults {
        results,
        credential_revision,
        publication: received,
    })) = submission
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("legacy image result must reach its submission receiver");
    };
    assert_eq!(results[0].detail.as_deref(), Some("first\nsecond"));
    assert_eq!(results[0].copy_text.as_deref(), Some("first\nsecond"));
    assert_eq!(credential_revision, 42);
    assert_eq!(received, publication);
}

#[test]
fn save_plugin_settings_service_settles_exact_tokens_and_preserves_execution_slot() {
    use std::collections::BTreeMap;
    for case in [
        "empty",
        "same",
        "ordinary",
        "secret",
        "mixed",
        "second-failure",
        "invalid-dto",
    ] {
        let root = TestRoot::new(case);
        let (service, manager) = settings_service(&root);
        let now = Instant::now();
        let first = service
            .schedule_command(
                manager.route("/demo first").unwrap().unwrap(),
                1,
                "/demo first".into(),
                now,
            )
            .unwrap();
        let running = first.dispatch.as_ref().unwrap().clone();
        let waiting = service
            .schedule_command(
                manager.route("/demo waiting").unwrap().unwrap(),
                2,
                "/demo waiting".into(),
                now,
            )
            .unwrap();
        assert!(waiting.dispatch.is_none());
        let untouched = service
            .prepare_command(
                manager.route("/demo later").unwrap().unwrap(),
                3,
                "/demo later".into(),
            )
            .unwrap();
        let settings = match case {
            "same" => BTreeMap::from([("flag".into(), json!(false))]),
            "ordinary" | "mixed" => BTreeMap::from([("flag".into(), json!(true))]),
            "invalid-dto" => BTreeMap::from([("flag".into(), json!("wrong"))]),
            _ => BTreeMap::new(),
        };
        let secrets = match case {
            "secret" | "mixed" | "invalid-dto" => {
                BTreeMap::from([("first".into(), Some("synthetic-first".into()))])
            }
            "second-failure" => BTreeMap::from([
                ("first".into(), Some("synthetic-first".into())),
                ("second".into(), Some("x".repeat(65537))),
            ]),
            _ => BTreeMap::new(),
        };
        let revision = manager.inventory().unwrap().revision;
        let outcome = service.save_settings("com.uipilot.demo", &settings, &secrets);
        let changed = matches!(case, "ordinary" | "secret" | "mixed" | "second-failure");
        assert_eq!(outcome.changed, changed, "{case}");
        assert_eq!(
            outcome.result.is_err(),
            matches!(case, "second-failure" | "invalid-dto"),
            "{case}"
        );
        if changed {
            assert_eq!(
                waiting.receiver.recv_timeout(Duration::from_secs(1)),
                Ok(None),
                "{case}"
            );
            assert!(!service
                .lock_submissions()
                .unwrap()
                .by_token
                .contains_key(&waiting.token));
        } else {
            assert_eq!(manager.inventory().unwrap().revision, revision, "{case}");
            assert!(service
                .lock_submissions()
                .unwrap()
                .by_token
                .contains_key(&waiting.token));
        }
        assert!(service
            .lock_submissions()
            .unwrap()
            .by_token
            .contains_key(&untouched.token));
        let next = service
            .schedule_command(
                manager.route("/demo next").unwrap().unwrap(),
                4,
                "/demo next".into(),
                now,
            )
            .unwrap();
        assert!(
            next.dispatch.is_none(),
            "{case}: old handler must keep running"
        );
        let completed = manager.scheduler().complete(&running.context, now).unwrap();
        assert!(!completed.accepted);
        assert_eq!(
            completed
                .next
                .unwrap()
                .candidate
                .owner
                .command()
                .expect("waiting request is a command")
                .submission_token,
            next.token
        );
    }
}

#[test]
fn save_plugin_network_toggle_settles_waiting_but_noop_preserves_it() {
    let root = TestRoot::new("network-toggle-retire");
    let (service, manager) = settings_service(&root);
    let now = Instant::now();
    let first = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    let running = first.dispatch.unwrap();
    service
        .set_network_access("com.uipilot.demo", true, None)
        .unwrap();
    assert_eq!(
        manager.scheduler().context_status(&running.context),
        super::PluginContextStatus::Current
    );
    let waiting = service
        .schedule_command(
            manager.route("/demo waiting").unwrap().unwrap(),
            2,
            "/demo waiting".into(),
            now,
        )
        .unwrap();
    service
        .set_network_access("com.uipilot.demo", true, None)
        .unwrap();
    assert!(service
        .lock_submissions()
        .unwrap()
        .by_token
        .contains_key(&waiting.token));
    service
        .set_network_access("com.uipilot.demo", false, None)
        .unwrap();
    assert_eq!(
        waiting.receiver.recv_timeout(Duration::from_secs(1)),
        Ok(None)
    );
    let next = service
        .schedule_command(
            manager.route("/demo next").unwrap().unwrap(),
            3,
            "/demo next".into(),
            now,
        )
        .unwrap();
    assert!(next.dispatch.is_none());
    let completed = manager.scheduler().complete(&running.context, now).unwrap();
    assert!(!completed.accepted);
    assert_eq!(
        completed
            .next
            .unwrap()
            .candidate
            .owner
            .command()
            .expect("waiting request is a command")
            .submission_token,
        next.token
    );
}

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
fn public_plugin_protocol_csp_allows_only_data_image_previews_and_denies_media() {
    assert!(super::PUBLIC_PLUGIN_CSP.contains("media-src 'none'"));
    assert!(super::PUBLIC_PLUGIN_CSP.contains("img-src data:"));
    assert!(!super::PUBLIC_PLUGIN_CSP.contains("img-src http:"));
    assert!(!super::PUBLIC_PLUGIN_CSP.contains("img-src https:"));
    assert!(!super::PUBLIC_PLUGIN_CSP.contains("blob:"));
}

fn multi_service(
    root: &TestRoot,
    recovery: bool,
) -> (Arc<PublicPluginService>, Arc<PublicPluginManager>) {
    let source = root.package();
    let mut value = manifest("mainResult");
    value.as_object_mut().unwrap().remove("command");
    value["minimumHostVersion"] = json!("0.5.0");
    value["permissions"] = json!(["clipboard.write"]);
    value["commands"] = json!([
        {"id":"upper", "defaultName":"upper", "activationMode":"live", "outputMode":"mainResult", "inputRequired":false},
        {"id":"constructor", "defaultName":"lower", "activationMode":"submit", "outputMode":"mainResult", "inputRequired":false}
    ]);
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost {
                version: [0, 5, 0],
                ..host()
            },
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
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([PublicPermission::ClipboardWrite]),
            now,
            |_| true,
        )
        .unwrap();
    if recovery {
        let transaction = manager
            .begin_uninstall("com.uipilot.demo", false)
            .unwrap()
            .unwrap();
        manager.abort_uninstall_before_commit(transaction).unwrap();
    }
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(Arc::clone(&manager)).is_ok());
    (service, manager)
}

#[test]
fn favorite_and_exact_override_noops_preserve_business_work_and_stored_choices() {
    for clear in [false, true] {
        let root = TestRoot::new("rename-stored-choice");
        let (service, manager) = multi_service(&root, false);
        if clear {
            service
                .rename_command("com.uipilot.demo", "upper", Some("upper"))
                .result
                .unwrap();
        }
        let current = root
            .0
            .join("app-data/public-plugins/state/com.uipilot.demo/state.json");
        let now = Instant::now();
        let a = service
            .schedule_command(
                manager.route("/upper A").unwrap().unwrap(),
                1,
                "/upper A".into(),
                now,
            )
            .unwrap();
        let running = a.dispatch.as_ref().unwrap();
        let favorite = service.set_command_favorite("com.uipilot.demo", "constructor", true);
        assert!(favorite.result.is_ok());
        assert!(!favorite.changed);
        assert_eq!(
            manager.scheduler().context_status(&running.context),
            super::PluginContextStatus::Current
        );
        let before = fs::read(&current).unwrap();
        let same = if clear { Some("upper") } else { None };
        let noop = service.rename_command("com.uipilot.demo", "upper", same);
        assert!(noop.result.is_ok());
        assert!(!noop.changed);
        assert_eq!(fs::read(&current).unwrap(), before);
        assert_eq!(
            manager.scheduler().context_status(&running.context),
            super::PluginContextStatus::Current
        );
        assert!(matches!(
            a.receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        let waiting = service
            .schedule_command(
                manager.route("/lower old").unwrap().unwrap(),
                2,
                "/lower old".into(),
                now,
            )
            .unwrap();
        let untouched = service
            .prepare_command(
                manager.route("/lower later").unwrap().unwrap(),
                3,
                "/lower later".into(),
            )
            .unwrap();
        let target = if clear { None } else { Some("upper") };
        let changed = service.rename_command("com.uipilot.demo", "upper", target);
        assert!(changed.result.is_ok());
        assert!(changed.changed);
        assert_eq!(a.receiver.try_recv(), Ok(None));
        assert_eq!(waiting.receiver.try_recv(), Ok(None));
        assert!(service
            .lock_submissions()
            .unwrap()
            .by_token
            .contains_key(&untouched.token));
        let wire: Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
        assert_eq!(wire["commands"]["upper"]["nameOverride"], json!(target));
        let before = fs::read(&current).unwrap();
        let noop = service.rename_command("com.uipilot.demo", "upper", target);
        assert!(noop.result.is_ok());
        assert!(!noop.changed);
        assert_eq!(fs::read(current).unwrap(), before);
    }
}

#[test]
fn rename_retains_running_until_completion_or_timeout_and_preserves_saved_sibling() {
    for renamed_id in ["upper", "constructor"] {
        for timeout in [false, true] {
            let root = TestRoot::new("rename-running");
            let (service, manager) = multi_service(&root, false);
            let now = Instant::now();
            let a = service
                .schedule_command(
                    manager.route("/upper A").unwrap().unwrap(),
                    1,
                    "/upper A".into(),
                    now,
                )
                .unwrap();
            let running = a.dispatch.as_ref().unwrap().clone();
            let old_waiting = service
                .schedule_command(
                    manager.route("/lower old").unwrap().unwrap(),
                    2,
                    "/lower old".into(),
                    now,
                )
                .unwrap();
            service
                .rename_command("com.uipilot.demo", renamed_id, Some("renamed"))
                .result
                .unwrap();
            let query = if renamed_id == "constructor" {
                "/renamed B  saved"
            } else {
                "/lower B  saved"
            };
            let b = service
                .schedule_command(manager.route(query).unwrap().unwrap(), 3, query.into(), now)
                .unwrap();
            assert!(
                b.dispatch.is_none(),
                "rename must retain the occupied Runtime slot"
            );
            assert_eq!(a.receiver.try_recv(), Ok(None));
            assert_eq!(old_waiting.receiver.try_recv(), Ok(None));
            let next = if timeout {
                let replacements = manager
                    .scheduler()
                    .expire_timeouts(running.deadline)
                    .unwrap();
                assert_eq!(replacements.len(), 1);
                let replacement = &replacements[0];
                assert!(!replacement.counts_as_fault);
                let candidate = manager
                    .replace_runtime_generation(
                        &replacement.plugin_id,
                        replacement.previous_generation,
                        replacement.new_generation,
                    )
                    .unwrap();
                manager
                    .finish_runtime_replacement(
                        &candidate.plugin_id,
                        candidate.generation,
                        candidate.activation_id,
                        candidate.admission_epoch,
                        running.deadline,
                    )
                    .unwrap()
                    .unwrap()
            } else {
                let completion = PluginCommandCompletion {
                    context: running.context.clone(),
                    failed: false,
                    response: Some(
                        json!({"requestId":running.context.request_id,"results":[{"id":"same","title":"stale"}]}),
                    ),
                };
                let outcome = manager.scheduler().complete(&running.context, now).unwrap();
                assert!(!outcome.accepted);
                service
                    .complete_submission_with(&completion, outcome, now, || {
                        panic!("retired completion must not fault")
                    })
                    .unwrap()
                    .unwrap()
            };
            assert_eq!(next.candidate.command_id, "constructor");
            assert_eq!(next.candidate.input, "B  saved");
            assert_eq!(
                next.candidate
                    .owner
                    .command()
                    .expect("waiting request is a command")
                    .submission_token,
                b.token
            );
            assert_eq!(
                manager.invocation_command_id(&next).unwrap().as_deref(),
                Some("constructor")
            );
        }
    }
}

#[test]
fn multi_command_service_binds_late_completion_to_saved_route_and_input() {
    let root = TestRoot::new("multi-command-service");
    let (service, manager) = multi_service(&root, false);
    let now = Instant::now();
    let a = service
        .schedule_command(
            manager.route("/upper A  original").unwrap().unwrap(),
            1,
            "/upper A  original".into(),
            now,
        )
        .unwrap();
    let dispatch_a = a.dispatch.as_ref().unwrap();
    assert_eq!(dispatch_a.candidate.command_id, "upper");
    assert_eq!(
        manager
            .invocation_command_id(dispatch_a)
            .unwrap()
            .as_deref(),
        Some("upper")
    );
    let b = service
        .schedule_command(
            manager.route("/lower B  original").unwrap().unwrap(),
            2,
            "/lower B  original".into(),
            now,
        )
        .unwrap();
    assert!(b.dispatch.is_none());
    // The current query can move independently; it must not rewrite the waiting submission.
    let changed_query = manager.route("/upper entirely different").unwrap().unwrap();
    assert_eq!(changed_query.input, "entirely different");
    let completion = PluginCommandCompletion {
        context: dispatch_a.context.clone(),
        failed: false,
        response: Some(
            json!({"requestId":dispatch_a.context.request_id, "results":[{"id":"same", "title":"A stale"}]}),
        ),
    };
    let outcome = manager
        .scheduler()
        .complete(&completion.context, now)
        .unwrap();
    assert!(!outcome.accepted);
    let dispatch_b = service
        .complete_submission_with(&completion, outcome, now, || panic!("must not fault"))
        .unwrap()
        .unwrap();
    assert_eq!(dispatch_b.candidate.command_id, "constructor");
    assert_eq!(dispatch_b.candidate.input, "B  original");
    assert_eq!(
        manager
            .invocation_command_id(&dispatch_b)
            .unwrap()
            .as_deref(),
        Some("constructor")
    );
    assert_eq!(a.receiver.recv_timeout(Duration::from_secs(1)), Ok(None));
    let pending = service.lock_submissions().unwrap();
    let saved = pending.by_token.get(&b.token).unwrap();
    assert_eq!(saved.route.command_id, "constructor");
    assert_eq!(saved.route.input, "B  original");
    drop(pending);
    let completion = PluginCommandCompletion {
        context: dispatch_b.context.clone(),
        failed: false,
        response: Some(
            json!({"requestId":dispatch_b.context.request_id, "results":[{"id":"same", "title":"B current", "defaultAction":{"type":"copyText", "text":"B private"}}]}),
        ),
    };
    let outcome = manager
        .scheduler()
        .complete(&completion.context, now)
        .unwrap();
    assert!(outcome.accepted);
    assert!(service
        .complete_submission_with(&completion, outcome, now, || panic!("must not fault"))
        .unwrap()
        .is_none());
    let Some(Ok(PublicPluginResponse::MainResults(results))) =
        b.receiver.recv_timeout(Duration::from_secs(1)).unwrap()
    else {
        panic!("B must receive its own results")
    };
    assert_eq!(results[0].title, "B current");
    assert_eq!(results[0].plugin_result_id, "same");
    assert_eq!(results[0].copy_text.as_deref(), Some("B private"));
    assert!(service.lock_submissions().unwrap().by_token.is_empty());
    assert!(service
        .lock_submissions()
        .unwrap()
        .token_by_request
        .is_empty());
}

#[test]
fn multi_command_recovery_preserves_latest_command_id_and_exact_input() {
    let root = TestRoot::new("multi-command-recovery");
    let (service, manager) = multi_service(&root, true);
    let now = Instant::now();
    let a = service
        .schedule_command(
            manager.route("/upper old  input").unwrap().unwrap(),
            1,
            "/upper old  input".into(),
            now,
        )
        .unwrap();
    let b = service
        .schedule_command(
            manager.route("/lower B  saved").unwrap().unwrap(),
            2,
            "/lower B  saved".into(),
            now,
        )
        .unwrap();
    let first = a.recovery.as_ref().unwrap();
    let second = b.recovery.as_ref().unwrap();
    let calls = AtomicU64::new(0);
    assert!(service
        .complete_runtime_recovery_with(first, now, |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            true
        })
        .unwrap()
        .is_none());
    let dispatch = service
        .complete_runtime_recovery_with(second, now, |_| panic!("one shared Runtime"))
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(dispatch.candidate.command_id, "constructor");
    assert_eq!(dispatch.candidate.input, "B  saved");
    assert_eq!(
        manager.invocation_command_id(&dispatch).unwrap().as_deref(),
        Some("constructor")
    );
    assert_eq!(
        service
            .lock_submissions()
            .unwrap()
            .by_token
            .get(&b.token)
            .unwrap()
            .route
            .command_id,
        "constructor"
    );
    assert_eq!(a.receiver.recv_timeout(Duration::from_secs(1)), Ok(None));
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
    assert_eq!(manager.invocation_command_id(&dispatch).unwrap(), None);

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

    assert_eq!(
        promoted
            .candidate
            .owner
            .command()
            .expect("waiting request is a command")
            .submission_token,
        second_token
    );
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
            prepared.manifest.legacy_command().unwrap().output_mode,
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
        assert_eq!(
            prepared.manifest.legacy_command().unwrap().default_name,
            command
        );
        assert_eq!(
            prepared.manifest.legacy_command().unwrap().output_mode,
            output_mode
        );
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
fn repository_multi_demo_stages_real_directory_and_archive_on_current_and_rejects_0_4_host() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = workspace.join("examples/public-plugins/com.uipilot.demo-multi/package");
    let new_host = PublicPluginHost {
        version: [0, 5, 0],
        ..host()
    };
    let historical_host = PublicPluginHost {
        version: [0, 4, 0],
        ..host()
    };
    for archive in [false, true] {
        let root = TestRoot::new(if archive {
            "multi-demo-archive"
        } else {
            "multi-demo-directory"
        });
        let package_source = if archive {
            let archive_path = root.0.join("multi.uipilot-plugin");
            archive_directory(&source, &archive_path);
            PublicPackageSource::Archive(archive_path)
        } else {
            PublicPackageSource::DevelopmentDirectory(source.clone())
        };
        assert_eq!(
            stage_public_package(package_source.clone(), &root.staging(), &historical_host)
                .unwrap_err(),
            PublicPackageError::IncompatibleApi
        );
        let current_root = TestRoot::new("multi-demo-current-host");
        let current = stage_public_package(
            package_source.clone(),
            &current_root.staging(),
            &host(),
        )
        .unwrap();
        let prepared = stage_public_package(package_source, &root.staging(), &new_host).unwrap();
        assert_eq!(current.manifest, prepared.manifest);
        assert_eq!(current.revalidate(), Ok(()));
        assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo-multi");
        assert!(prepared.manifest.is_multi_command());
        assert!(prepared.manifest.legacy_command().is_none());
        let commands = prepared.manifest.normalized_commands().collect::<Vec<_>>();
        assert_eq!(
            commands.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec!["upper", "lower"]
        );
        assert_eq!(commands[0].1.activation_mode, PublicActivationMode::Live);
        assert_eq!(commands[1].1.activation_mode, PublicActivationMode::Submit);
        assert!(commands
            .iter()
            .all(|(_, command)| command.output_mode == PublicOutputMode::MainResult));
        assert_eq!(
            prepared.manifest.permissions,
            vec![PublicPermission::ClipboardWrite]
        );
        assert_eq!(
            prepared
                .resources
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["dist/runtime.js", "plugin.json"]
        );
        let source_manifest: Value =
            serde_json::from_slice(&fs::read(source.join("plugin.json")).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&prepared.manifest).unwrap(),
            source_manifest
        );
        for file in ["plugin.json", "dist/runtime.js"] {
            assert_eq!(
                fs::read(prepared.package_root.join(file)).unwrap(),
                fs::read(source.join(file)).unwrap()
            );
        }
        assert_eq!(prepared.revalidate(), Ok(()));
    }
}

#[test]
fn repository_legacy_original_packages_stage_as_directory_and_archive_on_0_5_host() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let new_host = PublicPluginHost {
        version: [0, 5, 0],
        ..host()
    };
    for (id, mode) in [
        ("com.uipilot.demo-return", PublicOutputMode::MainResult),
        ("com.uipilot.demo-panel", PublicOutputMode::Panel),
        ("com.uipilot.demo-win", PublicOutputMode::Window),
        ("com.uipilot.pomodoro", PublicOutputMode::Window),
    ] {
        let source = workspace.join(format!("examples/public-plugins/{id}/package"));
        for archive in [false, true] {
            let root = TestRoot::new("legacy-demo-0-5");
            let package_source = if archive {
                let archive_path = root.0.join("legacy.uipilot-plugin");
                archive_directory(&source, &archive_path);
                PublicPackageSource::Archive(archive_path)
            } else {
                PublicPackageSource::DevelopmentDirectory(source.clone())
            };
            let prepared =
                stage_public_package(package_source, &root.staging(), &new_host).unwrap();
            assert_eq!(prepared.manifest.plugin_id, id);
            assert!(!prepared.manifest.is_multi_command());
            assert_eq!(
                prepared.manifest.legacy_command().unwrap().output_mode,
                mode
            );
            assert_eq!(
                prepared
                    .manifest
                    .normalized_commands()
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>(),
                vec!["default"]
            );
            let mut files = Vec::new();
            collect_files(&source, &source, &mut files);
            assert_eq!(
                prepared.resources.len(),
                files
                    .iter()
                    .filter(|(path, _)| path != "assets/sounds/timer-alarm.wav")
                    .count()
            );
            for (path, bytes) in files {
                assert_eq!(fs::read(prepared.package_root.join(path)).unwrap(), bytes);
            }
            assert_eq!(prepared.revalidate(), Ok(()));
        }
    }
}

#[test]
fn repository_credential_demo_stages_on_0_4_0_and_rejects_0_3_4_without_cli_build() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = workspace.join("examples/public-plugins/com.uipilot.demo-credentials/package");
    let old_root = TestRoot::new("credential-demo-old-host");
    let old_host = PublicPluginHost {
        version: [0, 3, 4],
        ..host()
    };
    assert!(matches!(
        stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source.clone()),
            &old_root.staging(),
            &old_host
        ),
        Err(PublicPackageError::IncompatibleApi)
    ));
    let root = TestRoot::new("credential-demo-0-4-host");
    let credential_host = PublicPluginHost {
        version: [0, 4, 0],
        ..host()
    };
    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source.clone()),
        &root.staging(),
        &credential_host,
    )
    .unwrap();
    assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo-credentials");
    assert_eq!(
        prepared.manifest.permissions,
        vec![PublicPermission::NetworkHttps]
    );
    assert_eq!(
        prepared.manifest.legacy_command().unwrap().output_mode,
        PublicOutputMode::MainResult
    );
    let manifest = serde_json::to_value(&prepared.manifest).unwrap();
    assert_eq!(
        manifest["network"],
        json!({"httpsHosts":["api.example.com"],"credentials":[
            {"id":"service-token","settingKey":"api-token","host":"api.example.com","type":"bearer"}
        ]})
    );
    assert_eq!(
        manifest["settings"],
        json!([{"key":"api-token","label":"Synthetic API token","type":"secret"}])
    );
    assert_eq!(
        prepared
            .resources
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["dist/runtime.js", "plugin.json"]
    );
    assert_eq!(
        fs::read(prepared.package_root.join("dist/runtime.js")).unwrap(),
        fs::read(source.join("dist/runtime.js")).unwrap()
    );
    assert_eq!(prepared.revalidate(), Ok(()));
    let current_root = TestRoot::new("credential-demo-current-host");
    let current = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &current_root.staging(),
        &host(),
    )
    .unwrap();
    assert_eq!(current.manifest, prepared.manifest);
    assert_eq!(current.revalidate(), Ok(()));
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
    assert_eq!(
        prepared.manifest.legacy_command().unwrap().default_name,
        "demo-panel"
    );
    assert_eq!(
        prepared.manifest.legacy_command().unwrap().output_mode,
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

#[test]
fn clipboard_history_host_fixture_stages_with_final_panel_contract() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let root = TestRoot::new("clipboard-history-host-fixture");
    let source = workspace.join("tests/fixtures/public-plugins/clipboard-history-host-fixture");
    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();

    assert_eq!(
        prepared.manifest.plugin_id,
        "com.uipilot.clipboard-history-host-fixture"
    );
    assert_eq!(
        prepared.manifest.legacy_command().unwrap().output_mode,
        PublicOutputMode::Panel
    );
    assert_eq!(
        prepared.manifest.permissions,
        vec![
            PublicPermission::UiPanel,
            PublicPermission::ClipboardHistoryRead,
            PublicPermission::ClipboardHistoryPaste,
        ]
    );
    assert_eq!(
        prepared.manifest.panel.as_ref().unwrap().host_keys,
        vec![
            PanelHostKeyDeclaration::Tab,
            PanelHostKeyDeclaration::ShiftTab,
            PanelHostKeyDeclaration::Enter,
        ]
    );
    assert_eq!(
        prepared
            .resources
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
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

#[test]
fn cli_pack_fixture_stages_empty_and_nested_directories() {
    let root = TestRoot::new("cli-pack-fixture");
    let archive = root.0.join("roundtrip.uipilot-plugin");
    fs::write(
        &archive,
        include_bytes!("../../../tests/fixtures/public-plugin-pack/roundtrip.uipilot-plugin"),
    )
    .unwrap();
    let prepared = package::stage(
        PublicPackageSource::Archive(archive),
        &root.staging(),
        &host(),
    )
    .unwrap();
    assert_eq!(prepared.manifest.plugin_id, "com.example.cli-pack");
    assert_eq!(
        fs::read(prepared.package_root.join("dist/runtime.js")).unwrap(),
        b"export {}\n"
    );
    assert!(prepared.package_root.join("empty").is_dir());
    assert!(prepared.package_root.join("nested/empty").is_dir());
    assert!(fs::read_dir(prepared.package_root.join("empty"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(prepared.package_root.join("nested/empty"))
        .unwrap()
        .next()
        .is_none());
    assert_eq!(prepared.revalidate(), Ok(()));
}

#[test]
fn multi_command_manifest_shared_fixtures() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json"
    ))
    .unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let parts = fixture["hostVersion"]
            .as_str()
            .unwrap()
            .split('.')
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        let host = PublicPluginHost {
            platform: PublicPlatform::Windows,
            version: [parts[0], parts[1], parts[2]],
            api_version: 1,
        };
        let result = super::manifest::parse_manifest(
            &serde_json::to_vec(&fixture["manifest"]).unwrap(),
            &host,
        );
        let actual = result.err().map(|e| format!("{e:?}"));
        assert_eq!(
            actual.as_deref(),
            fixture["rustError"].as_str(),
            "{}",
            fixture["name"]
        );
    }
}

#[test]
fn multi_command_normalization_preserves_wire_and_identity() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json"
    ))
    .unwrap();
    let host = PublicPluginHost {
        platform: PublicPlatform::Windows,
        version: [0, 5, 0],
        api_version: 1,
    };
    for fixture in fixtures
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rustError"].is_null())
    {
        let manifest = super::manifest::parse_manifest(
            &serde_json::to_vec(&fixture["manifest"]).unwrap(),
            &host,
        )
        .unwrap();
        if manifest.is_multi_command() {
            let historical_host = PublicPluginHost {
                version: [0, 4, 0],
                ..host
            };
            assert_eq!(
                super::manifest::parse_manifest(
                    &serde_json::to_vec(&fixture["manifest"]).unwrap(),
                    &historical_host
                ),
                Err(PublicPackageError::IncompatibleApi),
                "historical Host must reject {}",
                fixture["name"]
            );
        }
        let expected = fixture["manifest"]["commands"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| item["id"].as_str().unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec!["default"]);
        assert_eq!(
            manifest
                .normalized_commands()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            manifest.is_multi_command(),
            fixture["manifest"].get("commands").is_some()
        );
        assert_eq!(
            manifest.legacy_command().is_some(),
            !manifest.is_multi_command()
        );
        for (id, command) in manifest.normalized_commands() {
            assert_eq!(manifest.command_by_id(id), Some(command));
        }
        assert!(manifest.command_by_id("missing").is_none());
        let serialized = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            serialized.get("command").is_some(),
            !manifest.is_multi_command()
        );
        assert_eq!(
            super::manifest::parse_manifest(&serde_json::to_vec(&serialized).unwrap(), &host)
                .unwrap(),
            manifest
        );
    }
}

#[test]
fn multi_command_manifest_rejects_duplicate_json_fields() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json"
    ))
    .unwrap();
    let raw = serde_json::to_string(&fixtures[0]["manifest"]).unwrap();
    let host = PublicPluginHost {
        platform: PublicPlatform::Windows,
        version: [0, 5, 0],
        api_version: 1,
    };
    for (from, to) in [
        ("\"id\":\"default\"", "\"id\":\"default\",\"id\":\"other\""),
        (
            "\"defaultName\":\"upper\"",
            "\"defaultName\":\"upper\",\"defaultName\":\"other\"",
        ),
    ] {
        let invalid = raw.replace(from, to);
        assert_ne!(invalid, raw);
        assert_eq!(
            super::manifest::parse_manifest(invalid.as_bytes(), &host),
            Err(PublicPackageError::InvalidPackage)
        );
    }
}

#[test]
fn cli_multi_command_pack_fixture_stages_on_0_5_0() {
    let root = TestRoot::new("cli-multi-command-pack-fixture");
    let archive = root.0.join("multi-command.uipilot-plugin");
    fs::write(
        &archive,
        include_bytes!("../../../tests/fixtures/public-plugin-pack/multi-command.uipilot-plugin"),
    )
    .unwrap();
    let minimum_host = PublicPluginHost {
        version: [0, 5, 0],
        ..host()
    };
    let prepared = package::stage(
        PublicPackageSource::Archive(archive.clone()),
        &root.staging(),
        &minimum_host,
    )
    .unwrap();
    assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo-multi");
    assert!(prepared.manifest.is_multi_command());
    assert!(prepared.manifest.legacy_command().is_none());
    let commands = prepared.manifest.normalized_commands().collect::<Vec<_>>();
    assert_eq!(
        commands.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec!["upper", "lower"]
    );
    assert_eq!(commands[0].1.activation_mode, PublicActivationMode::Live);
    assert_eq!(commands[1].1.activation_mode, PublicActivationMode::Submit);
    assert!(commands
        .iter()
        .all(|(_, command)| command.output_mode == PublicOutputMode::MainResult));
    assert_eq!(
        prepared
            .resources
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["dist/runtime.js", "plugin.json"]
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("examples/public-plugins/com.uipilot.demo-multi/package");
    assert_eq!(
        serde_json::to_value(&prepared.manifest).unwrap(),
        serde_json::from_slice::<Value>(&fs::read(source.join("plugin.json")).unwrap()).unwrap()
    );
    for file in ["plugin.json", "dist/runtime.js"] {
        assert_eq!(
            fs::read(prepared.package_root.join(file)).unwrap(),
            fs::read(source.join(file)).unwrap()
        );
    }
    assert_eq!(prepared.revalidate(), Ok(()));
    let historical_host = PublicPluginHost {
        version: [0, 4, 0],
        ..host()
    };
    assert_eq!(
        package::stage(
            PublicPackageSource::Archive(archive),
            &root.staging(),
            &historical_host
        )
        .unwrap_err(),
        PublicPackageError::IncompatibleApi
    );
}

fn panel_action_fixture(
    name: &str,
) -> (
    TestRoot,
    Arc<PublicPluginService>,
    Arc<PublicPluginManager>,
    Arc<crate::plugin_panel::PluginPanelController>,
    crate::plugin_panel::PanelSessionIdentity,
) {
    panel_action_fixture_with_image(name, false)
}
fn panel_action_fixture_with_image(name: &str, images: bool) -> (TestRoot, Arc<PublicPluginService>, Arc<PublicPluginManager>, Arc<crate::plugin_panel::PluginPanelController>, crate::plugin_panel::PanelSessionIdentity) {
    let root = TestRoot::new(name);
    let source = root.package();
    let mut value = manifest("panel");
    value["minimumHostVersion"] = json!("0.6.0");
    value["panel"]["runtimeActions"] = json!(true);
    if images {
        value["minimumHostVersion"] = json!("0.21.0");
        value["command"]["inputCapabilities"] = json!(["image.ocr"]);
    }
    value["settings"] = json!([{ "key":"synthetic", "label":"Synthetic", "type":"secret" }]);
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost::current(PublicPlatform::Windows),
            [],
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
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([PublicPermission::UiPanel]),
            now,
            |_| true,
        )
        .unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    let route = manager.route("/demo").unwrap().unwrap();
    let controller = Arc::new(crate::plugin_panel::PluginPanelController::default());
    let identity = controller
        .open_session(crate::plugin_panel::PanelOwner {
            plugin_id: route.plugin_id,
            plugin_generation: route.generation,
            activation_id: route.activation_id,
            admission_epoch: route.admission_epoch,
            command_label: "demo".into(),
            request_id: "initial".into(),
            submission_token: "initial-command".into(),
            argument: String::new(),
            host_keys: vec![],
            host_key_focus: super::PanelHostKeyFocus::Content,
        })
        .unwrap();
    assert!(controller.mark_ready(
        &identity.content_label,
        identity.session_epoch,
        false,
        false
    ));
    if images { controller.configure_images(identity.session_epoch, None).unwrap(); }
    (root, service, manager, controller, identity)
}

#[path = "panel_image_service_tests.rs"]
mod panel_image_service_tests;

fn panel_action_input() -> super::panel_action::PanelActionRequest {
    super::panel_action::PanelActionRequest {
        image_id: None,
        action: "save".into(),
        payload: json!({"title":"draft"}),
    }
}

#[test]
fn panel_action_update_and_ack_remain_busy_until_host_settlement() {
    let (_root, service, _manager, controller, identity) =
        panel_action_fixture("panel-action-settled-ready");
    controller
        .claim_delivery_settlement(identity.session_epoch, "initial", "initial-command")
        .unwrap();
    let admit = || {
        controller.admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
    };
    assert_eq!(
        admit().err(),
        Some(super::panel_action::PanelActionError::PanelRequestBusyError)
    );
    assert!(controller.mark_ack(&identity.content_label, identity.session_epoch, "initial"));
    assert_eq!(
        admit().err(),
        Some(super::panel_action::PanelActionError::PanelRequestBusyError)
    );
    controller
        .wait_until_acked(identity.session_epoch, "initial", Duration::from_secs(1))
        .unwrap();
    let (_dispatch, _receiver) = admit().unwrap();
    assert_eq!(
        admit().err(),
        Some(super::panel_action::PanelActionError::PanelRequestBusyError)
    );
}

#[test]
fn panel_action_busy_does_not_replace_running_command_or_waiter() {
    let (_root, service, manager, controller, identity) = panel_action_fixture("panel-action-busy");
    let now = Instant::now();
    let first = service
        .schedule_command(
            manager.route("/demo first").unwrap().unwrap(),
            1,
            "/demo first".into(),
            now,
        )
        .unwrap();
    let waiting = service
        .schedule_command(
            manager.route("/demo waiting").unwrap().unwrap(),
            2,
            "/demo waiting".into(),
            now,
        )
        .unwrap();
    let error = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .err();
    assert_eq!(
        error,
        Some(super::panel_action::PanelActionError::PanelRequestBusyError)
    );
    let completed = manager
        .scheduler()
        .complete(&first.dispatch.unwrap().context, now)
        .unwrap();
    assert!(!completed.accepted);
    assert_eq!(
        completed.next.unwrap().candidate.owner.token(),
        Some(waiting.token.as_str())
    );
}

#[test]
fn panel_action_close_barrier_revokes_late_storage_and_keeps_physical_deadline() {
    use super::{runtime::PluginApiOperation, PluginApiRequest, PluginContextStatus};
    use std::sync::Barrier;
    let (_root, service, manager, controller, identity) =
        panel_action_fixture("panel-action-close");
    for (label, epoch) in [
        ("main", identity.session_epoch),
        (identity.content_label.as_str(), identity.session_epoch + 1),
    ] {
        assert_eq!(
            controller
                .admit_panel_action(&service, label, epoch, panel_action_input())
                .err(),
            Some(super::panel_action::PanelActionError::ExpiredPanelSession)
        );
    }
    let mut wrong_activation = identity.clone();
    wrong_activation.activation_id += 1;
    assert_eq!(
        manager
            .admit_panel_action(&wrong_activation, panel_action_input(), Instant::now())
            .err(),
        Some(super::panel_action::PanelActionError::ExpiredPanelSession)
    );
    let (request, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    let context = request.context.clone();
    let label = super::runtime_label(&context.plugin_id, context.plugin_generation).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let late_barrier = barrier.clone();
    let late_manager = manager.clone();
    let late_context = context.clone();
    let late = std::thread::spawn(move || {
        late_barrier.wait();
        late_manager
            .execute_api(
                &label,
                PluginApiRequest {
                    context: late_context,
                    operation: PluginApiOperation::StorageSet,
                    key: Some("late".into()),
                    value: Some(json!(true)),
                    notification: None,
                },
            )
            .result
    });
    controller.host_hidden();
    barrier.wait();
    assert_eq!(
        late.join().unwrap(),
        Err(PluginRuntimeError::ExpiredRequest)
    );
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(
            super::panel_action::PanelActionError::ExpiredPanelSession
        ))
    );
    assert_eq!(
        manager.scheduler().context_status(&context),
        PluginContextStatus::Expired
    );
    assert_eq!(
        controller
            .admit_panel_action(
                &service,
                &identity.content_label,
                identity.session_epoch,
                panel_action_input()
            )
            .err(),
        Some(super::panel_action::PanelActionError::ExpiredPanelSession)
    );
    let command = service
        .schedule_command(
            manager.route("/demo next").unwrap().unwrap(),
            1,
            "/demo next".into(),
            Instant::now(),
        )
        .unwrap();
    assert!(
        command.dispatch.is_none(),
        "retired handler still occupies Runtime"
    );
    assert!(manager
        .expire_network_timeouts(request.deadline - Duration::from_millis(1))
        .unwrap()
        .is_empty());
    let outcome = manager
        .scheduler()
        .complete(&context, Instant::now())
        .unwrap();
    assert!(!outcome.accepted);
    assert_eq!(
        outcome.next.unwrap().candidate.owner.token(),
        Some(command.token.as_str())
    );
    assert!(manager
        .scheduler()
        .complete(&context, Instant::now())
        .is_err());
}

#[test]
fn panel_action_host_submit_cancels_once_and_late_completion_cannot_publish() {
    let (_root, service, manager, controller, identity) =
        panel_action_fixture("panel-action-host-submit");
    let (action, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    let command = service
        .schedule_command(
            manager.route("/demo next").unwrap().unwrap(),
            1,
            "/demo next".into(),
            Instant::now(),
        )
        .unwrap();
    assert!(command.dispatch.is_none());
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(
            super::panel_action::PanelActionError::PanelRequestCancelledError
        ))
    );
    assert!(!service.settle_panel_action(&action.context, Ok(json!("late")), true));
    let completed = manager
        .scheduler()
        .complete(&action.context, Instant::now())
        .unwrap();
    assert!(!completed.accepted);
    assert_eq!(
        completed.next.as_ref().unwrap().candidate.owner.token(),
        Some(command.token.as_str())
    );
    assert!(receiver.try_recv().is_err());
    // A late physical completion must not retain a ticket after Host settled the sender.
    assert!(!service.settle_panel_action(&action.context, Ok(json!("late")), true));
    assert_eq!(
        manager.claim_panel_completion(&identity, &action.context),
        Err(super::panel_action::PanelActionError::ExpiredPanelSession)
    );
    manager
        .scheduler()
        .complete(&completed.next.unwrap().context, Instant::now())
        .unwrap();
    let (fresh, fresh_receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    assert!(
        manager
            .scheduler()
            .complete(&fresh.context, Instant::now())
            .unwrap()
            .accepted
    );
    // Old-context cleanup must not steal the new response's ticket.
    assert!(!service.settle_panel_action(&action.context, Ok(json!("old")), true));
    assert!(service.settle_panel_action(&fresh.context, Ok(json!("fresh")), true));
    assert_eq!(
        fresh_receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Ok(json!("fresh")))
    );
}

#[test]
fn panel_action_host_submit_cancels_when_completion_takes_sender_before_host_scan() {
    use super::panel_action::PanelActionError as E;
    let (_root, service, manager, controller, identity) =
        panel_action_fixture("panel-action-host-submit-race");
    let (action, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    let prepared = service
        .prepare_command(
            manager.route("/demo next").unwrap().unwrap(),
            1,
            "/demo next".into(),
        )
        .unwrap();
    let token = prepared.token.clone();
    let (enqueued, wait_enqueued) = std::sync::mpsc::channel();
    let (taken, wait_taken) = std::sync::mpsc::channel();
    let completion_service = service.clone();
    let completion_manager = manager.clone();
    let context = action.context.clone();
    let completion = std::thread::spawn(move || {
        wait_enqueued.recv_timeout(Duration::from_secs(5)).unwrap();
        let outcome = completion_manager
            .scheduler()
            .complete(&context, Instant::now())
            .unwrap();
        assert!(!outcome.accepted);
        assert_eq!(
            outcome.next.unwrap().candidate.owner.token(),
            Some(token.as_str())
        );
        // Same Expired result selected by complete_submission for an unaccepted completion.
        assert!(completion_service.settle_panel_action_with(
            &context,
            Err(E::ExpiredPanelSession),
            true,
            || taken.send(()).unwrap(),
        ));
    });
    let command = controller
        .admit_current_dispatch(identity.session_epoch, "initial", "initial-command", || {
            service.admit_prepared_command_with(prepared, Instant::now(), || {
                assert_eq!(
                    manager.scheduler().context_status(&action.context),
                    super::PluginContextStatus::Expired
                );
                assert!(manager
                    .expire_network_timeouts(action.deadline - Duration::from_millis(1))
                    .unwrap()
                    .is_empty());
                enqueued.send(()).unwrap();
                wait_taken.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(!service
                    .content_actions
                    .lock()
                    .unwrap()
                    .contains_key(&action.context));
                // Completion has taken the sender but cannot enter this controller yet.
                assert_eq!(
                    receiver.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Empty)
                );
            })
        })
        .unwrap();
    assert!(
        command.dispatch.is_none(),
        "Host must wait for physical completion"
    );
    completion.join().unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(E::PanelRequestCancelledError))
    );
    assert_eq!(
        receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    assert!(!service.settle_panel_action(&action.context, Ok(json!("late")), true));
    assert!(manager
        .scheduler()
        .complete(&action.context, Instant::now())
        .is_err());
    assert_eq!(
        manager.claim_panel_completion(&identity, &action.context),
        Err(E::ExpiredPanelSession)
    );
}

#[test]
fn panel_action_completion_rechecks_replacement_and_old_timeout_preserves_new_generation() {
    let (_root, service, manager, controller, identity) =
        panel_action_fixture("panel-action-replace");
    let (action, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    assert!(
        manager
            .scheduler()
            .complete(&action.context, Instant::now())
            .unwrap()
            .accepted
    );
    // This replacement wins the controller lock before settlement. Old success must become expiry.
    let replacement = controller
        .open_session(crate::plugin_panel::PanelOwner {
            plugin_id: identity.plugin_id.clone(),
            plugin_generation: identity.generation + 1,
            activation_id: identity.activation_id + 1,
            admission_epoch: identity.admission_epoch + 1,
            command_label: "demo".into(),
            request_id: "replacement".into(),
            submission_token: "replacement".into(),
            argument: String::new(),
            host_keys: vec![],
            host_key_focus: super::PanelHostKeyFocus::Content,
        })
        .unwrap();
    assert!(!service.settle_panel_action(&action.context, Ok(json!("late")), true));
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(
            super::panel_action::PanelActionError::ExpiredPanelSession
        ))
    );
    assert!(controller
        .teardown_generation(&identity.plugin_id, identity.generation)
        .is_none());
    assert_eq!(controller.live_identity(), Some(replacement));
}

#[test]
fn panel_action_completed_response_is_retired_by_credential_change_before_settlement() {
    let (_root, service, manager, controller, identity) =
        panel_action_fixture("panel-action-completed-credential");
    let (action, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    assert!(
        manager
            .scheduler()
            .complete(&action.context, Instant::now())
            .unwrap()
            .accepted
    );
    let mut effects = super::activation::ManagementEffects::default();
    manager
        .save_secret_with_effects(&identity.plugin_id, "synthetic", None, &mut effects)
        .unwrap();
    assert!(effects.retired.iter().any(|(c, _)| c == &action.context));
    assert!(service.settle_panel_action(&action.context, Ok(json!("must not publish")), true));
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(
            super::panel_action::PanelActionError::ExpiredPanelSession
        ))
    );
    let (next, next_receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    assert!(
        manager
            .scheduler()
            .complete(&next.context, Instant::now())
            .unwrap()
            .accepted
    );
    assert!(service.settle_panel_action(&next.context, Ok(json!("current")), true));
    assert_eq!(
        next_receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Ok(json!("current")))
    );
}

#[test]
fn panel_action_timeout_and_transport_errors_keep_their_fixed_error_names() {
    for error in [
        super::panel_action::PanelActionError::PanelRequestTimeoutError,
        super::panel_action::PanelActionError::PanelRequestUnavailableError,
    ] {
        let (_root, service, manager, controller, identity) =
            panel_action_fixture("panel-action-terminal-error");
        let (action, receiver) = controller
            .admit_panel_action(
                &service,
                &identity.content_label,
                identity.session_epoch,
                panel_action_input(),
            )
            .unwrap();
        manager.retire_panel_action(&action.context);
        assert!(service.settle_panel_action(&action.context, Err(error), true));
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Err(error))
        );
        assert!(
            !manager
                .scheduler()
                .complete(&action.context, Instant::now())
                .unwrap()
                .accepted
        );
    }
}

#[test]
fn panel_action_all_session_removal_entries_retire_the_exact_registration() {
    for removal in ["session", "plugin", "current", "hidden", "replacement"] {
        let (_root, service, manager, controller, identity) =
            panel_action_fixture(&format!("panel-action-removal-{removal}"));
        let (action, receiver) = controller
            .admit_panel_action(
                &service,
                &identity.content_label,
                identity.session_epoch,
                panel_action_input(),
            )
            .unwrap();
        match removal {
            "session" => {
                controller.teardown_session(Some(identity.session_epoch));
            }
            "plugin" => {
                controller.teardown_plugin(&identity.plugin_id);
            }
            "current" => {
                controller.teardown_current(identity.session_epoch, "initial", "initial-command");
            }
            "hidden" => controller.host_hidden(),
            _ => {
                controller.open_session(crate::plugin_panel::PanelOwner {
                    plugin_id: identity.plugin_id,
                    plugin_generation: identity.generation,
                    activation_id: identity.activation_id,
                    admission_epoch: identity.admission_epoch,
                    command_label: "demo".into(),
                    request_id: "replacement".into(),
                    submission_token: "replacement".into(),
                    argument: String::new(),
                    host_keys: vec![],
                    host_key_focus: super::PanelHostKeyFocus::Content,
                });
            }
        }
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Err(
                super::panel_action::PanelActionError::ExpiredPanelSession
            )),
            "{removal}"
        );
        assert_eq!(
            manager.scheduler().context_status(&action.context),
            super::PluginContextStatus::Expired
        );
        assert!(
            !manager
                .scheduler()
                .complete(&action.context, Instant::now())
                .unwrap()
                .accepted
        );
    }
}

#[test]
fn panel_action_poisoned_controller_hide_still_retires_registered_authority() {
    let (_root, service, manager, controller, identity) =
        panel_action_fixture("panel-action-poisoned-controller");
    let (action, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            panel_action_input(),
        )
        .unwrap();
    controller.poison_core_for_test();
    controller.host_hidden();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(
            super::panel_action::PanelActionError::ExpiredPanelSession
        ))
    );
    assert_eq!(
        manager.scheduler().context_status(&action.context),
        super::PluginContextStatus::Expired
    );
    assert_eq!(
        controller
            .admit_panel_action(
                &service,
                &identity.content_label,
                identity.session_epoch,
                panel_action_input()
            )
            .err(),
        Some(super::panel_action::PanelActionError::PanelRequestUnavailableError)
    );
    assert!(
        !manager
            .scheduler()
            .complete(&action.context, Instant::now())
            .unwrap()
            .accepted
    );
}

fn window_action_fixture(
    name: &str,
) -> (
    TestRoot,
    Arc<PublicPluginService>,
    Arc<PublicPluginManager>,
    Arc<crate::plugin_window::PluginWindowController>,
    crate::plugin_window::WindowActionIdentity,
) {
    window_action_fixture_options(name, false)
}
fn window_action_fixture_options(
    name: &str,
    files: bool,
) -> (
    TestRoot,
    Arc<PublicPluginService>,
    Arc<PublicPluginManager>,
    Arc<crate::plugin_window::PluginWindowController>,
    crate::plugin_window::WindowActionIdentity,
) {
    let root = TestRoot::new(name);
    let source = root.package();
    let mut value = manifest("window");
    value["minimumHostVersion"] = json!("0.9.0");
    value["window"]["runtimeActions"] = json!(true);
    value["settings"] = json!([{ "key":"synthetic", "label":"Synthetic", "type":"secret" }]);
    if files {
        value["minimumHostVersion"] = json!("0.13.0");
        value["permissions"] = json!(["ui.window", "files.openText"]);
    }
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost::current(PublicPlatform::Windows),
            [],
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
        .commit_with_readiness(
            "main",
            &prepared.token,
            if files {
                BTreeSet::from([PublicPermission::UiWindow, PublicPermission::FilesOpenText])
            } else {
                BTreeSet::from([PublicPermission::UiWindow])
            },
            now,
            |_| true,
        )
        .unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    let route = manager.route("/demo").unwrap().unwrap();
    let controller = Arc::new(crate::plugin_window::PluginWindowController::default());
    let owner = crate::plugin_window::PluginWindowOwner {
        ui_intent_epoch: 1,
        submission_token: "initial-command".into(),
        request_id: "initial".into(),
        control_value: "/demo".into(),
        plugin_id: route.plugin_id,
        plugin_generation: route.generation,
        activation_id: route.activation_id,
        admission_epoch: route.admission_epoch,
    };
    let transaction = controller.submit(owner.clone()).unwrap();
    assert!(controller.advance(
        &owner,
        crate::plugin_window::PluginWindowPhase::AwaitingReady,
        crate::plugin_window::PluginWindowPhase::Visible
    ));
    assert!(controller.activate_timer_session(&owner));
    let identity = crate::plugin_window::WindowActionIdentity {
        owner,
        content_label: transaction.content_label,
        session_generation: transaction.timer_session_generation,
        document_generation: transaction.document_generation,
    };
    (root, service, manager, controller, identity)
}
fn admit_window_action_for_test(
    controller: &Arc<crate::plugin_window::PluginWindowController>,
    service: &Arc<PublicPluginService>,
    identity: &crate::plugin_window::WindowActionIdentity,
) -> (
    super::ScheduledPluginRequest,
    std::sync::mpsc::Receiver<Result<Value, super::window_action::WindowActionError>>,
) {
    controller
        .admit_window_action(
            service,
            &identity.content_label,
            identity.session_generation,
            identity.document_generation,
            panel_action_input(),
        )
        .unwrap()
}

#[test]
fn window_action_admission_requires_content_exact_document_active_session_and_idle_runtime() {
    use super::window_action::WindowActionError as E;
    let (_root, service, manager, controller, i) = window_action_fixture("window-action-admit");
    for (label, session, document) in [
        ("main", i.session_generation, i.document_generation),
        (
            "plugin-shell-com.example.demo",
            i.session_generation,
            i.document_generation,
        ),
        (
            "plugin-panel-content-demo",
            i.session_generation,
            i.document_generation,
        ),
        (
            i.content_label.as_str(),
            i.session_generation + 1,
            i.document_generation,
        ),
        (
            i.content_label.as_str(),
            i.session_generation,
            i.document_generation + 1,
        ),
    ] {
        assert_eq!(
            controller
                .admit_window_action(&service, label, session, document, panel_action_input())
                .err(),
            Some(E::ExpiredWindowSessionError)
        );
    }
    let mut wrong = i.clone();
    wrong.owner.activation_id += 1;
    assert_eq!(
        manager
            .admit_window_action(&wrong, panel_action_input(), Instant::now())
            .err(),
        Some(E::ExpiredWindowSessionError)
    );
    let command = service
        .schedule_command(
            manager.route("/demo running").unwrap().unwrap(),
            2,
            "/demo running".into(),
            Instant::now(),
        )
        .unwrap();
    assert_eq!(
        controller
            .admit_window_action(
                &service,
                &i.content_label,
                i.session_generation,
                i.document_generation,
                panel_action_input()
            )
            .err(),
        Some(E::WindowRequestBusyError)
    );
    manager
        .scheduler()
        .complete(&command.dispatch.unwrap().context, Instant::now())
        .unwrap();
    let mut next = i.owner.clone();
    next.request_id = "next".into();
    let tx = controller.submit(next.clone()).unwrap();
    assert_eq!(
        tx.document_generation, i.document_generation,
        "submit reuses the same document"
    );
    assert!(tx.timer_session_generation > i.session_generation);
    assert_eq!(
        controller
            .admit_window_action(
                &service,
                &i.content_label,
                tx.timer_session_generation,
                tx.document_generation,
                panel_action_input()
            )
            .err(),
        Some(E::WindowRequestBusyError)
    );
}

#[test]
fn window_action_hide_retires_before_waiting_short_lease_and_keeps_physical_slot() {
    use super::window_action::WindowActionError as E;
    use super::{runtime::PluginApiOperation, PluginApiRequest, PluginContextStatus};
    let (_root, service, manager, controller, i) = window_action_fixture("window-action-hide");
    let (action, receiver) = admit_window_action_for_test(&controller, &service, &i);
    let lease = controller
        .begin_window_call(&i.content_label, i.session_generation, false)
        .unwrap();
    let closing = controller.clone();
    let id = i.owner.plugin_id.clone();
    let (closed, done) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        closed.send(closing.begin_timer_session_close(&id)).unwrap();
    });
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(2)),
        Ok(Err(E::ExpiredWindowSessionError))
    );
    assert!(
        matches!(done.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
        "only the short lease blocks hide"
    );
    assert_eq!(
        manager.scheduler().context_status(&action.context),
        PluginContextStatus::Expired
    );
    let label =
        super::runtime_label(&action.context.plugin_id, action.context.plugin_generation).unwrap();
    assert_eq!(
        manager
            .execute_api(
                &label,
                PluginApiRequest {
                    context: action.context.clone(),
                    operation: PluginApiOperation::StorageSet,
                    key: Some("late".into()),
                    value: Some(json!(true)),
                    notification: None
                }
            )
            .result,
        Err(PluginRuntimeError::ExpiredRequest)
    );
    drop(lease);
    assert!(done.recv_timeout(Duration::from_secs(1)).unwrap());
    thread.join().unwrap();
    assert!(
        controller.finish_timer_session_hide(&i.owner.plugin_id, false),
        "hide failure still revokes"
    );
    assert_eq!(
        controller
            .admit_window_action(
                &service,
                &i.content_label,
                i.session_generation,
                i.document_generation,
                panel_action_input()
            )
            .err(),
        Some(E::ExpiredWindowSessionError)
    );
    let next = service
        .schedule_command(
            manager.route("/demo next").unwrap().unwrap(),
            3,
            "/demo next".into(),
            Instant::now(),
        )
        .unwrap();
    assert!(next.dispatch.is_none());
    assert!(manager
        .expire_network_timeouts(action.deadline - Duration::from_millis(1))
        .unwrap()
        .is_empty());
    let outcome = manager
        .scheduler()
        .complete(&action.context, Instant::now())
        .unwrap();
    assert!(!outcome.accepted);
    assert_eq!(
        outcome.next.unwrap().candidate.owner.token(),
        Some(next.token.as_str())
    );
    assert!(manager
        .scheduler()
        .complete(&action.context, Instant::now())
        .is_err());
}

#[test]
fn window_action_completion_ticket_observes_credential_and_host_command_retirement() {
    use super::content_action::ContentActionError as E;
    use super::window_action::WindowActionError as W;
    for mode in ["credential", "command", "disable"] {
        let (_root, service, manager, controller, i) =
            window_action_fixture(&format!("window-action-ticket-{mode}"));
        let (action, receiver) = admit_window_action_for_test(&controller, &service, &i);
        assert!(
            manager
                .scheduler()
                .complete(&action.context, Instant::now())
                .unwrap()
                .accepted
        );
        if mode == "credential" {
            let mut effects = super::activation::ManagementEffects::default();
            manager
                .save_secret_with_effects(&i.owner.plugin_id, "synthetic", None, &mut effects)
                .unwrap();
            assert!(effects.retired.iter().any(|(c, _)| c == &action.context));
        } else if mode == "command" {
            service
                .schedule_command(
                    manager.route("/demo newer").unwrap().unwrap(),
                    3,
                    "/demo newer".into(),
                    Instant::now(),
                )
                .unwrap();
        } else {
            manager
                .set_enabled_with_readiness(&i.owner.plugin_id, false, |_| true)
                .unwrap();
        }
        service.settle_window_action(&action.context, Ok(json!("must not publish")), true);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Err(if mode == "command" {
                W::WindowRequestCancelledError
            } else {
                W::ExpiredWindowSessionError
            }))
        );
        assert!(!service.settle_window_action(&action.context, Ok(json!("duplicate")), true));
        assert_eq!(
            manager.claim_window_completion(&i, &action.context),
            Err(E::Expired)
        );
    }
}

#[test]
fn window_action_document_hooks_revoke_latest_reused_session_and_ignore_old_document() {
    use super::window_action::WindowActionError as E;
    let (_root, service, manager, controller, i) = window_action_fixture("window-action-doc");
    let (old, receiver) = admit_window_action_for_test(&controller, &service, &i);
    let mut owner = i.owner.clone();
    owner.request_id = "reused".into();
    let tx = controller.submit(owner.clone()).unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(E::ExpiredWindowSessionError))
    );
    manager
        .scheduler()
        .complete(&old.context, Instant::now())
        .unwrap();
    assert!(controller.advance(
        &owner,
        crate::plugin_window::PluginWindowPhase::AwaitingAck,
        crate::plugin_window::PluginWindowPhase::Visible
    ));
    assert!(controller.activate_timer_session(&owner));
    let reused = crate::plugin_window::WindowActionIdentity {
        owner: owner.clone(),
        content_label: tx.content_label.clone(),
        session_generation: tx.timer_session_generation,
        document_generation: tx.document_generation,
    };
    let (_, receiver) = admit_window_action_for_test(&controller, &service, &reused);
    assert!(controller.revoke_document(&owner.plugin_id, i.document_generation));
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(E::ExpiredWindowSessionError))
    );
    let replacement = controller.submit(owner.clone()).unwrap();
    assert_ne!(replacement.document_generation, i.document_generation);
    assert!(!controller.revoke_document(&owner.plugin_id, i.document_generation));
    let mut next_owner = owner.clone();
    next_owner.plugin_generation += 1;
    next_owner.activation_id += 1;
    controller.submit(next_owner.clone()).unwrap();
    assert!(!controller.invalidate_generation(&owner.plugin_id, owner.plugin_generation));
    assert_eq!(
        controller.owner_for_content(&tx.content_label),
        Some(next_owner)
    );
}

#[test]
fn window_action_uncertain_delivery_keeps_deadline_and_fixed_terminal_errors() {
    use super::content_action::ContentActionError as E;
    use super::window_action::WindowActionError as W;
    for (error, expected) in [
        (E::Unavailable, W::WindowRequestUnavailableError),
        (E::Timeout, W::WindowRequestTimeoutError),
    ] {
        let (_root, service, manager, controller, i) =
            window_action_fixture("window-action-terminal");
        let (action, receiver) = admit_window_action_for_test(&controller, &service, &i);
        manager.retire_panel_action(&action.context);
        assert!(service.settle_window_action(&action.context, Err(error), true));
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Err(expected))
        );
        assert!(manager
            .expire_network_timeouts(action.deadline - Duration::from_millis(1))
            .unwrap()
            .is_empty());
        let expired = manager.expire_network_timeouts(action.deadline).unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].expired, action.context);
    }
}

#[test]
fn window_action_completion_taken_before_lifecycle_scan_still_expires() {
    use super::content_action::ContentActionError as E;
    let (_root, service, manager, controller, i) =
        window_action_fixture("window-action-taken-ticket");
    let (action, receiver) = admit_window_action_for_test(&controller, &service, &i);
    manager
        .scheduler()
        .complete(&action.context, Instant::now())
        .unwrap();
    let (taken, wait_taken) = std::sync::mpsc::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let completion_service = service.clone();
    let context = action.context.clone();
    let completing = std::thread::spawn(move || {
        assert!(completion_service.settle_content_action_with(
            &context,
            Ok(json!("private")),
            true,
            || {
                taken.send(()).unwrap();
                wait_release.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        ));
    });
    wait_taken.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut effects = super::activation::ManagementEffects::default();
    manager
        .save_secret_with_effects(&i.owner.plugin_id, "synthetic", None, &mut effects)
        .unwrap();
    service.settle_management_effects(&effects);
    release.send(()).unwrap();
    completing.join().unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Err(
            super::window_action::WindowActionError::ExpiredWindowSessionError
        ))
    );
    assert_eq!(
        manager.claim_window_completion(&i, &action.context),
        Err(E::Expired)
    );
}

#[test]
fn window_action_every_controller_retirement_path_settles_once() {
    for path in [
        "remove",
        "uninstall",
        "generation",
        "session",
        "document",
        "submit",
        "shutdown",
    ] {
        let (_root, service, manager, controller, i) =
            window_action_fixture(&format!("window-action-removal-{path}"));
        let (action, receiver) = admit_window_action_for_test(&controller, &service, &i);
        match path {
            "remove" => {
                assert!(controller.remove_plugin(&i.owner.plugin_id));
            }
            "uninstall" => {
                assert!(controller.close_for_uninstall(&i.owner.plugin_id));
            }
            "generation" => {
                assert!(
                    controller.invalidate_generation(&i.owner.plugin_id, i.owner.plugin_generation)
                );
            }
            "session" => {
                assert!(controller.revoke_timer_session(&i.owner.plugin_id, i.session_generation));
            }
            "document" => {
                assert!(controller.revoke_document(&i.owner.plugin_id, i.document_generation));
            }
            "submit" => {
                controller.submit(i.owner.clone()).unwrap();
            }
            _ => service.shutdown(),
        }
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Err(
                super::window_action::WindowActionError::ExpiredWindowSessionError
            )),
            "{path}"
        );
        assert_eq!(
            manager.scheduler().context_status(&action.context),
            super::PluginContextStatus::Expired
        );
        assert!(
            !manager
                .scheduler()
                .complete(&action.context, Instant::now())
                .unwrap()
                .accepted
        );
        assert!(!service.settle_window_action(&action.context, Ok(json!("late")), true));
        assert!(receiver.try_recv().is_err());
    }
}

fn result_action_service(
    root: &TestRoot,
) -> (
    Arc<PublicPluginService>,
    Arc<PublicPluginManager>,
    crate::result_registry::ResultRegistry,
    super::result_action::ResultActionOrigin,
) {
    use super::result_action::*;
    use crate::result_registry::{QueryDomain, ResultRegistry};
    let source = root.package();
    let mut value = manifest("mainResult");
    value["minimumHostVersion"] = json!("0.10.0");
    value["runtime"]["resultActions"] = json!(true);
    value["permissions"] = json!([]);
    value["settings"] = json!([{"key":"flag","label":"Flag","type":"boolean","default":false}]);
    write_package(&source, &value);
    let app_data = root.0.join("app-data");
    let manager = Arc::new(
        PublicPluginManager::load(
            &app_data,
            PublicPluginHost::current(PublicPlatform::Windows),
            [],
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
    let route = manager.route("/demo saved input").unwrap().unwrap();
    let registry = ResultRegistry::default();
    registry.on_show("source".into());
    let token = registry
        .begin_query(QueryDomain::Plugin, "source", 1)
        .unwrap();
    let rows=parse(&super::PluginRequestContext{plugin_id:route.plugin_id.clone(),plugin_generation:route.generation,request_id:"public-request-0000000000000001".into()},json!({"requestId":"public-request-0000000000000001","results":[{"id":"same","title":"Save","actions":[{"id":"save","title":"Save","type":"runtime","payload":null}]}]})).unwrap();
    let response = manager
        .publish_result_actions(&route, false.into(), || {
            registry.publish_prepared_result_actions(
                token,
                registry
                    .prepare_result_actions(route.clone(), rows)
                    .unwrap(),
            )
        })
        .unwrap()
        .unwrap();
    let origin = registry
        .resolve_result_action(&ExecuteResultActionRequest {
            request_id: response.request_id,
            result_id: response.items[0].result_id.clone(),
            action_id: response.items[0].result_actions.as_ref().unwrap().actions[0]
                .action_id
                .clone(),
        })
        .unwrap();
    let service = Arc::new(PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    (service, manager, registry, origin)
}
#[test]
fn result_action_observer_precommit_failure_leaves_no_pending_claim_or_physical_request() {
    use super::result_action::*;
    let root = TestRoot::new("result-action-precommit");
    let (service, manager, registry, origin) = result_action_service(&root);
    let legacy = Arc::new(crate::plugins::PluginManager::new());
    assert!(matches!(
        service.prepare_result_action_with_observer(
            legacy.clone(),
            registry.clone(),
            origin.clone(),
            || Err(ResultActionErrorCode::ActionUnavailable)
        ),
        Err(ResultActionErrorCode::ActionUnavailable)
    ));
    assert!(service.result_action.lock().unwrap().is_none());
    let (arm, watch) = std::sync::mpsc::channel();
    let (request, _receiver) = service
        .prepare_result_action_with_observer(legacy, registry.clone(), origin.clone(), || Ok(arm))
        .unwrap();
    assert_eq!(watch.recv().unwrap(), request.deadline);
    assert_eq!(
        manager.scheduler().context_status(&request.context),
        super::PluginContextStatus::Current
    );
    assert!(matches!(
        registry.claim_result_action(&origin, None),
        Err(ResultActionErrorCode::ActionInProgress)
    ));
    service.settle_result_action(&request.context, ResultActionErrorCode::ActionCancelled);
}
#[test]
fn result_action_authenticated_malformed_completion_settles_once_and_forgery_selects_nothing() {
    use super::result_action::*;
    for case in ["valid", "malformed", "throw"] {
        let root = TestRoot::new("result-action-complete");
        let (service, manager, registry, origin) = result_action_service(&root);
        let (arm, _watch) = std::sync::mpsc::channel();
        let (request, receiver) = service
            .prepare_result_action_with_observer(
                Arc::new(crate::plugins::PluginManager::new()),
                registry.clone(),
                origin,
                || Ok(arm),
            )
            .unwrap();
        let mut forged = PluginCommandCompletion {
            context: request.context.clone(),
            failed: false,
            response: Some(json!({"requestId":request.context.request_id,"results":[]})),
        };
        forged.context.request_id = "public-request-ffffffffffffffff".into();
        assert!(manager
            .complete(&request.candidate.plugin_id, &forged, Instant::now())
            .is_err());
        assert!(service.complete_result_action(&forged, true).is_none());
        assert!(service.result_action.lock().unwrap().is_some());
        let completion = PluginCommandCompletion {
            context: request.context.clone(),
            failed: case == "throw",
            response: Some(if case == "malformed" {
                json!({"requestId":request.context.request_id,"data":{}})
            } else {
                json!({"requestId":request.context.request_id,"results":[]})
            }),
        };
        let accepted = manager
            .complete(
                &super::runtime_label(
                    &request.context.plugin_id,
                    request.context.plugin_generation,
                )
                .unwrap(),
                &completion,
                Instant::now(),
            )
            .unwrap()
            .accepted;
        assert!(accepted);
        assert_eq!(
            manager.scheduler().context_status(&request.context),
            super::PluginContextStatus::Expired
        );
        let (_, success, fault) = service
            .complete_result_action(&completion, accepted)
            .unwrap();
        assert_eq!(success, case == "valid");
        assert_eq!(fault, case != "valid");
        let outcome = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        match (case, outcome) {
            ("valid", ResultActionOutcome::ResultsUpdated { response }) => {
                assert!(response.items.is_empty())
            }
            (
                "malformed",
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::ActionInvalidResponse,
                    source_consumed: true,
                },
            ) => {}
            (
                "throw",
                ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::ActionFailed,
                    source_consumed: true,
                },
            ) => {}
            _ => panic!("wrong result action outcome"),
        };
        assert!(
            !service.settle_result_action(&request.context, ResultActionErrorCode::ActionTimedOut)
        );
        assert!(service.complete_result_action(&completion, true).is_none());
    }
}
#[test]
fn result_action_uncertain_transport_keeps_slot_and_original_autonomous_deadline_after_settlement()
{
    use super::result_action::*;
    use std::{sync::mpsc, thread};
    let root = TestRoot::new("result-action-transport-deadline");
    let (service, manager, registry, origin) = result_action_service(&root);
    let (clock, advance) = mpsc::channel();
    let (done, observed) = mpsc::channel();
    let (worker_done, joined) = mpsc::channel();
    let manager_for_watch = manager.clone();
    let (request, receiver) = service
        .prepare_result_action_with_observer(
            Arc::new(crate::plugins::PluginManager::new()),
            registry,
            origin,
            move || {
                let (arm, watch) = mpsc::channel();
                thread::spawn(move || {
                    let deadline = watch.recv().unwrap();
                    advance.recv().unwrap();
                    let expired = manager_for_watch.expire_network_timeouts(deadline).unwrap();
                    done.send(expired).unwrap();
                    worker_done.send(()).unwrap();
                });
                Ok(arm)
            },
        )
        .unwrap();
    manager.retire_panel_action(&request.context);
    assert!(
        service.settle_result_action(&request.context, ResultActionErrorCode::ActionUnavailable)
    );
    assert!(matches!(
        receiver.recv().unwrap(),
        ResultActionOutcome::Failed {
            code: ResultActionErrorCode::ActionUnavailable,
            source_consumed: true
        }
    ));
    assert!(!service.settle_result_action(&request.context, ResultActionErrorCode::ActionTimedOut));
    clock.send(()).unwrap();
    let expired = observed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].expired, request.context);
    assert_eq!(
        expired[0].new_generation,
        request.context.plugin_generation + 1
    );
    joined.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(manager
        .expire_network_timeouts(request.deadline + Duration::from_secs(1))
        .unwrap()
        .is_empty());
}

#[test]
fn result_action_timeout_promotes_waiting_command_with_fresh_publication_owner() {
    use std::sync::mpsc;
    let root = TestRoot::new("result-action-timeout-waiter");
    let (service, manager, registry, origin) = result_action_service(&root);
    let (arm, _watch) = mpsc::channel();
    let (action, _receiver) = service
        .prepare_result_action_with_observer(
            Arc::new(crate::plugins::PluginManager::new()),
            registry,
            origin,
            || Ok(arm),
        )
        .unwrap();
    let submission = service
        .schedule_command(
            manager.route("/demo next input").unwrap().unwrap(),
            2,
            "/demo next input".into(),
            Instant::now(),
        )
        .unwrap();
    assert!(submission.dispatch.is_none());
    let expired = manager.expire_network_timeouts(action.deadline).unwrap();
    assert_eq!(expired.len(), 1);
    let replacement = manager
        .replace_runtime_generation(
            &action.context.plugin_id,
            action.context.plugin_generation,
            expired[0].new_generation,
        )
        .unwrap();
    let next = manager
        .finish_runtime_replacement(
            &replacement.plugin_id,
            replacement.generation,
            replacement.activation_id,
            replacement.admission_epoch,
            Instant::now(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.candidate.input, "next input");
    service
        .bind_request(&submission.token, &next.context)
        .unwrap();
    let pending_route = service
        .lock_submissions()
        .unwrap()
        .by_token
        .get(&submission.token)
        .unwrap()
        .route
        .clone();
    assert!(
        manager
            .publish_result_actions(&pending_route, false.into(), || ())
            .is_ok(),
        "waiting command must use its authenticated replacement owner"
    );
    let completion = PluginCommandCompletion {
        context: next.context.clone(),
        failed: false,
        response: Some(
            json!({"requestId":next.context.request_id,"results":[{"id":"same","title":"Fresh","actions":[]}]}),
        ),
    };
    let outcome = manager
        .complete(
            &super::runtime_label(&next.context.plugin_id, next.context.plugin_generation).unwrap(),
            &completion,
            Instant::now(),
        )
        .unwrap();
    service
        .complete_submission_with(&completion, outcome, Instant::now(), || {
            panic!("not a fault")
        })
        .unwrap();
    let Some(Ok(PublicPluginResponse::MainActionResults { route, results })) = submission
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("fresh result response")
    };
    assert_eq!(route.generation, replacement.generation);
    assert_eq!(route.input, "next input");
    let registry = crate::result_registry::ResultRegistry::default();
    registry.on_show("next".into());
    let token = registry
        .begin_query(crate::result_registry::QueryDomain::Plugin, "next", 2)
        .unwrap();
    assert!(manager
        .publish_result_actions(&route, false.into(), || registry
            .publish_prepared_result_actions(
                token,
                registry
                    .prepare_result_actions(route.clone(), results)
                    .unwrap()
            ))
        .unwrap()
        .is_some());
}

#[test]
fn result_action_same_generation_stale_prepared_commands_never_rebind_or_emit() {
    use std::collections::BTreeMap;
    for setting in [false, true] {
        let root = TestRoot::new("result-action-stale-prepared");
        let (service, manager, _registry, _origin) = result_action_service(&root);
        let prepared = service
            .prepare_command(
                manager.route("/demo old").unwrap().unwrap(),
                2,
                "/demo old".into(),
            )
            .unwrap();
        let old_context = prepared.request_context().clone();
        if setting {
            manager
                .save_settings(
                    "com.uipilot.demo",
                    &BTreeMap::from([("flag".into(), json!(true))]),
                )
                .unwrap();
        } else {
            manager.rename("com.uipilot.demo", Some("renamed")).unwrap();
        }
        let rejected = service
            .admit_prepared_command(prepared, Instant::now())
            .unwrap();
        assert!(rejected.dispatch.is_none());
        assert!(matches!(
            rejected.receiver.recv().unwrap(),
            Some(Err(PluginRuntimeError::Unavailable))
        ));
        assert_eq!(
            manager.scheduler().context_status(&old_context),
            super::PluginContextStatus::Expired
        );
    }
}
#[test]
fn result_action_bound_route_rejects_mismatched_context_token_command_or_input() {
    let root = TestRoot::new("result-action-bound-proof");
    let (service, manager, _registry, _origin) = result_action_service(&root);
    let saved = manager.route("/demo exact input").unwrap().unwrap();
    let submission = service
        .schedule_command(saved.clone(), 2, "/demo exact input".into(), Instant::now())
        .unwrap();
    let request = submission.dispatch.unwrap();
    assert!(manager
        .bind_result_command_route(&saved, &request.context, "wrong-token")
        .is_err());
    let mut forged = request.context.clone();
    forged.request_id = "public-request-ffffffffffffffff".into();
    assert!(manager
        .bind_result_command_route(&saved, &forged, &submission.token)
        .is_err());
    let mut altered = saved.clone();
    altered.command_id = "other".into();
    assert!(manager
        .bind_result_command_route(&altered, &request.context, &submission.token)
        .is_err());
    altered = saved.clone();
    altered.input = "different".into();
    assert!(manager
        .bind_result_command_route(&altered, &request.context, &submission.token)
        .is_err());
    manager
        .set_command_favorite("com.uipilot.demo", "default", true)
        .unwrap();
    manager.rename("com.uipilot.demo", None).unwrap();
    assert!(manager
        .bind_result_command_route(&saved, &request.context, &submission.token)
        .is_ok());
    manager.rename("com.uipilot.demo", Some("renamed")).unwrap();
    assert!(manager
        .bind_result_command_route(&saved, &request.context, &submission.token)
        .is_err());
}
#[test]
fn result_action_rejected_unsent_bind_preserves_and_returns_the_exact_latest_successor() {
    let root = TestRoot::new("result-action-unsent-successor");
    let (service, manager, _registry, _origin) = result_action_service(&root);
    let prepared = service
        .prepare_command(
            manager.route("/demo old").unwrap().unwrap(),
            2,
            "/demo old".into(),
        )
        .unwrap();
    manager.rename("com.uipilot.demo", Some("renamed")).unwrap();
    let mut successor = None;
    let rejected = service
        .admit_prepared_command_with(prepared, Instant::now(), || {
            successor = Some(
                service
                    .schedule_command(
                        manager.route("/renamed new").unwrap().unwrap(),
                        3,
                        "/renamed new".into(),
                        Instant::now(),
                    )
                    .unwrap(),
            );
        })
        .unwrap();
    let successor = successor.unwrap();
    assert!(successor.dispatch.is_none());
    let dispatch = rejected.dispatch.unwrap();
    assert_eq!(
        dispatch.candidate.owner.token(),
        Some(successor.token.as_str())
    );
    assert_eq!(dispatch.candidate.input, "new");
    assert!(matches!(
        rejected.receiver.recv().unwrap(),
        Some(Err(PluginRuntimeError::Unavailable))
    ));
    assert_eq!(
        manager.scheduler().context_status(&dispatch.context),
        super::PluginContextStatus::Current
    );
}

#[test]
fn result_action_review_current_timeout_faults_but_retired_timeout_does_not() {
    use std::sync::mpsc;
    for retirement in ["current", "source", "logical"] {
        let root = TestRoot::new("result-action-review-timeout-fault");
        let (service, manager, registry, origin) = result_action_service(&root);
        let source = origin.ticket.request_id().to_owned();
        let (arm, _watch) = mpsc::channel();
        let (action, _receiver) = service
            .prepare_result_action_with_observer(
                Arc::new(crate::plugins::PluginManager::new()),
                registry.clone(),
                origin,
                || Ok(arm),
            )
            .unwrap();
        match retirement {
            "source" => {
                registry.retire_result_actions(&source);
            }
            "logical" => manager.retire_panel_action(&action.context),
            _ => {}
        }
        let replacement = manager.expire_network_timeouts(action.deadline).unwrap();
        assert_eq!(replacement.len(), 1);
        assert_eq!(
            replacement[0].counts_as_fault,
            retirement == "current",
            "{retirement}"
        );
    }
}
#[test]
fn result_action_review_management_invalidation_settles_immediately_even_on_partial_error() {
    use super::result_action::*;
    use std::sync::mpsc;
    for partial_error in [false, true] {
        let root = TestRoot::new("result-action-review-management-cleanup");
        let (service, manager, registry, origin) = result_action_service(&root);
        let legacy = Arc::new(crate::plugins::PluginManager::new());
        let (arm, _watch) = mpsc::channel();
        let (action, receiver) = service
            .prepare_result_action_with_observer(legacy.clone(), registry.clone(), origin, || {
                Ok(arm)
            })
            .unwrap();
        let result = crate::commands::finish_public_plugin_management(
            &service,
            super::ManagementOutcome {
                result: if partial_error {
                    Err(super::PublicPluginManagementError::Unavailable)
                } else {
                    Ok(())
                },
                changed: true,
            },
            &registry,
            &legacy,
            || {},
        );
        assert_eq!(result.is_err(), partial_error);
        assert!(
            matches!(
                receiver.try_recv(),
                Ok(ResultActionOutcome::Failed {
                    code: ResultActionErrorCode::ActionCancelled,
                    source_consumed: true
                })
            ),
            "invalidation must settle the pending caller before returning"
        );
        assert!(
            !service.settle_result_action(&action.context, ResultActionErrorCode::ActionTimedOut)
        );
        assert_eq!(
            manager.scheduler().context_status(&action.context),
            super::PluginContextStatus::Expired
        );
        let next = service
            .schedule_command(
                manager.route("/demo next").unwrap().unwrap(),
                2,
                "/demo next".into(),
                Instant::now(),
            )
            .unwrap();
        assert!(
            next.dispatch.is_none(),
            "retirement must retain physical JS occupancy"
        );
    }
}

pub(super) fn with_running_result_action_for_test(
    legacy: Arc<crate::plugins::PluginManager>,
    operation: impl FnOnce(
        &PublicPluginService,
        &PublicPluginManager,
        &crate::result_registry::ResultRegistry,
        &super::ScheduledPluginRequest,
        &std::sync::mpsc::Receiver<super::result_action::ResultActionOutcome>,
    ),
) {
    let root = TestRoot::new("result-action-review-legacy");
    let (service, manager, registry, origin) = result_action_service(&root);
    let (arm, _watch) = std::sync::mpsc::channel();
    let (action, receiver) = service
        .prepare_result_action_with_observer(legacy, registry.clone(), origin, || Ok(arm))
        .unwrap();
    operation(&service, &manager, &registry, &action, &receiver);
}

#[test]
fn file_open_manager_requires_declarations_grants_and_current_identity() {
    let (_root, _service, m, _c, i) = window_action_fixture_options("file-open-authority", true);
    assert!(m.files_open_enabled(&i.owner));
    for field in 0..3 {
        let mut owner = i.owner.clone();
        match field {
            0 => owner.activation_id += 1,
            1 => owner.admission_epoch += 1,
            _ => owner.plugin_generation += 1,
        };
        assert!(!m.files_open_enabled(&owner));
    }
    let (_r, _s, legacy, _c, j) = window_action_fixture("file-open-legacy");
    assert!(!legacy.files_open_enabled(&j.owner));
    m.set_enabled_with_readiness("com.uipilot.demo", false, |_| true)
        .unwrap();
    assert!(!m.files_open_enabled(&i.owner));
}

#[test]
fn current_host_matches_compiled_release_version() {
    let current = PublicPluginHost::current(PublicPlatform::Windows);
    let version = format!(
        "{}.{}.{}",
        current.version[0], current.version[1], current.version[2]
    );
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    let tauri: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    let frontend: Value = serde_json::from_str(include_str!("../../../package.json")).unwrap();
    assert_eq!(tauri["version"], version);
    assert_eq!(frontend["version"], version);
}

#[test]
fn current_host_accepts_translation_package_directory_and_archive() {
    let root = TestRoot::new("current-host-translation");
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("examples/public-plugins/com.uipilot.translate/package");
    let archive = root.0.join("translation.uipilot-plugin");
    archive_directory(&package, &archive);
    let mut sources = vec![
        PublicPackageSource::DevelopmentDirectory(package),
        PublicPackageSource::Archive(archive),
    ];
    // Release verification can include the exact archive delivered to the user.
    if let Some(archive) = std::env::var_os("UIPILOT_TEST_TRANSLATION_ARCHIVE") {
        sources.push(PublicPackageSource::Archive(archive.into()));
    }
    for source in sources {
        let prepared = stage_public_package(source, &root.staging(), &host()).unwrap();
        assert_eq!(prepared.manifest.plugin_id, "com.uipilot.translate");
        assert_eq!(
            prepared.manifest.legacy_command().unwrap().output_mode,
            PublicOutputMode::Panel
        );
        assert_eq!(prepared.revalidate(), Ok(()));
    }
}

#[test]
fn current_host_still_rejects_packages_requiring_a_future_release() {
    let root = TestRoot::new("current-host-future");
    let package = root.package();
    let mut value = manifest("panel");
    value["minimumHostVersion"] = json!(format!("{}.0.0", host().version[0] + 1));
    write_package(&package, &value);
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(package),
        &root,
        PublicPackageError::IncompatibleApi,
    );
}


pub(super) fn with_panel_service_for_test(operation: impl FnOnce(&PublicPluginService, &PublicPluginManager, &crate::plugin_panel::PluginPanelController)) {
    let (_root, service, manager, controller, _) = panel_action_fixture("share-submit-admission");
    operation(&service, &manager, &controller);
}

#[test]
fn sharing_fix_panel_action_dispatch_never_reaches_unrelated_any_listener() {
    use tauri::Listener;
    let (_root, service, _manager, controller, identity) = panel_action_fixture("private-panel-dispatch");
    let mut input = panel_action_input();
    input.payload = json!({"text":" edited\n原文 ", "escape":"</script>\u{2028}\u{2029}\"\\"});
    let (action, _receiver) = controller.admit_panel_action(&service, &identity.content_label, identity.session_epoch, input).unwrap();
    let app = tauri::test::mock_app();
    let label = super::runtime_label(&action.context.plugin_id, action.context.plugin_generation).unwrap();
    tauri::WebviewWindowBuilder::new(&app, &label, Default::default()).build().unwrap();
    let unrelated = tauri::WebviewWindowBuilder::new(&app, "plugin-runtime-unrelated", Default::default()).build().unwrap();
    let leaks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = leaks.clone();
    unrelated.listen_any("uipilot-public-plugin-command", move |_| { observed.fetch_add(1, Ordering::SeqCst); });
    service.dispatch_to_runtime(app.handle(), &action, super::PluginInvocationTheme::Dark, "2026-11-06T00:00:00Z".into()).unwrap();
    assert_eq!(leaks.load(Ordering::SeqCst), 0, "PanelAction original escaped through the event bus");
    // MockRuntime does not run eval. Execute the same production serializer and
    // generated bootstrap in Node VM to verify both sides of the private wire.
    let (kind, invocation) = service.dispatch_invocation(&action, super::PluginInvocationTheme::Dark, "2026-11-06T00:00:00Z".into()).unwrap();
    let expected = invocation["payload"].clone();
    let script = super::runtime::public_runtime_dispatch(&super::PluginCommandDispatch {
        context: action.context.clone(), kind: kind.into(), invocation,
    }).unwrap();
    let bootstrap = super::runtime::public_runtime_bootstrap_with_panel(false, true);
    let mut child = std::process::Command::new("node")
        .args(["-e", r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
let input = ''; process.stdin.setEncoding('utf8'); process.stdin.on('data', s => input += s);
process.stdin.on('end', async () => {
  try {
    const { bootstrap, script, expected } = JSON.parse(input);
    const calls = [], completions = [];
    const window = { __TAURI_INTERNALS__: { invoke: async (command, args) => {
      assert.equal(command, 'complete_plugin_command'); completions.push(args);
    } }, testModule: { onCommand() { throw Error('wrong handler'); }, async onPanelAction(next, api) {
      assert(Object.isFrozen(next.payload)); calls.push(next); return { requestId: next.requestId, data: { ok: true } };
    } } };
    const document = { title: '', documentElement: { dataset: { runtimeEntry: '/runtime.js' } } };
    const context = vm.createContext({ window, document, TextEncoder, setTimeout, Object, Reflect, Error, TypeError, Promise, WeakSet, Set });
    vm.runInContext(bootstrap.replace('const module = entry ? await import(entry) : null;', 'const module = window.testModule;'), context);
    await new Promise(setImmediate);
    assert.equal(document.title, 'uipilot-public-plugin-ready');
    await vm.runInContext(script, context);
    assert.equal(calls.length, 1);
    assert.deepEqual(JSON.parse(JSON.stringify(calls[0].payload)), expected);
    assert.equal(completions.length, 1);
    assert.equal(completions[0].completion.response.data.ok, true);
    console.log('production private dispatch script + generated bootstrap: 1 PanelAction, exact payload, 1 completion');
  } catch (error) { console.error(error); process.exitCode = 1; }
});
"#])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), &json!({"bootstrap":bootstrap,"script":script,"expected":expected})).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
