use std::{
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
};

use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ},
        UI::Shell::{IShellLinkW, ShellLink, SLGP_FLAGS, SLGP_RAWPATH},
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShortcutMetadata {
    pub(crate) executable: Option<PathBuf>,
    pub(crate) icon: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShortcutError {
    InvalidShortcut,
    ComUnavailable,
}

pub(crate) fn load_shortcut(path: &Path) -> Result<ShortcutMetadata, ShortcutError> {
    let mut metadata = load_shortcut_with(path, read_raw_shortcut_path)?;
    metadata.icon = super::icon::from_shortcut(path);
    Ok(metadata)
}

fn load_shortcut_with<F>(path: &Path, read_raw_path: F) -> Result<ShortcutMetadata, ShortcutError>
where
    F: FnOnce(&Path, SLGP_FLAGS) -> Result<Option<Vec<u16>>, ShortcutError>,
{
    let executable = read_raw_path(path, SLGP_RAWPATH)?
        .as_deref()
        .and_then(validate_raw_executable_wide);
    Ok(ShortcutMetadata {
        executable,
        icon: None,
    })
}

fn validate_raw_executable_wide(raw: &[u16]) -> Option<PathBuf> {
    let end = raw.iter().position(|unit| *unit == 0).unwrap_or(raw.len());
    let value = String::from_utf16(&raw[..end]).ok()?;
    validate_raw_executable(&value)
}

fn validate_raw_executable(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('%') || raw.contains('\0') {
        return None;
    }
    let path = Path::new(raw);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Prefix(prefix)), Some(Component::RootDir))
            if matches!(prefix.kind(), Prefix::Disk(_)) => {}
        _ => return None,
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn read_raw_shortcut_path(
    path: &Path,
    flags: SLGP_FLAGS,
) -> Result<Option<Vec<u16>>, ShortcutError> {
    let shortcut_path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let link: IShellLinkW = unsafe {
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| ShortcutError::ComUnavailable)?
    };
    let persist: IPersistFile = link.cast().map_err(|_| ShortcutError::ComUnavailable)?;
    unsafe {
        persist
            .Load(PCWSTR(shortcut_path.as_ptr()), STGM_READ)
            .map_err(|_| ShortcutError::InvalidShortcut)?;
    }

    let mut raw_path = vec![0_u16; 260];
    let result = unsafe { link.GetPath(&mut raw_path, std::ptr::null_mut(), flags.0 as u32) };
    if result.is_err() || raw_path.first() == Some(&0) {
        return Ok(None);
    }
    Ok(Some(raw_path))
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use windows::{
        core::{Interface, PCWSTR},
        Win32::{
            Security::Cryptography::{CryptStringToBinaryW, CRYPT_STRING_BASE64},
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH},
        },
    };

    use super::{
        load_shortcut, load_shortcut_with, validate_raw_executable, validate_raw_executable_wide,
        ShortcutError, ShortcutMetadata,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }

    fn decode_icon(icon: &str) -> Vec<u8> {
        let payload = icon.strip_prefix("data:image/png;base64,").unwrap();
        let payload = payload.encode_utf16().collect::<Vec<_>>();
        let mut length = 0;
        unsafe {
            CryptStringToBinaryW(&payload, CRYPT_STRING_BASE64, None, &mut length, None, None)
        }
        .unwrap();
        let mut decoded = vec![0; length as usize];
        unsafe {
            CryptStringToBinaryW(
                &payload,
                CRYPT_STRING_BASE64,
                Some(decoded.as_mut_ptr()),
                &mut length,
                None,
                None,
            )
        }
        .unwrap();
        decoded.truncate(length as usize);
        decoded
    }

    struct ComGuard;

    impl ComGuard {
        fn initialize() -> Self {
            let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            assert!(result.is_ok(), "COM initialization failed: {result:?}");
            Self
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn create_shortcut(target: &str) -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("uipilot-shortcut-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let shortcut = directory.join("App.lnk");
        let shortcut_wide = wide(shortcut.to_str().unwrap());
        let target_wide = wide(target);
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(target_wide.as_ptr())).unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            persist.Save(PCWSTR(shortcut_wide.as_ptr()), true).unwrap();
        }
        shortcut
    }

    #[test]
    fn native_loader_reads_a_real_shortcut_without_target_io() {
        let _com = ComGuard::initialize();
        let shortcut = create_shortcut(r"Z:\missing\NativeApp.exe");

        let metadata = load_shortcut(&shortcut).unwrap();

        assert_eq!(
            metadata.executable,
            Some(PathBuf::from(r"Z:\missing\NativeApp.exe"))
        );
        let icon = metadata.icon.as_deref().unwrap();
        assert!(icon.len() <= 65_536);
        let png = decode_icon(icon);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[8..12].try_into().unwrap()), 13);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 32);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 32);
        assert_eq!(png[24], 8);
        assert_eq!(png[25], 6);
        fs::remove_dir_all(shortcut.parent().unwrap()).unwrap();
    }

    #[test]
    fn shortcut_loader_requests_only_raw_path() {
        let flags = Cell::new(None);
        let metadata = load_shortcut_with(Path::new(r"C:\Menu\App.lnk"), |_, requested| {
            flags.set(Some(requested));
            Ok(Some(wide(r"C:\Apps\App.exe")))
        })
        .unwrap();

        assert_eq!(flags.get(), Some(SLGP_RAWPATH));
        assert_eq!(
            metadata,
            ShortcutMetadata {
                executable: Some(PathBuf::from(r"C:\Apps\App.exe")),
                icon: None,
            }
        );
    }

    #[test]
    fn unsafe_or_non_executable_targets_have_no_mapping() {
        for raw in [
            r"%LOCALAPPDATA%\App.exe",
            r"App.exe",
            r"C:relative\App.exe",
            r"\\server\App.exe",
            r"\\?\C:\App.exe",
            r"\\.\C:\App.exe",
            r"C:\App.cmd",
            "",
        ] {
            assert_eq!(validate_raw_executable(raw), None, "accepted {raw}");
        }
    }

    #[test]
    fn nonexistent_drive_absolute_exe_is_kept_without_io() {
        assert_eq!(
            validate_raw_executable(r"Z:\missing\App.EXE"),
            Some(PathBuf::from(r"Z:\missing\App.EXE")),
        );
    }

    #[test]
    fn empty_and_invalid_utf16_raw_targets_have_no_mapping() {
        assert_eq!(validate_raw_executable_wide(&[]), None);
        assert_eq!(validate_raw_executable_wide(&[0xD800, 0]), None);
    }

    #[test]
    fn damaged_shortcut_is_reported_but_missing_target_is_valid() {
        let damaged = load_shortcut_with(Path::new(r"C:\Menu\Bad.lnk"), |_, _| {
            Err(ShortcutError::InvalidShortcut)
        });
        let missing = load_shortcut_with(Path::new(r"C:\Menu\NoTarget.lnk"), |_, _| Ok(None));

        assert_eq!(damaged, Err(ShortcutError::InvalidShortcut));
        assert_eq!(
            missing,
            Ok(ShortcutMetadata {
                executable: None,
                icon: None,
            })
        );
    }
}
