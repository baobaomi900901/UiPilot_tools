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
#[cfg(target_os = "macos")]
mod webview_reload_guard;
#[cfg(target_os = "macos")]
mod macos_host;
#[cfg(target_os = "macos")]
pub use macos_host::run;

// The schema generator uses the same Rust model without loading a plugin service.
#[cfg(target_os = "macos")]
#[path = "public_plugins/manifest.rs"]
#[allow(dead_code)]
mod manifest;
#[cfg(target_os = "macos")]
pub fn public_plugin_manifest_schema() -> serde_json::Value {
    serde_json::to_value(manifest::public_manifest_v1_schema())
        .expect("public plugin schema must serialize")
}
