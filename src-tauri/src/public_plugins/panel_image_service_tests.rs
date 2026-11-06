use super::*;

#[test]
fn image_commands_discover_two_plugins_in_the_shared_order_without_vendor_priority() {
    let (root, _service, manager, _controller, _identity) =
        panel_action_fixture_with_image("image-order", true);
    let source = root.0.join("second");
    fs::create_dir_all(&source).unwrap();
    let mut value = manifest("panel");
    value["pluginId"] = json!("com.uipilot.baidu-ocr");
    value["name"] = json!("Zebra");
    value["command"]["defaultName"] = json!("zebraocr");
    value["command"]["inputCapabilities"] = json!(["image.ocr"]);
    value["minimumHostVersion"] = json!("0.21.0");
    value["panel"]["runtimeActions"] = json!(true);
    write_package(&source, &value);
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
    let ordinary: Vec<_> = manager
        .command_suggestions("")
        .unwrap()
        .into_iter()
        .map(|row| row.plugin_id)
        .collect();
    let images: Vec<_> = manager
        .image_commands("")
        .unwrap()
        .into_iter()
        .map(|(row, reason)| {
            assert!(reason.is_none());
            row.plugin_id
        })
        .collect();
    assert_eq!(images.len(), 2);
    assert_eq!(images, ordinary);
    assert_eq!(images[0], "com.uipilot.demo");
}
#[test]
fn panel_image_service_dispatch_handoff_and_cancellation_follow_real_admission() {
    use crate::public_plugins::panel_action::PanelActionError as E;
    for mode in ["replace", "clear", "hide", "success"] {
        let (_root, service, manager, controller, identity) =
            panel_action_fixture_with_image(mode, true);
        manager.launcher_images.begin_import(1).unwrap();
        let launcher = manager
            .launcher_images
            .finish_import(
                1,
                crate::launcher_image::normalize_rgba(1, 1, &[1, 2, 3, 255]).unwrap(),
            )
            .unwrap();
        controller
            .begin_image_import(&manager, "main", identity.session_epoch, 1)
            .unwrap();
        let panel = controller
            .finish_image_import(
                &manager,
                "main",
                identity.session_epoch,
                1,
                launcher.clone(),
            )
            .unwrap();
        manager.launcher_images.clear(2).unwrap();
        let request = crate::public_plugins::panel_action::PanelActionRequest::from_value(
            json!({"action":"recognize", "imageId":panel.id,"payload":{"providerId":"baidu"}}),
        )
        .unwrap();
        for (label, epoch) in [
            ("main", identity.session_epoch),
            (identity.content_label.as_str(), identity.session_epoch + 1),
        ] {
            assert_eq!(
                controller
                    .admit_panel_action(&service, label, epoch, request.clone())
                    .err(),
                Some(E::ExpiredPanelSession)
            );
        }
        let mut stale = request.clone();
        stale.image_id = Some(launcher.id.clone());
        assert_eq!(
            controller
                .admit_panel_action(
                    &service,
                    &identity.content_label,
                    identity.session_epoch,
                    stale
                )
                .err(),
            Some(E::PanelRequestCancelledError)
        );
        let (dispatch, receiver) = controller
            .admit_panel_action(
                &service,
                &identity.content_label,
                identity.session_epoch,
                request,
            )
            .unwrap();
        let (kind, invocation) = service
            .dispatch_invocation(
                &dispatch,
                crate::public_plugins::PluginInvocationTheme::Dark,
                "2026-11-06T00:00:00Z".into(),
            )
            .unwrap();
        assert_eq!(kind, "panelAction");
        let id = invocation["image"]["id"].as_str().unwrap();
        assert_ne!(id, panel.id);
        assert_eq!(
            manager.claim_ocr_image(&dispatch.context, id).unwrap().png,
            launcher.png
        );
        assert_eq!(
            manager.claim_ocr_image(&dispatch.context, id).unwrap_err(),
            "serviceAlreadyUsed"
        );
        match mode {
            "replace" => {
                controller
                    .begin_image_import(&manager, "main", identity.session_epoch, 2)
                    .unwrap();
                controller
                    .finish_image_import(&manager, "main", identity.session_epoch, 2, launcher)
                    .unwrap();
            }
            "clear" => controller
                .clear_image(&manager, &identity.content_label, identity.session_epoch)
                .unwrap(),
            "hide" => {
                controller.host_hidden();
            }
            _ => {}
        }
        if mode == "success" {
            let completion = manager
                .scheduler()
                .complete(&dispatch.context, Instant::now())
                .unwrap();
            assert!(completion.accepted);
            assert!(service.settle_panel_action(
                &dispatch.context,
                Ok(json!({"ok":true,"text":"first\nsecond","lines":["first","second"]})),
                true
            ));
            assert_eq!(
                receiver
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap()
                    .unwrap()["text"],
                "first\nsecond"
            );
        } else {
            assert!(manager
                .bound_ocr_image(&dispatch.context, Some(id))
                .is_err());
            assert!(service
                .dispatch_invocation(
                    &dispatch,
                    crate::public_plugins::PluginInvocationTheme::Dark,
                    String::new()
                )
                .is_err());
            if mode != "hide" {
                assert!(
                    receiver.try_recv().is_err(),
                    "cancel waits for the physical slot"
                );
            }
            assert!(
                !manager
                    .scheduler()
                    .complete(&dispatch.context, Instant::now())
                    .unwrap()
                    .accepted
            );
            service.settle_panel_action(&dispatch.context, Err(E::ExpiredPanelSession), true);
            let error = receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap_err();
            assert_eq!(
                error,
                if mode == "hide" {
                    E::ExpiredPanelSession
                } else {
                    E::PanelRequestCancelledError
                }
            );
            if mode == "replace" {
                controller
                    .begin_image_import(&manager, "main", identity.session_epoch, 3)
                    .unwrap();
                let c = controller
                    .finish_image_import(
                        &manager,
                        "main",
                        identity.session_epoch,
                        3,
                        crate::launcher_image::normalize_rgba(1, 1, &[4, 5, 6, 255]).unwrap(),
                    )
                    .unwrap();
                let latest = crate::public_plugins::panel_action::PanelActionRequest::from_value(
                    json!({"action":"recognize","payload":null,"imageId":c.id}),
                )
                .unwrap();
                assert!(
                    controller
                        .admit_panel_action(
                            &service,
                            &identity.content_label,
                            identity.session_epoch,
                            latest
                        )
                        .is_ok(),
                    "latest image starts after physical completion without Busy"
                );
            }
        }
    }
}

