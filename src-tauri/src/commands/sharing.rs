use super::*;
use crate::plugin_share::{Delivery, Payload, Phase, View};

fn check_packet(request: &tauri::ipc::Request<'_>) -> Result<(), CommandError> {
    let valid = match request.body() {
        tauri::ipc::InvokeBody::Json(value) => crate::plugin_share::envelope(value),
        tauri::ipc::InvokeBody::Raw(bytes) => bytes.len() <= 65536,
    };
    if valid {
        Ok(())
    } else {
        Err(CommandError::window_failed())
    }
}

#[tauri::command]
pub(crate) fn plugin_share_begin(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    public: State<'_, Arc<PublicPluginService>>,
    session_epoch: String,
    payload: Payload,
    request: tauri::ipc::Request<'_>,
) -> Result<(), CommandError> {
    check_packet(&request)?;
    let epoch = parse_panel_session_epoch(&session_epoch)?;
    let identity = controller
        .begin_content_call(webview.label(), epoch, true)
        .map_err(map_storage_panel_call_error)?;
    let rows = {
        let manager = public.manager()?;
        if !manager.share_source_enabled(&identity) {
            return Err(CommandError::window_failed());
        }
        manager.share_catalog()?
    };
    controller
        .share_begin(webview.label(), epoch, payload, rows)
        .map_err(|_| CommandError::window_failed())?;
    let content = app
        .get_webview(&identity.content_label)
        .ok_or_else(CommandError::window_failed)?;
    if content.hide().is_err() {
        controller.share_fail(
            &controller
                .share_view()
                .map(|s| s.transfer_id)
                .unwrap_or_default(),
        );
        return Err(CommandError::window_failed());
    }
    app.get_webview("main")
        .ok_or_else(CommandError::window_failed)?
        .set_focus()
        .map_err(|_| CommandError::window_failed())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn plugin_share_state<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    app: AppHandle<R>,
    controller: State<'_, Arc<PluginPanelController>>,
    request: tauri::ipc::Request<'_>,
) -> Result<Option<View>, CommandError> {
    check_packet(&request)?;
    require_main_label(webview.label())?;
    let state = controller.share_view();
    if let Some(state) = state.as_ref().filter(|s| s.phase == Phase::Error) {
        if let Some(identity) = controller.share_failed_identity(&state.transfer_id) {
            if controller
                .teardown_session(Some(identity.session_epoch))
                .is_some()
            {
                if let Some(content) = app.get_webview(&identity.content_label) {
                    let _ = content.close();
                }
            }
        }
    }
    if !crate::plugin_share::envelope(&state) {
        return Err(CommandError::window_failed());
    }
    Ok(state)
}

#[tauri::command]
pub(crate) async fn plugin_share_select(
    webview: tauri::Webview,
    app: AppHandle,
    public: State<'_, Arc<PublicPluginService>>,
    controller: State<'_, Arc<PluginPanelController>>,
    settings: State<'_, SettingsStore>,
    transfer_id: String,
    plugin_id: String,
    receiver_id: String,
    request: tauri::ipc::Request<'_>,
) -> Result<(), CommandError> {
    check_packet(&request)?;
    require_main_label(webview.label())?;
    let receiver = controller
        .share_select(&transfer_id, &plugin_id, &receiver_id)
        .map_err(|_| CommandError::window_failed())?;
    let result = async {
        if !public.manager()?.share_catalog()?.contains(&receiver) {
            return Err(CommandError::window_failed());
        }
        let target = open_plugin_panel_impl(
            app.clone(),
            public.clone(),
            controller.clone(),
            OpenPluginPanelInput {
                image_id: None,
                plugin_id,
                command_id: Some(receiver.command_id),
                argument: String::new(),
                stable_command_target: None,
            },
            Some(transfer_id.clone()),
        )
        .await?
        .ok_or_else(CommandError::window_failed)?;
        let epoch = target.session_epoch.clone();
        controller
            .share_target(&transfer_id, target)
            .map_err(|_| CommandError::window_failed())?;
        submit_plugin_panel_impl(
            app.clone(),
            public,
            controller.clone(),
            settings,
            SubmitPluginPanelInput {
                session_epoch: epoch,
                argument: String::new(),
                ui_intent_epoch: 0,
            },
        )
        .await?;
        Ok(())
    }
    .await;
    if result.is_err()
        && controller
            .share_view()
            .is_some_and(|s| s.transfer_id == transfer_id)
    {
        controller.share_fail(&transfer_id);
        if let Some(identity) = controller.share_failed_identity(&transfer_id) {
            plugin_panel::teardown(&app, &controller, Some(identity.session_epoch));
        }
    }
    result
}

