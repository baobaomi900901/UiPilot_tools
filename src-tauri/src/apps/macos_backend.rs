use std::{
    collections::BTreeMap,
    fs::{self, File, Metadata},
    io::{self, Read},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use block2::RcBlock;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSBundle, NSError, NSString, NSURL};
use rustix::fs::{open, Mode, OFlags};
use sha2::{Digest, Sha256};

use super::{
    Application, ApplicationLaunchTarget, DiscoveryDiagnostics, DiscoveryError, DiscoverySnapshot,
};

const MAX_ENTRIES: usize = 65_536;
const MAX_DEPTH: usize = 8;
const PLIST_LIMIT: usize = 1024 * 1024;
const ICON_LIMIT: usize = 8 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 49_134;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
}

impl From<&Metadata> for FileIdentity {
    fn from(meta: &Metadata) -> Self {
        Self { device: meta.dev(), inode: meta.ino(), size: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()) }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BundleTarget {
    root: PathBuf,
    path: PathBuf,
    identifier: String,
    directory: FileIdentity,
    executable: FileIdentity,
    info: FileIdentity,
}

fn bounded_file(path: &Path, limit: usize) -> Option<(Vec<u8>, FileIdentity)> {
    if fs::canonicalize(path).ok()? != path { return None; }
    let file = File::from(open(path, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK, Mode::empty()).ok()?);
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > limit as u64 { return None; }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then(|| (bytes, (&meta).into()))
}

fn component(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(part)) if part == name)
        && components.next().is_none() && !name.contains('\0')
}

