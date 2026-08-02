use serde::Serialize;

use crate::file_index::OpenIndexedPath;

use self::windows::path_auth::AuthenticatedPathIdentity;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileCategory {
    All,
    Folder,
    Excel,
    Word,
    Ppt,
    Pdf,
    Image,
    Video,
    Audio,
    Archive,
}
impl FileCategory {
    pub(crate) fn parse_wire(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "folder" => Some(Self::Folder),
            "excel" => Some(Self::Excel),
            "word" => Some(Self::Word),
            "ppt" => Some(Self::Ppt),
            "pdf" => Some(Self::Pdf),
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            "archive" => Some(Self::Archive),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EverythingPathAction {
    identity: AuthenticatedPathIdentity,
}

impl EverythingPathAction {
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
    Everything(EverythingPathAction),
}

impl From<OpenIndexedPath> for FileExecutionAction {
    fn from(action: OpenIndexedPath) -> Self {
        Self::Indexed(action)
    }
}

impl From<EverythingPathAction> for FileExecutionAction {
    fn from(action: EverythingPathAction) -> Self {
        Self::Everything(action)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileIndexStatus {
    Building,
    Ready,
    Partial,
    #[cfg(test)]
    Rebuilding,
    #[cfg(test)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedFileDraft {
    pub(crate) action: FileExecutionAction,
    pub(crate) name: String,
    pub(crate) kind: FileResultKind,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_utc: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedFileBatch {
    pub(crate) index_revision: u64,
    pub(crate) items: Vec<PublishedFileDraft>,
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

pub(crate) mod everything;
pub(crate) mod windows;

#[cfg(test)]
mod tests {
    use super::{
        windows::path_auth::AuthenticatedPathIdentity, EverythingPathAction, FileExecutionAction,
        FilePathKind, FileResultKind, PublishedFileBatch, PublishedFileDraft,
    };

    #[test]
    fn published_file_batch_preserves_everything_action_fields() {
        let action = FileExecutionAction::Everything(EverythingPathAction::for_test(
            AuthenticatedPathIdentity {
                display_path: r"C:\Visible\report.pdf".into(),
                volume_guid_path: r"\\?\Volume{PUBLISHED}\".into(),
                relative_path: r"docs\report.pdf".into(),
                volume_serial: 42,
                file_id: [7; 16],
                kind: FilePathKind::File,
            },
        ));
        let batch = PublishedFileBatch {
            index_revision: 17,
            items: vec![PublishedFileDraft {
                action: action.clone(),
                name: "report.pdf".into(),
                kind: FileResultKind::File,
                size_bytes: Some(123),
                modified_utc: "2026-07-30T00:00:00.000Z".into(),
                full_path: r"C:\Visible\report.pdf".into(),
            }],
        };

        assert_eq!(batch.index_revision, 17);
        assert_eq!(batch.items.len(), 1);
        let draft = &batch.items[0];
        assert_eq!(draft.action, action);
        assert_eq!(draft.name, "report.pdf");
        assert_eq!(draft.kind, FileResultKind::File);
        assert_eq!(draft.size_bytes, Some(123));
        assert_eq!(draft.modified_utc, "2026-07-30T00:00:00.000Z");
        assert_eq!(draft.full_path, r"C:\Visible\report.pdf");
    }
    #[test]
    fn production_indexed_action_and_open_path_remain_available() {
        let action_source = include_str!("mod.rs").replace("\r\n", "\n");
        let index_source = include_str!("../file_index/mod.rs").replace("\r\n", "\n");

        assert!(action_source.contains("use crate::file_index::OpenIndexedPath;"));
        assert!(!action_source.contains("#[cfg(test)]\nuse crate::file_index::OpenIndexedPath;"));
        assert!(action_source.contains("    Indexed(OpenIndexedPath),"));
        assert!(!action_source.contains("#[cfg(test)]\n    Indexed(OpenIndexedPath),"));
        assert!(action_source.contains("impl From<OpenIndexedPath> for FileExecutionAction"));
        assert!(index_source.contains("pub(crate) struct OpenIndexedPath"));
        assert!(!index_source.contains(
            "#[cfg(test)]\n#[derive(Clone, Debug, Eq, PartialEq)]\npub(crate) struct OpenIndexedPath"
        ));
        assert!(index_source.contains("pub(crate) fn execute_indexed_path("));
    }
}
