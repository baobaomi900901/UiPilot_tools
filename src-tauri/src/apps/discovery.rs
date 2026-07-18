use std::{
    collections::HashSet,
    ffi::{c_void, OsString},
    fs::{self, Metadata},
    io,
    os::windows::{ffi::OsStringExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

use windows::{
    core::GUID,
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
        System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_Programs, SHGetKnownFolderPath, KF_FLAG_DONT_VERIFY,
            KNOWN_FOLDER_FLAG,
        },
    },
};

use super::{
    app_id,
    shortcut::{load_shortcut, ShortcutError, ShortcutMetadata},
    Application, DiscoveryDiagnostics, DiscoveryError, DiscoverySnapshot, RootKind, StartMenuRoot,
};
use crate::{model::ResultItem, result_registry::ResultAction};

struct KnownFolderPath(*mut u16);

impl Drop for KnownFolderPath {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast::<c_void>())) };
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn known_folder_roots() -> Result<[StartMenuRoot; 2], DiscoveryError> {
    known_folder_roots_with(known_folder_path)
}

fn known_folder_path(
    id: &GUID,
    flags: KNOWN_FOLDER_FLAG,
    token: Option<HANDLE>,
) -> Result<PathBuf, ()> {
    let raw = unsafe { SHGetKnownFolderPath(id, flags, token) }.map_err(|_| ())?;
    let owned = KnownFolderPath(raw.0);
    let mut length = 0;
    unsafe {
        while *owned.0.add(length) != 0 {
            length += 1;
        }
        Ok(PathBuf::from(OsString::from_wide(
            std::slice::from_raw_parts(owned.0, length),
        )))
    }
}

fn known_folder_roots_with<F>(mut provider: F) -> Result<[StartMenuRoot; 2], DiscoveryError>
where
    F: FnMut(&GUID, KNOWN_FOLDER_FLAG, Option<HANDLE>) -> Result<PathBuf, ()>,
{
    let user = provider(&FOLDERID_Programs, KF_FLAG_DONT_VERIFY, None)
        .map_err(|_| DiscoveryError::KnownFolderQuery)?;
    let common = provider(&FOLDERID_CommonPrograms, KF_FLAG_DONT_VERIFY, None)
        .map_err(|_| DiscoveryError::KnownFolderQuery)?;
    Ok([
        StartMenuRoot {
            kind: RootKind::User,
            path: user,
        },
        StartMenuRoot {
            kind: RootKind::Common,
            path: common,
        },
    ])
}

fn is_reparse(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

fn classify_root_metadata(
    metadata: io::Result<Metadata>,
) -> Result<Option<Metadata>, DiscoveryError> {
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DiscoveryError::RootUnavailable),
    };
    if is_reparse(&metadata) {
        return Err(DiscoveryError::RootReparsePoint);
    }
    if !metadata.is_dir() {
        return Err(DiscoveryError::RootNotDirectory);
    }
    Ok(Some(metadata))
}

fn classify_child_metadata(
    metadata: io::Result<Metadata>,
    diagnostics: &mut DiscoveryDiagnostics,
) -> Option<Metadata> {
    match metadata {
        Ok(metadata) => Some(metadata),
        Err(_) => {
            diagnostics.inaccessible_entries += 1;
            None
        }
    }
}

fn normalize_relative_path(path: &Path) -> Option<String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_str()?),
            _ => return None,
        }
    }
    (!components.is_empty()).then(|| components.join("\\").to_lowercase())
}

struct Candidate {
    root_kind: RootKind,
    shortcut: PathBuf,
    normalized_relative: String,
    display_name: String,
}

fn collect_candidates(
    root_kind: RootKind,
    root: &Path,
    directory: &Path,
    diagnostics: &mut DiscoveryDiagnostics,
    candidates: &mut Vec<Candidate>,
) -> io::Result<()> {
    let entries = fs::read_dir(directory)?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                diagnostics.inaccessible_entries += 1;
                continue;
            }
        };
        let path = entry.path();
        let Some(metadata) = classify_child_metadata(fs::symlink_metadata(&path), diagnostics)
        else {
            continue;
        };
        if is_reparse(&metadata) {
            diagnostics.reparse_entries += 1;
            continue;
        }
        let relative = match path.strip_prefix(root) {
            Ok(relative) => relative,
            Err(_) => {
                diagnostics.inaccessible_entries += 1;
                continue;
            }
        };
        let Some(normalized_relative) = normalize_relative_path(relative) else {
            diagnostics.non_unicode_entries += 1;
            continue;
        };
        if metadata.is_dir() {
            if collect_candidates(root_kind, root, &path, diagnostics, candidates).is_err() {
                diagnostics.inaccessible_entries += 1;
            }
            continue;
        }
        if !metadata.is_file()
            || !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        {
            continue;
        }
        let Some(display_name) = path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            diagnostics.non_unicode_entries += 1;
            continue;
        };
        candidates.push(Candidate {
            root_kind,
            shortcut: path,
            normalized_relative,
            display_name,
        });
    }
    Ok(())
}

