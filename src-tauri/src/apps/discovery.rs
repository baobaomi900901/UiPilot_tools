use std::{
    collections::HashSet,
    ffi::{c_void, OsString},
    fs::{self, Metadata},
    io,
    os::windows::{ffi::OsStringExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

use windows::{
    core::{GUID, HRESULT, PWSTR},
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
        System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_CommonStartMenu, FOLDERID_Programs,
            FOLDERID_StartMenu, KF_FLAG_DONT_VERIFY, KNOWN_FOLDER_FLAG,
        },
    },
};

use super::{
    app_id, appsfolder,
    shortcut::{load_shortcut, ShortcutError, ShortcutMetadata},
    Application, ApplicationEntryKind, ApplicationLaunchTarget, DiscoveryDiagnostics,
    DiscoveryError, DiscoverySnapshot, RootKind, StartMenuRoot,
};
use crate::{model::ResultItem, result_registry::ResultAction};

#[link(name = "shell32")]
unsafe extern "system" {
    #[link_name = "SHGetKnownFolderPath"]
    fn sh_get_known_folder_path(
        id: *const GUID,
        flags: u32,
        token: HANDLE,
        output: *mut PWSTR,
    ) -> HRESULT;
}

struct KnownFolderPath<F: FnOnce(*mut u16)> {
    pointer: *mut u16,
    free: Option<F>,
}

impl<F: FnOnce(*mut u16)> Drop for KnownFolderPath<F> {
    fn drop(&mut self) {
        self.free
            .take()
            .expect("known folder deallocator is missing")(self.pointer);
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn known_folder_roots() -> Result<[StartMenuRoot; 4], DiscoveryError> {
    known_folder_roots_with(known_folder_path)
}

fn known_folder_path(
    id: &GUID,
    flags: KNOWN_FOLDER_FLAG,
    token: Option<HANDLE>,
) -> Result<PathBuf, ()> {
    known_folder_path_with(
        id,
        flags,
        token,
        |id, flags, token, output| unsafe {
            sh_get_known_folder_path(id, flags.0 as u32, token.unwrap_or_default(), output)
        },
        |pointer| unsafe { CoTaskMemFree(Some(pointer.cast::<c_void>())) },
    )
}

fn known_folder_path_with<C, F>(
    id: &GUID,
    flags: KNOWN_FOLDER_FLAG,
    token: Option<HANDLE>,
    call: C,
    free: F,
) -> Result<PathBuf, ()>
where
    C: FnOnce(&GUID, KNOWN_FOLDER_FLAG, Option<HANDLE>, *mut PWSTR) -> HRESULT,
    F: FnOnce(*mut u16),
{
    let mut raw = PWSTR::null();
    let status = call(id, flags, token, &mut raw);
    let owned = KnownFolderPath {
        pointer: raw.0,
        free: Some(free),
    };
    if status.is_err() || owned.pointer.is_null() {
        return Err(());
    }
    let mut length = 0;
    unsafe {
        while *owned.pointer.add(length) != 0 {
            length += 1;
        }
        Ok(PathBuf::from(OsString::from_wide(
            std::slice::from_raw_parts(owned.pointer, length),
        )))
    }
}

fn known_folder_roots_with<F>(mut provider: F) -> Result<[StartMenuRoot; 4], DiscoveryError>
where
    F: FnMut(&GUID, KNOWN_FOLDER_FLAG, Option<HANDLE>) -> Result<PathBuf, ()>,
{
    let user = provider(&FOLDERID_Programs, KF_FLAG_DONT_VERIFY, None)
        .map_err(|_| DiscoveryError::KnownFolderQuery)?;
    let common = provider(&FOLDERID_CommonPrograms, KF_FLAG_DONT_VERIFY, None)
        .map_err(|_| DiscoveryError::KnownFolderQuery)?;
    let user_top_level = provider(&FOLDERID_StartMenu, KF_FLAG_DONT_VERIFY, None)
        .map_err(|_| DiscoveryError::KnownFolderQuery)?;
    let common_top_level = provider(&FOLDERID_CommonStartMenu, KF_FLAG_DONT_VERIFY, None)
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
        StartMenuRoot {
            kind: RootKind::UserTopLevel,
            path: user_top_level,
        },
        StartMenuRoot {
            kind: RootKind::CommonTopLevel,
            path: common_top_level,
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
        if metadata.is_dir() && !root_kind.recurses() {
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
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: candidate.shortcut,
                executable: metadata.executable,
            },
            icon: metadata.icon,
            aliases: Vec::new(),
            use_count: 0,
        });
    }
    Ok(DiscoverySnapshot {
        applications,
        diagnostics,
    })
}

