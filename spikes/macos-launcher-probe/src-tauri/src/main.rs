use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[tauri::command]
fn check_origin(window: tauri::WebviewWindow) -> Result<String, String> {
    let url = window.url().map_err(|e| e.to_string())?;
    println!(
        "protected command reached: label={} scheme={} host={} port={:?}",
        window.label(), url.scheme(), url.host_str().unwrap_or(""), url.port(),
    );
    Ok("local-command-reached".into())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let result = app.global_shortcut().on_shortcut("Control+Alt+Shift+U", |app, _, event| {
                if event.state != ShortcutState::Pressed { return; }
                if let Some(main) = app.get_webview_window("main") {
                    if let Err(err) = main.show().and_then(|_| main.set_focus()) {
                        eprintln!("focus request failed: {err}");
                    }
                }
            });
            if let Err(err) = result { eprintln!("shortcut registration failed: {err}"); }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![check_origin])
        .run(tauri::generate_context!())
        .expect("probe startup failed");
}
