use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use crate::atomic_file::{
    commit_with_backup, quarantine_invalid, read_optional, replace_current, AtomicPaths,
};

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

        if let Some(index) = candidate
            .entries
            .iter()
            .position(|entry| entry.fingerprint == prepared.fingerprint)
        {
            let id = candidate.entries[index].id.clone();
            if index == 0 {
                return Ok(CaptureOutcome::Unchanged {
                    id,
                    revision: candidate.revision,
                });
            }
            candidate.entries[index].recency_rank =
                take_next_decimal(&mut candidate.next_recency_rank)?;
            increment_revision(&mut candidate)?;
            sort_entries(&mut candidate);
            self.persist_candidate(&mut state, candidate, None, Vec::new())?;
            return Ok(CaptureOutcome::MovedToFront {
                id,
                revision: state.document.revision.clone(),
            });
        }

        let id = take_next_decimal(&mut candidate.next_id)?;
        let mut payload = prepared.payload;
        let image_file = if let HistoryEntryPayload::Image { png_file, .. } = &mut payload {
            let file = format!("{id}.png");
            *png_file = file.clone();
            Some(file)
        } else {
            None
        };
        candidate.entries.push(HistoryEntry {
            id: id.clone(),
            captured_at: prepared.captured_at,
            recency_rank: take_next_decimal(&mut candidate.next_recency_rank)?,
            fingerprint: prepared.fingerprint,
            payload,
        });
        increment_revision(&mut candidate)?;
        sort_entries(&mut candidate);
        let obsolete = enforce_capacity(&mut candidate)?;
        let new_image = image_file.zip(prepared.image_bytes);
        self.persist_candidate(&mut state, candidate, new_image, obsolete)?;
        Ok(CaptureOutcome::Stored {
            id,
            revision: state.document.revision.clone(),
        })
    }

    pub(crate) fn snapshot(&self) -> Result<ClipboardHistorySnapshot, ClipboardHistoryError> {
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

fn parse_decimal(value: &str) -> Result<u64, ()> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return Err(());
    }
    value.parse::<u64>().map_err(|_| ())
}

fn take_next_decimal(value: &mut String) -> Result<String, ClipboardHistoryError> {
    let current = parse_decimal(value).map_err(|_| ClipboardHistoryError::Storage)?;
    let next = current
        .checked_add(1)
        .ok_or(ClipboardHistoryError::Storage)?;
    *value = next.to_string();
    Ok(current.to_string())
}

fn increment_revision(document: &mut HistoryDocument) -> Result<(), ClipboardHistoryError> {
    let revision = parse_decimal(&document.revision).map_err(|_| ClipboardHistoryError::Storage)?;
    document.revision = revision
        .checked_add(1)
        .ok_or(ClipboardHistoryError::Storage)?
        .to_string();
    Ok(())
}

fn sort_entries(document: &mut HistoryDocument) {
    document.entries.sort_by(|left, right| {
        let left_rank = parse_decimal(&left.recency_rank).unwrap_or(0);
        let right_rank = parse_decimal(&right.recency_rank).unwrap_or(0);
        right_rank.cmp(&left_rank)
    });
}

fn enforce_capacity(document: &mut HistoryDocument) -> Result<Vec<String>, ClipboardHistoryError> {
    let mut obsolete = Vec::new();
    loop {
        let total_image_bytes = total_image_bytes(&document.entries)?;
        if document.entries.len() <= MAX_ENTRIES
            && total_image_bytes <= MAX_TOTAL_IMAGE_PNG_BYTES as u64
        {
            break;
        }
        if let Some(entry) = document.entries.pop() {
            obsolete.extend(image_files(&[entry]));
        } else {
            break;
        }
    }
    Ok(obsolete)
}

fn total_image_bytes(entries: &[HistoryEntry]) -> Result<u64, ClipboardHistoryError> {
    entries.iter().try_fold(0_u64, |total, entry| {
        let bytes = match &entry.payload {
            HistoryEntryPayload::Image { png_bytes, .. } => *png_bytes,
            _ => 0,
        };
        total
            .checked_add(bytes)
            .ok_or(ClipboardHistoryError::Storage)
    })
}

fn image_files(entries: &[HistoryEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match &entry.payload {
            HistoryEntryPayload::Image { png_file, .. } => Some(png_file.clone()),
            _ => None,
        })
        .collect()
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
