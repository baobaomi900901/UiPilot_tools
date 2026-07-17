#[cfg(feature = "test-instrumentation")]
mod security_probe;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build());

    #[cfg(feature = "test-instrumentation")]
    let builder = builder.invoke_handler(tauri::generate_handler![security_probe::load_settings]);

    builder
        .setup(|_app| {
            #[cfg(feature = "test-instrumentation")]
            security_probe::setup(_app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
