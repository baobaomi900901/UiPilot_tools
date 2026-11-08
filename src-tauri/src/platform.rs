// The standalone installer helper does not use the host.
#![cfg(not(feature = "installer-helper-build"))]

mod host_capabilities;

#[cfg(windows)]
include!("lib.rs");

#[cfg(target_os = "macos")]
#[path = "atomic_file_macos.rs"]
mod atomic_file;
#[cfg(target_os = "macos")]
mod settings;
// Share clipboard value/preview semantics without installing an observer or
// exposing the still-unavailable macOS history service.
#[cfg(target_os = "macos")]
mod clipboard_history;
#[cfg(target_os = "macos")]
mod macos_keychain;
#[cfg(target_os = "macos")]
mod quick_evaluation;
#[cfg(target_os = "macos")]
mod macos_quick_evaluation;
#[cfg(target_os = "macos")]
mod web_search;
#[cfg(target_os = "macos")]
mod macos_browser_open;
#[cfg(target_os = "macos")]
mod macos_login_item;
// Shared plugin foundations and management; plugin execution remains closed.
#[cfg(target_os = "macos")]
#[path = "public_plugins_macos.rs"]
mod public_plugins;
#[cfg(target_os = "macos")]
mod webview_reload_guard;
#[cfg(target_os = "macos")]
mod apps;
#[cfg(target_os = "macos")]
mod result_registry;
#[cfg(target_os = "macos")]
mod model;
#[cfg(target_os = "macos")]
mod macos_apps;
#[cfg(target_os = "macos")]
mod macos_command_tap;
#[cfg(target_os = "macos")]
mod macos_host;
#[cfg(target_os = "macos")]
mod macos_plugin_management;
#[cfg(target_os = "macos")]
pub use macos_host::run;

// The schema generator uses the same Rust model without loading a plugin service.
#[cfg(target_os = "macos")]
#[path = "public_plugins/manifest.rs"]
#[allow(dead_code)]
pub(crate) mod manifest;
#[cfg(target_os = "macos")]
pub fn public_plugin_manifest_schema() -> serde_json::Value {
    serde_json::to_value(manifest::public_manifest_v1_schema())
        .expect("public plugin schema must serialize")
}

#[cfg(target_os = "macos")]
#[allow(dead_code)] // Awaiting the Find backend and its independent authorization checks.
mod macos_find_identity;

#[cfg(target_os = "macos")]
#[allow(dead_code)] // Private worker bridge, pending Find controller and scope integration.
mod macos_find_query;

#[cfg(all(target_os = "macos", feature = "macos-find-preview"))]
mod find_window;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Neither the default host nor plugin windows register file search IPC.
mod macos_find_results;
#[cfg(all(target_os = "macos", feature = "macos-find-qa-probe"))]
pub fn run_system_qa_probe() -> Result<(), String> {
    macos_find_results::run_system_qa_probe()
}
#[cfg(all(target_os = "macos", feature = "macos-find-qa-probe"))]
pub fn qa_shared_workspace_identity() -> Result<String, &'static str> {
    macos_find_results::qa_shared_workspace_identity()
}
#[cfg(all(target_os = "macos", feature = "macos-find-preview"))]
mod macos_find_host;