#[tauri::command]
pub(crate) fn plugin_share_claim<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    request_id: String,
    request: tauri::ipc::Request<'_>,
) -> Result<Option<Delivery>, CommandError> {
    check_packet(&request)?;
    controller
        .share_claim(
            webview.label(),
            parse_panel_session_epoch(&session_epoch)?,
            &request_id,
        )
        .map_err(|_| CommandError::window_failed())
}
#[tauri::command]
pub(crate) fn plugin_share_received<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    request_id: String,
    transfer_id: String,
    accepted: bool,
    request: tauri::ipc::Request<'_>,
) -> Result<(), CommandError> {
    check_packet(&request)?;
    controller
        .share_received(
            webview.label(),
            parse_panel_session_epoch(&session_epoch)?,
            &request_id,
            &transfer_id,
            accepted,
        )
        .map_err(|_| CommandError::window_failed())
}
#[tauri::command]
pub(crate) fn plugin_share_ready_failed<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    controller: State<'_, Arc<PluginPanelController>>,
    session_epoch: String,
    request_id: String,
    request: tauri::ipc::Request<'_>,
) -> Result<(), CommandError> {
    check_packet(&request)?;
    let identity = controller
        .begin_content_call(
            webview.label(),
            parse_panel_session_epoch(&session_epoch)?,
            true,
        )
        .map_err(map_storage_panel_call_error)?;
    if let Some(view) = controller.share_view().filter(|s| {
        s.target
            .as_ref()
            .is_some_and(|t| t.session_epoch == identity.session_epoch.to_string())
    }) {
        // This document can only fail its own current initialization.
        if controller
            .share_claim(webview.label(), identity.session_epoch, &request_id)
            .is_ok()
        {
            controller.share_fail(&view.transfer_id);
        }
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn plugin_share_end(
    webview: tauri::Webview,
    app: AppHandle,
    controller: State<'_, Arc<PluginPanelController>>,
    transfer_id: String,
    request: tauri::ipc::Request<'_>,
) -> Result<(), CommandError> {
    check_packet(&request)?;
    require_main_label(webview.label())?;
    let closing = controller.live_identity();
    if let Some(identity) = controller
        .share_end(&transfer_id)
        .map_err(|_| CommandError::window_failed())?
    {
        let content = app
            .get_webview(&identity.content_label)
            .ok_or_else(CommandError::window_failed)?;
        content.show().map_err(|_| CommandError::window_failed())?;
        controller
            .mark_content_visible(identity.session_epoch)
            .map_err(map_panel_bounds_error)?;
        content
            .set_focus()
            .map_err(|_| CommandError::window_failed())?;
    } else if let Some(identity) = closing {
        plugin_panel::teardown(&app, &controller, Some(identity.session_epoch));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tauri::{Listener, LogicalPosition, LogicalSize, WebviewBuilder, WebviewUrl};
    fn request(command: &str, body: serde_json::Value) -> tauri::webview::InvokeRequest {
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        }
    }
    struct Caller(tauri::Webview<tauri::test::MockRuntime>);
    impl AsRef<tauri::Webview<tauri::test::MockRuntime>> for Caller {
        fn as_ref(&self) -> &tauri::Webview<tauri::test::MockRuntime> {
            &self.0
        }
    }
    fn ipc(
        view: &tauri::Webview<tauri::test::MockRuntime>,
        request: tauri::webview::InvokeRequest,
    ) -> Result<tauri::ipc::InvokeResponseBody, serde_json::Value> {
        tauri::test::get_ipc_response(&Caller(view.clone()), request)
    }
    #[test]
    fn plugin_share_generated_ipc_checks_actual_child_and_never_broadcasts_body_or_catalog() {
        let controller = Arc::new(PluginPanelController::default());
        let (source, target) = controller.share_test_target();
        let app = tauri::test::mock_builder()
            .manage(Arc::clone(&controller))
            .invoke_handler(tauri::generate_handler![
                plugin_share_state,
                plugin_share_claim,
                plugin_share_received
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let parent = app.get_window("main").unwrap();
        let child = |label: &str| {
            parent
                .add_child(
                    WebviewBuilder::new(label, WebviewUrl::App("index.html".into())),
                    LogicalPosition::new(0., 0.),
                    LogicalSize::new(10., 10.),
                )
                .unwrap()
        };
        let source_webview = child(&source.content_label);
        let target_webview = child(&target.content_label);
        let unrelated = child("plugin-runtime-unrelated");
        assert_eq!(target_webview.window().label(), main.label());
        let leaks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for name in [
            "uipilot-plugin-share",
            "uipilot-public-plugin-command",
            "uipilot-plugin-panel-update",
        ] {
            let leaks = Arc::clone(&leaks);
            unrelated.listen_any(name, move |_| {
                leaks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        }
        for caller in [&source_webview, &target_webview, &unrelated] {
            let result = ipc(caller, request("plugin_share_state", json!({})));
            assert_eq!(result.unwrap_err()["code"], "invalidCaller");
        }
        assert!(ipc(
            main.as_ref(),
            request("plugin_share_state", json!({"padding":"x".repeat(65536)}))
        )
        .is_err());
        let state: serde_json::Value = ipc(main.as_ref(), request("plugin_share_state", json!({})))
            .unwrap()
            .deserialize()
            .unwrap();
        assert_eq!(state["phase"], "delivering");
        assert!(!state.to_string().contains("原文"));
        let args = json!({"sessionEpoch":target.session_epoch.to_string(),"requestId":"init"});
        for caller in [&source_webview, &unrelated, main.as_ref()] {
            assert!(ipc(caller, request("plugin_share_claim", args.clone())).is_err());
        }
        let delivery: serde_json::Value =
            ipc(&target_webview, request("plugin_share_claim", args.clone()))
                .unwrap()
                .deserialize()
                .unwrap();
        assert_eq!(delivery["payload"]["text"], " edited\n原文  ");
        assert!(ipc(&target_webview, request("plugin_share_claim", args)).is_err());
        let received = json!({"sessionEpoch":target.session_epoch.to_string(),"requestId":"init","transferId":"1","accepted":false});
        assert!(ipc(
            &source_webview,
            request("plugin_share_received", received.clone())
        )
        .is_err());
        assert!(ipc(&target_webview, request("plugin_share_received", received)).is_ok());
        let state: serde_json::Value = ipc(main.as_ref(), request("plugin_share_state", json!({})))
            .unwrap()
            .deserialize()
            .unwrap();
        assert_eq!(state["phase"], "error");
        assert_eq!(state["targetLabel"], "Translate");
        assert!(controller.live_identity().is_none());
        assert_eq!(leaks.load(std::sync::atomic::Ordering::SeqCst), 0);
        let production = include_str!("sharing.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!production.contains("emit"));
    }
}
