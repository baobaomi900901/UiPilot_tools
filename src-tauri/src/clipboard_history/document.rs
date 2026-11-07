//! Pure mutations shared by Windows and macOS persistence adapters.
//! Call only on a disposable candidate clone: an overflow may follow partial
//! candidate changes. The persistence adapter publishes only successful results.
use super::{model::{HistoryDocument, HistoryEntry, HistoryEntryPayload, MAX_ENTRIES,
    MAX_TOTAL_IMAGE_PNG_BYTES}, CaptureOutcome, ClipboardHistoryError, ClipboardHistoryEntrySummary};
use super::preview::{text_fingerprint, text_preview};

pub(super) struct Mutation {
    pub(super) outcome: CaptureOutcome,
    pub(super) obsolete_images: Vec<String>,
}

pub(super) fn capture_text(document: &mut HistoryDocument, text: String, captured_at: String)
    -> Result<Mutation, ClipboardHistoryError> {
    capture(document, text_fingerprint(&text), captured_at,
        HistoryEntryPayload::Text { text_preview: text_preview(&text), text })
}

pub(super) fn capture(document: &mut HistoryDocument, fingerprint: String,
    captured_at: String, mut payload: HistoryEntryPayload) -> Result<Mutation, ClipboardHistoryError> {
    if let Some(index) = document.entries.iter().position(|entry| entry.fingerprint == fingerprint) {
        let id = document.entries[index].id.clone();
        if index == 0 {
            return Ok(Mutation { outcome: CaptureOutcome::Unchanged { id, revision: document.revision.clone() }, obsolete_images: Vec::new() });
        }
        document.entries[index].recency_rank = take_next_decimal(&mut document.next_recency_rank)?;
        increment_revision(document)?;
        sort_entries(document);
        return Ok(Mutation { outcome: CaptureOutcome::MovedToFront { id, revision: document.revision.clone() }, obsolete_images: Vec::new() });
    }
    let id = take_next_decimal(&mut document.next_id)?;
    if let HistoryEntryPayload::Image { png_file, .. } = &mut payload { *png_file = format!("{id}.png"); }
    document.entries.push(HistoryEntry { id: id.clone(), captured_at,
        recency_rank: take_next_decimal(&mut document.next_recency_rank)?, fingerprint, payload });
    increment_revision(document)?;
    sort_entries(document);
    let obsolete_images = enforce_capacity(document)?;
    Ok(Mutation { outcome: CaptureOutcome::Stored { id, revision: document.revision.clone() }, obsolete_images })
}

pub(super) fn remove(document: &mut HistoryDocument, id: &str) -> Result<bool, ClipboardHistoryError> {
    let Some(index) = document.entries.iter().position(|entry| entry.id == id) else { return Ok(false); };
    document.entries.remove(index);
    increment_revision(document)?;
    Ok(true)
}
pub(super) fn clear(document: &mut HistoryDocument) -> Result<(), ClipboardHistoryError> {
    document.entries.clear();
    increment_revision(document)
}

pub(super) fn text_summary(entry: &HistoryEntry) -> Option<ClipboardHistoryEntrySummary> {
    let HistoryEntryPayload::Text { text_preview, .. } = &entry.payload else { return None; };
    Some(ClipboardHistoryEntrySummary::Text { id: entry.id.clone(), captured_at: entry.captured_at.clone(), text_preview: text_preview.clone() })
}

pub(super) fn parse_decimal(value: &str) -> Result<u64, ()> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return Err(());
    }
    value.parse::<u64>().map_err(|_| ())
}

pub(super) fn take_next_decimal(value: &mut String) -> Result<String, ClipboardHistoryError> {
    let current = parse_decimal(value).map_err(|_| ClipboardHistoryError::Storage)?;
    let next = current
        .checked_add(1)
        .ok_or(ClipboardHistoryError::Storage)?;
    *value = next.to_string();
    Ok(current.to_string())
}

pub(super) fn increment_revision(document: &mut HistoryDocument) -> Result<(), ClipboardHistoryError> {
    let revision = parse_decimal(&document.revision).map_err(|_| ClipboardHistoryError::Storage)?;
    document.revision = revision
        .checked_add(1)
        .ok_or(ClipboardHistoryError::Storage)?
        .to_string();
    Ok(())
}

pub(super) fn sort_entries(document: &mut HistoryDocument) {
    document.entries.sort_by(|left, right| {
        let left_rank = parse_decimal(&left.recency_rank).unwrap_or(0);
        let right_rank = parse_decimal(&right.recency_rank).unwrap_or(0);
        right_rank.cmp(&left_rank)
    });
}

pub(super) fn enforce_capacity(document: &mut HistoryDocument) -> Result<Vec<String>, ClipboardHistoryError> {
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

pub(super) fn total_image_bytes(entries: &[HistoryEntry]) -> Result<u64, ClipboardHistoryError> {
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

pub(super) fn image_files(entries: &[HistoryEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match &entry.payload {
            HistoryEntryPayload::Image { png_file, .. } => Some(png_file.clone()),
            _ => None,
        })
        .collect()
}


#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
