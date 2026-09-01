#![allow(dead_code, unused_imports)]
// Task 3 lands the host-owned clipboard history core before Task 4 wires it into
// the public plugin lifecycle service. Keep the core warning-clean while it is
// intentionally unused by non-test code.

mod model;
mod observer;
mod paste;
mod preview;
mod service;
mod store;

pub(crate) use model::{
    CaptureOutcome, ClipboardCapture, ClipboardHistoryBridgeError, ClipboardHistoryEntrySummary,
    ClipboardHistoryError, ClipboardHistoryPasteError, ClipboardHistoryPasteOutcome,
    ClipboardHistoryPasteStatus, ClipboardHistoryRecord, ClipboardHistoryRecordPayload,
    ClipboardHistorySnapshot, IgnoredCaptureReason,
};
pub(crate) use observer::{
    normalize_clipboard_formats, ClipboardFormatSnapshot, ClipboardImageSnapshot,
    ClipboardObserver, ClipboardObserverHandle, ClipboardReadError, ClipboardReader,
};
pub(crate) use paste::{
    paste_clipboard_history_record, send_ctrl_v_to_foreground_target, ClipboardHistoryPasteDriver,
    ClipboardHistoryPasteWrite,
};
pub(crate) use service::ClipboardHistoryService;
pub(crate) use store::ClipboardHistoryStore;

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use super::*;
    use crate::clipboard_history::{
        model::{
            MAX_ENTRIES, MAX_THUMBNAIL_PNG_BYTES, MAX_TOTAL_IMAGE_PNG_BYTES, THUMBNAIL_MAX_EDGE,
        },
        preview::decode_data_url_for_test,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct ScriptedClipboardReader {
        outcomes: Mutex<VecDeque<Result<Option<ClipboardCapture>, ClipboardReadError>>>,
        calls: AtomicUsize,
    }

    impl ScriptedClipboardReader {
        fn push(&self, outcome: Result<Option<ClipboardCapture>, ClipboardReadError>) {
            self.outcomes.lock().unwrap().push_back(outcome);
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl ClipboardReader for ScriptedClipboardReader {
        fn read_capture(&self) -> Result<Option<ClipboardCapture>, ClipboardReadError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(None))
        }
    }

    #[derive(Default)]
    struct ManualClipboardObserver {
        starts: AtomicUsize,
        stops: AtomicUsize,
        callback: Mutex<Option<Arc<dyn Fn() + Send + Sync + 'static>>>,
    }

    impl ManualClipboardObserver {
        fn trigger(&self) {
            let callback = self.callback.lock().unwrap().clone();
            if let Some(callback) = callback {
                callback();
            }
        }

        fn starts(&self) -> usize {
            self.starts.load(Ordering::SeqCst)
        }

        fn stops(&self) -> usize {
            self.stops.load(Ordering::SeqCst)
        }
    }

    impl ClipboardObserver for Arc<ManualClipboardObserver> {
        fn start(
            &self,
            callback: Arc<dyn Fn() + Send + Sync + 'static>,
        ) -> Result<Box<dyn ClipboardObserverHandle>, ClipboardHistoryError> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            *self.callback.lock().unwrap() = Some(callback);
            Ok(Box::new(ManualClipboardObserverHandle {
                observer: Arc::clone(self),
            }))
        }
    }

    struct ManualClipboardObserverHandle {
        observer: Arc<ManualClipboardObserver>,
    }

    impl ClipboardObserverHandle for ManualClipboardObserverHandle {
        fn stop(&self) {
            self.observer.stops.fetch_add(1, Ordering::SeqCst);
            *self.observer.callback.lock().unwrap() = None;
        }
    }

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

    fn history_service(
        dir: &TestDir,
        reader: Arc<ScriptedClipboardReader>,
        observer: Arc<ManualClipboardObserver>,
    ) -> Arc<ClipboardHistoryService> {
        ClipboardHistoryService::load_with_dependencies(
            dir.path(),
            reader,
            Arc::new(observer),
            Duration::ZERO,
        )
        .unwrap()
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
    fn snapshot_serializes_only_camel_case_summary_fields() {
        let dir = TestDir::new("snapshot-dto");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        capture_text(
            &store,
            "raw secret value that must not be serialized",
            "2026-08-30T01:00:00Z",
        );

        let value = serde_json::to_value(store.snapshot().unwrap()).unwrap();
        let entry = &value["entries"][0];
        assert_eq!(value["revision"], "1");
        assert_eq!(entry["kind"], "text");
        assert_eq!(entry["capturedAt"], "2026-08-30T01:00:00Z");
        assert_eq!(
            entry["textPreview"],
            "raw secret value that must not be serialized"
        );
        assert!(entry.get("text").is_none());
        assert!(entry.get("text_preview").is_none());
        assert!(entry.get("captured_at").is_none());
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
    fn capacity_keeps_fifty_most_recent_entries() {
        let dir = TestDir::new("capacity");
        let store = ClipboardHistoryStore::load(dir.path()).unwrap();
        assert_eq!(MAX_ENTRIES, 50);
        assert_eq!(MAX_TOTAL_IMAGE_PNG_BYTES, 500 * 1024 * 1024);
        let mut ids = Vec::new();
        for index in 0..55 {
            ids.push(capture_text(
                &store,
                &format!("text-{index}"),
                &format!("2026-08-30T01:00:{index:02}Z"),
            ));
        }

        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.entries.len(), MAX_ENTRIES);
        assert_eq!(snapshot.entries[0].id(), ids[54]);
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
    fn paste_payload_retrieval_keeps_content_host_side_and_reports_unavailable_records() {
        let dir = TestDir::new("paste-payload");
        let source = dir.path().join("source");
        fs::create_dir_all(&source).unwrap();
        let file = source.join("one.txt");
        fs::write(&file, "one").unwrap();
        let store = ClipboardHistoryStore::load(dir.path().join("history").as_path()).unwrap();

        let text_id = capture_text(&store, "full secret text", "2026-08-30T01:00:00Z");
        let files_id = match store
            .capture(ClipboardCapture::files(
                vec![file.clone()],
                "2026-08-30T01:00:01Z",
            ))
            .unwrap()
        {
            CaptureOutcome::Stored { id, .. } => id,
            other => panic!("expected stored files capture, got {other:?}"),
        };
        let image_id = match store
            .capture(ClipboardCapture::image(
                vec![255, 0, 0, 255],
                1,
                1,
                "2026-08-30T01:00:02Z",
            ))
            .unwrap()
        {
            CaptureOutcome::Stored { id, .. } => id,
            other => panic!("expected stored image capture, got {other:?}"),
        };

        assert_eq!(
            store.record_for_paste(&text_id).unwrap().payload,
            ClipboardHistoryRecordPayload::Text {
                text: "full secret text".into()
            }
        );
        assert_eq!(
            store.record_for_paste(&files_id).unwrap().payload,
            ClipboardHistoryRecordPayload::Files {
                paths: vec![file.clone()]
            }
        );
        match store.record_for_paste(&image_id).unwrap().payload {
            ClipboardHistoryRecordPayload::Image { png, width, height } => {
                assert_eq!((width, height), (1, 1));
                assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            }
            other => panic!("expected image payload, got {other:?}"),
        }
        assert_eq!(
            store.record_for_paste("999"),
            Err(ClipboardHistoryPasteError::RecordNotFound)
        );

        fs::remove_file(&file).unwrap();
        assert_eq!(
            store.record_for_paste(&files_id),
            Err(ClipboardHistoryPasteError::RecordUnavailable)
        );
        fs::remove_file(store.image_path_for_test(&image_id).unwrap()).unwrap();
        assert_eq!(
            store.record_for_paste(&image_id),
            Err(ClipboardHistoryPasteError::RecordUnavailable)
        );
    }

    #[test]
    fn format_normalization_prefers_files_then_image_then_text() {
        let text = Some("fallback".into());
        let image = Some(ClipboardImageSnapshot {
            rgba: vec![1, 2, 3, 0xff],
            width: 1,
            height: 1,
        });
        let files = Some(vec![PathBuf::from(r"C:\tmp\a.txt")]);

        assert!(matches!(
            normalize_clipboard_formats(
                ClipboardFormatSnapshot {
                    files: files.clone(),
                    image: image.clone(),
                    text: text.clone()
                },
                "2026-08-30T01:00:00Z",
            ),
            Some(ClipboardCapture::Files { .. })
        ));
        assert!(matches!(
            normalize_clipboard_formats(
                ClipboardFormatSnapshot {
                    files: Some(Vec::new()),
                    image: image.clone(),
                    text: text.clone()
                },
                "2026-08-30T01:00:00Z",
            ),
            Some(ClipboardCapture::Image { .. })
        ));
        assert_eq!(
            normalize_clipboard_formats(
                ClipboardFormatSnapshot {
                    files: None,
                    image: None,
                    text
                },
                "2026-08-30T01:00:00Z",
            ),
            Some(ClipboardCapture::text("fallback", "2026-08-30T01:00:00Z"))
        );
    }

    #[test]
    fn service_records_only_when_authorized_and_fans_out_to_current_plugins() {
        let dir = TestDir::new("service-gating");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), Arc::clone(&observer));

        reader.push(Ok(Some(ClipboardCapture::text(
            "ignored",
            "2026-08-30T01:00:00Z",
        ))));
        service.capture_current_for_test().unwrap();
        assert_eq!(reader.calls(), 0);
        assert_eq!(
            service.snapshot("com.example.one").unwrap(),
            ClipboardHistorySnapshot::default()
        );

        service
            .sync_authorized_plugins(["com.example.one".into(), "com.example.two".into()])
            .unwrap();
        assert_eq!(observer.starts(), 1);
        reader.push(Ok(Some(ClipboardCapture::text(
            "shared",
            "2026-08-30T01:00:01Z",
        ))));
        observer.trigger();
        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            1
        );
        assert_eq!(
            service.snapshot("com.example.two").unwrap().entries.len(),
            1
        );

        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();
        reader.push(Ok(Some(ClipboardCapture::text(
            "one-only",
            "2026-08-30T01:00:02Z",
        ))));
        observer.trigger();
        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            2
        );
        assert_eq!(
            service.snapshot("com.example.two").unwrap().entries.len(),
            1
        );

        service
            .sync_authorized_plugins(Vec::<String>::new())
            .unwrap();
        assert_eq!(observer.stops(), 1);
        reader.push(Ok(Some(ClipboardCapture::text(
            "stopped",
            "2026-08-30T01:00:03Z",
        ))));
        service.capture_current_for_test().unwrap();
        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            2
        );
    }

    #[test]
    fn service_retries_busy_reads_then_skips_or_captures() {
        let dir = TestDir::new("busy-retry");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), observer);
        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();

        reader.push(Err(ClipboardReadError::Busy));
        reader.push(Err(ClipboardReadError::Busy));
        reader.push(Ok(Some(ClipboardCapture::text(
            "after-busy",
            "2026-08-30T01:00:00Z",
        ))));
        service.capture_current_for_test().unwrap();
        assert_eq!(reader.calls(), 3);
        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            1
        );

        reader.push(Err(ClipboardReadError::Busy));
        reader.push(Err(ClipboardReadError::Busy));
        reader.push(Err(ClipboardReadError::Busy));
        service.capture_current_for_test().unwrap();
        assert_eq!(reader.calls(), 6);
        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            1
        );
    }

    #[test]
    fn unsupported_clipboard_formats_are_ignored() {
        let dir = TestDir::new("unsupported");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), observer);
        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();

        reader.push(Ok(None));
        service.capture_current_for_test().unwrap();

        assert_eq!(
            service.snapshot("com.example.one").unwrap(),
            ClipboardHistorySnapshot::default()
        );
    }

    #[test]
    fn retain_data_uninstall_preserves_history_and_complete_uninstall_deletes_it() {
        let dir = TestDir::new("service-uninstall");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), observer);
        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();
        reader.push(Ok(Some(ClipboardCapture::text(
            "kept",
            "2026-08-30T01:00:00Z",
        ))));
        service.capture_current_for_test().unwrap();

        service.uninstall("com.example.one", true).unwrap();
        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            1
        );

        service.uninstall("com.example.one", false).unwrap();
        assert_eq!(
            service.snapshot("com.example.one").unwrap(),
            ClipboardHistorySnapshot::default()
        );
    }

    #[test]
    fn restore_feedback_suppression_skips_the_next_observer_capture() {
        let dir = TestDir::new("restore-suppression");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), observer);
        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();
        reader.push(Ok(Some(ClipboardCapture::text(
            "first",
            "2026-08-30T01:00:00Z",
        ))));
        reader.push(Ok(Some(ClipboardCapture::text(
            "second",
            "2026-08-30T01:00:01Z",
        ))));
        service.capture_current_for_test().unwrap();
        service.capture_current_for_test().unwrap();
        let first_id = service.snapshot("com.example.one").unwrap().entries[1].id();

        service.move_to_front("com.example.one", &first_id).unwrap();
        let moved = service.snapshot("com.example.one").unwrap();
        service.suppress_next_observer_change().unwrap();
        reader.push(Ok(Some(ClipboardCapture::text(
            "first",
            "2026-08-30T01:00:02Z",
        ))));
        service.capture_current_for_test().unwrap();

        assert_eq!(service.snapshot("com.example.one").unwrap(), moved);
    }

    #[test]
    fn complete_paste_moves_record_to_front_and_suppresses_restore_feedback() {
        let dir = TestDir::new("paste-complete");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), observer);
        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();
        reader.push(Ok(Some(ClipboardCapture::text(
            "first",
            "2026-08-30T01:00:00Z",
        ))));
        reader.push(Ok(Some(ClipboardCapture::text(
            "second",
            "2026-08-30T01:00:01Z",
        ))));
        service.capture_current_for_test().unwrap();
        service.capture_current_for_test().unwrap();
        let first_id = service.snapshot("com.example.one").unwrap().entries[1].id();

        assert_eq!(
            service
                .record_for_paste("com.example.one", &first_id)
                .unwrap()
                .payload,
            ClipboardHistoryRecordPayload::Text {
                text: "first".into()
            }
        );
        service
            .begin_paste_restore_suppression()
            .expect("paste should suppress its pending clipboard observer feedback");
        service
            .complete_paste("com.example.one", &first_id)
            .expect("paste completion should update recency");
        let moved = service.snapshot("com.example.one").unwrap();
        assert_eq!(moved.entries[0].id(), first_id);
        reader.push(Ok(Some(ClipboardCapture::text(
            "first",
            "2026-08-30T01:00:02Z",
        ))));
        service.capture_current_for_test().unwrap();

        assert_eq!(service.snapshot("com.example.one").unwrap(), moved);
    }

    #[test]
    fn cancelled_paste_restore_suppression_does_not_skip_later_user_clipboard_changes() {
        let dir = TestDir::new("paste-suppression-cancel");
        let reader = Arc::new(ScriptedClipboardReader::default());
        let observer = Arc::new(ManualClipboardObserver::default());
        let service = history_service(&dir, Arc::clone(&reader), observer);
        service
            .sync_authorized_plugins(["com.example.one".into()])
            .unwrap();
        service.begin_paste_restore_suppression().unwrap();
        service.cancel_paste_restore_suppression().unwrap();

        reader.push(Ok(Some(ClipboardCapture::text(
            "user-change-after-failed-paste",
            "2026-08-30T01:00:00Z",
        ))));
        service.capture_current_for_test().unwrap();

        assert_eq!(
            service.snapshot("com.example.one").unwrap().entries.len(),
            1,
            "failed paste write must not leave a stale suppression token"
        );
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

        for index in 1..=MAX_ENTRIES {
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
