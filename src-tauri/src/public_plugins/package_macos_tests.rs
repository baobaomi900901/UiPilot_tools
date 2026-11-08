use super::*;
#[test]
fn actual_demo_settings_reads_without_executing_source() {
    let host = super::super::manifest::PublicPluginHost::current(
        super::super::manifest::PublicPlatform::Macos,
    );
    let source = fixture_path();
    let package = read_directory(&source, &host).unwrap();
    assert_eq!(package.manifest().plugin_id, "com.uipilot.demo-settings");
}
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zip::{write::SimpleFileOptions, ZipWriter};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "uipilot-package-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn host() -> PublicPluginHost {
    PublicPluginHost::current(super::super::manifest::PublicPlatform::Macos)
}
fn demo() -> VerifiedPackage {
    read_directory(fixture_path().as_path(), &host()).unwrap()
}
fn copy_demo(temp: &Temp) -> PathBuf {
    let path = temp.0.join("package");
    fs::create_dir(&path).unwrap();
    for (name, bytes) in demo().resources() {
        fs::write(path.join(name), bytes).unwrap();
    }
    path
}
fn zip_bytes(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        if name.ends_with('/') {
            writer
                .add_directory(name, SimpleFileOptions::default())
                .unwrap();
        } else {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}
fn entries() -> Vec<(String, Vec<u8>)> {
    demo()
        .resources()
        .map(|(p, b)| (p.into(), b.to_vec()))
        .collect()
}
fn archive(temp: &Temp, bytes: &[u8]) -> PathBuf {
    let p = temp.0.join("input.uipilot-plugin");
    fs::write(&p, bytes).unwrap();
    p
}
#[test]
fn actual_directory_archive_restore_share_snapshot_and_source_changes_do_not_mutate_it() {
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    let first = read_directory(&dir, &host()).unwrap();
    let zipped = read_archive(&archive(&temp, &zip_bytes(&entries())), &host()).unwrap();
    assert_eq!(first.digest(), zipped.digest());
    assert_eq!(first.encode_snapshot(), zipped.encode_snapshot());
    let restored = restore_snapshot(first.encode_snapshot(), &host()).unwrap();
    assert_eq!(restored.digest(), first.digest());
    let old = first.resource("runtime.js").unwrap().to_vec();
    fs::write(
        dir.join("runtime.js"),
        b"export function onCommand() {return null}",
    )
    .unwrap();
    let second = read_directory(&dir, &host()).unwrap();
    assert_ne!(second.digest(), first.digest());
    assert_eq!(first.resource("runtime.js").unwrap(), old);
}
#[test]
fn real_duplicate_zip_entry_is_not_hidden_by_library_name_deduplication() {
    let temp = Temp::new();
    let mut list = entries();
    list.push(("x.js".into(), b"export const x=1".to_vec()));
    list.push(("y.js".into(), b"export const y=2".to_vec()));
    let mut bytes = zip_bytes(&list);
    let positions: Vec<usize> = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(i, s)| (s == b"y.js").then_some(i))
        .collect();
    assert_eq!(positions.len(), 2);
    for i in positions {
        bytes[i..i + 4].copy_from_slice(b"x.js");
    }
    assert!(read_archive(&archive(&temp, &bytes), &host()).is_err());
}
#[test]
fn links_and_cross_platform_path_aliases_reject_without_writing_source() {
    for kind in ["symlink", "hardlink", "reserved", "nfc"] {
        let temp = Temp::new();
        let dir = copy_demo(&temp);
        match kind {
            "symlink" => symlink(dir.join("runtime.js"), dir.join("linked.js")).unwrap(),
            "hardlink" => fs::hard_link(dir.join("runtime.js"), dir.join("linked.js")).unwrap(),
            "reserved" => fs::write(dir.join("CON.js"), b"x").unwrap(),
            _ => fs::write(dir.join("e\u{301}.js"), b"x").unwrap(),
        };
        assert!(read_directory(&dir, &host()).is_err());
    }
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    let link = temp.0.join("link");
    symlink(&dir, &link).unwrap();
    assert!(read_directory(&link, &host()).is_err());
}
#[test]
fn zip_traversal_case_collision_parent_conflict_and_legitimate_explicit_parent() {
    let temp = Temp::new();
    for extra in [
        vec![("../escape.js", b"x".to_vec())],
        vec![("A.js", vec![]), ("a.js", vec![])],
        vec![("a.js", vec![]), ("a.js/b.js", vec![])],
        vec![("nested/", vec![]), ("nested", vec![])],
    ] {
        let mut list = entries();
        list.extend(extra.into_iter().map(|(p, b)| (p.into(), b)));
        assert!(read_archive(&archive(&temp, &zip_bytes(&list)), &host()).is_err());
        assert!(!temp.0.join("escape.js").exists());
    }
    let mut list = entries();
    list.push(("nested/module.js".into(), b"export const value=1".to_vec()));
    list.push(("nested/".into(), vec![]));
    assert!(read_archive(&archive(&temp, &zip_bytes(&list)), &host()).is_ok());
}
#[test]
fn oversized_file_encoded_snapshot_and_compressed_bomb_fail_with_bounds() {
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    fs::write(
        dir.join("large.js"),
        vec![b'x'; MAX_FILE_BYTES as usize + 1],
    )
    .unwrap();
    assert!(read_directory(&dir, &host()).is_err());
    fs::remove_file(dir.join("large.js")).unwrap();
    fs::write(dir.join("a.js"), vec![b'a'; MAX_FILE_BYTES as usize]).unwrap();
    fs::write(dir.join("b.js"), vec![b'b'; MAX_FILE_BYTES as usize]).unwrap();
    assert!(read_directory(&dir, &host()).is_err());
    let mut list = entries();
    list.push(("bomb.js".into(), vec![b'x'; MAX_FILE_BYTES as usize + 1024]));
    let bytes = zip_bytes(&list);
    assert!(bytes.len() < 10000);
    assert!(read_archive(&archive(&temp, &bytes), &host()).is_err());
}
#[test]
fn restore_revalidates_digest_manifest_paths_and_exact_snapshot_fields() {
    let original = demo();
    let mut raw: Value = serde_json::from_slice(original.encode_snapshot()).unwrap();
    raw["digest"] = json!("0".repeat(64));
    assert!(restore_snapshot(&serde_json::to_vec(&raw).unwrap(), &host()).is_err());
    let mut raw: Value = serde_json::from_slice(original.encode_snapshot()).unwrap();
    raw["unexpected"] = json!(1);
    assert!(restore_snapshot(&serde_json::to_vec(&raw).unwrap(), &host()).is_err());
    let mut record: Snapshot = serde_json::from_slice(original.encode_snapshot()).unwrap();
    let manifest = record
        .resources
        .iter_mut()
        .find(|r| r.path == "plugin.json")
        .unwrap();
    let mut value: Value =
        serde_json::from_slice(&STANDARD.decode(&manifest.base64).unwrap()).unwrap();
    value["supportedPlatforms"] = json!(["windows"]);
    manifest.base64 = STANDARD.encode(serde_json::to_vec(&value).unwrap());
    let resources = record
        .resources
        .iter()
        .map(|r| (r.path.clone(), STANDARD.decode(&r.base64).unwrap()))
        .collect();
    record.digest = seal(&record.directories, &resources);
    assert!(restore_snapshot(&serde_json::to_vec(&record).unwrap(), &host()).is_err());
    record.resources[0].path = "../outside.js".into();
    let resources = record
        .resources
        .iter()
        .map(|r| (r.path.clone(), STANDARD.decode(&r.base64).unwrap()))
        .collect();
    record.digest = seal(&record.directories, &resources);
    assert!(restore_snapshot(&serde_json::to_vec(&record).unwrap(), &host()).is_err());
}
#[test]
fn unsupported_output_platform_missing_entry_and_bad_icon_are_rejected() {
    for kind in ["platform", "panel", "action", "entry", "icon"] {
        let temp = Temp::new();
        let dir = copy_demo(&temp);
        let path = dir.join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match kind {
            "platform" => manifest["supportedPlatforms"] = json!(["windows"]),
            "panel" => manifest["command"]["outputMode"] = json!("panel"),
            "action" => manifest["runtime"]["resultActions"] = json!(true),
            "entry" => manifest["runtime"]["entry"] = json!("missing.js"),
            _ => fs::write(dir.join("icon.png"), b"not PNG").unwrap(),
        };
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(read_directory(&dir, &host()).is_err());
    }
}
#[test]
fn final_fd_checks_reject_file_directory_and_root_changes_during_capture() {
    for kind in ["file", "directory", "root"] {
        let temp = Temp::new();
        let dir = copy_demo(&temp);
        fs::create_dir(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/module.js"), b"export const x=1").unwrap();
        let changed = dir.clone();
        let old = temp.0.join("old");
        BEFORE_VALIDATE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || match kind {
                "file" => fs::write(changed.join("runtime.js"), b"changed after read").unwrap(),
                "directory" => {
                    fs::rename(changed.join("nested"), old).unwrap();
                    fs::create_dir(changed.join("nested")).unwrap();
                }
                _ => {
                    fs::rename(&changed, old).unwrap();
                    fs::create_dir(changed).unwrap();
                }
            }))
        });
        assert!(read_directory(&dir, &host()).is_err());
    }
}
#[test]
fn file_directory_depth_counts_and_css_references_follow_shared_rules() {
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    for n in 0..MAX_FILES {
        fs::write(dir.join(format!("m{n}.js")), b"x").unwrap();
    }
    assert!(read_directory(&dir, &host()).is_err());
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    for n in 0..=MAX_DIRECTORIES {
        fs::create_dir(dir.join(format!("d{n}"))).unwrap();
    }
    assert!(read_directory(&dir, &host()).is_err());
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    let mut child = dir.clone();
    for _ in 0..=MAX_DEPTH {
        child = child.join("nested");
        fs::create_dir(&child).unwrap();
    }
    assert!(read_directory(&dir, &host()).is_err());
    let temp = Temp::new();
    let dir = copy_demo(&temp);
    fs::write(dir.join("style.css"), b"@import 'other.css';").unwrap();
    fs::write(dir.join("other.css"), b"body {}").unwrap();
    assert!(read_directory(&dir, &host()).is_ok());
    fs::write(
        dir.join("style.css"),
        b"a{background:url(https://example.invalid/x)}",
    )
    .unwrap();
    assert!(read_directory(&dir, &host()).is_err());
}
#[test]
fn archive_metadata_truncation_count_mismatch_local_traversal_and_spoofed_size_reject() {
    let temp = Temp::new();
    let valid = zip_bytes(&entries());
    let end = valid.windows(4).rposition(|b| b == b"PK\x05\x06").unwrap();
    let mut count = valid.clone();
    count[end + 10..end + 12].copy_from_slice(&3u16.to_le_bytes());
    assert!(read_archive(&archive(&temp, &count), &host()).is_err());
    assert!(read_archive(&archive(&temp, &valid[..valid.len() - 1]), &host()).is_err());
    let mut local = valid.clone();
    let name_len = u16::from_le_bytes(local[26..28].try_into().unwrap()) as usize;
    assert!(name_len > 3);
    local[30..33].copy_from_slice(b"../");
    assert!(read_archive(&archive(&temp, &local), &host()).is_err());
    let mut list = entries();
    list.push(("bomb.js".into(), vec![b'x'; MAX_FILE_BYTES as usize + 1024]));
    let mut bomb = zip_bytes(&list);
    let positions: Vec<usize> = bomb
        .windows(4)
        .enumerate()
        .filter_map(|(i, b)| (b == b"PK\x01\x02").then_some(i))
        .collect();
    for at in positions {
        let n = u16::from_le_bytes(bomb[at + 28..at + 30].try_into().unwrap()) as usize;
        if &bomb[at + 46..at + 46 + n] == b"bomb.js" {
            bomb[at + 24..at + 28].copy_from_slice(&(MAX_FILE_BYTES as u32).to_le_bytes());
        }
    }
    assert!(read_archive(&archive(&temp, &bomb), &host()).is_err());
}

fn fixture_path() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|root| root.join("examples/public-plugins/com.uipilot.demo-settings/package"))
        .find(|path| path.is_dir())
        .expect("tracked repository fixture")
}