#[test]
fn panel_image_import_session_and_sequence_guards_preserve_newer_state() {
    let (_root, _service, manager, controller, identity) =
        panel_action_fixture_with_image("panel-image-imports", true);
    let image = crate::launcher_image::normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap();
    assert!(controller
        .begin_image_import(&manager, "wrong", identity.session_epoch, 1)
        .is_err());
    assert!(controller
        .begin_image_import(&manager, "main", identity.session_epoch + 1, 1)
        .is_err());
    controller
        .begin_image_import(&manager, "main", identity.session_epoch, 1)
        .unwrap();
    controller
        .begin_image_import(&manager, "main", identity.session_epoch, 2)
        .unwrap();
    let b = controller
        .finish_image_import(&manager, "main", identity.session_epoch, 2, image.clone())
        .unwrap();
    assert!(controller
        .finish_image_import(&manager, "main", identity.session_epoch, 1, image.clone())
        .is_err());
    assert!(controller
        .clear_image(&manager, "main", identity.session_epoch)
        .is_err());
    controller
        .begin_image_import(&manager, "main", identity.session_epoch, 3)
        .unwrap();
    controller.host_hidden();
    assert!(controller
        .finish_image_import(&manager, "main", identity.session_epoch, 3, image)
        .is_err());
    assert!(!b.id.is_empty());
}
