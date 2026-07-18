use std::{
    fmt, fs,
    fs::OpenOptions,
    io,
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use windows::{
    core::PCWSTR,
    Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MOVE_FILE_FLAGS,
    },
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(crate) struct AtomicPaths {
    current: PathBuf,
    backup: PathBuf,
}

impl AtomicPaths {
    pub(crate) fn new(directory: &Path, file_name: &str) -> Self {
        Self {
            current: directory.join(file_name),
            backup: directory.join(format!("{file_name}.backup")),
        }
    }

    pub(crate) fn current(&self) -> &Path {
        &self.current
    }

    pub(crate) fn backup(&self) -> &Path {
        &self.backup
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtomicFileError {
    Read,
    CandidateWrite,
    BackupWrite,
    BackupReplace,
    CurrentReplace,
    InvalidQuarantine,
}

impl fmt::Display for AtomicFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Read => "atomic file read failed",
            Self::CandidateWrite => "atomic candidate write failed",
            Self::BackupWrite => "atomic backup write failed",
            Self::BackupReplace => "atomic backup replace failed",
            Self::CurrentReplace => "atomic current replace failed",
            Self::InvalidQuarantine => "atomic invalid quarantine failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AtomicFileError {}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, AtomicFileError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(AtomicFileError::Read),
    }
}

pub(crate) fn quarantine_invalid(path: &Path) -> Result<(), AtomicFileError> {
    let destination = sibling_temp(path, "invalid");
    replace_file(path, &destination, MOVE_FILE_FLAGS(0))
        .map_err(|_| AtomicFileError::InvalidQuarantine)
}

pub(crate) fn commit_with_backup(
    paths: &AtomicPaths,
    previous: Option<&[u8]>,
    candidate: &[u8],
) -> Result<(), AtomicFileError> {
    commit_with(paths, previous, candidate, write_synced, replace_file)
}

pub(crate) fn replace_without_backup(
    destination: &Path,
    candidate: &[u8],
) -> Result<(), AtomicFileError> {
    let candidate_temp = sibling_temp(destination, "temp");
    if write_synced(&candidate_temp, candidate).is_err() {
        remove_temp(&candidate_temp);
        return Err(AtomicFileError::CandidateWrite);
    }
    if replace_file(&candidate_temp, destination, replace_flags()).is_err() {
        remove_temp(&candidate_temp);
        return Err(AtomicFileError::CurrentReplace);
    }
    Ok(())
}

fn commit_with<W, R>(
    paths: &AtomicPaths,
    previous: Option<&[u8]>,
    candidate: &[u8],
    mut write_synced: W,
    mut replace: R,
) -> Result<(), AtomicFileError>
where
    W: FnMut(&Path, &[u8]) -> io::Result<()>,
    R: FnMut(&Path, &Path, MOVE_FILE_FLAGS) -> io::Result<()>,
{
    let candidate_temp = sibling_temp(paths.current(), "temp");
    if write_synced(&candidate_temp, candidate).is_err() {
        remove_temp(&candidate_temp);
        return Err(AtomicFileError::CandidateWrite);
    }

    let backup_temp = previous.map(|_| sibling_temp(paths.backup(), "temp"));
    if let (Some(previous), Some(backup_temp)) = (previous, backup_temp.as_deref()) {
        if write_synced(backup_temp, previous).is_err() {
            remove_temp(&candidate_temp);
            remove_temp(backup_temp);
            return Err(AtomicFileError::BackupWrite);
        }
        if replace(backup_temp, paths.backup(), replace_flags()).is_err() {
            remove_temp(&candidate_temp);
            remove_temp(backup_temp);
            return Err(AtomicFileError::BackupReplace);
        }
    }

    if replace(&candidate_temp, paths.current(), replace_flags()).is_err() {
        remove_temp(&candidate_temp);
        if let Some(backup_temp) = backup_temp.as_deref() {
            remove_temp(backup_temp);
        }
        return Err(AtomicFileError::CurrentReplace);
    }
    Ok(())
}

fn replace_flags() -> MOVE_FILE_FLAGS {
    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
}

fn sibling_temp(destination: &Path, label: &str) -> PathBuf {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let file_name = destination.file_name().unwrap_or_default();
    let mut name = file_name.to_os_string();
    name.push(format!(".{label}-{}-{id}", std::process::id()));
    destination.with_file_name(name)
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    Ok(())
}

fn replace_file(source: &Path, destination: &Path, flags: MOVE_FILE_FLAGS) -> io::Result<()> {
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    unsafe {
        MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(destination.as_ptr()), flags)
            .map_err(|_| io::Error::last_os_error())
    }
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL code unit",
        ));
    }
    value.push(0);
    Ok(value)
}