fn discover_from_roots<I, F>(roots: I, mut resolve: F) -> Result<DiscoverySnapshot, DiscoveryError>
where
    I: IntoIterator<Item = StartMenuRoot>,
    F: FnMut(&Path) -> Result<ShortcutMetadata, ShortcutError>,
{
    let mut diagnostics = DiscoveryDiagnostics::default();
    let mut candidates = Vec::new();
    for root in roots {
        if classify_root_metadata(fs::symlink_metadata(&root.path))?.is_none() {
            diagnostics.missing_roots += 1;
            continue;
        }
        collect_candidates(
            root.kind,
            &root.path,
            &root.path,
            &mut diagnostics,
            &mut candidates,
        )
        .map_err(|_| DiscoveryError::RootUnavailable)?;
    }
    candidates.sort_by(|left, right| {
        left.normalized_relative
            .cmp(&right.normalized_relative)
            .then_with(|| left.root_kind.identity().cmp(right.root_kind.identity()))
    });

    let mut ids = HashSet::with_capacity(candidates.len());
    let mut applications = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let metadata = match resolve(&candidate.shortcut) {
            Ok(metadata) => metadata,
            Err(ShortcutError::InvalidShortcut) => {
                diagnostics.invalid_shortcuts += 1;
                continue;
            }
            Err(ShortcutError::ComUnavailable) => return Err(DiscoveryError::ComUnavailable),
        };
        if metadata.executable.is_none() {
            diagnostics.unmapped_executables += 1;
        }
        let id = app_id(candidate.root_kind, &candidate.normalized_relative)?;
        if !ids.insert(id.clone()) {
            return Err(DiscoveryError::DuplicateAppId);
        }
        applications.push(Application {
            app_id: id,
            display_name: candidate.display_name,
            shortcut: candidate.shortcut,
            executable: metadata.executable,
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        });
    }
    Ok(DiscoverySnapshot {
        applications,
        diagnostics,
    })
}

pub(crate) fn discover() -> Result<DiscoverySnapshot, DiscoveryError> {
    let roots = known_folder_roots()?;
    if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
        return Err(DiscoveryError::ComUnavailable);
    }
    let _com = ComGuard;
    discover_from_roots(roots, load_shortcut)
}

