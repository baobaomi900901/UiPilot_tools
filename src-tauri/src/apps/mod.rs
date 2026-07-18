use std::{
    fmt::{self, Write},
    path::PathBuf,
};

use windows::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptHash, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE,
    BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM,
};

mod cache;
mod discovery;
mod rank;
mod shortcut;

pub(crate) use cache::{start_initial_refresh, AppCache};

pub(crate) fn rank(applications: &[Application], query: &str) -> Vec<Application> {
    rank::rank(applications, query)
}

pub(crate) fn discover() -> Result<DiscoverySnapshot, DiscoveryError> {
    discovery::discover()
}

pub(crate) fn registry_entry(
    application: &Application,
) -> (
    crate::model::ResultItem,
    crate::result_registry::ResultAction,
) {
    discovery::registry_entry(application)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootKind {
    User,
    Common,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StartMenuRoot {
    pub(crate) kind: RootKind,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Application {
    pub(crate) app_id: String,
    pub(crate) display_name: String,
    pub(crate) shortcut: PathBuf,
    pub(crate) executable: Option<PathBuf>,
    pub(crate) icon: Option<String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) use_count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiscoveryDiagnostics {
    pub(crate) missing_roots: u64,
    pub(crate) inaccessible_entries: u64,
    pub(crate) reparse_entries: u64,
    pub(crate) non_unicode_entries: u64,
    pub(crate) invalid_shortcuts: u64,
    pub(crate) unmapped_executables: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoverySnapshot {
    pub(crate) applications: Vec<Application>,
    pub(crate) diagnostics: DiscoveryDiagnostics,
}

impl RootKind {
    fn identity(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Common => "common",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryError {
    KnownFolderQuery,
    RootNotDirectory,
    RootUnavailable,
    RootReparsePoint,
    ComUnavailable,
    HashFailed,
    DuplicateAppId,
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::KnownFolderQuery => "known folder query failed",
            Self::RootNotDirectory => "start menu root is not a directory",
            Self::RootUnavailable => "start menu root is unavailable",
            Self::RootReparsePoint => "start menu root is a reparse point",
            Self::ComUnavailable => "COM is unavailable",
            Self::HashFailed => "application identity hashing failed",
            Self::DuplicateAppId => "duplicate application identity",
        })
    }
}

impl std::error::Error for DiscoveryError {}

struct AlgorithmHandle(BCRYPT_ALG_HANDLE);

impl Drop for AlgorithmHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptCloseAlgorithmProvider(self.0, 0);
        }
    }
}

fn app_id(root_kind: RootKind, relative_shortcut_path: &str) -> Result<String, DiscoveryError> {
    let preimage = format!(
        "start-menu-v1\0{}\0{}",
        root_kind.identity(),
        relative_shortcut_path.to_lowercase()
    );
    let mut raw_handle = BCRYPT_ALG_HANDLE::default();
    let open_status = unsafe {
        BCryptOpenAlgorithmProvider(
            &mut raw_handle,
            BCRYPT_SHA256_ALGORITHM,
            None,
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        )
    };
    if open_status.is_err() {
        return Err(DiscoveryError::HashFailed);
    }
    let handle = AlgorithmHandle(raw_handle);
    let mut digest = [0_u8; 32];
    let hash_status = unsafe { BCryptHash(handle.0, None, preimage.as_bytes(), &mut digest) };
    if hash_status.is_err() {
        return Err(DiscoveryError::HashFailed);
    }

    let mut result = String::with_capacity(68);
    result.push_str("app-");
    for byte in digest {
        write!(result, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{app_id, RootKind};

    #[test]
    fn app_id_has_fixed_vectors_and_case_only_stability() {
        assert_eq!(
            app_id(RootKind::User, "Tools\\WeChat.lnk").unwrap(),
            "app-8fe952e53691106c491156368e5c9b70bd56a3fc0b2a43455a9a40c765d56f9f",
        );
        assert_eq!(
            app_id(RootKind::Common, "tools\\wechat.lnk").unwrap(),
            "app-567d9db3933c49330028523bda654c90a540288b10f56b10f6375cc6ddb1fae0",
        );
        assert_eq!(
            app_id(RootKind::User, "Tools\\WeChat.lnk"),
            app_id(RootKind::User, "tools\\wechat.LNK"),
        );
        assert_ne!(
            app_id(RootKind::User, "Tools\\WeChat.lnk"),
            app_id(RootKind::User, "Other\\WeChat.lnk"),
        );
    }
}
