fn baidu_panel_fixture(
    dir: &TestDir,
    gate: Arc<PluginNetworkAuthorityGate>,
) -> (
    Arc<PublicPluginManager>,
    Arc<crate::public_plugins::PublicPluginService>,
    Arc<crate::plugin_panel::PluginPanelController>,
    crate::plugin_panel::PanelSessionIdentity,
    Arc<crate::credentials::CredentialStore>,
    ScheduledPluginRequest,
    std::sync::mpsc::Receiver<Result<Value, crate::public_plugins::panel_action::PanelActionError>>,
    String,
) {
    write_baidu_ocr_package(dir);
    let path = dir.source().join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["minimumHostVersion"] = json!("0.21.0");
    manifest["command"]["outputMode"] = json!("panel");
    manifest["panel"] = json!({"entry":"dist/panel.html", "runtimeActions":true});
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("ui.panel"));
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(dir.source().join("dist/panel.html"), "<html></html>").unwrap();
    let manager = Arc::new(manager_with_network_authority(dir, gate));
    let now = Instant::now();
    let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
    manager
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([
                PublicPermission::NetworkHttps,
                PublicPermission::ClipboardWrite,
                PublicPermission::UiPanel,
            ]),
            now,
            |_| true,
        )
        .unwrap();
    manager
        .set_permission_access_with_effects(
            "com.example.network",
            PublicPermission::CredentialsBaiduOcr,
            true,
            &manager.state.inventory_revision().unwrap().to_string(),
            &mut ManagementEffects::default(),
        )
        .unwrap();
    let service = Arc::new(crate::public_plugins::PublicPluginService::default());
    assert!(service.manager.set(manager.clone()).is_ok());
    let controller = Arc::new(crate::plugin_panel::PluginPanelController::default());
    let route = manager
        .panel_route("com.example.network", "")
        .unwrap()
        .unwrap();
    let identity = controller
        .open_session(crate::plugin_panel::PanelOwner {
            plugin_id: route.plugin_id.clone(),
            plugin_generation: route.generation,
            activation_id: route.activation_id,
            admission_epoch: route.admission_epoch,
            command_label: route.command_label.clone(),
            request_id: "initial".into(),
            submission_token: "initial".into(),
            argument: String::new(),
            host_keys: vec![],
            host_key_focus: super::super::PanelHostKeyFocus::Content,
        })
        .unwrap();
    controller
        .configure_images(identity.session_epoch, None)
        .unwrap();
    assert!(crate::plugin_panel::content_ready(
        &controller,
        &identity.content_label,
        identity.session_epoch
    ));
    // The real command prepare/bind/admit/dispatch path can initialize an empty image Panel.
    let prepared = service.prepare_command(route, 1, String::new()).unwrap();
    let initial = service.admit_prepared_command(prepared, now).unwrap();
    let dispatch = initial.dispatch.as_ref().unwrap();
    let (_, invocation) = service
        .dispatch_invocation(
            dispatch,
            super::super::PluginInvocationTheme::Dark,
            String::new(),
        )
        .unwrap();
    assert!(invocation.get("image").is_none());
    let completion = super::super::PluginCommandCompletion {
        context: dispatch.context.clone(),
        failed: false,
        response: Some(json!({"requestId":dispatch.context.request_id,"data":{}})),
    };
    let outcome = manager.scheduler.complete(&dispatch.context, now).unwrap();
    service
        .complete_submission_with(&completion, outcome, now, || panic!("empty panel fault"))
        .unwrap();
    assert!(initial
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .is_ok());
    manager.launcher_images.begin_import(1).unwrap();
    let original = manager
        .launcher_images
        .finish_import(
            1,
            crate::launcher_image::normalize_rgba(1, 1, &[255, 0, 0, 255]).unwrap(),
        )
        .unwrap();
    controller
        .begin_image_import(&manager, "main", identity.session_epoch, 1)
        .unwrap();
    let image = controller
        .finish_image_import(&manager, "main", identity.session_epoch, 1, original)
        .unwrap();
    manager.launcher_images.clear(2).unwrap();
    let request = super::super::panel_action::PanelActionRequest::from_value(
        json!({"action":"recognize","imageId":image.id,"payload":{"providerId":"baidu"}}),
    )
    .unwrap();
    let (dispatch, receiver) = controller
        .admit_panel_action(
            &service,
            &identity.content_label,
            identity.session_epoch,
            request,
        )
        .unwrap();
    let (_, invocation) = service
        .dispatch_invocation(
            &dispatch,
            super::super::PluginInvocationTheme::Dark,
            String::new(),
        )
        .unwrap();
    let id = invocation["image"]["id"].as_str().unwrap().to_owned();
    assert_ne!(id, image.id);
    let store = Arc::new(crate::credentials::CredentialStore::new(
        dir.path().join("synthetic-panel-baidu"),
    ));
    store.synthetic_baidu_for_test();
    (
        manager, service, controller, identity, store, dispatch, receiver, id,
    )
}

