//! Shared runtime error vocabulary; platform services keep their original mappings.
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginRuntimeError {
    InvalidContext,
    ExpiredRequest,
    InvalidCaller,
    InvalidOperation,
    PermissionDenied,
    InvalidNotification,
    InvalidDelay,
    ScheduleLimitExceeded,
    AlreadyPublished,
    MessageStoreUnavailable,
    Storage,
    Unavailable,
}

impl fmt::Display for PluginRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidContext => "InvalidContext",
            Self::ExpiredRequest => "ExpiredRequestError",
            Self::InvalidCaller => "InvalidCaller",
            Self::InvalidOperation => "InvalidOperation",
            Self::PermissionDenied => "PermissionDenied",
            Self::InvalidNotification => "InvalidNotification",
            Self::InvalidDelay => "InvalidDelay",
            Self::ScheduleLimitExceeded => "ScheduleLimitExceeded",
            Self::AlreadyPublished => "AlreadyPublished",
            Self::MessageStoreUnavailable => "MessageStoreUnavailable",
            Self::Storage => "StorageError",
            Self::Unavailable => "RuntimeUnavailable",
        })
    }
}

impl std::error::Error for PluginRuntimeError {}
