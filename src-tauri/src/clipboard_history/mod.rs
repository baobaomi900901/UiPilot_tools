#![allow(dead_code, unused_imports)]
// Task 3 lands the host-owned clipboard history core before Task 4 wires it into
// the public plugin lifecycle service. Keep the core warning-clean while it is
// intentionally unused by non-test code.

mod model;
mod preview;
mod store;

pub(crate) use model::{
    CaptureOutcome, ClipboardCapture, ClipboardHistoryEntrySummary, ClipboardHistorySnapshot,
    IgnoredCaptureReason,
};
pub(crate) use store::ClipboardHistoryStore;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::clipboard_history::{
        model::{MAX_ENTRIES, MAX_THUMBNAIL_PNG_BYTES, THUMBNAIL_MAX_EDGE},
        preview::decode_data_url_for_test,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-clipboard-history-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn capture_text(store: &ClipboardHistoryStore, text: &str, captured_at: &str) -> String {
        match store
            .capture(ClipboardCapture::text(text, captured_at))
            .unwrap()
        {
            CaptureOutcome::Stored { id, .. } | CaptureOutcome::MovedToFront { id, .. } => id,
            other => panic!("expected stored text capture, got {other:?}"),
        }
    }

    fn rgba_noise(width: u32, height: u32) -> Vec<u8> {
        let mut seed = 0x1234_5678_u32;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            pixels.push((seed >> 24) as u8);
            pixels.push((seed >> 16) as u8);
            pixels.push((seed >> 8) as u8);
            pixels.push(0xff);
        }
        pixels
    }

    #[test]
    fn text_capture_redacts_preview_and_keeps_restorable_content() {
        let dir = TestDir::new("text-preview");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        let long_text = format!("  第一行\n\t第二行  {}", "界".repeat(130));
        let id = capture_text(&store, &long_text, "2026-08-30T01:00:00Z");

        let snapshot = store.snapshot().unwrap();
        match &snapshot.entries[0] {
            ClipboardHistoryEntrySummary::Text { text_preview, .. } => {
                assert!(text_preview.chars().count() <= 120);
                assert!(!text_preview.contains('\n'));
                assert!(!text_preview.contains('\t'));
                assert!(!text_preview.contains(&"界".repeat(121)));
            }
            other => panic!("expected text summary, got {other:?}"),
        }
        assert_eq!(store.text_for_test(&id).unwrap(), long_text);

        capture_text(&store, " \n\t  ", "2026-08-30T01:00:01Z");
        match &store.snapshot().unwrap().entries[0] {
            ClipboardHistoryEntrySummary::Text { text_preview, .. } => {
                assert_eq!(text_preview, "");
            }
            other => panic!("expected text summary, got {other:?}"),
        }
    }

    #[test]
    fn fingerprints_dedupe_and_move_existing_without_changing_captured_at() {
        let dir = TestDir::new("dedupe");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        let alpha = capture_text(&store, "alpha", "2026-08-30T01:00:00Z");
        let beta = capture_text(&store, "beta", "2026-08-30T01:00:01Z");

        let moved = store
            .capture(ClipboardCapture::text("alpha", "2026-08-30T01:00:02Z"))
            .unwrap();
        assert_eq!(
            moved,
            CaptureOutcome::MovedToFront {
                id: alpha.clone(),
                revision: "3".into()
            }
        );

        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.revision, "3");
        assert_eq!(snapshot.entries[0].id(), alpha);
        assert_eq!(snapshot.entries[1].id(), beta);
        assert_eq!(snapshot.entries[0].captured_at(), "2026-08-30T01:00:00Z");

        let duplicate = store
            .capture(ClipboardCapture::text("alpha", "2026-08-30T01:00:03Z"))
            .unwrap();
        assert_eq!(
            duplicate,
            CaptureOutcome::Unchanged {
                id: alpha,
                revision: "3".into()
            }
        );
    }

    #[test]
    fn ids_survive_restart_and_deleted_ids_are_not_reused() {
        let dir = TestDir::new("restart");
        let first = {
            let store = ClipboardHistoryStore::load(dir.path()).unwrap();
            capture_text(&store, "one", "2026-08-30T01:00:00Z")
        };

        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        assert_eq!(store.snapshot().unwrap().entries[0].id(), first);
        assert!(store.remove(&first).unwrap());
        let second = capture_text(&store, "two", "2026-08-30T01:00:01Z");
        assert_ne!(second, first);
    }

    #[test]
    fn capacity_keeps_twenty_most_recent_entries() {
        let dir = TestDir::new("capacity");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        let mut ids = Vec::new();
        for index in 0..25 {
            ids.push(capture_text(
                &store,
                &format!("text-{index}"),
                &format!("2026-08-30T01:00:{index:02}Z"),
            ));
        }

        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.entries.len(), MAX_ENTRIES);
        assert_eq!(snapshot.entries[0].id(), ids[24]);
        assert_eq!(snapshot.entries.last().unwrap().id(), ids[5]);
        assert!(!snapshot.entries.iter().any(|entry| entry.id() == ids[0]));
    }

    #[test]
    fn file_summaries_redact_paths_preserve_order_and_report_availability() {
        let dir = TestDir::new("files");
        let source = dir.path().join("source");
        fs::create_dir_all(&source).unwrap();
        let one = source.join("one.txt");
        let two = source.join("two.txt");
        fs::write(&one, "one").unwrap();
        fs::write(&two, "two").unwrap();

        let store = ClipboardHistoryStore::load(dir.path().join("history").as_path()).unwrap();
        let first = match store
            .capture(ClipboardCapture::files(
                vec![one.clone(), two.clone()],
                "2026-08-30T01:00:00Z",
            ))
            .unwrap()
        {
            CaptureOutcome::Stored { id, .. } => id,
            other => panic!("expected stored files capture, got {other:?}"),
        };
        let second = match store
            .capture(ClipboardCapture::files(
                vec![two.clone(), one.clone()],
                "2026-08-30T01:00:01Z",
            ))
            .unwrap()
        {
            CaptureOutcome::Stored { id, .. } => id,
            other => panic!("expected ordered files to be distinct, got {other:?}"),
        };
        assert_ne!(first, second);

        let snapshot = store.snapshot().unwrap();
        match &snapshot.entries[0] {
            ClipboardHistoryEntrySummary::Files {
                first_file_name,
                file_count,
                available,
                ..
            } => {
                assert_eq!(first_file_name, "two.txt");
                assert_eq!(*file_count, 2);
                assert!(*available);
                assert!(!first_file_name.contains(source.to_string_lossy().as_ref()));
            }
            other => panic!("expected files summary, got {other:?}"),
        }

        fs::remove_file(two).unwrap();
        match &store.snapshot().unwrap().entries[0] {
            ClipboardHistoryEntrySummary::Files { available, .. } => assert!(!*available),
            other => panic!("expected files summary, got {other:?}"),
        }
    }

    #[test]
    fn image_capture_stores_png_thumbnail_and_eviction_removes_files() {
        let dir = TestDir::new("images");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        let id = match store
            .capture(ClipboardCapture::image(
                rgba_noise(300, 180),
                300,
                180,
                "2026-08-30T01:00:00Z",
            ))
            .unwrap()
        {
            CaptureOutcome::Stored { id, .. } => id,
            other => panic!("expected stored image capture, got {other:?}"),
        };

        let snapshot = store.snapshot().unwrap();
        match &snapshot.entries[0] {
            ClipboardHistoryEntrySummary::Image {
                preview_data_url,
                width,
                height,
                ..
            } => {
                assert_eq!((*width, *height), (300, 180));
                let png = decode_data_url_for_test(preview_data_url);
                assert!(png.len() <= MAX_THUMBNAIL_PNG_BYTES);
                let (thumbnail_width, thumbnail_height) =
                    store.thumbnail_dimensions_for_test(&id).unwrap();
                assert!(thumbnail_width <= THUMBNAIL_MAX_EDGE);
                assert!(thumbnail_height <= THUMBNAIL_MAX_EDGE);
            }
            other => panic!("expected image summary, got {other:?}"),
        }
        assert!(store.image_path_for_test(&id).unwrap().exists());

        for index in 1..=20 {
            store
                .capture(ClipboardCapture::image(
                    vec![index as u8, 0, 0, 0xff],
                    1,
                    1,
                    &format!("2026-08-30T01:00:{index:02}Z"),
                ))
                .unwrap();
        }
        assert_eq!(store.snapshot().unwrap().entries.len(), MAX_ENTRIES);
        assert!(!store.image_path_for_test(&id).unwrap().exists());
    }

    #[test]
    fn oversized_images_are_ignored() {
        let dir = TestDir::new("oversized-image");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        let outcome = store
            .capture(ClipboardCapture::image(
                rgba_noise(2200, 1800),
                2200,
                1800,
                "2026-08-30T01:00:00Z",
            ))
            .unwrap();
        assert_eq!(
            outcome,
            CaptureOutcome::Ignored {
                reason: IgnoredCaptureReason::ImageTooLarge
            }
        );
        assert!(store.snapshot().unwrap().entries.is_empty());
    }

    #[test]
    fn corrupted_index_is_quarantined_and_starts_empty() {
        let dir = TestDir::new("corrupt");
        fs::write(dir.path().join("index.json"), b"{not-json").unwrap();
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();

        assert_eq!(
            store.snapshot().unwrap(),
            ClipboardHistorySnapshot::default()
        );
        assert!(!dir.path().join("index.json").exists());
        assert!(fs::read_dir(dir.path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("index.json.invalid-")));
    }
}
