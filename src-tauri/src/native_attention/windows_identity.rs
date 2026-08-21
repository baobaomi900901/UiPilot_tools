use std::{
    fs,
    mem::ManuallyDrop,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
};

use windows::{
    core::{Interface, PCWSTR, PWSTR},
    Win32::{
        Globalization::{CompareStringOrdinal, CSTR_EQUAL},
        Storage::{
            EnhancedStorage::PKEY_AppUserModel_ID,
            FileSystem::{
                MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_IGNORE_MERGE_ERRORS,
            },
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, CoUninitialize,
            IPersistFile,
            StructuredStorage::{
                PropVariantClear, PropVariantToStringAlloc, PROPVARIANT, PROPVARIANT_0,
                PROPVARIANT_0_0, PROPVARIANT_0_0_0,
            },
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, STGM_READ,
        },
        System::Variant::VT_LPWSTR,
        UI::Shell::{
            FOLDERID_Programs, IShellLinkW, PropertiesSystem::IPropertyStore, SHGetKnownFolderPath,
            SetCurrentProcessExplicitAppUserModelID, ShellLink, KF_FLAG_CREATE, SLGP_RAWPATH,
        },
    },
};

const KNOWN_AUMIDS: [&str; 2] = ["com.uipilot.launcher", "com.uipilot.launcher.dev"];
static PROCESS_IDENTITY_READY: AtomicBool = AtomicBool::new(false);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BuildIdentity {
    pub(crate) aumid: &'static str,
    pub(crate) shortcut_name: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShortcutOwnership {
    Owned,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdentityError {
    ProcessIdentity,
    ShortcutThread,
    ShortcutUnavailable,
    UnknownShortcut,
}

pub(crate) const fn build_identity(debug: bool) -> BuildIdentity {
    if debug {
        BuildIdentity {
            aumid: "com.uipilot.launcher.dev",
            shortcut_name: "UiPilot Dev.lnk",
        }
    } else {
        BuildIdentity {
            aumid: "com.uipilot.launcher",
            shortcut_name: "UiPilot.lnk",
        }
    }
}

pub(crate) const fn current_identity() -> BuildIdentity {
    build_identity(cfg!(debug_assertions))
}

pub(crate) fn prepare_process_identity() -> Result<(), IdentityError> {
    let identity = current_identity();
    let aumid = wide(identity.aumid.as_ref());
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(aumid.as_ptr())) };
    let ready = result.is_ok();
    PROCESS_IDENTITY_READY.store(ready, Ordering::Release);
    if ready {
        Ok(())
    } else {
        eprintln!("[native-attention] process identity unavailable");
        Err(IdentityError::ProcessIdentity)
    }
}

pub(crate) fn prepare_shortcut() -> Result<BuildIdentity, IdentityError> {
    if !PROCESS_IDENTITY_READY.load(Ordering::Acquire) {
        return Err(IdentityError::ProcessIdentity);
    }
    let identity = current_identity();
    thread::Builder::new()
        .name("uipilot-toast-shortcut".into())
        .spawn(move || prepare_shortcut_sta(identity))
        .map_err(|_| IdentityError::ShortcutThread)?
        .join()
        .map_err(|_| IdentityError::ShortcutThread)??;
    Ok(identity)
}

fn prepare_shortcut_sta(identity: BuildIdentity) -> Result<(), IdentityError> {
    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    if result.is_err() {
        return Err(IdentityError::ShortcutUnavailable);
    }
    let _guard = ComGuard;
    prepare_shortcut_in_apartment(identity)
}

