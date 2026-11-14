//! Internal immutable package snapshots, not an installer or execution grant.
//! Sources are read-only; execution must consume these owned bytes, never reopen
//! the development path. Stable-stat observations are not a filesystem-wide CAS.
use super::{
    icon,
    manifest::{
        parse_manifest, PublicManifestV1, PublicOutputMode, PublicPackageError, PublicPluginHost,
    },
    package_rules::*,
    storage_document::validate_mac_json,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};
use zip::{CompressionMethod, ZipArchive};
#[path = "package_zip_macos.rs"]
mod zip_preflight;
pub(super) const MAX_SNAPSHOT_BYTES: usize = 5 * 1024 * 1024;
type Result<T> = std::result::Result<T, PublicPackageError>;
fn checked<T, E>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|_| PublicPackageError::InvalidPackage)
}
#[derive(Clone)]
pub(super) struct VerifiedPackage {
    manifest: PublicManifestV1,
    digest: String,
    resources: BTreeMap<String, Vec<u8>>,
    encoded: Vec<u8>,
}
impl VerifiedPackage {
    pub(super) fn runtime_modules(&self) -> Result<super::runtime_modules::RuntimeModuleGraph> {
        super::runtime_modules::RuntimeModuleGraph::new(
            &self.manifest.runtime.entry,
            self.resources().filter(|(path, _)| path.ends_with(".js")),
        ).map_err(|_| PublicPackageError::InvalidPackage)
    }
    pub(super) fn manifest(&self) -> &PublicManifestV1 {
        &self.manifest
    }
    pub(super) fn digest(&self) -> &str {
        &self.digest
    }
    pub(super) fn resource(&self, path: &str) -> Option<&[u8]> {
        self.resources.get(path).map(Vec::as_slice)
    }
    pub(super) fn resources(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.resources
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
    }
    pub(super) fn encode_snapshot(&self) -> &[u8] {
        &self.encoded
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    schema: u32,
    digest: String,
    directories: Vec<String>,
    resources: Vec<Resource>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    path: String,
    base64: String,
}
#[derive(Default)]
struct Tree {
    entries: HashSet<String>,
    directories: BTreeMap<String, String>,
    resources: BTreeMap<String, Vec<u8>>,
    total: u64,
}
impl Tree {
    fn insert(&mut self, path: &str, bytes: Option<Vec<u8>>) -> Result<()> {
        let path = canonical_relative_path(path)?;
        if !self.entries.insert(fold_path(&path)) {
            return Err(PublicPackageError::InvalidPackage);
        }
        register_parent_directories(&path, &mut self.directories)?;
        match bytes {
            None => register_directory(&path, &mut self.directories)?,
            Some(bytes) => {
                validate_package_resource_path(&path)?;
                if bytes.len() as u64 > MAX_FILE_BYTES || self.resources.len() >= MAX_FILES {
                    return Err(PublicPackageError::InvalidPackage);
                }
                self.total = self
                    .total
                    .checked_add(bytes.len() as u64)
                    .filter(|n| *n <= MAX_TOTAL_BYTES)
                    .ok_or(PublicPackageError::InvalidPackage)?;
                self.resources.insert(path, bytes);
            }
        }
        Ok(())
    }
    fn finish(self, host: &PublicPluginHost) -> Result<VerifiedPackage> {
        if self
            .resources
            .keys()
            .any(|p| self.directories.contains_key(&fold_path(p)))
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        let manifest_bytes = self
            .resources
            .get("plugin.json")
            .ok_or(PublicPackageError::InvalidPackage)?;
        validate_mac_json(manifest_bytes).ok_or(PublicPackageError::InvalidPackage)?;
        let manifest = parse_manifest(manifest_bytes, host)?;
        if manifest.runtime.result_actions
            || manifest.window.is_some()
            || manifest.panel.is_some()
            || manifest
                .normalized_commands()
                .any(|(_, c)| c.output_mode != PublicOutputMode::MainResult)
        {
            return Err(PublicPackageError::UnsupportedPermission);
        }
        if !self.resources.contains_key(&manifest.runtime.entry)
            || self.resources.contains_key("assets/sounds/timer-alarm.wav")
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        for (path, bytes) in &self.resources {
            match validate_package_resource_path(path)? {
                "image/png" => {
                    if path != icon::ICON_PATH {
                        return Err(PublicPackageError::InvalidPackage);
                    }
                    icon::validate_png(bytes)?;
                }
                "text/css" => {
                    validate_css_text(path, checked(std::str::from_utf8(bytes))?, &|p| {
                        self.resources.contains_key(p)
                    })?
                }
                _ => {}
            }
        }
        let directories: Vec<String> = self
            .directories
            .into_values()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let digest = seal(&directories, &self.resources);
        let snapshot = Snapshot {
            schema: 1,
            digest: digest.clone(),
            directories,
            resources: self
                .resources
                .iter()
                .map(|(path, bytes)| Resource {
                    path: path.clone(),
                    base64: STANDARD.encode(bytes),
                })
                .collect(),
        };
        let encoded = checked(serde_json::to_vec(&snapshot))?;
        if encoded.len() > MAX_SNAPSHOT_BYTES {
            return Err(PublicPackageError::InvalidPackage);
        }
        Ok(VerifiedPackage {
            manifest,
            digest,
            resources: self.resources,
            encoded,
        })
    }
}
fn seal(directories: &[String], resources: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    hash.update(b"UIPILOT-MAC-PACKAGE-SNAPSHOT\0SHA256-V1\0");
    hash.update((directories.len() as u32).to_le_bytes());
    for path in directories {
        hash.update((path.len() as u32).to_le_bytes());
        hash.update(path.as_bytes());
    }
    hash.update((resources.len() as u32).to_le_bytes());
    for (path, bytes) in resources {
        hash.update((path.len() as u32).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}
pub(super) fn restore_snapshot(bytes: &[u8], host: &PublicPluginHost) -> Result<VerifiedPackage> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(PublicPackageError::InvalidPackage);
    }
    validate_mac_json(bytes).ok_or(PublicPackageError::InvalidPackage)?;
    let snapshot: Snapshot = checked(serde_json::from_slice(bytes))?;
    if snapshot.schema != 1
        || snapshot.resources.len() > MAX_FILES
        || snapshot.directories.len() > MAX_DIRECTORIES
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut tree = Tree::default();
    for directory in snapshot.directories {
        tree.insert(&directory, None)?;
    }
    for resource in snapshot.resources {
        if resource.base64.len() > ((MAX_FILE_BYTES as usize + 2) / 3) * 4 {
            return Err(PublicPackageError::InvalidPackage);
        }
        tree.insert(
            &resource.path,
            Some(checked(STANDARD.decode(resource.base64))?),
        )?;
    }
    let verified = tree.finish(host)?;
    if verified.digest() != snapshot.digest {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(verified)
}
const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    size: i64,
    mtime: i64,
    mnano: i64,
    ctime: i64,
    cnano: i64,
    mode: u32,
    links: u64,
}
fn stamp(file: &File) -> Result<Stamp> {
    let s = checked(fs::fstat(file))?;
    Ok(Stamp {
        dev: s.st_dev as u64,
        ino: s.st_ino as u64,
        size: s.st_size,
        mtime: s.st_mtime,
        mnano: s.st_mtime_nsec as i64,
        ctime: s.st_ctime,
        cnano: s.st_ctime_nsec as i64,
        mode: s.st_mode as u32,
        links: s.st_nlink as u64,
    })
}
struct Held {
    file: Arc<File>,
    parent: Option<Arc<File>>,
    name: String,
    stamp: Stamp,
}
impl Held {
    fn verify(&self) -> Result<()> {
        if stamp(&self.file)? != self.stamp {
            return Err(PublicPackageError::InvalidPackage);
        }
        if let Some(parent) = &self.parent {
            let named = checked(fs::statat(
                &**parent,
                self.name.as_str(),
                AtFlags::SYMLINK_NOFOLLOW,
            ))?;
            if (named.st_dev as u64, named.st_ino as u64) != (self.stamp.dev, self.stamp.ino) {
                return Err(PublicPackageError::InvalidPackage);
            }
        }
        Ok(())
    }
}
fn read_held(file: &File, limit: u64) -> Result<Vec<u8>> {
    let before = stamp(file)?;
    if FileType::from_raw_mode(before.mode as _) != FileType::RegularFile
        || before.links != 1
        || before.size < 0
        || before.size as u64 > limit
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut bytes = Vec::new();
    checked(file.take(limit + 1).read_to_end(&mut bytes))?;
    if bytes.len() as u64 > limit || bytes.len() as i64 != before.size || stamp(file)? != before {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(bytes)
}
fn scan(directory: Arc<File>, prefix: &str, tree: &mut Tree, held: &mut Vec<Held>) -> Result<()> {
    let entries = checked(fs::Dir::read_from(&*directory))?;
    for entry in entries {
        let entry = checked(entry)?;
        let name = checked(entry.file_name().to_str())?;
        if matches!(name, "." | "..") {
            continue;
        }
        let path = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        canonical_relative_path(&path)?;
        let named = checked(fs::statat(&*directory, name, AtFlags::SYMLINK_NOFOLLOW))?;
        let directory_kind = FileType::from_raw_mode(named.st_mode) == FileType::Directory;
        if !directory_kind && FileType::from_raw_mode(named.st_mode) != FileType::RegularFile {
            return Err(PublicPackageError::InvalidPackage);
        }
        let file = Arc::new(File::from(checked(fs::openat(
            &*directory,
            name,
            if directory_kind {
                DIR_FLAGS
            } else {
                FILE_FLAGS
            },
            Mode::empty(),
        ))?));
        let before = stamp(&file)?;
        if (before.dev, before.ino) != (named.st_dev as u64, named.st_ino as u64) {
            return Err(PublicPackageError::InvalidPackage);
        }
        held.push(Held {
            file: file.clone(),
            parent: Some(directory.clone()),
            name: name.into(),
            stamp: before,
        });
        if directory_kind {
            tree.insert(&path, None)?;
            scan(file, &path, tree, held)?;
        } else {
            tree.insert(&path, Some(read_held(&file, MAX_FILE_BYTES)?))?;
        }
    }
    Ok(())
}
pub(super) fn read_directory(path: &Path, host: &PublicPluginHost) -> Result<VerifiedPackage> {
    let root = Arc::new(File::from(checked(
        crate::atomic_file::open_data_directory(path, false),
    )?));
    let before = stamp(&root)?;
    let mut held = vec![Held {
        file: root.clone(),
        parent: None,
        name: String::new(),
        stamp: before,
    }];
    let mut tree = Tree::default();
    scan(root, "", &mut tree, &mut held)?;
    #[cfg(test)]
    BEFORE_VALIDATE.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
    for entry in &held {
        entry.verify()?;
    }
    let named = File::from(checked(crate::atomic_file::open_data_directory(
        path, false,
    ))?);
    if stamp(&named)? != before {
        return Err(PublicPackageError::InvalidPackage);
    }
    tree.finish(host)
}
pub(super) fn read_archive(path: &Path, host: &PublicPluginHost) -> Result<VerifiedPackage> {
    if path.extension().and_then(|s| s.to_str()) != Some("uipilot-plugin") {
        return Err(PublicPackageError::InvalidPackage);
    }
    let parent_path = path.parent().ok_or(PublicPackageError::InvalidPackage)?;
    let name = path.file_name().ok_or(PublicPackageError::InvalidPackage)?;
    let parent = File::from(checked(crate::atomic_file::open_data_directory(
        parent_path,
        false,
    ))?);
    let parent_stamp = stamp(&parent)?;
    let file = File::from(checked(fs::openat(
        &parent,
        name,
        FILE_FLAGS,
        Mode::empty(),
    ))?);
    let before = stamp(&file)?;
    let bytes = read_held(&file, MAX_TOTAL_BYTES)?;
    let named = checked(fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW))?;
    let current_parent = File::from(checked(crate::atomic_file::open_data_directory(
        parent_path,
        false,
    ))?);
    if (named.st_dev as u64, named.st_ino as u64) != (before.dev, before.ino)
        || stamp(&current_parent)? != parent_stamp
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let (count, central_start) = zip_preflight::standard_directory(&bytes)?;
    let mut archive = checked(ZipArchive::new(Cursor::new(bytes)))?;
    if archive.len() != count
        || archive.central_directory_start() != central_start as u64
        || archive.offset() != 0
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    if archive.len() > MAX_FILES + MAX_DIRECTORIES {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut tree = Tree::default();
    for index in 0..archive.len() {
        let mut entry = checked(archive.by_index(index))?;
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || invalid_unix_mode(entry.unix_mode(), entry.is_dir())
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        let raw = checked(std::str::from_utf8(entry.name_raw()))?.to_owned();
        if entry.is_dir() {
            if entry.size() != 0 {
                return Err(PublicPackageError::InvalidPackage);
            }
            tree.insert(
                raw.strip_suffix('/')
                    .ok_or(PublicPackageError::InvalidPackage)?,
                None,
            )?;
            continue;
        }
        canonical_relative_path(&raw)?;
        validate_package_resource_path(&raw)?;
        if entry.size() > MAX_FILE_BYTES {
            return Err(PublicPackageError::InvalidPackage);
        }
        let expected = entry.size();
        let mut bytes = Vec::new();
        checked(
            entry
                .by_ref()
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes),
        )?;
        if bytes.len() as u64 != expected || bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(PublicPackageError::InvalidPackage);
        }
        tree.insert(&raw, Some(bytes))?;
    }
    tree.finish(host)
}
#[cfg(test)]
thread_local! {static BEFORE_VALIDATE:std::cell::RefCell<Option<Box<dyn FnOnce()>>>=std::cell::RefCell::new(None);}
#[cfg(test)]
#[path = "package_macos_tests.rs"]
mod tests;