fn discover_with<I, R, P>(
    roots: I,
    resolve: R,
    packaged: P,
) -> Result<DiscoverySnapshot, DiscoveryError>
where
    I: IntoIterator<Item = StartMenuRoot>,
    R: FnMut(&Path) -> Result<ShortcutMetadata, ShortcutError>,
    P: FnOnce() -> Result<DiscoverySnapshot, DiscoveryError>,
{
    let mut snapshot = discover_from_roots(roots, resolve)?;
    let packaged = packaged()?;
    let mut ids: HashSet<_> = snapshot
        .applications
        .iter()
        .map(|application| application.app_id.clone())
        .collect();
    for application in packaged.applications {
        if !ids.insert(application.app_id.clone()) {
            return Err(DiscoveryError::DuplicateAppId);
        }
        snapshot.applications.push(application);
    }
    snapshot.diagnostics.missing_roots += packaged.diagnostics.missing_roots;
    snapshot.diagnostics.inaccessible_entries += packaged.diagnostics.inaccessible_entries;
    snapshot.diagnostics.reparse_entries += packaged.diagnostics.reparse_entries;
    snapshot.diagnostics.non_unicode_entries += packaged.diagnostics.non_unicode_entries;
    snapshot.diagnostics.invalid_shortcuts += packaged.diagnostics.invalid_shortcuts;
    snapshot.diagnostics.unmapped_executables += packaged.diagnostics.unmapped_executables;
    snapshot.diagnostics.invalid_packaged_names += packaged.diagnostics.invalid_packaged_names;
    snapshot.diagnostics.invalid_packaged_aumids += packaged.diagnostics.invalid_packaged_aumids;
    Ok(snapshot)
}

pub(crate) fn discover() -> Result<DiscoverySnapshot, DiscoveryError> {
    let roots = known_folder_roots()?;
    if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
        return Err(DiscoveryError::ComUnavailable);
    }
    let _com = ComGuard;
    discover_with(roots, load_shortcut, appsfolder::discover)
}

