use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive};

use super::{
    manifest::{parse_manifest, PublicManifestV1},
    PreparedPublicPlugin, PublicPackageError, PublicPackageSource, PublicPluginHost,
    PublicResource,
};

const MAX_DIRECTORIES: usize = 64;
const MAX_FILES: usize = 256;
const MAX_DEPTH: usize = 8;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 240;
const MAX_COMPONENT_BYTES: usize = 100;

static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(0);

pub(super) fn load_existing(
    package_root: &Path,
    host: &PublicPluginHost,
    expected_digest: &str,
) -> Result<(PublicManifestV1, BTreeMap<String, PublicResource>), PublicPackageError> {
    let snapshot = scan_snapshot(package_root)?;
    if snapshot.digest != expected_digest {
        return Err(PublicPackageError::InvalidPackage);
    }
    let manifest_bytes = fs::read(package_root.join("plugin.json"))
        .map_err(|_| PublicPackageError::InvalidPackage)?;
    let manifest = parse_manifest(&manifest_bytes, host)?;
    validate_manifest_entries(&manifest, &snapshot.resources)?;
    validate_css_references(package_root, &snapshot.resources)?;
    Ok((manifest, snapshot.resources))
}

pub(super) fn remove_package_tree(path: PathBuf) {
    remove_transaction(path);
}

pub(super) fn stage(
    source: PublicPackageSource,
    staging_root: &Path,
    host: &PublicPluginHost,
) -> Result<PreparedPublicPlugin, PublicPackageError> {
    fs::create_dir_all(staging_root).map_err(|_| PublicPackageError::InvalidPackage)?;
    if !ordinary_directory(staging_root) {
        return Err(PublicPackageError::InvalidPackage);
    }
    let transaction_root = create_transaction(staging_root)?;
    let guard = TransactionGuard::new(transaction_root);
    let package_root = guard.path().join("package");

    match source {
        PublicPackageSource::Archive(source) => {
            copy_archive_snapshot(&source, guard.path(), &package_root)?;
        }
        PublicPackageSource::DevelopmentDirectory(source) => {
            copy_development_snapshot(&source, &package_root)?;
        }
    }

    let first = scan_snapshot(&package_root)?;
    let manifest_bytes = fs::read(package_root.join("plugin.json"))
        .map_err(|_| PublicPackageError::InvalidPackage)?;
    let manifest = parse_manifest(&manifest_bytes, host)?;
    validate_manifest_entries(&manifest, &first.resources)?;
    validate_css_references(&package_root, &first.resources)?;
    make_snapshot_read_only(&package_root, &first.resources)?;

    let second = scan_snapshot(&package_root)?;
    if first.digest != second.digest || first.resources != second.resources {
        return Err(PublicPackageError::InvalidPackage);
    }

    Ok(PreparedPublicPlugin::new(
        guard.into_path(),
        package_root,
        manifest,
        second.digest,
        second.resources,
    ))
}

