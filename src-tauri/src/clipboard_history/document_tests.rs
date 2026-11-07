use super::*;
#[test]
fn duplicate_text_retains_id_and_timestamp_and_clear_does_not_reuse_id() {
    let mut document = HistoryDocument::default();
    let first = capture_text(&mut document, "first".into(), "t1".into()).unwrap();
    assert!(matches!(first.outcome, CaptureOutcome::Stored { ref id, ref revision } if id == "1" && revision == "1"));
    capture_text(&mut document, "second".into(), "t2".into()).unwrap();
    let moved = capture_text(&mut document, "first".into(), "t3".into()).unwrap();
    assert!(matches!(moved.outcome, CaptureOutcome::MovedToFront { ref id, ref revision } if id == "1" && revision == "3"));
    assert_eq!(document.entries[0].captured_at, "t1");
    let unchanged = capture_text(&mut document, "first".into(), "t4".into()).unwrap();
    assert!(matches!(unchanged.outcome, CaptureOutcome::Unchanged { ref revision, .. } if revision == "3"));
    clear(&mut document).unwrap();
    capture_text(&mut document, "third".into(), "t5".into()).unwrap();
    assert_eq!(document.entries[0].id, "3");
}

#[test]
fn fifty_most_recent_entries_and_removal_keep_monotonic_counters() {
    let mut value = HistoryDocument::default();
    for n in 0..51 { capture_text(&mut value, format!("value{n}"), "t".into()).unwrap(); }
    assert_eq!(value.entries.len(), 50);
    assert_eq!(value.entries[0].id, "51");
    assert_eq!(value.entries[49].id, "2");
    assert!(!remove(&mut value, "1").unwrap());
    assert_eq!(value.revision, "51");
    assert!(remove(&mut value, "51").unwrap());
    assert_eq!(value.revision, "52");
    assert_eq!(value.next_id, "52");
    assert_eq!(value.next_recency_rank, "52");
}

#[test]
fn windows_image_names_and_byte_capacity_preserve_obsolete_list() {
    let mut value = HistoryDocument::default();
    for n in 1..=2 {
        let payload = HistoryEntryPayload::Image { width: 1, height: 1, png_file: String::new(),
            png_bytes: (MAX_TOTAL_IMAGE_PNG_BYTES / 2 + 1) as u64, thumbnail_data_url: "preview".into(),
            thumbnail_width: 1, thumbnail_height: 1 };
        let mutation = capture(&mut value, format!("{n:064x}"), "t".into(), payload).unwrap();
        assert!(matches!(&value.entries[0].payload, HistoryEntryPayload::Image { png_file, .. } if png_file == &format!("{n}.png")));
        if n == 2 { assert_eq!(mutation.obsolete_images, vec!["1.png"]); }
    }
    assert_eq!(value.entries.len(), 1);
}

#[test]
fn overflow_candidates_are_discardable_without_publishing() {
    for field in ["id", "rank", "revision"] {
        let mut current = HistoryDocument::default();
        match field { "id" => current.next_id = u64::MAX.to_string(), "rank" => current.next_recency_rank = u64::MAX.to_string(), _ => current.revision = u64::MAX.to_string() }
        let saved = current.clone();
        let mut candidate = current.clone();
        assert!(capture_text(&mut candidate, "new".into(), "t".into()).is_err());
        assert_eq!(current, saved);
    }
    let mut current = HistoryDocument::default();
    capture_text(&mut current, "old".into(), "t".into()).unwrap();
    current.revision = u64::MAX.to_string();
    let mut candidate = current.clone();
    assert!(remove(&mut candidate, "1").is_err());
    let mut candidate = current.clone();
    assert!(clear(&mut candidate).is_err());
    assert_eq!(current.entries.len(), 1);
}
