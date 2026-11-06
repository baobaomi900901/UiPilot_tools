use super::*;
use serde_json::json;

struct Caller<'a>(&'a tauri::Webview<tauri::test::MockRuntime>);
impl AsRef<tauri::Webview<tauri::test::MockRuntime>> for Caller<'_> {
    fn as_ref(&self) -> &tauri::Webview<tauri::test::MockRuntime> { self.0 }
}

#[test]
fn image_ipc_accepts_main_with_panel_child_and_rejects_child_callers() {
    // MockRuntime creates no native window and never reads clipboard contents.
    let app = tauri::test::mock_builder()
        .invoke_handler(tauri::generate_handler![
            import_launcher_image,
            get_launcher_clipboard_sequence,
            clear_launcher_image
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let panel = app.get_window("main").unwrap().add_child(
        tauri::WebviewBuilder::new("plugin-panel-ocr", Default::default()),
        tauri::LogicalPosition::new(0, 60),
        tauri::LogicalSize::new(600, 400),
    ).unwrap();
    assert!(app.get_webview_window("main").is_none());
    let invoke = |caller: &tauri::Webview<tauri::test::MockRuntime>, cmd: &str, body| {
        tauri::test::get_ipc_response(&Caller(caller), tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        }).unwrap_err()
    };
    // Invalid payload intentionally stops before image/state/clipboard access:
    // reaching our structured error proves the real command binder admitted main.
    let error = invoke(main.as_ref(), "import_launcher_image", json!({"input": {}}));
    assert_eq!(error["code"], "invalidImage", "main import returned {error}");
    // A child shares the main OS window but must never inherit main's authority.
    for (cmd, body) in [
        ("import_launcher_image", json!({"input": {}})),
        ("get_launcher_clipboard_sequence", json!({})),
        ("clear_launcher_image", json!({"importSequence": 1})),
    ] {
        assert_eq!(invoke(&panel, cmd, body)["code"], "invalidCaller", "{cmd}");
    }
}
