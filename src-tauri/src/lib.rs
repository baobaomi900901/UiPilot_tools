use std::sync::Arc;

#[cfg_attr(not(test), allow(dead_code))]
mod atomic_file;

#[cfg_attr(not(test), allow(dead_code))]
mod apps;
// ponytail: Task 2 defines the protocol before Task 5 wires commands; remove these allows then.
#[cfg_attr(not(test), allow(dead_code))]
mod model;
#[cfg_attr(not(test), allow(dead_code))]
mod result_registry;

#[cfg(feature = "test-instrumentation")]
mod security_probe;

pub fn run() {
    let app_cache = Arc::new(apps::AppCache::new());
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(Arc::clone(&app_cache));

    #[cfg(feature = "test-instrumentation")]
    let builder = builder.invoke_handler(tauri::generate_handler![security_probe::load_settings]);

    builder
        .setup(move |_app| {
            #[cfg(feature = "test-instrumentation")]
            security_probe::setup(_app)?;

            let _ = apps::start_initial_refresh(Arc::clone(&app_cache))?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
