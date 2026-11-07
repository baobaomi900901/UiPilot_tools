use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use crate::atomic_file::{
    commit_with_backup, quarantine_invalid, read_optional, replace_current, AtomicPaths,
};

use super::document::{parse_decimal, take_next_decimal, increment_revision, sort_entries, image_files};

use super::{
    model::{
        CaptureOutcome, ClipboardCapture, ClipboardHistoryEntrySummary, ClipboardHistoryError,
        ClipboardHistoryPasteError, ClipboardHistoryRecord, ClipboardHistoryRecordPayload,
        ClipboardHistorySnapshot, HistoryDocument, HistoryEntry, HistoryEntryPayload,
        IgnoredCaptureReason, INDEX_SCHEMA, MAX_ENTRIES, MAX_IMAGE_PNG_BYTES,
        MAX_TOTAL_IMAGE_PNG_BYTES,
    },
    preview::{
        files_fingerprint, image_fingerprint, prepare_image, text_fingerprint, text_preview,
    },
};

struct ReadyStore {
    document: HistoryDocument,
    raw: Option<Vec<u8>>,
}

pub(crate) struct ClipboardHistoryStore {
    root: PathBuf,
    images_root: PathBuf,
    paths: AtomicPaths,
    state: Mutex<ReadyStore>,
}

struct PreparedCapture {
    fingerprint: String,
    captured_at: String,
    payload: HistoryEntryPayload,
    image_bytes: Option<Vec<u8>>,
}

enum PreparedCaptureOutcome {
    Ready(PreparedCapture),
    Ignored(IgnoredCaptureReason),
}

impl ClipboardHistoryStore {
    pub(crate) fn load(root: &Path) -> Result<Self, ClipboardHistoryError> {
        fs::create_dir_all(root).map_err(|_| ClipboardHistoryError::Storage)?;
        let images_root = root.join("images");
        fs::create_dir_all(&images_root).map_err(|_| ClipboardHistoryError::Storage)?;
        let paths = AtomicPaths::new(root, "index.json");
        let ready = match load_document(&paths) {
            Ok(ready) => ready,
            Err(()) => {
                let _ = quarantine_invalid(paths.current());
                ReadyStore {
                    document: HistoryDocument::default(),
                    raw: None,
                }
            }
        };
        Ok(Self {
            root: root.to_path_buf(),
            images_root,
            paths,
            state: Mutex::new(ready),
        })
    }

    pub(crate) fn capture(
        &self,
        capture: ClipboardCapture,
    ) -> Result<CaptureOutcome, ClipboardHistoryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
        let mut candidate = state.document.clone();
        let prepared = match self.prepare_capture(capture)? {
            PreparedCaptureOutcome::Ready(prepared) => prepared,
            PreparedCaptureOutcome::Ignored(reason) => {
                return Ok(CaptureOutcome::Ignored { reason })
            }
        };