pub(crate) fn registry_entry(application: &Application) -> (ResultItem, ResultAction) {
    (
        ResultItem {
            result_id: String::new(),
            title: application.display_name.clone(),
            subtitle: None,
            icon: None,
        },
        ResultAction::LaunchApplication {
            app_id: application.app_id.clone(),
            shortcut: application.shortcut.clone(),
            executable: application.executable.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        ffi::OsString,
        fs, io,
        os::windows::ffi::OsStringExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use windows::Win32::UI::Shell::{
        FOLDERID_CommonPrograms, FOLDERID_Programs, KF_FLAG_DONT_VERIFY,
    };

    use super::{
        classify_child_metadata, classify_root_metadata, discover_from_roots,
        known_folder_roots_with, normalize_relative_path,
    };
    use crate::apps::shortcut::{ShortcutError, ShortcutMetadata};
    use crate::{
        apps::{Application, DiscoveryDiagnostics, DiscoveryError, RootKind, StartMenuRoot},
        result_registry::ResultAction,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-discovery-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn child(&self, relative: &str) -> PathBuf {
            self.0.join(relative)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn roots(user: &Path, common: &Path) -> [StartMenuRoot; 2] {
        [
            StartMenuRoot {
                kind: RootKind::User,
                path: user.to_path_buf(),
            },
            StartMenuRoot {
                kind: RootKind::Common,
                path: common.to_path_buf(),
            },
        ]
    }

    fn no_target(_: &Path) -> Result<ShortcutMetadata, ShortcutError> {
        Ok(ShortcutMetadata { executable: None })
    }

    fn titles(applications: &[Application]) -> Vec<&str> {
        applications
            .iter()
            .map(|application| application.display_name.as_str())
            .collect()
    }

    #[test]
    fn known_folder_provider_uses_exact_ids_flags_and_null_token() {
        let mut calls = Vec::new();
        let result = known_folder_roots_with(|id, flags, token| {
            calls.push((*id, flags, token));
            Ok(PathBuf::from(if *id == FOLDERID_Programs {
                r"C:\KnownUser"
            } else {
                r"C:\KnownCommon"
            }))
        })
        .unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, FOLDERID_Programs);
        assert_eq!(calls[1].0, FOLDERID_CommonPrograms);
        assert!(calls
            .iter()
            .all(|(_, flags, token)| *flags == KF_FLAG_DONT_VERIFY && token.is_none()));
        assert_eq!(result[0].kind, RootKind::User);
        assert_eq!(result[1].kind, RootKind::Common);
    }

    #[test]
    fn known_folder_failure_has_a_fixed_error() {
        assert_eq!(
            known_folder_roots_with(|_, _, _| Err(())).unwrap_err(),
            DiscoveryError::KnownFolderQuery,
        );
    }

    #[test]
    fn scans_only_regular_lnk_files_in_deterministic_order() {
        let user = TempRoot::new("files-user");
        let common = TempRoot::new("files-common");
        fs::create_dir_all(user.child("Nested")).unwrap();
        fs::write(user.child("Two.LNK"), []).unwrap();
        fs::write(user.child(r"Nested\One.lnk"), []).unwrap();
        fs::write(user.child("Ignore.exe"), []).unwrap();
        fs::create_dir_all(user.child("Folder.lnk")).unwrap();

        let snapshot = discover_from_roots(roots(user.path(), common.path()), no_target).unwrap();

        assert_eq!(titles(&snapshot.applications), ["One", "Two"]);
        assert!(snapshot.applications.iter().all(|app| app.icon.is_none()));
        assert_eq!(snapshot.diagnostics.unmapped_executables, 2);
    }

    #[test]
    fn missing_root_is_empty_and_existing_bad_root_is_an_error() {
        let parent = TempRoot::new("root-states");
        let missing = parent.child("missing");
        let valid = parent.child("valid");
        fs::create_dir(&valid).unwrap();
        let snapshot = discover_from_roots(roots(&missing, &valid), no_target).unwrap();
        assert_eq!(snapshot.diagnostics.missing_roots, 1);

        let file_root = parent.child("not-a-directory");
        fs::write(&file_root, []).unwrap();
        assert_eq!(
            discover_from_roots(roots(&file_root, &missing), no_target).unwrap_err(),
            DiscoveryError::RootNotDirectory,
        );
        assert_eq!(
            classify_root_metadata(Err(io::Error::from(io::ErrorKind::PermissionDenied)))
                .unwrap_err(),
            DiscoveryError::RootUnavailable,
        );
    }

    #[test]
    fn inaccessible_child_and_invalid_shortcut_are_skipped() {
        let mut diagnostics = DiscoveryDiagnostics::default();
        assert!(classify_child_metadata(
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            &mut diagnostics,
        )
        .is_none());
        assert_eq!(diagnostics.inaccessible_entries, 1);

        let user = TempRoot::new("invalid-user");
        let common = TempRoot::new("invalid-common");
        fs::write(user.child("Bad.lnk"), []).unwrap();
        fs::write(user.child("Valid.lnk"), []).unwrap();
        let snapshot = discover_from_roots(roots(user.path(), common.path()), |path| {
            if path.file_name().and_then(|name| name.to_str()) == Some("Bad.lnk") {
                Err(ShortcutError::InvalidShortcut)
            } else {
                no_target(path)
            }
        })
        .unwrap();

        assert_eq!(titles(&snapshot.applications), ["Valid"]);
        assert_eq!(snapshot.diagnostics.invalid_shortcuts, 1);
    }

    #[test]
    fn non_unicode_relative_path_is_rejected_without_lossy_text() {
        let invalid = OsString::from_wide(&[0xD800]);
        let relative = PathBuf::from(invalid).join("App.lnk");
        assert_eq!(normalize_relative_path(&relative), None);
    }

    #[test]
    fn duplicate_ids_fail_and_duplicate_names_remain_distinct() {
        let root = TempRoot::new("duplicate-root");
        fs::write(root.child("App.lnk"), []).unwrap();
        let duplicate_roots = [
            StartMenuRoot {
                kind: RootKind::User,
                path: root.path().to_path_buf(),
            },
            StartMenuRoot {
                kind: RootKind::User,
                path: root.path().to_path_buf(),
            },
        ];
        assert_eq!(
            discover_from_roots(duplicate_roots, no_target).unwrap_err(),
            DiscoveryError::DuplicateAppId,
        );

        let user = TempRoot::new("same-name-user");
        let common = TempRoot::new("same-name-common");
        fs::create_dir_all(user.child("A")).unwrap();
        fs::create_dir_all(user.child("B")).unwrap();
        fs::write(user.child(r"A\App.lnk"), []).unwrap();
        fs::write(user.child(r"B\App.lnk"), []).unwrap();
        let snapshot = discover_from_roots(roots(user.path(), common.path()), no_target).unwrap();
        assert_eq!(titles(&snapshot.applications), ["App", "App"]);
        assert_ne!(
            snapshot.applications[0].app_id,
            snapshot.applications[1].app_id
        );
    }

    #[test]
    fn target_and_absolute_root_do_not_change_entry_identity() {
        let first = TempRoot::new("identity-first");
        let second = TempRoot::new("identity-second");
        let missing_first = first.child("missing-common");
        let missing_second = second.child("missing-common");
        fs::write(first.child("App.lnk"), []).unwrap();
        fs::write(second.child("App.lnk"), []).unwrap();
        let first_snapshot = discover_from_roots(roots(first.path(), &missing_first), |_| {
            Ok(ShortcutMetadata {
                executable: Some(PathBuf::from(r"C:\One.exe")),
            })
        })
        .unwrap();
        let second_snapshot = discover_from_roots(roots(second.path(), &missing_second), |_| {
            Ok(ShortcutMetadata {
                executable: Some(PathBuf::from(r"D:\Two.exe")),
            })
        })
        .unwrap();

        assert_eq!(
            first_snapshot.applications[0].app_id,
            second_snapshot.applications[0].app_id
        );
    }

    #[test]
    fn registry_entry_keeps_private_values_out_of_the_dto() {
        let application = Application {
            app_id: "app-secret".into(),
            display_name: "Calculator".into(),
            shortcut: PathBuf::from(r"C:\Private\Calculator.lnk"),
            executable: Some(PathBuf::from(r"C:\Private\Calculator.exe")),
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        };

        let (item, action) = crate::apps::registry_entry(&application);

        assert_eq!(item.icon, None);
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("app-secret"));
        assert!(!json.contains("Private"));
        assert!(matches!(
            action,
            ResultAction::LaunchApplication { app_id, .. } if app_id == "app-secret"
        ));
    }

    #[test]
    fn discovery_errors_have_fixed_path_free_messages() {
        let expected = [
            (
                DiscoveryError::KnownFolderQuery,
                "known folder query failed",
            ),
            (
                DiscoveryError::RootNotDirectory,
                "start menu root is not a directory",
            ),
            (
                DiscoveryError::RootUnavailable,
                "start menu root is unavailable",
            ),
            (
                DiscoveryError::RootReparsePoint,
                "start menu root is a reparse point",
            ),
            (DiscoveryError::ComUnavailable, "COM is unavailable"),
            (
                DiscoveryError::HashFailed,
                "application identity hashing failed",
            ),
            (
                DiscoveryError::DuplicateAppId,
                "duplicate application identity",
            ),
        ];
        for (error, message) in expected {
            assert_eq!(error.to_string(), message);
            assert!(!error.to_string().contains('\\'));
            assert!(!error.to_string().contains(':'));
        }
    }

    #[test]
    #[ignore = "run by scripts/test-start-menu-boundary.ps1"]
    fn junction_does_not_escape_injected_root() {
        let user = PathBuf::from(
            std::env::var_os("UIPILOT_TEST_USER_ROOT").expect("missing injected user root"),
        );
        let common = PathBuf::from(
            std::env::var_os("UIPILOT_TEST_COMMON_ROOT").expect("missing injected common root"),
        );
        let sentinel = PathBuf::from(
            std::env::var_os("UIPILOT_TEST_OUTSIDE_SENTINEL")
                .expect("missing injected outside sentinel"),
        );
        let sentinel_name = sentinel
            .file_name()
            .expect("sentinel has no name")
            .to_owned();
        let sentinel_seen = Cell::new(false);

        let snapshot = discover_from_roots(roots(&user, &common), |path| {
            if path.file_name() == Some(sentinel_name.as_os_str()) {
                sentinel_seen.set(true);
                panic!("scanner reached the outside sentinel");
            }
            no_target(path)
        })
        .unwrap();

        assert_eq!(snapshot.diagnostics.reparse_entries, 1);
        assert!(!sentinel_seen.get());
    }
}