fn remove_temp(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use std::{
        fs, io,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS;

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-atomic-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn seeded_paths(dir: &TestDir) -> AtomicPaths {
        let paths = AtomicPaths::new(dir.path(), "settings.json");
        fs::write(paths.current(), b"old-current").unwrap();
        fs::write(paths.backup(), b"older-backup").unwrap();
        paths
    }

    fn assert_only_current_and_backup(paths: &AtomicPaths) {
        let mut names = fs::read_dir(paths.current().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            ["settings.json", "settings.json.backup"].map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn second_commit_preserves_first_commit_as_backup() {
        let dir = TestDir::new("two-commits");
        let paths = AtomicPaths::new(dir.path(), "settings.json");

        commit_with_backup(&paths, None, br#"{"value":1}"#).unwrap();
        commit_with_backup(&paths, Some(br#"{"value":1}"#), br#"{"value":2}"#).unwrap();

        assert_eq!(fs::read(paths.current()).unwrap(), br#"{"value":2}"#);
        assert_eq!(fs::read(paths.backup()).unwrap(), br#"{"value":1}"#);
        assert_only_current_and_backup(&paths);
    }

    #[test]
    fn candidate_temp_failure_preserves_disk_state() {
        let dir = TestDir::new("candidate-write-failure");
        let paths = seeded_paths(&dir);

        let error = commit_with(
            &paths,
            Some(b"old-current"),
            b"candidate",
            |_path, _bytes| Err(io::Error::other("candidate write")),
            |_source, _destination, _flags| panic!("replace must not run"),
        )
        .unwrap_err();

        assert_eq!(error, AtomicFileError::CandidateWrite);
        assert_eq!(fs::read(paths.current()).unwrap(), b"old-current");
        assert_eq!(fs::read(paths.backup()).unwrap(), b"older-backup");
        assert_only_current_and_backup(&paths);
    }

    #[test]
    fn backup_temp_failure_preserves_disk_state() {
        let dir = TestDir::new("backup-write-failure");
        let paths = seeded_paths(&dir);
        let mut writes = 0;

        let error = commit_with(
            &paths,
            Some(b"old-current"),
            b"candidate",
            |path, bytes| {
                writes += 1;
                if writes == 2 {
                    Err(io::Error::other("backup write"))
                } else {
                    write_synced(path, bytes)
                }
            },
            |_source, _destination, _flags| panic!("replace must not run"),
        )
        .unwrap_err();

        assert_eq!(error, AtomicFileError::BackupWrite);
        assert_eq!(fs::read(paths.current()).unwrap(), b"old-current");
        assert_eq!(fs::read(paths.backup()).unwrap(), b"older-backup");
        assert_only_current_and_backup(&paths);
    }

    #[test]
    fn backup_move_failure_preserves_disk_state() {
        let dir = TestDir::new("backup-move-failure");
        let paths = seeded_paths(&dir);

        let error = commit_with(
            &paths,
            Some(b"old-current"),
            b"candidate",
            write_synced,
            |_source, _destination, flags| {
                assert_eq!(flags, replace_flags());
                Err(io::Error::other("backup move"))
            },
        )
        .unwrap_err();

        assert_eq!(error, AtomicFileError::BackupReplace);
        assert_eq!(fs::read(paths.current()).unwrap(), b"old-current");
        assert_eq!(fs::read(paths.backup()).unwrap(), b"older-backup");
        assert_only_current_and_backup(&paths);
    }

    #[test]
    fn current_move_failure_keeps_current_and_refreshes_backup() {
        let dir = TestDir::new("current-move-failure");
        let paths = seeded_paths(&dir);
        let mut replacements = 0;

        let error = commit_with(
            &paths,
            Some(b"old-current"),
            b"candidate",
            write_synced,
            |source, destination, flags: MOVE_FILE_FLAGS| {
                replacements += 1;
                assert_eq!(flags, replace_flags());
                if replacements == 2 {
                    Err(io::Error::other("current move"))
                } else {
                    replace_file(source, destination, flags)
                }
            },
        )
        .unwrap_err();

        assert_eq!(error, AtomicFileError::CurrentReplace);
        assert_eq!(fs::read(paths.current()).unwrap(), b"old-current");
        assert_eq!(fs::read(paths.backup()).unwrap(), b"old-current");
        assert_only_current_and_backup(&paths);
    }

    #[test]
    fn read_optional_ignores_sibling_temp_files() {
        let dir = TestDir::new("ignore-temp");
        let paths = AtomicPaths::new(dir.path(), "settings.json");
        fs::write(dir.path().join("settings.json.temp-1-1"), b"candidate").unwrap();
        fs::write(
            dir.path().join("settings.json.backup.temp-1-2"),
            b"previous",
        )
        .unwrap();

        assert_eq!(read_optional(paths.current()).unwrap(), None);
        assert_eq!(read_optional(paths.backup()).unwrap(), None);
    }

    #[test]
    fn read_optional_reads_exact_file_and_rejects_other_io_errors() {
        let dir = TestDir::new("read-optional");
        let path = dir.path().join("settings.json");

        assert_eq!(read_optional(&path).unwrap(), None);
        fs::write(&path, b"settings").unwrap();
        assert_eq!(read_optional(&path).unwrap(), Some(b"settings".to_vec()));
        assert_eq!(
            read_optional(dir.path()).unwrap_err(),
            AtomicFileError::Read
        );
    }

    #[test]
    fn quarantine_invalid_uses_unique_non_overwriting_siblings() {
        let dir = TestDir::new("quarantine");
        let path = dir.path().join("settings.json");

        fs::write(&path, b"first-invalid").unwrap();
        quarantine_invalid(&path).unwrap();
        fs::write(&path, b"second-invalid").unwrap();
        quarantine_invalid(&path).unwrap();

        assert!(!path.exists());
        let mut quarantined = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect::<Vec<_>>();
        quarantined.sort_by_key(|entry| entry.file_name());
        assert_eq!(quarantined.len(), 2);
        assert!(quarantined.iter().all(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("settings.json.invalid-")));
        let contents = quarantined
            .iter()
            .map(|entry| fs::read(entry.path()).unwrap())
            .collect::<Vec<_>>();
        assert!(contents.contains(&b"first-invalid".to_vec()));
        assert!(contents.contains(&b"second-invalid".to_vec()));
    }

    #[test]
    fn replace_without_backup_only_replaces_current() {
        let dir = TestDir::new("replace-without-backup");
        let paths = AtomicPaths::new(dir.path(), "validation-data.json");
        fs::write(paths.current(), b"old").unwrap();

        replace_without_backup(paths.current(), b"new").unwrap();

        assert_eq!(fs::read(paths.current()).unwrap(), b"new");
        assert!(!paths.backup().exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn errors_have_fixed_path_free_messages() {
        fn assert_std_error<T: std::error::Error>() {}
        assert_std_error::<AtomicFileError>();

        let cases = [
            (AtomicFileError::Read, "atomic file read failed"),
            (
                AtomicFileError::CandidateWrite,
                "atomic candidate write failed",
            ),
            (AtomicFileError::BackupWrite, "atomic backup write failed"),
            (
                AtomicFileError::BackupReplace,
                "atomic backup replace failed",
            ),
            (
                AtomicFileError::CurrentReplace,
                "atomic current replace failed",
            ),
            (
                AtomicFileError::InvalidQuarantine,
                "atomic invalid quarantine failed",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert!(!error.to_string().contains(':'));
        }
    }
}
