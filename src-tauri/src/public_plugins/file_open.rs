//! Bounded, user-selected text import. Paths never cross the renderer bridge.
use serde::Serialize;
use std::io::Read;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum FileOpenError {
    FileOpenBusyError,
    ExpiredWindowSessionError,
    FileOpenPermissionError,
    FileOpenUnsupportedError,
    FileOpenTooLargeError,
    FileOpenEncodingError,
    FileOpenFailedError,
}
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenedText {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) byte_length: usize,
}

pub(crate) const MAX_TEXT_BYTES: usize = 8192;
fn read_bounded(name: String, reader: impl Read) -> Result<OpenedText, FileOpenError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FileOpenError::FileOpenFailedError)?;
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(FileOpenError::FileOpenTooLargeError);
    }
    let byte_length = bytes.len();
    let content = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let text = std::str::from_utf8(content).map_err(|_| FileOpenError::FileOpenEncodingError)?;
    if text.contains('\0') {
        return Err(FileOpenError::FileOpenEncodingError);
    }
    Ok(OpenedText {
        name,
        text: text.into(),
        byte_length,
    })
}

// Lexical validation precedes every filesystem call, including drive probing.
fn ordinary_drive_path(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() < 4 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return false;
    }
    value[3..].split('\\').all(|part| {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || ":/<>\"|?*".contains(c))
        {
            return false;
        }
        let base = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_uppercase();
        !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
            && !(["COM", "LPT"].iter().any(|prefix| {
                base.strip_prefix(prefix).is_some_and(|n| {
                    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&n)
                })
            }))
    })
}

