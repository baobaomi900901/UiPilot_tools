use serde::Serialize;

use crate::file_index::OpenIndexedPath;

use self::windows::path_auth::AuthenticatedPathIdentity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EverythingPathAction {
    identity: AuthenticatedPathIdentity,
}

impl EverythingPathAction {
    #[allow(dead_code)]
    pub(crate) fn new(identity: AuthenticatedPathIdentity) -> Self {
        Self { identity }
    }

    pub(crate) fn identity(&self) -> &AuthenticatedPathIdentity {
        &self.identity
    }

    #[cfg(test)]
    pub(crate) fn for_test(identity: AuthenticatedPathIdentity) -> Self {
        Self::new(identity)
    }

    #[cfg(test)]
    pub(crate) fn kind_for_test(&self) -> FilePathKind {
        self.identity.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionAction {
    Indexed(OpenIndexedPath),
    #[allow(dead_code)]
    Everything(EverythingPathAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileIndexStatus {
    Building,
    Ready,
    Partial,
    Rebuilding,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileResultKind {
    File,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileResultItem {
    pub(crate) result_id: String,
    pub(crate) name: String,
    pub(crate) kind: FileResultKind,
    pub(crate) size_bytes: Option<String>,
    pub(crate) modified_utc: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileSearchResponse {
    pub(crate) request_id: String,
    pub(crate) index_revision: String,
    pub(crate) total: String,
    pub(crate) status: FileIndexStatus,
    pub(crate) items: Vec<FileResultItem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FilePathKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionOutcome {
    FileRevealRequested,
    FolderOpenRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionError {
    SearchUnavailable,
    Stale,
    NotFound,
    OpenFailed,
}

pub(crate) mod windows;
