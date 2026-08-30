use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) const MAX_ENTRIES: usize = 20;
pub(crate) const MAX_IMAGE_PNG_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_TOTAL_IMAGE_PNG_BYTES: usize = 100 * 1024 * 1024;
pub(crate) const THUMBNAIL_MAX_EDGE: u32 = 256;
pub(crate) const MAX_THUMBNAIL_PNG_BYTES: usize = 256 * 1024;

pub(super) const INDEX_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardCapture {
    Text {
        text: String,
        captured_at: String,
    },
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        captured_at: String,
    },
    Files {
        paths: Vec<PathBuf>,
        captured_at: String,
    },
}

impl ClipboardCapture {
    pub(crate) fn text(text: impl Into<String>, captured_at: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            captured_at: captured_at.into(),
        }
    }

    pub(crate) fn image(
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        captured_at: impl Into<String>,
    ) -> Self {
        Self::Image {
            rgba,
            width,
            height,
            captured_at: captured_at.into(),
        }
    }

    pub(crate) fn files(paths: Vec<PathBuf>, captured_at: impl Into<String>) -> Self {
        Self::Files {
            paths,
            captured_at: captured_at.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum IgnoredCaptureReason {
    EmptyFileList,
    InvalidImage,
    ImageTooLarge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CaptureOutcome {
    Stored { id: String, revision: String },
    MovedToFront { id: String, revision: String },
    Unchanged { id: String, revision: String },
    Ignored { reason: IgnoredCaptureReason },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ClipboardHistoryEntrySummary {
    Text {
        id: String,
        captured_at: String,
        text_preview: String,
    },
    Image {
        id: String,
        captured_at: String,
        preview_data_url: String,
        width: u32,
        height: u32,
    },
    Files {
        id: String,
        captured_at: String,
        first_file_name: String,
        file_count: usize,
        available: bool,
    },
}

impl ClipboardHistoryEntrySummary {
    pub(crate) fn id(&self) -> String {
        match self {
            Self::Text { id, .. } | Self::Image { id, .. } | Self::Files { id, .. } => id.clone(),
        }
    }

    pub(crate) fn captured_at(&self) -> &str {
        match self {
            Self::Text { captured_at, .. }
            | Self::Image { captured_at, .. }
            | Self::Files { captured_at, .. } => captured_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardHistorySnapshot {
    pub(crate) revision: String,
    pub(crate) entries: Vec<ClipboardHistoryEntrySummary>,
}

impl Default for ClipboardHistorySnapshot {
    fn default() -> Self {
        Self {
            revision: "0".into(),
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardHistoryError {
    InvalidCapture,
    Storage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardHistoryBridgeError {
    PermissionDenied,
    ExpiredPanelSession,
    Unavailable,
}

impl ClipboardHistoryBridgeError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "PermissionDenied",
            Self::ExpiredPanelSession => "ExpiredPanelSession",
            Self::Unavailable => "ClipboardHistoryUnavailable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClipboardHistoryRecord {
    pub(crate) id: String,
    pub(crate) captured_at: String,
    pub(crate) payload: ClipboardHistoryRecordPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardHistoryRecordPayload {
    Text {
        text: String,
    },
    Image {
        png: Vec<u8>,
        width: u32,
        height: u32,
    },
    Files {
        paths: Vec<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardHistoryPasteError {
    PermissionDenied,
    ExpiredPanelSession,
    RecordNotFound,
    RecordUnavailable,
    PasteTargetUnavailable,
    ClipboardWriteFailed,
}

impl ClipboardHistoryPasteError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "PermissionDenied",
            Self::ExpiredPanelSession => "ExpiredPanelSession",
            Self::RecordNotFound => "RecordNotFound",
            Self::RecordUnavailable => "RecordUnavailable",
            Self::PasteTargetUnavailable => "PasteTargetUnavailable",
            Self::ClipboardWriteFailed => "ClipboardWriteFailed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardHistoryPasteOutcome {
    pub(crate) outcome: ClipboardHistoryPasteStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ClipboardHistoryPasteStatus {
    Admitted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HistoryDocument {
    pub(super) schema: u32,
    pub(super) revision: String,
    pub(super) next_id: String,
    pub(super) next_recency_rank: String,
    pub(super) entries: Vec<HistoryEntry>,
}

impl Default for HistoryDocument {
    fn default() -> Self {
        Self {
            schema: INDEX_SCHEMA,
            revision: "0".into(),
            next_id: "1".into(),
            next_recency_rank: "1".into(),
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HistoryEntry {
    pub(super) id: String,
    pub(super) captured_at: String,
    pub(super) recency_rank: String,
    pub(super) fingerprint: String,
    pub(super) payload: HistoryEntryPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum HistoryEntryPayload {
    Text {
        text: String,
        text_preview: String,
    },
    Image {
        width: u32,
        height: u32,
        png_file: String,
        png_bytes: u64,
        thumbnail_data_url: String,
        thumbnail_width: u32,
        thumbnail_height: u32,
    },
    Files {
        paths: Vec<PathBuf>,
    },
}