fn create_transaction(staging_root: &Path) -> Result<PathBuf, PublicPackageError> {
    for _ in 0..100 {
        let id = NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed);
        let path = staging_root.join(format!("public-prepare-{}-{id:016x}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) if ordinary_directory(&path) => return Ok(path),
            Ok(()) => {
                let _ = fs::remove_dir(&path);
                return Err(PublicPackageError::InvalidPackage);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(PublicPackageError::InvalidPackage),
        }
    }
    Err(PublicPackageError::InvalidPackage)
}

fn copy_archive_snapshot(
    source: &Path,
    transaction_root: &Path,
    package_root: &Path,
) -> Result<(), PublicPackageError> {
    if source.extension().and_then(|value| value.to_str()) != Some("uipilot-plugin")
        || !ordinary_file(source)
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let source_length = fs::metadata(source)
        .map_err(|_| PublicPackageError::InvalidPackage)?
        .len();
    if source_length > MAX_TOTAL_BYTES {
        return Err(PublicPackageError::InvalidPackage);
    }
    let archive_path = transaction_root.join("candidate.uipilot-plugin");
    let copied = fs::copy(source, &archive_path).map_err(|_| PublicPackageError::InvalidPackage)?;
    if copied != source_length || !ordinary_file(&archive_path) {
        return Err(PublicPackageError::InvalidPackage);
    }
    fs::create_dir(package_root).map_err(|_| PublicPackageError::InvalidPackage)?;
    extract_archive(&archive_path, package_root)
}

fn extract_archive(archive_path: &Path, package_root: &Path) -> Result<(), PublicPackageError> {
    let file = File::open(archive_path).map_err(|_| PublicPackageError::InvalidPackage)?;
    let mut archive = ZipArchive::new(file).map_err(|_| PublicPackageError::InvalidPackage)?;
    if archive.len() > MAX_FILES + MAX_DIRECTORIES {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut paths = HashSet::new();
    let mut files = 0_usize;
    let mut directories = BTreeMap::new();
    let mut total_bytes = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        let raw_name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        let is_directory = entry.is_dir();
        let name = if is_directory {
            raw_name
                .strip_suffix('/')
                .ok_or(PublicPackageError::InvalidPackage)?
        } else {
            raw_name
        };
        let canonical = canonical_relative_path(name)?;
        if !paths.insert(crate::file_index::fold_name(&canonical))
            || invalid_unix_mode(entry.unix_mode(), is_directory)
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        register_parent_directories(&canonical, &mut directories)?;
        let destination = package_root.join(path_from_canonical(&canonical));

        if is_directory {
            register_directory(&canonical, &mut directories)?;
            fs::create_dir_all(&destination).map_err(|_| PublicPackageError::InvalidPackage)?;
            continue;
        }
        validate_public_resource_path(&canonical)?;
        files = files
            .checked_add(1)
            .filter(|value| *value <= MAX_FILES)
            .ok_or(PublicPackageError::InvalidPackage)?;
        if entry.size() > MAX_FILE_BYTES {
            return Err(PublicPackageError::InvalidPackage);
        }
        total_bytes = total_bytes
            .checked_add(entry.size())
            .filter(|value| *value <= MAX_TOTAL_BYTES)
            .ok_or(PublicPackageError::InvalidPackage)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|_| PublicPackageError::InvalidPackage)?;
        }
        let expected_size = entry.size();
        let mut output =
            File::create(&destination).map_err(|_| PublicPackageError::InvalidPackage)?;
        let copied = io::copy(&mut entry.by_ref().take(MAX_FILE_BYTES + 1), &mut output)
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        output
            .flush()
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        if copied != expected_size || copied > MAX_FILE_BYTES || !ordinary_file(&destination) {
            return Err(PublicPackageError::InvalidPackage);
        }
    }
    Ok(())
}

fn copy_development_snapshot(source: &Path, package_root: &Path) -> Result<(), PublicPackageError> {
    if !ordinary_directory(source) {
        return Err(PublicPackageError::InvalidPackage);
    }
    fs::create_dir(package_root).map_err(|_| PublicPackageError::InvalidPackage)?;
    let mut context = CopyContext::default();
    copy_directory(source, source, package_root, &mut context)
}

#[derive(Default)]
struct CopyContext {
    paths: HashSet<String>,
    directories: usize,
    files: usize,
    total_bytes: u64,
}

fn copy_directory(
    root: &Path,
    source: &Path,
    destination_root: &Path,
    context: &mut CopyContext,
) -> Result<(), PublicPackageError> {
    for entry in fs::read_dir(source).map_err(|_| PublicPackageError::InvalidPackage)? {
        let entry = entry.map_err(|_| PublicPackageError::InvalidPackage)?;
        let source_path = entry.path();
        let relative = source_path
            .strip_prefix(root)
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        let raw = relative
            .to_str()
            .ok_or(PublicPackageError::InvalidPackage)?
            .replace('\\', "/");
        let canonical = canonical_relative_path(&raw)?;
        if !context
            .paths
            .insert(crate::file_index::fold_name(&canonical))
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|_| PublicPackageError::InvalidPackage)?;
        if is_reparse_point(&metadata) {
            return Err(PublicPackageError::InvalidPackage);
        }
        let destination = destination_root.join(path_from_canonical(&canonical));
        if metadata.is_dir() {
            context.directories = context
                .directories
                .checked_add(1)
                .filter(|value| *value <= MAX_DIRECTORIES)
                .ok_or(PublicPackageError::InvalidPackage)?;
            fs::create_dir(&destination).map_err(|_| PublicPackageError::InvalidPackage)?;
            copy_directory(root, &source_path, destination_root, context)?;
        } else if metadata.is_file() {
            validate_public_resource_path(&canonical)?;
            context.files = context
                .files
                .checked_add(1)
                .filter(|value| *value <= MAX_FILES)
                .ok_or(PublicPackageError::InvalidPackage)?;
            if metadata.len() > MAX_FILE_BYTES {
                return Err(PublicPackageError::InvalidPackage);
            }
            context.total_bytes = context
                .total_bytes
                .checked_add(metadata.len())
                .filter(|value| *value <= MAX_TOTAL_BYTES)
                .ok_or(PublicPackageError::InvalidPackage)?;
            let copied = fs::copy(&source_path, &destination)
                .map_err(|_| PublicPackageError::InvalidPackage)?;
            if copied != metadata.len()
                || !ordinary_file(&destination)
                || !ordinary_file(&source_path)
            {
                return Err(PublicPackageError::InvalidPackage);
            }
        } else {
            return Err(PublicPackageError::InvalidPackage);
        }
    }
    Ok(())
}