#[cfg(windows)]
pub(crate) fn read_selected(path: &std::path::Path) -> Result<OpenedText, FileOpenError> {
    use std::{
        fs::{self, OpenOptions},
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
    };
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{
                GetDriveTypeW, GetFileType, GetFinalPathNameByHandleW,
                FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FILE_TYPE_DISK, VOLUME_NAME_DOS,
            },
        },
    };
    use FileOpenError as E;
    let value = path
        .to_str()
        .filter(|s| ordinary_drive_path(s))
        .ok_or(E::FileOpenUnsupportedError)?;
    let local = |v: &str| {
        let root: Vec<u16> = v[..3].encode_utf16().chain(Some(0)).collect();
        matches!(unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }, 2 | 3)
    };
    if !local(value) {
        return Err(E::FileOpenUnsupportedError);
    }
    let mut prefix = std::path::PathBuf::from(&value[..3]);
    for part in value[3..].split('\\') {
        prefix.push(part);
        let meta = fs::symlink_metadata(&prefix).map_err(|_| E::FileOpenFailedError)?;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(E::FileOpenUnsupportedError);
        }
    }
    if !fs::symlink_metadata(path)
        .map_err(|_| E::FileOpenFailedError)?
        .is_file()
    {
        return Err(E::FileOpenUnsupportedError);
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|_| E::FileOpenFailedError)?;
    let meta = file.metadata().map_err(|_| E::FileOpenFailedError)?;
    let handle = HANDLE(file.as_raw_handle());
    if !meta.is_file()
        || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || unsafe { GetFileType(handle) } != FILE_TYPE_DISK
    {
        return Err(E::FileOpenUnsupportedError);
    }
    let mut final_path = vec![0u16; 32768];
    let n = unsafe { GetFinalPathNameByHandleW(handle, &mut final_path, VOLUME_NAME_DOS) } as usize;
    if n == 0 || n >= final_path.len() {
        return Err(E::FileOpenUnsupportedError);
    }
    let resolved = String::from_utf16(&final_path[..n]).map_err(|_| E::FileOpenUnsupportedError)?;
    let resolved = resolved
        .strip_prefix(r"\\?\")
        .ok_or(E::FileOpenUnsupportedError)?;
    if !ordinary_drive_path(resolved) || !local(resolved) || !resolved.eq_ignore_ascii_case(value) {
        return Err(E::FileOpenUnsupportedError);
    }
    if meta.len() > MAX_TEXT_BYTES as u64 {
        return Err(E::FileOpenTooLargeError);
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(E::FileOpenUnsupportedError)?
        .to_owned();
    read_bounded(name, file)
}
#[cfg(not(windows))]
pub(crate) fn read_selected(_: &std::path::Path) -> Result<OpenedText, FileOpenError> {
    Err(FileOpenError::FileOpenUnsupportedError)
}

/// Runs entirely on the blocking worker. Its caller owns the physical permit on that stack.
/// Even cancellation/error must recheck ownership before focus or publication.
pub(crate) fn select_and_read(
    check: impl Fn() -> Result<(), FileOpenError>,
    choose: impl FnOnce() -> Result<Option<std::path::PathBuf>, FileOpenError>,
    read: impl FnOnce(&std::path::Path) -> Result<OpenedText, FileOpenError>,
    focus: impl FnOnce() -> Result<(), FileOpenError>,
) -> Result<Option<OpenedText>, FileOpenError> {
    check()?;
    let selected = choose();
    check()?;
    let result = match selected {
        Ok(Some(path)) => read(&path).map(Some),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    };
    check()?;
    focus()?;
    check()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_open_reader_exact_empty_bom_and_strict_encoding() {
        for (bytes, text) in [
            (vec![], "".into()),
            (b"\xef\xbb\xbf x\r\n".to_vec(), " x\r\n".into()),
            (vec![b'x'; 8192], "x".repeat(8192)),
        ] {
            let len = bytes.len();
            let value = read_bounded("a.txt".into(), bytes.as_slice()).unwrap();
            assert_eq!(value.text, text);
            assert_eq!(value.byte_length, len);
        }
        for bytes in [vec![0xff], vec![0], b"\xff\xfea\0".to_vec()] {
            assert_eq!(
                read_bounded("a".into(), bytes.as_slice()),
                Err(FileOpenError::FileOpenEncodingError)
            );
        }
        assert_eq!(
            read_bounded("a".into(), vec![b'x'; 8193].as_slice()),
            Err(FileOpenError::FileOpenTooLargeError)
        );
    }
    #[test]
    fn file_open_reader_never_consumes_above_limit_plus_one() {
        struct Growing(usize);
        impl Read for Growing {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                self.0 += b.len();
                assert!(self.0 <= 8193);
                b.fill(b'x');
                Ok(b.len())
            }
        }
        assert_eq!(
            read_bounded("a".into(), Growing(0)),
            Err(FileOpenError::FileOpenTooLargeError)
        );
    }
    #[test]
    fn file_open_rejects_nonordinary_paths_before_io() {
        for path in [
            r"relative.txt",
            r"C:foo.txt",
            r"\\server\share\file",
            r"\\?\C:\file",
            r"\\.\NUL",
            r"C:\a:stream",
            r"C:\a\..\b",
            r"C:\a\.\b",
            r"C:\NUL.txt",
            r"C:\NUL .txt",
            r"C:\COM1 .json",
            r"C:\COM1",
            r"C:\LPT¹.txt",
            r"C:\trailing.\a",
            r"C:/a",
            r"C:\a\\b",
        ] {
            assert!(!ordinary_drive_path(path), "{path}");
        }
        assert!(ordinary_drive_path(r"C:\folder\文件.txt"));
    }
    #[test]
    fn file_open_checks_before_adapter_read_focus_and_delivery_on_all_outcomes() {
        use std::cell::Cell;
        for fail_at in 1..=4 {
            let checks = Cell::new(0);
            let chosen = Cell::new(false);
            let read = Cell::new(false);
            let focused = Cell::new(false);
            let result = select_and_read(
                || {
                    checks.set(checks.get() + 1);
                    if checks.get() == fail_at {
                        Err(FileOpenError::ExpiredWindowSessionError)
                    } else {
                        Ok(())
                    }
                },
                || {
                    chosen.set(true);
                    Ok(Some("fixture".into()))
                },
                |_| {
                    read.set(true);
                    Ok(OpenedText {
                        name: "a".into(),
                        text: "".into(),
                        byte_length: 0,
                    })
                },
                || {
                    focused.set(true);
                    Ok(())
                },
            );
            assert_eq!(result, Err(FileOpenError::ExpiredWindowSessionError));
            assert_eq!(chosen.get(), fail_at > 1);
            assert_eq!(read.get(), fail_at > 2);
            assert_eq!(focused.get(), fail_at > 3);
        }
        for outcome in [Ok(None), Err(FileOpenError::FileOpenFailedError)] {
            let focused = Cell::new(false);
            let result = select_and_read(
                || Ok(()),
                || outcome.clone(),
                |_| panic!("cancel/error must not read"),
                || {
                    focused.set(true);
                    Ok(())
                },
            );
            assert_eq!(result, outcome.map(|_| None));
            assert!(focused.get());
        }
    }
    #[test]
    #[cfg(windows)]
    fn file_open_windows_handle_reads_only_generated_ordinary_fixtures() {
        let root = std::env::temp_dir().join(format!("uipilot-file-open-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("text.json");
        std::fs::write(&file, b"\xef\xbb\xbf{bad JSON}\r\n").unwrap();
        let value = read_selected(&file).unwrap();
        assert_eq!(value.name, "text.json");
        assert_eq!(value.text, "{bad JSON}\r\n");
        assert_eq!(value.byte_length, 15);
        assert_eq!(
            read_selected(&root),
            Err(FileOpenError::FileOpenUnsupportedError)
        );
        let link = root.join("linked.json");
        if std::os::windows::fs::symlink_file(&file, &link).is_ok() {
            assert_eq!(
                read_selected(&link),
                Err(FileOpenError::FileOpenUnsupportedError)
            );
            std::fs::remove_file(link).unwrap();
        }
        std::fs::write(&file, vec![b'x'; 8193]).unwrap();
        assert_eq!(
            read_selected(&file),
            Err(FileOpenError::FileOpenTooLargeError)
        );
        std::fs::write(&file, b"\xff").unwrap();
        assert_eq!(
            read_selected(&file),
            Err(FileOpenError::FileOpenEncodingError)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
