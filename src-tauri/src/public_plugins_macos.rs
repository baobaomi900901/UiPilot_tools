//! Shared foundations, not an enabled macOS plugin host.
#![allow(dead_code)]

pub(crate) use crate::manifest;
pub(crate) use manifest::PublicPackageError;
#[path = "public_plugins/data_scope.rs"]
mod data_scope;
#[path = "public_plugins/icon.rs"]
mod icon;
#[path = "public_plugins/package_macos.rs"]
mod package_macos;
#[path = "public_plugins/package_rules.rs"]
mod package_rules;
pub(crate) use data_scope::PluginDataScope;
use data_scope::{authorize_plugin_scope, valid_json_value};
#[path = "public_plugins/lifecycle_macos.rs"]
mod lifecycle_macos;
#[path = "public_plugins/private_file_macos.rs"]
mod private_file_macos;
#[path = "public_plugins/runtime_data_macos.rs"]
mod runtime_data_macos;
#[path = "public_plugins/settings_macos.rs"]
mod settings_macos;
#[path = "public_plugins/settings_values.rs"]
mod settings_values;
#[path = "public_plugins/storage_macos.rs"]
mod storage;
#[path = "public_plugins/storage_document.rs"]
mod storage_document;

#[path = "public_plugins/call_session.rs"]
mod call_session;
#[path = "public_plugins/data_call_gate.rs"]
mod data_call_gate;
#[path = "public_plugins/main_result_contract.rs"]
mod main_result_contract;
#[path = "public_plugins/main_result_macos.rs"]
mod main_result_macos;
#[path = "public_plugins/network/mod.rs"]
mod network;
#[path = "public_plugins/request_contract.rs"]
mod request_contract;
#[path = "public_plugins/runtime_error.rs"]
mod runtime_error;
#[path = "public_plugins/runtime_result_macos.rs"]
mod runtime_result_macos;
#[path = "public_plugins/sensitive_bytes.rs"]
mod sensitive_bytes;
#[path = "public_plugins/webview_audio_guard.rs"]
mod webview_audio_guard;

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_alias_is_the_existing_host_type() {
        fn same_type(
            value: super::manifest::PublicManifestV1,
        ) -> crate::manifest::PublicManifestV1 {
            value
        }
        let _: fn(super::manifest::PublicManifestV1) -> crate::manifest::PublicManifestV1 =
            same_type;
        assert_eq!(
            super::manifest::public_manifest_v1_schema(),
            crate::manifest::public_manifest_v1_schema(),
        );
    }
}

#[cfg(test)]
#[path = "public_plugins/lifecycle_registry_macos_tests.rs"]
mod lifecycle_registry_macos_tests;

#[path = "public_plugins/management_macos.rs"]
pub(crate) mod management_macos;

#[path = "public_plugins/launcher_runtime_macos.rs"]
pub(crate) mod launcher_runtime_macos;
