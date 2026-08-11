fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "open_find_window",
            "prepare_find_initialization",
            "commit_find_ready",
            "get_find_ready_status",
            "set_find_pinned",
            "set_find_preview_preference",
            "hide_find_window",
            "search_apps",
            "publish_plugin_results",
            "search_files",
            "execute_result",
            "list_plugins",
            "install_plugin",
            "reload_plugin",
            "delete_plugin",
            "load_settings",
            "save_settings",
            "save_hotkey",
            "set_file_preview_preference",
            "set_theme_preference",
            "hide_launcher",
        ]),
    ))
    .expect("failed to build Tauri application");
}