#[tokio::test]
async fn panel_image_real_service_path_preserves_success_after_launcher_clear() {
    let dir = TestDir::new("baidu-panel-success");
    let (gate, requests) = PluginNetworkAuthorityGate::baidu_responses_for_test(vec![
        (
            200,
            br#"{"access_token":"synthetic-token","scope":"brain_ocr","expires_in":3600}"#.to_vec(),
        ),
        (
            200,
            br#"{"words_result":[{"words":"first"},{"words":"second"}]}"#.to_vec(),
        ),
    ]);
    let (manager, service, _controller, _identity, store, dispatch, receiver, id) =
        baidu_panel_fixture(&dir, gate);
    let context = &dispatch.context;
    let response = manager
        .baidu_ocr(
            PluginRuntimeIdentity {
                plugin_id: context.plugin_id.clone(),
                generation: context.plugin_generation,
            },
            baidu_input(context, &id),
            &store,
        )
        .await
        .unwrap();
    assert_eq!(response.text, "first\nsecond");
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert!(
        manager
            .scheduler
            .complete(context, Instant::now())
            .unwrap()
            .accepted
    );
    let data = super::super::panel_action::response_data(&context.request_id,json!({"requestId":context.request_id,"data":{"ok":true,"text":response.text,"lines":response.lines}})).unwrap();
    assert!(service.settle_panel_action(context, Ok(data), true));
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()["text"],
        "first\nsecond"
    );
}

#[tokio::test]
async fn panel_image_late_authentication_and_ocr_bodies_cannot_restore_authority() {
    for phase in [1, 2] {
        let dir = TestDir::new(&format!("baidu-panel-late-{phase}"));
        // This fixed fake body satisfies both service responses, allowing the same
        // cancellation-ignoring transport to pause at either boundary.
        let (gate, barrier) = PluginNetworkAuthorityGate::paused_body_for_test(200,
            br#"{"access_token":"synthetic-token","scope":"brain_ocr","expires_in":3600,"words_result":[{"words":"stale"}]}"#.to_vec());
        let (manager, service, controller, identity, store, dispatch, receiver, id) =
            baidu_panel_fixture(&dir, gate);
        let task = tokio::spawn({
            let manager = manager.clone();
            let store = store.clone();
            let context = dispatch.context.clone();
            async move {
                manager
                    .baidu_ocr(
                        PluginRuntimeIdentity {
                            plugin_id: context.plugin_id.clone(),
                            generation: context.plugin_generation,
                        },
                        baidu_input(&context, &id),
                        &store,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), barrier.reached.notified())
            .await
            .unwrap();
        if phase == 2 {
            barrier.release.notify_one();
            tokio::time::timeout(Duration::from_secs(2), barrier.reached.notified())
                .await
                .unwrap();
        }
        controller
            .clear_image(&manager, &identity.content_label, identity.session_epoch)
            .unwrap();
        barrier.release.notify_one();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            "expiredRequest"
        );
        assert_eq!(
            barrier.sends.load(std::sync::atomic::Ordering::SeqCst),
            phase
        );
        assert!(
            !manager
                .scheduler
                .complete(&dispatch.context, Instant::now())
                .unwrap()
                .accepted
        );
        service.settle_panel_action(&dispatch.context, Ok(json!({"text":"stale"})), true);
        assert!(receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_err());
    }
}

#[tokio::test]
async fn panel_image_replacement_clear_hide_permission_and_credentials_cancel_pending_service() {
    for mode in ["replace", "clear", "hide", "permission", "credentials"] {
        let dir = TestDir::new(&format!("baidu-panel-{mode}"));
        let (gate, started) = PluginNetworkAuthorityGate::blocking_for_test();
        let (manager, _service, controller, identity, store, dispatch, _receiver, id) =
            baidu_panel_fixture(&dir, gate);
        let task = tokio::spawn({
            let manager = manager.clone();
            let store = store.clone();
            let context = dispatch.context.clone();
            async move {
                manager
                    .baidu_ocr(
                        PluginRuntimeIdentity {
                            plugin_id: context.plugin_id.clone(),
                            generation: context.plugin_generation,
                        },
                        baidu_input(&context, &id),
                        &store,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        match mode {
            "replace" => {
                controller
                    .begin_image_import(&manager, "main", identity.session_epoch, 2)
                    .unwrap();
                controller
                    .finish_image_import(
                        &manager,
                        "main",
                        identity.session_epoch,
                        2,
                        crate::launcher_image::normalize_rgba(1, 1, &[0, 255, 0, 255]).unwrap(),
                    )
                    .unwrap();
            }
            "clear" => controller
                .clear_image(&manager, &identity.content_label, identity.session_epoch)
                .unwrap(),
            "hide" => {
                controller.host_hidden();
            }
            "permission" => {
                manager
                    .set_permission_access_with_effects(
                        &identity.plugin_id,
                        PublicPermission::CredentialsBaiduOcr,
                        false,
                        &manager.state.inventory_revision().unwrap().to_string(),
                        &mut ManagementEffects::default(),
                    )
                    .unwrap();
            }
            _ => store.disable_baidu_for_test(),
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            "expiredRequest",
            "{mode}"
        );
        assert!(
            !manager
                .scheduler
                .complete(&dispatch.context, Instant::now())
                .unwrap()
                .accepted
        );
    }
}
