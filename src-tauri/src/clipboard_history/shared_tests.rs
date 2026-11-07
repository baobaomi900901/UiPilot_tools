use std::path::PathBuf;

use super::preview::files_fingerprint;
use super::{
    model::{MAX_THUMBNAIL_PNG_BYTES, THUMBNAIL_MAX_EDGE},
    preview::{
        decode_data_url_for_test, image_fingerprint, prepare_image, text_fingerprint,
        text_preview, windows_files_fingerprint,
    },
    ClipboardCapture, ClipboardFormatSnapshot, ClipboardHistoryEntrySummary,
    ClipboardHistorySnapshot, ClipboardImageSnapshot, normalize_clipboard_formats,
};

#[test]
fn windows_file_fingerprints_keep_persisted_vectors() {
    let vectors = [
        (vec![], "a2df60d00da1638390dae83167367c3c5001f64127d23d512ae832c68a833f24"),
        (vec![r"C:\Foo\Bar.txt"], "eb0bbca3b3cb608e7bdb78d3e35414ed750526319f0bfc7931a12c3f2087d790"),
        (vec![r"C:\中文\a.txt", r"D:\two.png"], "b51f59ec14a377381eee04e26bbaa964861b09a9090a7c5944c3e06416185934"),
    ];
    for (paths, expected) in vectors {
        let paths: Vec<_> = paths.into_iter().map(PathBuf::from).collect();
        assert_eq!(windows_files_fingerprint(&paths), expected);
        #[cfg(windows)]
        assert_eq!(files_fingerprint(&paths), expected);
    }
    assert_eq!(windows_files_fingerprint(&[PathBuf::from(r"\\?\c:/foo/bar.txt")]),
        "eb0bbca3b3cb608e7bdb78d3e35414ed750526319f0bfc7931a12c3f2087d790");
}

#[test]
fn format_selection_preserves_priority_empty_fallback_and_invalid_image() {
    let image = ClipboardImageSnapshot { rgba: vec![1, 2, 3, 255], width: 1, height: 1 };
    let paths = vec![PathBuf::from("synthetic-file")];
    let mut snapshot = ClipboardFormatSnapshot {
        files: Some(paths.clone()), image: Some(image.clone()), text: Some("fallback".into()),
    };
    assert_eq!(normalize_clipboard_formats(snapshot.clone(), "time"),
        Some(ClipboardCapture::files(paths, "time")));
    snapshot.files = Some(vec![]);
    assert_eq!(normalize_clipboard_formats(snapshot.clone(), "time"),
        Some(ClipboardCapture::image(image.rgba, 1, 1, "time")));
    snapshot.image = Some(ClipboardImageSnapshot { rgba: vec![], width: 0, height: 1 });
    assert_eq!(normalize_clipboard_formats(snapshot.clone(), "time"),
        Some(ClipboardCapture::image(vec![], 0, 1, "time")));
    snapshot.image = None;
    assert_eq!(normalize_clipboard_formats(snapshot.clone(), "time"),
        Some(ClipboardCapture::text("fallback", "time")));
    snapshot.text = Some(String::new());
    assert_eq!(normalize_clipboard_formats(snapshot, "time"), Some(ClipboardCapture::text("", "time")));
    assert_eq!(normalize_clipboard_formats(ClipboardFormatSnapshot::default(), "time"), None);
}

#[test]
fn summary_dto_keeps_exact_public_fields_for_each_kind() {
    use serde_json::json;
    let snapshot = ClipboardHistorySnapshot { revision: "12".into(), entries: vec![
        ClipboardHistoryEntrySummary::Text { id: "1".into(), captured_at: "t".into(), text_preview: "preview".into() },
        ClipboardHistoryEntrySummary::Image { id: "2".into(), captured_at: "t".into(), preview_data_url: "data:image/png;base64,".into(), width: 1, height: 2 },
        ClipboardHistoryEntrySummary::Files { id: "3".into(), captured_at: "t".into(), first_file_name: "sample".into(), file_count: 2, available: false },
    ]};
    assert_eq!(serde_json::to_value(snapshot).unwrap(), json!({"revision":"12", "entries":[
        {"kind":"text","id":"1","capturedAt":"t","textPreview":"preview"},
        {"kind":"image","id":"2","capturedAt":"t","previewDataUrl":"data:image/png;base64,","width":1,"height":2},
        {"kind":"files","id":"3","capturedAt":"t","firstFileName":"sample","fileCount":2,"available":false}
    ]}));
    assert_eq!(serde_json::to_value(ClipboardHistorySnapshot::default()).unwrap(), json!({"revision":"0","entries":[]}));
}