        let mutation = super::document::capture(&mut candidate, prepared.fingerprint,
            prepared.captured_at, prepared.payload)?;
        if matches!(&mutation.outcome, CaptureOutcome::Unchanged { .. }) {
            return Ok(mutation.outcome);
        }
        let new_image = match &mutation.outcome {
            CaptureOutcome::Stored { id, .. } => prepared.image_bytes.map(|bytes| (format!("{id}.png"), bytes)),
            _ => None,
        };
        self.persist_candidate(&mut state, candidate, new_image, mutation.obsolete_images)?;
        Ok(mutation.outcome)
    }

    pub(crate) fn snapshot(&self) -> Result<ClipboardHistorySnapshot, ClipboardHistoryError> {
        self.snapshot_query(None)
    }

    pub(crate) fn snapshot_query(
        &self,
        query: Option<&str>,
    ) -> Result<ClipboardHistorySnapshot, ClipboardHistoryError> {
        if query.is_some_and(|query| query.chars().count() > 512) {
            return Err(ClipboardHistoryError::InvalidQuery);
        }
        let query = query.unwrap_or("").trim().to_lowercase();
        let state = self
            .state
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
        Ok(ClipboardHistorySnapshot {
            revision: state.document.revision.clone(),
            entries: state
                .document
                .entries
                .iter()
                .filter(|entry| {
                    query.is_empty()
                        || match &entry.payload {
                            HistoryEntryPayload::Text { text, .. } => {
                                text.to_lowercase().contains(&query)
                            }
                            HistoryEntryPayload::Files { paths } => paths.iter().any(|path| {
                                path.file_name().is_some_and(|name| {
                                    name.to_string_lossy().to_lowercase().contains(&query)
                                })
                            }),
                            HistoryEntryPayload::Image { .. } => false,
                        }
                })
                .map(|entry| summary(entry))
                .collect(),
        })
    }

    pub(crate) fn record_for_paste(
        &self,
        id: &str,
    ) -> Result<ClipboardHistoryRecord, ClipboardHistoryPasteError> {
        let entry = {
            let state = self
                .state
                .lock()
                .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?;
            state
                .document
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .cloned()
                .ok_or(ClipboardHistoryPasteError::RecordNotFound)?
        };
        let payload = match entry.payload {
            HistoryEntryPayload::Text { text, .. } => ClipboardHistoryRecordPayload::Text { text },
            HistoryEntryPayload::Image {
                width,
                height,
                png_file,
                ..
            } => {
                let path = self
                    .image_path_from_index(&png_file)
                    .ok_or(ClipboardHistoryPasteError::RecordUnavailable)?;
                let png =
                    fs::read(path).map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?;
                if png.is_empty() {
                    return Err(ClipboardHistoryPasteError::RecordUnavailable);
                }
                ClipboardHistoryRecordPayload::Image { png, width, height }
            }
            HistoryEntryPayload::Files { paths } => {
                if paths.is_empty() || !paths.iter().all(path_available) {
                    return Err(ClipboardHistoryPasteError::RecordUnavailable);
                }
                ClipboardHistoryRecordPayload::Files { paths }
            }
        };
        Ok(ClipboardHistoryRecord {
            id: entry.id,
            captured_at: entry.captured_at,
            payload,
        })
    }

    pub(crate) fn remove(&self, id: &str) -> Result<bool, ClipboardHistoryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
        let Some(index) = state
            .document
            .entries
            .iter()
            .position(|entry| entry.id == id)
        else {
            return Ok(false);
        };
        let mut candidate = state.document.clone();
        let removed = candidate.entries.remove(index);
        increment_revision(&mut candidate)?;
        self.persist_candidate(&mut state, candidate, None, image_files(&[removed]))?;
        Ok(true)
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&self) -> Result<(), ClipboardHistoryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
        let mut candidate = state.document.clone();
        let obsolete = image_files(&candidate.entries);
        candidate.entries.clear();
        increment_revision(&mut candidate)?;
        self.persist_candidate(&mut state, candidate, None, obsolete)
    }

    #[allow(dead_code)]
    pub(crate) fn move_to_front(&self, id: &str) -> Result<bool, ClipboardHistoryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
        let Some(index) = state
            .document
            .entries
            .iter()
            .position(|entry| entry.id == id)
        else {
            return Ok(false);
        };
        if index == 0 {
            return Ok(true);
        }
        let mut candidate = state.document.clone();
        candidate.entries[index].recency_rank =
            take_next_decimal(&mut candidate.next_recency_rank)?;
        increment_revision(&mut candidate)?;
        sort_entries(&mut candidate);
        self.persist_candidate(&mut state, candidate, None, Vec::new())?;
        Ok(true)
    }

    fn prepare_capture(
        &self,
        capture: ClipboardCapture,
    ) -> Result<PreparedCaptureOutcome, ClipboardHistoryError> {
        match capture {
            ClipboardCapture::Text { text, captured_at } => {
                Ok(PreparedCaptureOutcome::Ready(PreparedCapture {
                    fingerprint: text_fingerprint(&text),
                    captured_at,
                    payload: HistoryEntryPayload::Text {
                        text_preview: text_preview(&text),
                        text,
                    },
                    image_bytes: None,
                }))
            }
            ClipboardCapture::Image {
                rgba,
                width,
                height,
                captured_at,
            } => {
                let prepared = prepare_image(width, height, &rgba)?;
                if prepared.png.len() > MAX_IMAGE_PNG_BYTES {
                    return Ok(PreparedCaptureOutcome::Ignored(
                        IgnoredCaptureReason::ImageTooLarge,
                    ));
                }
                Ok(PreparedCaptureOutcome::Ready(PreparedCapture {
                    fingerprint: image_fingerprint(width, height, &rgba),
                    captured_at,
                    payload: HistoryEntryPayload::Image {
                        width,
                        height,
                        png_file: String::new(),
                        png_bytes: prepared.png.len() as u64,
                        thumbnail_data_url: prepared.thumbnail_data_url,
                        thumbnail_width: prepared.thumbnail_width,
                        thumbnail_height: prepared.thumbnail_height,
                    },
                    image_bytes: Some(prepared.png),
                }))
            }
            ClipboardCapture::Files { paths, captured_at } => {
                if paths.is_empty() {
                    return Ok(PreparedCaptureOutcome::Ignored(
                        IgnoredCaptureReason::EmptyFileList,
                    ));
                }
                Ok(PreparedCaptureOutcome::Ready(PreparedCapture {
                    fingerprint: files_fingerprint(&paths),
                    captured_at,
                    payload: HistoryEntryPayload::Files { paths },
                    image_bytes: None,
                }))
            }
        }
    }

    fn persist_candidate(
        &self,
        state: &mut ReadyStore,
        candidate: HistoryDocument,
        new_image: Option<(String, Vec<u8>)>,
        obsolete_images: Vec<String>,
    ) -> Result<(), ClipboardHistoryError> {
        if let Some((file, bytes)) = new_image.as_ref() {
            replace_current(&self.images_root.join(file), bytes)
                .map_err(|_| ClipboardHistoryError::Storage)?;
        }
        let raw = serde_json::to_vec(&candidate).map_err(|_| ClipboardHistoryError::Storage)?;
        if let Err(error) = commit_with_backup(&self.paths, state.raw.as_deref(), &raw) {
            if let Some((file, _)) = new_image.as_ref() {
                let _ = fs::remove_file(self.images_root.join(file));
            }
            return Err(match error {
                _ => ClipboardHistoryError::Storage,
            });
        }
        for file in obsolete_images {
            let _ = fs::remove_file(self.images_root.join(file));
        }
        state.document = candidate;
        state.raw = Some(raw);
        Ok(())
    }

    pub(crate) fn text_for_test(&self, id: &str) -> Option<String> {
        let state = self.state.lock().ok()?;
        state.document.entries.iter().find_map(|entry| {
            if entry.id != id {
                return None;
            }
            match &entry.payload {
                HistoryEntryPayload::Text { text, .. } => Some(text.clone()),
                _ => None,
            }
        })
    }

    pub(crate) fn image_path_for_test(&self, id: &str) -> Option<PathBuf> {
        let state = self.state.lock().ok()?;
        state
            .document
            .entries
            .iter()
            .find_map(|entry| {
                if entry.id != id {
                    return None;
                }
                match &entry.payload {
                    HistoryEntryPayload::Image { png_file, .. } => {
                        Some(self.images_root.join(png_file))
                    }
                    _ => None,
                }
            })
            .or_else(|| Some(self.images_root.join(format!("{id}.png"))))
    }

    fn image_path_from_index(&self, png_file: &str) -> Option<PathBuf> {
        let mut components = Path::new(png_file).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) if !name.is_empty() => {
                Some(self.images_root.join(name))
            }
            _ => None,
        }
    }

    pub(crate) fn thumbnail_dimensions_for_test(&self, id: &str) -> Option<(u32, u32)> {
        let state = self.state.lock().ok()?;
        state.document.entries.iter().find_map(|entry| {
            if entry.id != id {
                return None;
            }
            match &entry.payload {
                HistoryEntryPayload::Image {
                    thumbnail_width,
                    thumbnail_height,
                    ..
                } => Some((*thumbnail_width, *thumbnail_height)),
                _ => None,
            }
        })
    }
}