struct Snapshot {
    digest: String,
    resources: BTreeMap<String, PublicResource>,
}

pub(super) fn revalidate_snapshot(
    root: &Path,
    expected_digest: &str,
    expected_resources: &BTreeMap<String, PublicResource>,
) -> Result<(), PublicPackageError> {
    let snapshot = scan_snapshot(root)?;
    if snapshot.digest != expected_digest || &snapshot.resources != expected_resources {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}

fn scan_snapshot(root: &Path) -> Result<Snapshot, PublicPackageError> {
    if !ordinary_directory(root) {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut context = ScanContext::default();
    scan_directory(root, root, &mut context)?;
    if !context.resources.contains_key("plugin.json") {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut tree = Sha256::new();
    tree.update(b"UIPILOT-PUBLIC-PACKAGE\0SHA256-TREE-V1\0");
    tree.update(((context.directories.len() + context.resources.len()) as u32).to_le_bytes());
    for path in &context.directories {
        tree.update([1]);
        tree.update((path.len() as u32).to_le_bytes());
        tree.update(path.as_bytes());
    }
    for (path, resource) in &context.resources {
        tree.update([2]);
        tree.update((path.len() as u32).to_le_bytes());
        tree.update(path.as_bytes());
        tree.update(resource.length.to_le_bytes());
        tree.update(resource.sha256.as_bytes());
        tree.update(resource.mime.as_bytes());
    }
    Ok(Snapshot {
        digest: lower_hex(&tree.finalize()),
        resources: context.resources,
    })
}

#[derive(Default)]
struct ScanContext {
    paths: HashSet<String>,
    directories: BTreeSet<String>,
    files: usize,
    total_bytes: u64,
    resources: BTreeMap<String, PublicResource>,
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    context: &mut ScanContext,
) -> Result<(), PublicPackageError> {
    for entry in fs::read_dir(directory).map_err(|_| PublicPackageError::InvalidPackage)? {
        let entry = entry.map_err(|_| PublicPackageError::InvalidPackage)?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        let raw = relative
            .to_str()
            .ok_or(PublicPackageError::InvalidPackage)?
            .replace('\\', "/");
        let canonical = canonical_relative_path(&raw)?;
        if !context
            .paths
            .insert(crate::file_index::fold_name(&canonical))
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| PublicPackageError::InvalidPackage)?;
        if is_reparse_point(&metadata) {
            return Err(PublicPackageError::InvalidPackage);
        }
        if metadata.is_dir() {
            context.directories.insert(canonical);
            if context.directories.len() > MAX_DIRECTORIES {
                return Err(PublicPackageError::InvalidPackage);
            }
            scan_directory(root, &path, context)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(PublicPackageError::InvalidPackage);
        }
        let mime = validate_public_resource_path(&canonical)?;
        context.files = context
            .files
            .checked_add(1)
            .filter(|value| *value <= MAX_FILES)
            .ok_or(PublicPackageError::InvalidPackage)?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(PublicPackageError::InvalidPackage);
        }
        let bytes = fs::read(&path).map_err(|_| PublicPackageError::InvalidPackage)?;
        if bytes.len() as u64 != metadata.len() || !ordinary_file(&path) {
            return Err(PublicPackageError::InvalidPackage);
        }
        context.total_bytes = context
            .total_bytes
            .checked_add(bytes.len() as u64)
            .filter(|value| *value <= MAX_TOTAL_BYTES)
            .ok_or(PublicPackageError::InvalidPackage)?;
        context.resources.insert(
            canonical,
            PublicResource {
                mime,
                length: bytes.len() as u64,
                sha256: lower_hex(&Sha256::digest(&bytes)),
            },
        );
    }
    Ok(())
}

fn validate_manifest_entries(
    manifest: &PublicManifestV1,
    resources: &BTreeMap<String, PublicResource>,
) -> Result<(), PublicPackageError> {
    if !resources.contains_key(&manifest.runtime.entry)
        || manifest
            .window
            .as_ref()
            .is_some_and(|window| !resources.contains_key(&window.entry))
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}

fn validate_css_references(
    root: &Path,
    resources: &BTreeMap<String, PublicResource>,
) -> Result<(), PublicPackageError> {
    for (path, resource) in resources {
        if resource.mime != "text/css" {
            continue;
        }
        let css = fs::read_to_string(root.join(path_from_canonical(path)))
            .map_err(|_| PublicPackageError::InvalidPackage)?;
        let mut folded = css.clone();
        folded.make_ascii_lowercase();

        let mut offset = 0;
        while let Some(index) = folded[offset..].find("url(") {
            let start = offset + index + 4;
            let end = folded[start..]
                .find(')')
                .map(|end| start + end)
                .ok_or(PublicPackageError::InvalidPackage)?;
            let reference = css[start..end]
                .trim()
                .trim_matches(|character| character == '\'' || character == '"');
            validate_local_css_reference(path, reference, resources)?;
            offset = end + 1;
        }

        let mut offset = 0;
        while let Some(index) = folded[offset..].find("@import") {
            let start = offset + index + "@import".len();
            let end = folded[start..]
                .find(';')
                .map(|end| start + end)
                .ok_or(PublicPackageError::InvalidPackage)?;
            let import = css[start..end].trim();
            if !import.to_ascii_lowercase().starts_with("url(") {
                let (reference, remainder) = take_css_string(import)?;
                if !remainder.trim().is_empty() {
                    return Err(PublicPackageError::InvalidPackage);
                }
                validate_local_css_reference(path, reference, resources)?;
            }
            offset = end + 1;
        }
    }
    Ok(())
}

fn take_css_string(value: &str) -> Result<(&str, &str), PublicPackageError> {
    let quote = value
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
        .ok_or(PublicPackageError::InvalidPackage)?;
    let body = &value[quote.len_utf8()..];
    let end = body.find(quote).ok_or(PublicPackageError::InvalidPackage)?;
    Ok((&body[..end], &body[end + quote.len_utf8()..]))
}
fn validate_local_css_reference(
    stylesheet: &str,
    reference: &str,
    resources: &BTreeMap<String, PublicResource>,
) -> Result<(), PublicPackageError> {
    if reference.is_empty()
        || reference.contains(['?', '#', '\\', ':', '%'])
        || reference.starts_with('/')
        || reference.chars().any(char::is_control)
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut components = stylesheet
        .rsplit_once('/')
        .map_or_else(Vec::new, |(parent, _)| {
            parent.split('/').collect::<Vec<_>>()
        });
    for component in reference.split('/') {
        match component {
            "" => return Err(PublicPackageError::InvalidPackage),
            "." => {}
            ".." => {
                components.pop().ok_or(PublicPackageError::InvalidPackage)?;
            }
            component => components.push(component),
        }
    }
    let canonical = canonical_relative_path(&components.join("/"))?;
    if !resources.contains_key(&canonical) {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}
fn make_snapshot_read_only(
    root: &Path,
    resources: &BTreeMap<String, PublicResource>,
) -> Result<(), PublicPackageError> {
    for path in resources.keys() {
        let full_path = root.join(path_from_canonical(path));
        let mut permissions = fs::metadata(&full_path)
            .map_err(|_| PublicPackageError::InvalidPackage)?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(full_path, permissions)
            .map_err(|_| PublicPackageError::InvalidPackage)?;
    }
    Ok(())
}

pub(super) fn remove_transaction(path: PathBuf) {
    make_tree_writable(&path);
    let _ = fs::remove_dir_all(path);
}

fn make_tree_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !is_reparse_point(&metadata) {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_tree_writable(&entry.path());
            }
        }
    } else if metadata.is_file() && !is_reparse_point(&metadata) {
        make_file_writable(path);
    }
}