fn inspect(root: &Path, path: &Path) -> Option<(BundleTarget, plist::Dictionary)> {
    if !path.starts_with(root) || fs::canonicalize(path).ok()? != path { return None; }
    let dir = fs::symlink_metadata(path).ok()?;
    if !dir.is_dir() { return None; }
    let contents = path.join("Contents");
    let (bytes, info) = bounded_file(&contents.join("Info.plist"), PLIST_LIMIT)?;
    let plist = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?.into_dictionary()?;
    let identifier = plist.get("CFBundleIdentifier")?.as_string()?.trim();
    let exe = plist.get("CFBundleExecutable")?.as_string()?;
    if identifier.len() > 255 || identifier.split('.').any(|part| {
        part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
        || !component(exe) || plist.get("CFBundlePackageType").is_some_and(|v| v.as_string() != Some("APPL"))
    { return None; }
    let executable = contents.join("MacOS").join(exe);
    if fs::canonicalize(&executable).ok()? != executable { return None; }
    let meta = fs::symlink_metadata(&executable).ok()?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 { return None; }
    let target = BundleTarget {
        root: root.to_owned(), path: path.to_owned(), identifier: identifier.to_owned(),
        directory: (&dir).into(), executable: (&meta).into(), info,
    };
    Some((target, plist))
}

fn localized_name(path: &Path) -> Option<String> {
    let url = NSURL::from_file_path(path)?;
    let bundle = NSBundle::bundleWithURL(&url)?;
    for key in ["CFBundleDisplayName", "CFBundleName"] {
        if let Some(name) = bundle.objectForInfoDictionaryKey(&NSString::from_str(key))
            .and_then(|value| value.downcast::<NSString>().ok())
        {
            let name = name.to_string();
            if !name.trim().is_empty() { return Some(name); }
        }
    }
    None
}

fn bundle_icon(path: &Path, plist: &plist::Dictionary) -> Option<String> {
    let name = plist.get("CFBundleIconFile")?.as_string()?;
    if !component(name) { return None; }
    let name = if name.ends_with(".icns") { name.to_owned() } else { format!("{name}.icns") };
    let (bytes, _) = bounded_file(&path.join("Contents/Resources").join(name), ICON_LIMIT)?;
    if bytes.get(..4) != Some(b"icns") { return None; }
    let end = u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let mut entries = bytes.get(8..end)?;
    while entries.len() >= 8 {
        let size = u32::from_be_bytes(entries[4..8].try_into().ok()?) as usize;
        let png = entries.get(8..size)?;
        if png.starts_with(b"\x89PNG\r\n\x1a\n") && png.get(12..16) == Some(b"IHDR")
            && png.len() <= MAX_PNG_BYTES {
            let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
            let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
            if (1..=128).contains(&width) && (1..=128).contains(&height) {
                return Some(format!("data:image/png;base64,{}", STANDARD.encode(png)));
            }
        }
        entries = entries.get(size..)?;
    }
    None
}

pub(crate) fn application(root: &Path, path: &Path) -> Option<Application> {
    let (target, plist) = inspect(root, path)?;
    let display_name = localized_name(path)
        .or_else(|| plist.get("CFBundleDisplayName")?.as_string().map(str::to_owned))
        .or_else(|| plist.get("CFBundleName")?.as_string().map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .or_else(|| path.file_stem()?.to_str().map(str::to_owned))?;
    let digest = Sha256::digest(format!("macos-bundle-v1\0{}", target.identifier.to_lowercase()));
    Some(Application { app_id: format!("app-{digest:x}"), display_name,
        target: ApplicationLaunchTarget::Bundle(target), icon: bundle_icon(path, &plist), use_count: 0 })
}

pub(crate) fn discover() -> Result<DiscoverySnapshot, DiscoveryError> {
    let mut roots = vec![PathBuf::from("/System/Applications"), PathBuf::from("/Applications")];
    if let Some(home) = std::env::home_dir() { roots.push(home.join("Applications")); }
    discover_roots(&roots)
}

fn discover_roots(roots: &[PathBuf]) -> Result<DiscoverySnapshot, DiscoveryError> {
    let mut diagnostics = DiscoveryDiagnostics::default();
    let mut accepted = BTreeMap::new();
    let mut accessible = false;
    let mut visited = 0;
    for root in roots {
        if fs::canonicalize(root).ok().as_deref() != Some(root) {
            diagnostics.missing_roots += 1;
            continue;
        }
        let mut candidates = Vec::new();
        let mut directories = vec![(root.clone(), 0)];
        while let Some((directory, depth)) = directories.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                diagnostics.inaccessible_entries += 1;
                continue;
            };
            accessible = true;
            for entry in entries {
                visited += 1;
                if visited > MAX_ENTRIES { return Err(DiscoveryError::RootUnavailable); }
                let Ok(entry) = entry else { diagnostics.inaccessible_entries += 1; continue; };
                let Ok(kind) = entry.file_type() else { diagnostics.inaccessible_entries += 1; continue; };
                if kind.is_symlink() { diagnostics.reparse_entries += 1; continue; }
                if !kind.is_dir() { continue; }
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) {
                    candidates.push(path);
                } else if depth < MAX_DEPTH && !entry.file_name().to_string_lossy().starts_with('.') {
                    directories.push((path, depth + 1));
                }
            }
        }
        candidates.sort();
        for path in candidates {
            if let Some(app) = application(root, &path) { accepted.entry(app.app_id.clone()).or_insert(app); }
        }
    }
    if !accessible { return Err(DiscoveryError::RootUnavailable); }
    Ok(DiscoverySnapshot { applications: accepted.into_values().collect(), diagnostics })
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum OpenOutcome { LaunchRequested, ActivationRequested }

fn with_verified_url<T>(target: &BundleTarget, open: impl FnOnce(&NSURL) -> io::Result<T>) -> io::Result<T> {
    fn unavailable() -> io::Error { io::Error::other("application entry unavailable") }
    let reference = NSURL::from_file_path(&target.path)
        .and_then(|url| url.fileReferenceURL()).ok_or_else(unavailable)?;
    let path = reference.filePathURL().and_then(|url| url.to_file_path()).ok_or_else(unavailable)?;
    if target.path != path || inspect(&target.root, &path).map(|(current, _)| current).as_ref() != Some(target) {
        return Err(unavailable());
    }
    open(&reference)
}

pub(crate) fn open_application(target: &BundleTarget, admit: &dyn Fn() -> io::Result<()>) -> io::Result<OpenOutcome> {
    with_verified_url(target, |reference| {
        let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(&target.identifier));
        let existing = running.iter().any(|app| !app.isTerminated()
            && app.bundleURL().and_then(|url| url.to_file_path()).as_ref() == Some(&target.path));
        let configuration = NSWorkspaceOpenConfiguration::configuration();
        configuration.setActivates(true);
        // A running copy with the same identifier must not receive this request.
        configuration.setCreatesNewApplicationInstance(!existing && running.iter().any(|app| !app.isTerminated()));
        let expected = target.path.clone();
        let (sent, received) = std::sync::mpsc::sync_channel(1);
        let done = RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
            // AppKit supplies the NSRunningApplication for the duration of this callback.
            let actual = unsafe { app.as_ref() }.and_then(|app| app.bundleURL())
                .and_then(|url| url.to_file_path());
            let _ = sent.send(error.is_null() && actual.as_ref() == Some(&expected));
        });
        admit()?;
        NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
            reference, &configuration, Some(&done));
        if received.recv_timeout(std::time::Duration::from_secs(30)) != Ok(true) {
            return Err(io::Error::other("Launch Services did not confirm the requested application"));
        }
        Ok(if existing { OpenOutcome::ActivationRequested } else { OpenOutcome::LaunchRequested })
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    pub(crate) struct Fixture(pub(crate) PathBuf);
    impl Fixture {
        pub(crate) fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap()
                .join(format!("uipilot-app-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        pub(crate) fn bundle(&self, relative: &str, id: &str, name: &str) -> PathBuf {
            let path = self.0.join(relative);
            let contents = path.join("Contents");
            fs::create_dir_all(contents.join("MacOS")).unwrap();
            let mut plist = plist::Dictionary::new();
            for (key, value) in [("CFBundleIdentifier", id), ("CFBundleExecutable", "test"),
                ("CFBundlePackageType", "APPL"), ("CFBundleDisplayName", name)] {
                plist.insert(key.into(), plist::Value::String(value.into()));
            }
            plist::Value::Dictionary(plist).to_file_binary(contents.join("Info.plist")).unwrap();
            let exe = contents.join("MacOS/test");
            fs::write(&exe, b"test-only marker").unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn roots_and_paths_break_ties_and_nested_bundles_and_links_are_skipped() {
        let f = Fixture::new();
        f.bundle("system/Utilities/A.app", "com.fixture.one", "中文 优先");
        f.bundle("system/Utilities/Z.app", "com.fixture.one", "同目录重复项");
        f.bundle("common/Other.app", "com.fixture.one", "共享重复项");
        f.bundle("user/Other.app", "com.fixture.two", "中文 优先");
        f.bundle("system/Utilities/A.app/Contents/Hidden.app", "com.fixture.hidden", "nested");
        f.bundle("outside/Escape.app", "com.fixture.escape", "escaped");
        let bad = f.bundle("user/Broken.app", "com.fixture.broken", "bad");
        fs::remove_file(bad.join("Contents/MacOS/test")).unwrap();
        symlink(f.0.join("outside"), f.0.join("user/escape")).unwrap();
        symlink(f.0.join("user"), f.0.join("user/loop")).unwrap();
        let roots = ["system", "common", "user"].map(|root| f.0.join(root));
        let apps = discover_roots(&roots).unwrap().applications;
        assert_eq!(apps.len(), 2);
        assert_eq!(apps.iter().filter(|app| app.display_name == "中文 优先").count(), 2);
        let expected = application(&roots[0], &f.0.join("system/Utilities/A.app")).unwrap();
        assert!(apps.contains(&expected));
        assert_eq!(expected.app_id.len(), 68);
        assert!(expected.icon.is_none());
        assert_eq!(super::super::rank(&apps, "中文").len(), 2);
        assert!(discover_roots(&[f.0.join("absent")]).is_err());
    }

    #[test]
    fn replacement_and_deletion_reject_the_target_before_open() {
        let f = Fixture::new();
        let path = f.bundle("Space 中文.app", "com.fixture.open", "Space 中文");
        let (target, _) = inspect(&f.0, &path).unwrap();
        with_verified_url(&target, |url| { assert!(url.isFileReferenceURL()); Ok(()) }).unwrap();
        let exe = path.join("Contents/MacOS/test");
        fs::rename(&exe, path.join("old-executable")).unwrap();
        fs::write(&exe, b"replacement").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(with_verified_url::<()>(&target, |_| panic!("replacement must not open")).is_err());
        fs::rename(&path, f.0.join("moved.app")).unwrap();
        assert!(with_verified_url::<()>(&target, |_| panic!("absent bundle must not open")).is_err());
        symlink(f.0.join("moved.app"), &path).unwrap();
        assert!(with_verified_url::<()>(&target, |_| panic!("link must not open")).is_err());
    }

    #[test]
    fn real_system_roots_return_searchable_bundles_without_opening_them() {
        let apps = objc2::rc::autoreleasepool(|_| discover()).unwrap().applications;
        assert!(!apps.is_empty());
        assert!(apps.iter().any(|app| matches!(&app.target, ApplicationLaunchTarget::Bundle(target)
            if target.root == Path::new("/System/Applications"))));
        let icons = apps.iter().filter(|app| app.icon.is_some()).count();
        eprintln!("Mac application discovery returned {} unique bundles and {} icons", apps.len(), icons);
    }
}