fn load_document(paths: &AtomicPaths) -> Result<ReadyStore, ()> {
    let Some(raw) = read_optional(paths.current()).map_err(|_| ())? else {
        return Ok(ReadyStore {
            document: HistoryDocument::default(),
            raw: None,
        });
    };
    let mut document: HistoryDocument = serde_json::from_slice(&raw).map_err(|_| ())?;
    validate_document(&mut document)?;
    Ok(ReadyStore {
        document,
        raw: Some(raw),
    })
}

fn validate_document(document: &mut HistoryDocument) -> Result<(), ()> {
    if document.schema != INDEX_SCHEMA {
        return Err(());
    }
    parse_decimal(&document.revision)?;
    parse_decimal(&document.next_id)?;
    parse_decimal(&document.next_recency_rank)?;
    if document.entries.len() > MAX_ENTRIES {
        return Err(());
    }
    for entry in &document.entries {
        parse_decimal(&entry.id)?;
        parse_decimal(&entry.recency_rank)?;
        if entry.fingerprint.len() != 64
            || !entry
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(());
        }
    }
    sort_entries(document);
    Ok(())
}

fn summary(entry: &HistoryEntry) -> ClipboardHistoryEntrySummary {
    match &entry.payload {
        HistoryEntryPayload::Text { text_preview, .. } => ClipboardHistoryEntrySummary::Text {
            id: entry.id.clone(),
            captured_at: entry.captured_at.clone(),
            text_preview: text_preview.clone(),
        },
        HistoryEntryPayload::Image {
            width,
            height,
            thumbnail_data_url,
            ..
        } => ClipboardHistoryEntrySummary::Image {
            id: entry.id.clone(),
            captured_at: entry.captured_at.clone(),
            preview_data_url: thumbnail_data_url.clone(),
            width: *width,
            height: *height,
        },
        HistoryEntryPayload::Files { paths } => ClipboardHistoryEntrySummary::Files {
            id: entry.id.clone(),
            captured_at: entry.captured_at.clone(),
            first_file_name: paths
                .first()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            file_count: paths.len(),
            available: paths.iter().all(path_available),
        },
    }
}

fn path_available(path: &PathBuf) -> bool {
    match fs::metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound && path.exists(),
    }
}