pub(super) fn make_file_writable(path: &Path) {
    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o200);
        let _ = fs::set_permissions(path, permissions);
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{
                GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY,
                FILE_FLAGS_AND_ATTRIBUTES, INVALID_FILE_ATTRIBUTES,
            },
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let path = PCWSTR(wide.as_ptr());
        let attributes = unsafe { GetFileAttributesW(path) };
        if attributes != INVALID_FILE_ATTRIBUTES {
            let _ = unsafe {
                SetFileAttributesW(
                    path,
                    FILE_FLAGS_AND_ATTRIBUTES(attributes & !FILE_ATTRIBUTE_READONLY.0),
                )
            };
        }
    }
}
fn canonical_relative_path(value: &str) -> Result<String, PublicPackageError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.len() > MAX_PATH_BYTES
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let components = value.split('/').collect::<Vec<_>>();
    if components.is_empty() || components.len() > MAX_DEPTH {
        return Err(PublicPackageError::InvalidPackage);
    }
    if components.iter().any(|component| {
        component.is_empty()
            || *component == "."
            || *component == ".."
            || component.len() > MAX_COMPONENT_BYTES
            || component.ends_with(['.', ' '])
            || component
                .chars()
                .any(|character| character.is_control() || "<>:\"|?*".contains(character))
            || windows_reserved_component(component)
            || !component.nfc().eq(component.chars())
    }) {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(components.join("/"))
}