fn prepare_shortcut_in_apartment(identity: BuildIdentity) -> Result<(), IdentityError> {
    let programs = known_programs_path()?;
    fs::create_dir_all(&programs).map_err(|_| IdentityError::ShortcutUnavailable)?;
    let destination = programs.join(identity.shortcut_name);
    let executable = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|_| IdentityError::ShortcutUnavailable)?;

    if destination.exists() {
        let (target_is_current, aumid) = read_shortcut_identity(&destination, &executable)?;
        if shortcut_ownership(target_is_current, aumid.as_deref()) == ShortcutOwnership::Unknown {
            return Err(IdentityError::UnknownShortcut);
        }
    }

    let temporary = unique_temporary_path(&destination)?;
    let result = write_shortcut(&temporary, &executable, identity.aumid)
        .and_then(|()| commit_shortcut(&temporary, &destination));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn known_programs_path() -> Result<PathBuf, IdentityError> {
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_CREATE, None) }
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    let value = unsafe { raw.to_string() }.map_err(|_| IdentityError::ShortcutUnavailable);
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    value.map(PathBuf::from)
}

fn read_shortcut_identity(
    path: &Path,
    executable: &Path,
) -> Result<(bool, Option<String>), IdentityError> {
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|_| IdentityError::UnknownShortcut)?;
    let persist: IPersistFile = link.cast().map_err(|_| IdentityError::UnknownShortcut)?;
    let path_wide = wide(path.as_os_str());
    unsafe { persist.Load(PCWSTR(path_wide.as_ptr()), STGM_READ) }
        .map_err(|_| IdentityError::UnknownShortcut)?;

    let mut target = vec![0u16; 32_768];
    unsafe { link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }
        .map_err(|_| IdentityError::UnknownShortcut)?;
    let length = target
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(target.len());
    target.truncate(length);
    let target_path = PathBuf::from(String::from_utf16_lossy(&target));
    let target_is_current = fs::canonicalize(target_path)
        .ok()
        .is_some_and(|target| ordinal_path_eq(&target, executable));

    let store: IPropertyStore = link.cast().map_err(|_| IdentityError::UnknownShortcut)?;
    let aumid = read_aumid(&store).ok().flatten();
    Ok((target_is_current, aumid))
}

fn read_aumid(store: &IPropertyStore) -> Result<Option<String>, IdentityError> {
    let mut value = unsafe { store.GetValue(&PKEY_AppUserModel_ID) }
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    let string = unsafe { PropVariantToStringAlloc(&value) };
    let result = match string {
        Ok(raw) => {
            let value = unsafe { raw.to_string() };
            unsafe { CoTaskMemFree(Some(raw.0.cast())) };
            value
                .map_err(|_| IdentityError::ShortcutUnavailable)
                .map(|value| (!value.is_empty()).then_some(value))
        }
        Err(_) => Ok(None),
    };
    let _ = unsafe { PropVariantClear(&mut value) };
    result
}

fn write_shortcut(path: &Path, executable: &Path, aumid: &str) -> Result<(), IdentityError> {
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    let executable_wide = wide(executable.as_os_str());
    let working_directory = executable
        .parent()
        .ok_or(IdentityError::ShortcutUnavailable)?;
    let working_wide = wide(working_directory.as_os_str());
    unsafe { link.SetPath(PCWSTR(executable_wide.as_ptr())) }
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    unsafe { link.SetWorkingDirectory(PCWSTR(working_wide.as_ptr())) }
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    unsafe { link.SetIconLocation(PCWSTR(executable_wide.as_ptr()), 0) }
        .map_err(|_| IdentityError::ShortcutUnavailable)?;

    let store: IPropertyStore = link
        .cast()
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    set_aumid(&store, aumid)?;
    let persist: IPersistFile = link
        .cast()
        .map_err(|_| IdentityError::ShortcutUnavailable)?;
    let path_wide = wide(path.as_os_str());
    unsafe { persist.Save(PCWSTR(path_wide.as_ptr()), true) }
        .map_err(|_| IdentityError::ShortcutUnavailable)
}

