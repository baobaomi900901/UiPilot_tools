#![allow(dead_code)]
#[path = "../../../src-tauri/src/atomic_file_macos.rs"]
mod atomic_file;
#[path = "../../../src-tauri/src/public_plugins/data_scope.rs"]
mod data_scope;
#[path = "../../../src-tauri/src/public_plugins/manifest.rs"]
mod manifest;
use data_scope::{authorize_plugin_scope, valid_json_value, PluginDataScope};
#[path = "../../../src-tauri/src/public_plugins/main_result_contract.rs"]
mod main_result_contract;
#[path = "../../../src-tauri/src/public_plugins/private_file_macos.rs"]
mod private_file_macos;
#[path = "../../../src-tauri/src/public_plugins/runtime_data_macos.rs"]
mod runtime_data_macos;
#[path = "../../../src-tauri/src/public_plugins/runtime_error.rs"]
mod runtime_error;
#[path = "../../../src-tauri/src/public_plugins/runtime_result_macos.rs"]
mod runtime_result_macos;
#[path = "../../../src-tauri/src/public_plugins/settings_macos.rs"]
mod settings_macos;
#[path = "../../../src-tauri/src/public_plugins/settings_values.rs"]
mod settings_values;
#[path = "../../../src-tauri/src/public_plugins/storage_macos.rs"]
mod storage;
#[path = "../../../src-tauri/src/public_plugins/storage_document.rs"]
mod storage_document;
#[cfg(test)]
fn public_plugin_manifest_schema() -> serde_json::Value {
    serde_json::to_value(manifest::public_manifest_v1_schema()).expect("valid schema")
}
#[path = "../../../src-tauri/src/public_plugins/call_session.rs"]
mod call_session;
#[path = "../../../src-tauri/src/public_plugins/data_call_gate.rs"]
mod data_call_gate;
pub mod host;
#[path = "../../../src-tauri/src/public_plugins/request_contract.rs"]
mod request_contract;

#[path = "../../../src-tauri/src/public_plugins/icon.rs"]
mod icon;
#[path = "../../../src-tauri/src/public_plugins/lifecycle_macos.rs"]
mod lifecycle_macos;
#[path = "../../../src-tauri/src/public_plugins/package_macos.rs"]
mod package_macos;
#[path = "../../../src-tauri/src/public_plugins/package_rules.rs"]
mod package_rules;

pub(crate) use manifest::PublicPackageError;

/// Strict bounded-frame JSON check shared with the real private data service.
/// The helper's byte limit is checked before this parser is called.
pub fn validate_wire_json(bytes: &[u8]) -> Result<(), ()> {
    storage_document::validate_mac_json(bytes).ok_or(())
}

pub fn validate_launcher_invocation(value: &serde_json::Value, request_id: &str) -> Result<(), ()> {
    use request_contract::{PluginInvocation, PluginInvocationPlatform};
    use serde::Deserialize;
    let invocation = PluginInvocation::deserialize(value).map_err(|_| ())?;
    if invocation.api_version != 1
        || invocation.context.platform != PluginInvocationPlatform::Macos
        || invocation.request_id != request_id
    {
        return Err(());
    }
    Ok(())
}

#[path = "../../../src-tauri/src/public_plugins/runtime_modules.rs"]
pub mod runtime_modules;

/// Initialization has its own byte allowance; storage and RPC keep their limits.
pub fn validate_module_init_json(bytes: &[u8]) -> Result<(), ()> {
    storage_document::validate_bounded_mac_json(bytes, runtime_modules::MAX_MODULE_INIT_BYTES)
        .ok_or(())
}

#[path = "../../../src-tauri/src/public_plugins/host_command_names.rs"]
mod host_command_names;

#[cfg(test)]
mod init_json_tests {
    use super::*;
    #[test]
    fn large_init_preserves_duplicate_depth_and_storage_limits() {
        let padding = " ".repeat(6 * 1024 * 1024);
        let valid = format!(r#"{{"source":"{padding}"}}"#);
        assert!(validate_module_init_json(valid.as_bytes()).is_ok());
        assert!(validate_wire_json(valid.as_bytes()).is_err());
        let duplicate = format!(r#"{{"source":"{padding}","source":""}}"#);
        assert!(validate_module_init_json(duplicate.as_bytes()).is_err());
        assert!(validate_module_init_json(
            format!("{}0{}", "[".repeat(129), "]".repeat(129)).as_bytes()
        )
        .is_err());
    }
}
