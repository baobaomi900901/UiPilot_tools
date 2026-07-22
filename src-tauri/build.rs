fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "search_apps",
            "publish_plugin_results",
            "execute_result",
            "list_plugins",
            "reload_plugin",
            "delete_plugin",
            "load_settings",
            "save_settings",
            "save_hotkey",
            "hide_launcher",
        ]),
    ))
    .expect("failed to build Tauri application");
}
