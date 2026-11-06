//! Ignored, multi-process hardware probe using the actual private disk store and native
//! Documents bookmark implementation. Its only directory authority is an exclusive fixture.
//!
//! Copy the preview lib-test executable into a temporary app bundle, supply an Info.plist
//! with a bundle identifier/executable and sign the app once with just
//! com.apple.security.files.bookmarks.app-scope=true. Keep the same installed binary for all steps.
//! Under a mktemp -d uipilot-bookmark-proof-* root, create mode-0700 app-data and
//! home/Documents/图片 🐋/画稿📷.png. Pass the absolute scratch root in UIPILOT_BOOKMARK_FIXTURE.
//! Each invocation runs this exact ignored test with a fresh UIPILOT_BOOKMARK_STEP:
//! create, restore, forget, forgotten, create. Rename home to old-home, create a new home and
//! move the original Documents directory into it. Run reject, forget, forgotten; remove the fixture.

use super::{Record, Store};
use crate::macos_find_results::{DocumentsGrant, FindError};
use std::{fs, os::unix::fs::MetadataExt, path::{Path, PathBuf}};

#[test]
#[ignore = "requires the same signed preview executable in every process and a private synthetic fixture"]
fn signed_cross_process_documents_bookmark() {
    let root = PathBuf::from(std::env::var_os("UIPILOT_BOOKMARK_FIXTURE").expect("supply synthetic directory"));
    assert!(matches!(root.parent(), Some(p) if p == Path::new("/private/tmp") || p == Path::new("/Users/Shared")));
    assert!(root.file_name().unwrap().to_str().unwrap().starts_with("uipilot-bookmark-proof-"));
    assert_eq!(root.canonicalize().unwrap(), root);
    let metadata = fs::symlink_metadata(&root).unwrap();
    assert!(metadata.is_dir());
    assert_eq!(metadata.mode() & 0o777, 0o700);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    let docs = root.join("home/Documents");
    let store = Store::open(&root.join("app-data")).unwrap();
    let mode = std::env::var("UIPILOT_BOOKMARK_STEP").unwrap();
    match mode.as_str() {
        "create" => {
            assert!(matches!(store.read().unwrap(), Record::Forgotten));
            let record = DocumentsGrant::create_fixture_bookmark(&docs);
            store.write(&Record::Bookmark(record)).unwrap();
        }
        "restore" | "reject" => {
            let Record::Bookmark(bytes) = store.read().unwrap() else { panic!("expected durable bookmark"); };
            let restored = DocumentsGrant::verify_fixture_bookmark(&bytes, &docs);
            if mode == "restore" {
                restored.expect("new signed process failed to authorize the original document");
            } else {
                assert_eq!(restored, Err(FindError::Stale), "replacement must be positively identified");
            }
        }
        "forget" => {
            assert!(matches!(store.read().unwrap(), Record::Bookmark(_)));
            store.write(&Record::Forgotten).unwrap();
        }
        "forgotten" => {
            assert!(matches!(store.read().unwrap(), Record::Forgotten),
                "later process must not try to restore a forgotten credential");
        }
        _ => panic!("use create, restore, reject, forget or forgotten"),
    }
}