fn set_aumid(store: &IPropertyStore, aumid: &str) -> Result<(), IdentityError> {
    let wide = wide(aumid.as_ref());
    let bytes = wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or(IdentityError::ShortcutUnavailable)?;
    let allocation = unsafe { CoTaskMemAlloc(bytes) }.cast::<u16>();
    if allocation.is_null() {
        return Err(IdentityError::ShortcutUnavailable);
    }
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), allocation, wide.len()) };
    let mut value = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    pwszVal: PWSTR(allocation),
                },
            }),
        },
    };
    let result = unsafe { store.SetValue(&PKEY_AppUserModel_ID, &value) }
        .and_then(|()| unsafe { store.Commit() })
        .map_err(|_| IdentityError::ShortcutUnavailable);
    let _ = unsafe { PropVariantClear(&mut value) };
    result
}

fn commit_shortcut(temporary: &Path, destination: &Path) -> Result<(), IdentityError> {
    let temporary_wide = wide(temporary.as_os_str());
    let destination_wide = wide(destination.as_os_str());
    let result = if destination.exists() {
        unsafe {
            ReplaceFileW(
                PCWSTR(destination_wide.as_ptr()),
                PCWSTR(temporary_wide.as_ptr()),
                None,
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                None,
                None,
            )
        }
    } else {
        unsafe {
            MoveFileExW(
                PCWSTR(temporary_wide.as_ptr()),
                PCWSTR(destination_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    result.map_err(|_| IdentityError::ShortcutUnavailable)
}

fn unique_temporary_path(destination: &Path) -> Result<PathBuf, IdentityError> {
    let parent = destination
        .parent()
        .ok_or(IdentityError::ShortcutUnavailable)?;
    for _ in 0..16 {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.{}.{id}.tmp",
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("uipilot.lnk"),
            std::process::id(),
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(IdentityError::ShortcutUnavailable)
}

fn shortcut_ownership(target_is_current: bool, aumid: Option<&str>) -> ShortcutOwnership {
    if target_is_current || aumid.is_some_and(|value| KNOWN_AUMIDS.contains(&value)) {
        ShortcutOwnership::Owned
    } else {
        ShortcutOwnership::Unknown
    }
}

fn ordinal_path_eq(left: &Path, right: &Path) -> bool {
    let left = left.as_os_str().encode_wide().collect::<Vec<_>>();
    let right = right.as_os_str().encode_wide().collect::<Vec<_>>();
    (unsafe { CompareStringOrdinal(&left, &right, true) }) == CSTR_EQUAL
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::{build_identity, shortcut_ownership, ShortcutOwnership};

    #[test]
    fn build_identity_uses_fixed_debug_and_release_values() {
        let debug = build_identity(true);
        assert_eq!(debug.aumid, "com.uipilot.launcher.dev");
        assert_eq!(debug.shortcut_name, "UiPilot Dev.lnk");

        let release = build_identity(false);
        assert_eq!(release.aumid, "com.uipilot.launcher");
        assert_eq!(release.shortcut_name, "UiPilot.lnk");
    }

    #[test]
    fn shortcut_ownership_accepts_only_current_target_or_known_uipilot_identity() {
        assert_eq!(
            shortcut_ownership(true, Some("foreign.application")),
            ShortcutOwnership::Owned
        );
        assert_eq!(
            shortcut_ownership(false, Some("com.uipilot.launcher.dev")),
            ShortcutOwnership::Owned
        );
        assert_eq!(
            shortcut_ownership(false, Some("com.uipilot.launcher")),
            ShortcutOwnership::Owned
        );
        assert_eq!(
            shortcut_ownership(false, Some("foreign.application")),
            ShortcutOwnership::Unknown
        );
    }

    #[test]
    fn shortcut_sta_initialization_is_balanced_by_the_guard() {
        let source = include_str!("windows_identity.rs");
        let initialize = source.find("CoInitializeEx(").unwrap();
        let guard = source.find("let _guard = ComGuard;").unwrap();
        let uninitialize = source.rfind("CoUninitialize()").unwrap();

        assert!(initialize < guard && guard < uninitialize);
    }
}
