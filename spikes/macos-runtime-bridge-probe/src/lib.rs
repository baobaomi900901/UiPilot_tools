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
