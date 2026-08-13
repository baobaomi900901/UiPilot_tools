use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{json, Value};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use super::{
    manifest::PublicOutputMode, stage_public_package, PublicPackageError, PublicPackageSource,
    PublicPlatform, PublicPluginHost,
};
use crate::plugins::{PluginCatalog, Version};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-public-package-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn package(&self) -> PathBuf {
        let path = self.0.join("source");
        fs::create_dir(&path).unwrap();
        path
    }

    fn staging(&self) -> PathBuf {
        self.0.join("staging")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn accepts_archive_and_directory_packages() {
    for (name, archive, mode) in [
        ("directory-window", false, "window"),
        ("archive-main", true, "mainResult"),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &manifest(mode));
        let source = if archive {
            let path = root.0.join("candidate.uipilot-plugin");
            archive_directory(&source, &path);
            PublicPackageSource::Archive(path)
        } else {
            PublicPackageSource::DevelopmentDirectory(source)
        };

        let prepared = stage_public_package(source, &root.staging(), &host()).unwrap();
        assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo");
        assert_eq!(prepared.digest.len(), 64);
        assert_eq!(prepared.resources["plugin.json"].mime, "application/json");
        assert_eq!(
            prepared.resources["dist/runtime.js"].mime,
            "text/javascript"
        );
        assert_eq!(prepared.revalidate(), Ok(()));
        if archive {
            let runtime = prepared.package_root.join("dist/runtime.js");
            let mut permissions = fs::metadata(&runtime).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&runtime, permissions).unwrap();
            fs::write(&runtime, "tampered").unwrap();
            assert_eq!(
                prepared.revalidate(),
                Err(PublicPackageError::InvalidPackage)
            );
        }
        assert_eq!(
            prepared.manifest.command.output_mode,
            if mode == "window" {
                PublicOutputMode::Window
            } else {
                PublicOutputMode::MainResult
            }
        );
        let transaction = prepared.transaction_root().to_path_buf();
        assert!(prepared.package_root.starts_with(&transaction));
        drop(prepared);
        assert!(!transaction.exists());
    }
}

#[test]
fn rejects_resource_and_archive_path_variants() {
    for path in ["dist/icon.png", "dist/runtime.min.js", "dist/data.json"] {
        let root = TestRoot::new(path.replace(['/', '.'], "-").as_str());
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        let extra = path
            .split('/')
            .fold(source.clone(), |root, part| root.join(part));
        fs::create_dir_all(extra.parent().unwrap()).unwrap();
        fs::write(extra, "x").unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    for (name, entries) in [
        (
            "traversal",
            vec![
                ("../escape.js", b"x".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
        (
            "case-collision",
            vec![
                ("dist/runtime.js", b"x".as_slice()),
                ("dist/Runtime.js", b"y".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
        (
            "parent-case-collision",
            vec![
                ("Dist/a.js", b"x".as_slice()),
                ("dist/b.js", b"y".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
    ] {
        let root = TestRoot::new(name);
        let archive = root.0.join("candidate.uipilot-plugin");
        write_archive(&archive, &entries);
        assert_rejected(
            PublicPackageSource::Archive(archive),
            &root,
            PublicPackageError::InvalidPackage,
        );
        assert!(!root.0.join("escape.js").exists());
    }
}

#[test]
fn rejects_incompatible_or_malformed_and_preserves_legacy_loader() {
    for (name, field, replacement, expected) in [
        (
            "platform",
            "supportedPlatforms",
            json!(["macos"]),
            PublicPackageError::IncompatiblePlatform,
        ),
        (
            "api",
            "apiVersion",
            json!(2),
            PublicPackageError::IncompatibleApi,
        ),
        (
            "permission",
            "permissions",
            json!(["network.https"]),
            PublicPackageError::UnsupportedPermission,
        ),
        (
            "settings",
            "settings",
            json!([{"key":"limit","type":"number","label":"Limit","min":10,"max":5}]),
            PublicPackageError::InvalidPackage,
        ),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        let mut candidate = manifest("mainResult");
        candidate[field] = replacement;
        write_package(&source, &candidate);
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            expected,
        );
    }

    let root = TestRoot::new("legacy");
    let legacy = root.0.join("internal.math");
    fs::create_dir(&legacy).unwrap();
    fs::write(
        legacy.join("plugin.json"),
        r#"{"manifest":1,"id":"internal.math","version":"1.0.0","minHostVersion":"0.2.0","runtime":"index.html","feature":{"id":"calculate","trigger":"/math"},"permissions":["clipboard.writeText"]}"#,
    )
    .unwrap();
    fs::write(legacy.join("index.html"), "").unwrap();
    assert_eq!(
        PluginCatalog::load(&root.0, Version::new(0, 2, 0))
            .unwrap()
            .entry_count_for_test(),
        1
    );
}

fn host() -> PublicPluginHost {
    PublicPluginHost::current(PublicPlatform::Windows)
}

fn manifest(mode: &str) -> Value {
    let window = mode == "window";
    let mut value = json!({
        "schemaVersion":1,
        "pluginId":"com.uipilot.demo",
        "version":"1.0.0",
        "apiVersion":1,
        "minimumHostVersion":"0.2.0",
        "name":"Demo",
        "supportedPlatforms":["windows"],
        "command":{
            "defaultName":"demo",
            "activationMode":if window { "submit" } else { "live" },
            "outputMode":mode,
            "inputRequired":false
        },
        "runtime":{"entry":"dist/runtime.js"},
        "permissions":if window { json!(["ui.window"]) } else { json!([]) }
    });
    if window {
        value["window"] = json!({"entry":"dist/window.html"});
    }
    value
}

fn write_package(root: &Path, manifest: &Value) {
    fs::create_dir(root.join("dist")).unwrap();
    fs::write(
        root.join("plugin.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("dist/runtime.js"),
        "export function onCommand() {}",
    )
    .unwrap();
    if manifest["command"]["outputMode"] == "window" {
        fs::write(root.join("dist/window.html"), "<!doctype html>").unwrap();
    }
}

fn assert_rejected(source: PublicPackageSource, root: &TestRoot, expected: PublicPackageError) {
    assert_eq!(
        stage_public_package(source, &root.staging(), &host()).unwrap_err(),
        expected
    );
    assert!(!root.staging().exists() || fs::read_dir(root.staging()).unwrap().next().is_none());
}

fn archive_directory(source: &Path, destination: &Path) {
    let mut entries = Vec::new();
    collect_files(source, source, &mut entries);
    let borrowed = entries
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    write_archive(destination, &borrowed);
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(root, &path, output);
        } else {
            output.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(path).unwrap(),
            ));
        }
    }
}

fn write_archive(destination: &Path, entries: &[(&str, &[u8])]) {
    let mut archive = ZipWriter::new(File::create(destination).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (path, bytes) in entries {
        archive.start_file(*path, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
}
