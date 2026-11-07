fn main() {
    let helper_build = std::env::var_os("CARGO_FEATURE_INSTALLER_HELPER_BUILD").is_some();
    #[cfg(target_os = "windows")]
    {
        if helper_build {
            let manifest = std::path::Path::new("windows/uipilot-everything-installer.manifest")
                .canonicalize()
                .expect("installer Helper manifest must exist");
            println!("cargo:rerun-if-changed={}", manifest.display());
            println!(
                "cargo:rustc-link-arg-bin=uipilot-everything-installer=/MANIFESTUAC:level='asInvoker' uiAccess='false'"
            );
            println!(
                "cargo:rustc-link-arg-bin=uipilot-everything-installer=/MANIFESTINPUT:{}",
                manifest.display()
            );
            println!("cargo:rustc-link-arg-bin=uipilot-everything-installer=/MANIFEST:EMBED");
        } else {
            // Cargo's tests-only linker args omit lib unit tests. Embed the same
            // Common Controls dependency for every ordinary Windows link target;
            // preserve Tauri's app resource manifest and the separate helper path.
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
            );
            // Ordinary binaries already embed Tauri's manifest via resource.lib.
            // Keep that resource instead of generating a second manifest with ID 1.
            println!("cargo:rustc-link-arg-bin=uipilot=/MANIFEST:NO");
            println!("cargo:rustc-link-arg-bin=generate_public_plugin_schema=/MANIFEST:NO");
        }
    }
    if helper_build {
        return;
    }
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_host_capabilities",
            "open_find_window",
            "cancel_find_query",
            "open_find_document",
            "get_find_documents_access",
            "grant_find_documents_access",
            "revoke_find_documents_access",
            "prepare_find_initialization",
            "commit_find_ready",
            "get_find_ready_status",
            "set_find_pinned",
            "set_settings_pinned",
            "validate_youdao_translation",
            "list_credentials",
            "add_credential",
            "save_credential",
            "set_credential_enabled",
            "delete_credential",
            "set_find_preview_preference",
            "load_find_thumbnail",
            "hide_find_window",
            "select_public_plugin_directory",
            "list_builtin_features",
            "set_builtin_feature_enabled",
            "set_builtin_feature_name",
            "set_builtin_feature_hotkey",
            "list_public_plugins",
            "cancel_translation_selection",
            "get_translation_selection_settings",
            "set_translation_selection_hotkey",
            "set_public_plugin_command_hotkey",
            "resolve_public_plugin_command_entry",
            "get_public_plugin_diagnostics",
            "copy_public_plugin_diagnostics",
            "list_quicklinks",
            "save_quicklink",
            "delete_quicklink",
            "choose_quicklink_icon",
            "prepare_public_plugin_install",
            "commit_public_plugin_install",
            "cancel_public_plugin_install",
            "set_plugin_enabled",
            "set_public_plugin_network_access",
            "set_public_plugin_permission_access",
            "set_plugin_favorite",
            "set_plugin_effective_name",
            "save_plugin_settings",
            "uninstall_plugin",
            "plugin_network_request",
            "plugin_youdao_translate",
            "plugin_api_call",
            "complete_plugin_command",
            "plugin_window_content_ready",
            "plugin_window_content_ack",
            "plugin_window_content_close",
            "plugin_window_content_toggle_pin",
            "plugin_window_storage_get",
            "plugin_window_storage_set",
            "plugin_window_storage_remove",
            "plugin_window_timer_get_state",
            "plugin_window_timer_start",
            "plugin_window_timer_stop",
            "plugin_window_timer_reset",
            "plugin_panel_content_ready",
            "plugin_panel_content_ack",
            "plugin_panel_host_key_enqueue",
            "plugin_panel_host_key_ack",
            "plugin_panel_request_hide_admit",
            "plugin_panel_request_hide_admit_observed",
            "plugin_panel_request_hide_commit",
            "plugin_panel_focus_host_input",
            "plugin_panel_set_status",
            "plugin_panel_actions_submit",
            "plugin_panel_focus_host_input_ack",
            "plugin_window_open_text",
            "plugin_window_request",
            "plugin_panel_request",
            "plugin_panel_storage_get",
            "plugin_panel_storage_set",
            "plugin_panel_storage_remove",
            "plugin_panel_clipboard_history_list",
            "plugin_panel_clipboard_history_copy",
            "plugin_panel_clipboard_write_text",
            "plugin_panel_speech_voices",
            "plugin_panel_speech_play",
            "plugin_panel_speech_stop",
            "plugin_panel_speech_state",
            "plugin_panel_clipboard_history_paste",
            "plugin_panel_clipboard_history_remove",
            "plugin_panel_clipboard_history_clear",
            "open_plugin_panel",
            "open_plugin_panel_actions",
            "submit_plugin_panel",
            "set_plugin_panel_bounds",
            "close_plugin_panel",
            "commit_plugin_window_transfer",
            "get_public_plugin_window_identity",
            "set_plugin_window_pinned",
            "close_plugin_window",
            "get_message_summary",
            "open_message_center",
            "read_message_center",
            "clear_messages",
            "search_apps",
            "publish_plugin_results",
            "search_files",
            "execute_result",
            "execute_result_action",
            "retire_result_actions",
            "list_plugins",
            "install_plugin",
            "reload_plugin",
            "delete_plugin",
            "load_settings",
            "save_settings",
            "save_hotkey",
            "set_screenshot_hotkey",
            "select_screenshot_save_directory",
            "set_screenshot_save_directory",
            "set_hotkey_recording",
            "set_file_preview_preference",
            "set_theme_preference",
            "start_screenshot",
            "get_screenshot_state",
            "read_screenshot_image",
            "release_screenshot_image",
            "copy_screenshot",
            "save_screenshot",
            "retake_screenshot",
            "close_screenshot",
            "set_web_search_engine",
            "get_macos_login_item",
            "set_macos_login_item",
            "open_macos_login_item_settings",
            "set_builtin_feature_favorite",
            "hide_launcher",
        ]),
    ))
    .expect("failed to build Tauri application");
}