fn windows_reserved_component(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or_default();
    let lower = stem.to_ascii_lowercase();
    matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || lower.get(3..).is_some_and(|suffix| {
            (lower.starts_with("com") || lower.starts_with("lpt"))
                && suffix.len() == 1
                && matches!(suffix.as_bytes()[0], b'1'..=b'9')
        })
}

fn register_parent_directories(
    path: &str,
    directories: &mut BTreeMap<String, String>,
) -> Result<(), PublicPackageError> {
    let components = path.split('/').collect::<Vec<_>>();
    for end in 1..components.len() {
        register_directory(&components[..end].join("/"), directories)?;
    }
    Ok(())
}

fn register_directory(
    path: &str,
    directories: &mut BTreeMap<String, String>,
) -> Result<(), PublicPackageError> {
    let folded = crate::file_index::fold_name(path);
    if directories
        .get(&folded)
        .is_some_and(|existing| existing != path)
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    directories.insert(folded, path.to_owned());
    if directories.len() > MAX_DIRECTORIES {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}
fn validate_public_resource_path(path: &str) -> Result<&'static str, PublicPackageError> {
    if path == "plugin.json" {
        return Ok("application/json");
    }
    let basename = path
        .rsplit('/')
        .next()
        .ok_or(PublicPackageError::InvalidPackage)?;
    let parts = basename.split('.').collect::<Vec<_>>();
    if parts.len() != 2 || parts[0].is_empty() {
        return Err(PublicPackageError::InvalidPackage);
    }
    match parts[1] {
        "html" => Ok("text/html"),
        "js" => Ok("text/javascript"),
        "css" => Ok("text/css"),
        _ => Err(PublicPackageError::InvalidPackage),
    }
}

fn path_from_canonical(path: &str) -> PathBuf {
    path.split('/').collect()
}

fn invalid_unix_mode(mode: Option<u32>, directory: bool) -> bool {
    let Some(mode) = mode else {
        return false;
    };
    let file_type = mode & 0o170000;
    file_type != 0 && file_type != if directory { 0o040000 } else { 0o100000 }
}

fn ordinary_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !is_reparse_point(&metadata))
}

fn ordinary_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !is_reparse_point(&metadata))
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[usize::from(byte >> 4)] as char);
        value.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    value
}

struct TransactionGuard {
    path: Option<PathBuf>,
}

impl TransactionGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> &Path {
        self.path.as_deref().expect("transaction path missing")
    }

    fn into_path(mut self) -> PathBuf {
        self.path.take().expect("transaction path missing")
    }
}

impl Drop for TransactionGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            remove_transaction(path);
        }
    }
}