pub(crate) fn registry_entry(application: &Application) -> (ResultItem, ResultAction) {
    (
        ResultItem {
            result_id: String::new(),
            title: application.display_name.clone(),
            subtitle: Some(
                match application.entry_kind() {
                    ApplicationEntryKind::DesktopShortcut => "应用程序",
                    ApplicationEntryKind::PackagedApp => "打包的应用程序",
                }
                .into(),
            ),
            icon: application.icon.clone(),
        },
        ResultAction::LaunchApplication {
            app_id: application.app_id.clone(),
            target: application.target.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        collections::HashSet,
        ffi::OsString,
        fs, io,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering},
    };

    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{E_FAIL, S_OK},
            UI::Shell::{
                FOLDERID_CommonPrograms, FOLDERID_CommonStartMenu, FOLDERID_Programs,
                FOLDERID_StartMenu, KF_FLAG_DONT_VERIFY,
            },
        },
    };

    use super::{
        classify_child_metadata, classify_root_metadata, discover_from_roots, discover_with,
        known_folder_path_with, known_folder_roots_with, normalize_relative_path,
    };
    use crate::apps::shortcut::{ShortcutError, ShortcutMetadata};
    use crate::{
        apps::{
            Application, ApplicationLaunchTarget, DiscoveryDiagnostics, DiscoveryError,
            DiscoverySnapshot, RootKind, StartMenuRoot,
        },
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
        Ok(ShortcutMetadata {
            executable: None,
            icon: None,
        })
    }

    fn titles(applications: &[Application]) -> Vec<&str> {
        applications
            .iter()
            .map(|application| application.display_name.as_str())
            .collect()
    }

    #[test]
    fn known_folder_provider_uses_exact_ids_flags_and_null_token() {
        let expected = [
            (FOLDERID_Programs, RootKind::User),
            (FOLDERID_CommonPrograms, RootKind::Common),
            (FOLDERID_StartMenu, RootKind::UserTopLevel),
            (FOLDERID_CommonStartMenu, RootKind::CommonTopLevel),
        ];
        let mut calls = Vec::new();
        let result = known_folder_roots_with(|id, flags, token| {
            calls.push((*id, flags, token));
            Ok(PathBuf::from(format!(r"C:\Known\{}", calls.len())))
        })
        .unwrap();

        assert_eq!(calls.len(), expected.len());
        for (index, (guid, kind)) in expected.into_iter().enumerate() {
            assert_eq!(calls[index].0, guid);
            assert_eq!(calls[index].1, KF_FLAG_DONT_VERIFY);
            assert!(calls[index].2.is_none());
            assert_eq!(result[index].kind, kind);
        }
    }

    fn four_roots(
        user: &Path,
        common: &Path,
        user_top_level: &Path,
        common_top_level: &Path,
    ) -> [StartMenuRoot; 4] {
        [
            StartMenuRoot {
                kind: RootKind::User,
                path: user.to_path_buf(),
            },
            StartMenuRoot {
                kind: RootKind::Common,
                path: common.to_path_buf(),
            },
            StartMenuRoot {
                kind: RootKind::UserTopLevel,
                path: user_top_level.to_path_buf(),
            },
            StartMenuRoot {
                kind: RootKind::CommonTopLevel,
                path: common_top_level.to_path_buf(),
            },
        ]
    }

    #[test]
    fn every_known_folder_failure_has_the_fixed_hard_error() {
        for failed_at in 0..4 {
            let calls = Cell::new(0);
            let result = known_folder_roots_with(|_, _, _| {
                let current = calls.get();
                calls.set(current + 1);
                if current == failed_at {
                    Err(())
                } else {
                    Ok(PathBuf::from(format!(r"C:\Known\{current}")))
                }
            });

            assert_eq!(result, Err(DiscoveryError::KnownFolderQuery));
            assert_eq!(calls.get(), failed_at + 1);
        }
    }

    #[test]
    fn raw_known_folder_result_frees_failure_pointer_and_rejects_null_success() {
        let dangling = NonNull::<u16>::dangling().as_ptr();
        let failure_freed = Cell::new(false);
        let failure = known_folder_path_with(
            &FOLDERID_Programs,
            KF_FLAG_DONT_VERIFY,
            None,
            |id, flags, token, output| {
                assert_eq!(id, &FOLDERID_Programs);
                assert_eq!(flags, KF_FLAG_DONT_VERIFY);
                assert!(token.is_none());
                unsafe { *output = PWSTR(dangling) };
                E_FAIL
            },
            |pointer| {
                assert_eq!(pointer, dangling);
                failure_freed.set(true);
            },
        );
        assert_eq!(failure, Err(()));
        assert!(failure_freed.get());

        let null_freed = Cell::new(false);
        let null_success = known_folder_path_with(
            &FOLDERID_Programs,
            KF_FLAG_DONT_VERIFY,
            None,
            |_, _, _, _| S_OK,
            |pointer| {
                assert!(pointer.is_null());
                null_freed.set(true);
            },
        );
        assert_eq!(null_success, Err(()));
        assert!(null_freed.get());
    }

    #[test]
    fn raw_known_folder_result_frees_success_pointer_without_lossy_unicode() {
        let mut raw = [0xD800_u16, 0];
        let raw_pointer = raw.as_mut_ptr();
        let freed = Cell::new(false);
        let path = known_folder_path_with(
            &FOLDERID_StartMenu,
            KF_FLAG_DONT_VERIFY,
            None,
            |id, flags, token, output| {
                assert_eq!(id, &FOLDERID_StartMenu);
                assert_eq!(flags, KF_FLAG_DONT_VERIFY);
                assert!(token.is_none());
                unsafe { *output = PWSTR(raw_pointer) };
                S_OK
            },
            |pointer| {
                assert_eq!(pointer, raw_pointer);
                freed.set(true);
            },
        )
        .unwrap();

        assert_eq!(path.as_os_str().encode_wide().collect::<Vec<_>>(), [0xD800]);
        assert!(freed.get());
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
    fn top_level_roots_scan_direct_lnk_but_never_descend() {
        let user = TempRoot::new("programs-user");
        let common = TempRoot::new("programs-common");
        let user_top = TempRoot::new("top-user");
        let common_top = TempRoot::new("top-common");

        fs::create_dir_all(user.child("Nested")).unwrap();
        fs::create_dir_all(common.child("Nested")).unwrap();
        fs::create_dir_all(user_top.child("Programs")).unwrap();
        fs::create_dir_all(user_top.child("Other")).unwrap();
        fs::create_dir_all(common_top.child("Programs")).unwrap();
        fs::write(user.child(r"Nested\UserProgram.lnk"), []).unwrap();
        fs::write(common.child(r"Nested\CommonProgram.lnk"), []).unwrap();
        fs::write(user_top.child("UserTop.LNK"), []).unwrap();
        fs::write(common_top.child("CommonTop.lnk"), []).unwrap();
        fs::write(user_top.child(r"Programs\Hidden.lnk"), []).unwrap();
        fs::write(user_top.child(r"Other\AlsoHidden.lnk"), []).unwrap();
        fs::write(common_top.child(r"Programs\CommonHidden.lnk"), []).unwrap();
        let non_unicode_directory = OsString::from_wide(&[0xD800]);
        fs::create_dir(user_top.path().join(non_unicode_directory)).unwrap();
        let non_unicode_shortcut = OsString::from_wide(&[0xD800, 0x002E, 0x006C, 0x006E, 0x006B]);
        fs::write(user_top.path().join(non_unicode_shortcut), []).unwrap();

        let snapshot = discover_from_roots(
            four_roots(
                user.path(),
                common.path(),
                user_top.path(),
                common_top.path(),
            ),
            no_target,
        )
        .unwrap();

        assert_eq!(
            titles(&snapshot.applications),
            ["CommonTop", "CommonProgram", "UserProgram", "UserTop"]
        );
        assert_eq!(snapshot.diagnostics.unmapped_executables, 4);
        assert_eq!(snapshot.diagnostics.non_unicode_entries, 1);
        assert!(snapshot.applications.iter().all(|application| matches!(
            &application.target,
            ApplicationLaunchTarget::Shortcut { .. }
        )));
    }

    #[test]
    fn same_filename_in_all_four_scopes_keeps_four_distinct_entries() {
        let user = TempRoot::new("scope-user");
        let common = TempRoot::new("scope-common");
        let user_top = TempRoot::new("scope-user-top");
        let common_top = TempRoot::new("scope-common-top");
        for root in [&user, &common, &user_top, &common_top] {
            fs::write(root.child("WeChat.lnk"), []).unwrap();
        }

        let snapshot = discover_from_roots(
            four_roots(
                user.path(),
                common.path(),
                user_top.path(),
                common_top.path(),
            ),
            no_target,
        )
        .unwrap();
        let ids: HashSet<_> = snapshot
            .applications
            .iter()
            .map(|application| application.app_id.as_str())
            .collect();

        assert_eq!(snapshot.applications.len(), 4);
        assert_eq!(ids.len(), 4);
        assert!(snapshot
            .applications
            .iter()
            .all(|application| application.display_name == "WeChat"));
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
    fn desktop_and_packaged_snapshots_merge_without_name_deduplication() {
        let user = TempRoot::new("merged-user");
        let common = TempRoot::new("merged-common");
        let safe_icon = "data:image/png;base64,iVBORw==".to_owned();
        fs::write(user.child("Settings.lnk"), []).unwrap();
        let packaged = DiscoverySnapshot {
            applications: vec![Application {
                app_id: "app-packaged".into(),
                display_name: "Settings".into(),
                target: ApplicationLaunchTarget::PackagedApp {
                    aumid: "family!settings".into(),
                },
                icon: Some(safe_icon.clone()),
                aliases: Vec::new(),
                use_count: 0,
            }],
            diagnostics: DiscoveryDiagnostics {
                invalid_packaged_aumids: 3,
                ..DiscoveryDiagnostics::default()
            },
        };

        let merged = discover_with(
            roots(user.path(), common.path()),
            |_| {
                Ok(ShortcutMetadata {
                    executable: None,
                    icon: Some(safe_icon.clone()),
                })
            },
            || Ok(packaged),
        )
        .unwrap();

        assert_eq!(titles(&merged.applications), ["Settings", "Settings"]);
        assert!(merged
            .applications
            .iter()
            .all(|application| application.icon.as_deref() == Some(safe_icon.as_str())));
        assert_eq!(merged.diagnostics.invalid_packaged_aumids, 3);
    }

    #[test]
    fn packaged_discovery_failure_rejects_the_complete_snapshot() {
        let user = TempRoot::new("packaged-error-user");
        let common = TempRoot::new("packaged-error-common");
        fs::write(user.child("Desktop.lnk"), []).unwrap();

        assert_eq!(
            discover_with(roots(user.path(), common.path()), no_target, || {
                Err(DiscoveryError::AppsFolderEnumeration)
            }),
            Err(DiscoveryError::AppsFolderEnumeration)
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
                icon: None,
            })
        })
        .unwrap();
        let second_snapshot = discover_from_roots(roots(second.path(), &missing_second), |_| {
            Ok(ShortcutMetadata {
                executable: Some(PathBuf::from(r"D:\Two.exe")),
                icon: None,
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
        let safe_icon = "data:image/png;base64,iVBORw==".to_owned();
        let desktop = Application {
            app_id: "app-desktop-secret".into(),
            display_name: "Calculator".into(),
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(r"C:\Private\Calculator.lnk"),
                executable: Some(PathBuf::from(r"C:\Private\Calculator.exe")),
            },
            icon: Some(safe_icon.clone()),
            aliases: Vec::new(),
            use_count: 0,
        };
        let packaged = Application {
            app_id: "app-packaged-secret".into(),
            display_name: "Calculator".into(),
            target: ApplicationLaunchTarget::PackagedApp {
                aumid: "family!secret".into(),
            },
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        };

        let (desktop_item, _) = crate::apps::registry_entry(&desktop);
        let (packaged_item, packaged_action) = crate::apps::registry_entry(&packaged);

        assert_eq!(desktop_item.subtitle.as_deref(), Some("应用程序"));
        assert_eq!(desktop_item.icon.as_deref(), Some(safe_icon.as_str()));
        assert_eq!(packaged_item.subtitle.as_deref(), Some("打包的应用程序"));
        assert_eq!(packaged_item.icon, None);
        let desktop_json = serde_json::to_string(&desktop_item).unwrap();
        assert!(desktop_json.contains(safe_icon.as_str()));
        assert!(!desktop_json.contains(r"C:\Private"));
        let json = serde_json::to_string(&packaged_item).unwrap();
        assert!(!json.contains("app-packaged-secret"));
        assert!(!json.contains("family!secret"));
        assert!(matches!(
            packaged_action,
            ResultAction::LaunchApplication {
                app_id,
                target: ApplicationLaunchTarget::PackagedApp { aumid },
            } if app_id == "app-packaged-secret" && aumid == "family!secret"
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
                DiscoveryError::AppsFolderUnavailable,
                "AppsFolder is unavailable",
            ),
            (
                DiscoveryError::AppsFolderEnumeration,
                "AppsFolder enumeration failed",
            ),
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

        let snapshot = discover_from_roots(
            [
                StartMenuRoot {
                    kind: RootKind::User,
                    path: user.clone(),
                },
                StartMenuRoot {
                    kind: RootKind::Common,
                    path: common.clone(),
                },
                StartMenuRoot {
                    kind: RootKind::UserTopLevel,
                    path: user.clone(),
                },
                StartMenuRoot {
                    kind: RootKind::CommonTopLevel,
                    path: common.clone(),
                },
            ],
            |path| {
                if path.file_name() == Some(sentinel_name.as_os_str()) {
                    sentinel_seen.set(true);
                    panic!("scanner reached the outside sentinel");
                }
                no_target(path)
            },
        )
        .unwrap();

        assert_eq!(snapshot.diagnostics.reparse_entries, 2);
        assert!(!sentinel_seen.get());

        let root_error = discover_from_roots(
            [
                StartMenuRoot {
                    kind: RootKind::UserTopLevel,
                    path: user.join("outside-link"),
                },
                StartMenuRoot {
                    kind: RootKind::Common,
                    path: common,
                },
            ],
            no_target,
        )
        .unwrap_err();
        assert_eq!(root_error, DiscoveryError::RootReparsePoint);
    }
}
