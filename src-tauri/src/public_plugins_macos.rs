//! Shared foundations, not an enabled macOS plugin host.
#![allow(dead_code)]

pub(crate) use crate::manifest;

#[path = "public_plugins/network/mod.rs"]
mod network;
#[path = "public_plugins/sensitive_bytes.rs"]
mod sensitive_bytes;
#[path = "public_plugins/webview_audio_guard.rs"]
mod webview_audio_guard;

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_alias_is_the_existing_host_type() {
        fn same_type(value: super::manifest::PublicManifestV1) -> crate::manifest::PublicManifestV1 {
            value
        }
        let _: fn(super::manifest::PublicManifestV1) -> crate::manifest::PublicManifestV1 = same_type;
        assert_eq!(
            super::manifest::public_manifest_v1_schema(),
            crate::manifest::public_manifest_v1_schema(),
        );
    }
}