#[test]
fn text_preview_is_bounded_without_changing_content_fingerprint() {
    assert_eq!(text_preview("\t 中文\n\r🙂  text \u{2003}"), "中文 🙂 text");
    assert_eq!(text_preview(" \t\n"), "");
    assert_eq!(text_preview(&"🙂".repeat(121)), "🙂".repeat(120));
    assert_eq!(text_preview(&format!("{} rest", "界".repeat(119))).chars().count(), 120);
    assert_ne!(text_fingerprint("a  b"), text_fingerprint("a b"));
    assert_ne!(text_fingerprint("a\n"), text_fingerprint("a"));
}

#[test]
fn image_preparation_rejects_invalid_sizes_and_buffers() {
    for (w, h, rgba) in [(0, 1, vec![]), (1, 0, vec![]), (1, 1, vec![0; 3]),
        (1, 1, vec![0; 5]), (u32::MAX, 2, vec![]), (1 << 30, 1, vec![])] {
        assert!(matches!(prepare_image(w, h, &rgba), Err(super::ClipboardHistoryError::InvalidCapture)));
    }
}

#[test]
fn image_png_roundtrips_and_thumbnail_respects_bounds() {
    let rgba = vec![13, 24, 35, 46, 57, 68, 79, 80];
    let prepared = prepare_image(2, 1, &rgba).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(&prepared.png)).read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut decoded).unwrap();
    assert_eq!((info.width, info.height, info.color_type), (2, 1, png::ColorType::Rgba));
    assert_eq!(&decoded[..info.buffer_size()], rgba.as_slice());
    assert_ne!(image_fingerprint(2, 1, &rgba), image_fingerprint(1, 2, &rgba));
    let prepared = prepare_image(512, 128, &vec![127; 512 * 128 * 4]).unwrap();
    assert_eq!((prepared.thumbnail_width, prepared.thumbnail_height), (THUMBNAIL_MAX_EDGE, 64));
    let thumbnail = decode_data_url_for_test(&prepared.thumbnail_data_url);
    assert!(thumbnail.len() <= MAX_THUMBNAIL_PNG_BYTES);
    let reader = png::Decoder::new(std::io::Cursor::new(thumbnail)).read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (256, 64));
}

#[cfg(target_os = "macos")]
#[test]
fn mac_file_fingerprints_separate_domain_order_and_list_boundaries() {
    let a = PathBuf::from("/fixture/a");
    let b = PathBuf::from("/fixture/b");
    assert_eq!(files_fingerprint(&[a.clone(), b.clone()]), files_fingerprint(&[a.clone(), b.clone()]));
    assert_ne!(files_fingerprint(&[a.clone(), b.clone()]), files_fingerprint(&[b.clone(), a.clone()]));
    assert_ne!(files_fingerprint(&[a.clone(), b.clone()]), files_fingerprint(&[PathBuf::from("/fixture/a/fixture/b")]));
    assert_ne!(files_fingerprint(&[]), files_fingerprint(&[PathBuf::new()]));
    assert_ne!(files_fingerprint(&[a.clone()]), windows_files_fingerprint(&[a]));
}

#[cfg(target_os = "macos")]
#[test]
fn mac_file_fingerprints_preserve_path_spelling_and_raw_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let pairs = [
        (PathBuf::from("/fixture/A"), PathBuf::from("/fixture/a")),
        (PathBuf::from("/fixture/a\\b"), PathBuf::from("/fixture/a/b")),
        (PathBuf::from("/fixture/é"), PathBuf::from("/fixture/e\u{301}")),
        (
            PathBuf::from(std::ffi::OsString::from_vec(b"/fixture/\xff".to_vec())),
            PathBuf::from(std::ffi::OsString::from_vec(b"/fixture/\xfe".to_vec())),
        ),
    ];
    for (left, right) in pairs {
        assert_ne!(files_fingerprint(&[left]), files_fingerprint(&[right]));
    }
}
