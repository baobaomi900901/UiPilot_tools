use std::ffi::c_void;

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FileAttributeTagInfo, FileBasicInfo, FileIdInfo, FileStandardInfo,
            GetDriveTypeW, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
            GetVolumeInformationByHandleW, GetVolumeInformationW,
            GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FILE_STANDARD_INFO, GETFINALPATHNAMEBYHANDLE_FLAGS, OPEN_EXISTING, VOLUME_NAME_DOS,
            VOLUME_NAME_GUID,
        },
        System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::{
            Shell::{
                Common::ITEMIDLIST, ILClone, ILCreateFromPathW, ILFindLastID, ILRemoveLastID,
                SHOpenFolderAndSelectItems, ShellExecuteExW, SEE_MASK_FLAG_NO_UI,
                SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

use crate::file_search::{FileExecutionError, FileExecutionOutcome, FilePathKind};

struct ResolvedPathExpectation {
    identity: AuthenticatedPathIdentity,
    filesystem_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedPathIdentity {
    pub(crate) display_path: String,
    pub(crate) volume_guid_path: String,
    pub(crate) relative_path: String,
    pub(crate) volume_serial: u32,
    pub(crate) file_id: [u8; 16],
    pub(crate) kind: FilePathKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedPathSnapshot {
    pub(crate) identity: AuthenticatedPathIdentity,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_filetime: u64,
}

#[cfg(test)]
pub(crate) struct LegacyPathExpectation<'a> {
    pub(crate) volume_guid_path: &'a str,
    pub(crate) volume_serial: u32,
    pub(crate) filesystem_name: &'a str,
    pub(crate) relative_path: &'a str,
    pub(crate) kind: FilePathKind,
}

pub(crate) fn authenticate_path(
    display_path: &str,
    expected_kind: FilePathKind,
) -> Result<AuthenticatedPathSnapshot, FileExecutionError> {
    let expected = expected_path_from_display(display_path, expected_kind)?;
    authenticate_path_with(
        display_path,
        &expected.identity,
        Some(&expected.filesystem_name),
        |relative, kind, _| {
            open_execution_component(&expected.identity.volume_guid_path, relative, kind)
        },
        |_, _, handle| inspect_execution_component(handle, &expected.identity),
    )
}

fn authenticate_path_with<H, O, I>(
    display_path: &str,
    expected: &AuthenticatedPathIdentity,
    expected_filesystem_name: Option<&str>,
    open: O,
    inspect: I,
) -> Result<AuthenticatedPathSnapshot, FileExecutionError>
where
    O: FnMut(&str, FilePathKind, ExecutionShare) -> Result<H, FileExecutionError>,
    I: FnMut(&str, FilePathKind, &H) -> Result<ComponentObservation, FileExecutionError>,
{
    let (handles, observation) = walk_expected_components_and_observe_with(
        expected,
        expected_filesystem_name,
        false,
        open,
        inspect,
    )?;
    let _handles = handles;
    Ok(AuthenticatedPathSnapshot {
        identity: AuthenticatedPathIdentity {
            display_path: display_path.into(),
            volume_guid_path: observation.volume_guid_path,
            relative_path: observation.relative_path,
            volume_serial: observation.volume_serial,
            file_id: observation.file_id,
            kind: observation.kind,
        },
        size_bytes: (observation.kind == FilePathKind::File).then_some(observation.size_bytes),
        modified_filetime: observation.modified_filetime,
    })
}

pub(crate) fn execute_authenticated_path(
    identity: &AuthenticatedPathIdentity,
) -> Result<FileExecutionOutcome, FileExecutionError> {
    execute_authenticated_path_with_shell(identity, None, execute_shell)
}

fn execute_authenticated_path_with_shell<S>(
    identity: &AuthenticatedPathIdentity,
    expected_filesystem_name: Option<&str>,
    shell: S,
) -> Result<FileExecutionOutcome, FileExecutionError>
where
    S: FnOnce(&str, FilePathKind) -> Result<FileExecutionOutcome, FileExecutionError>,
{
    execute_authenticated_path_with(
        identity,
        expected_filesystem_name,
        |relative, kind, _| open_execution_component(&identity.volume_guid_path, relative, kind),
        |_, _, handle| inspect_execution_component(handle, identity),
        shell,
    )
}

fn execute_authenticated_path_with<H, O, I, S>(
    identity: &AuthenticatedPathIdentity,
    expected_filesystem_name: Option<&str>,
    open: O,
    inspect: I,
    shell: S,
) -> Result<FileExecutionOutcome, FileExecutionError>
where
    O: FnMut(&str, FilePathKind, ExecutionShare) -> Result<H, FileExecutionError>,
    I: FnMut(&str, FilePathKind, &H) -> Result<ComponentObservation, FileExecutionError>,
    S: FnOnce(&str, FilePathKind) -> Result<FileExecutionOutcome, FileExecutionError>,
{
    let (handles, observation) = walk_expected_components_and_observe_with(
        identity,
        expected_filesystem_name,
        true,
        open,
        inspect,
    )?;
    let path = observation.shell_path;
    execute_with_components(handles, || shell(&path, identity.kind))
}

#[cfg(test)]
pub(crate) fn execute_legacy_indexed_path_with_shell<S>(
    expectation: LegacyPathExpectation<'_>,
    shell: S,
) -> Result<FileExecutionOutcome, FileExecutionError>
where
    S: FnOnce(&str, FilePathKind) -> Result<FileExecutionOutcome, FileExecutionError>,
{
    validate_relative_path(expectation.relative_path)?;
    let identity = AuthenticatedPathIdentity {
        display_path: joined_path(expectation.volume_guid_path, expectation.relative_path),
        volume_guid_path: normalize_guid(expectation.volume_guid_path)?,
        relative_path: expectation.relative_path.into(),
        volume_serial: expectation.volume_serial,
        file_id: [0; 16],
        kind: expectation.kind,
    };
    execute_authenticated_path_with_shell(&identity, Some(expectation.filesystem_name), shell)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionShare {
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComponentObservation {
    reparse: bool,
    kind: FilePathKind,
    shell_path: String,
    volume_guid_path: String,
    volume_serial: u32,
    filesystem_name: String,
    relative_path: String,
    file_id: [u8; 16],
    size_bytes: u64,
    modified_filetime: u64,
}

#[cfg(test)]
fn walk_expected_components_with<H, O, I>(
    expected: &AuthenticatedPathIdentity,
    open: O,
    inspect: I,
) -> Result<Vec<H>, FileExecutionError>
where
    O: FnMut(&str, FilePathKind, ExecutionShare) -> Result<H, FileExecutionError>,
    I: FnMut(&str, FilePathKind, &H) -> Result<ComponentObservation, FileExecutionError>,
{
    walk_expected_components_and_observe_with(expected, None, true, open, inspect)
        .map(|(handles, _)| handles)
}

fn walk_expected_components_and_observe_with<H, O, I>(
    expected: &AuthenticatedPathIdentity,
    expected_filesystem_name: Option<&str>,
    require_exact_relative_path: bool,
    mut open: O,
    mut inspect: I,
) -> Result<(Vec<H>, ComponentObservation), FileExecutionError>
where
    O: FnMut(&str, FilePathKind, ExecutionShare) -> Result<H, FileExecutionError>,
    I: FnMut(&str, FilePathKind, &H) -> Result<ComponentObservation, FileExecutionError>,
{
    validate_relative_path(&expected.relative_path)?;
    let expected_volume_guid_path = normalize_guid(&expected.volume_guid_path)?;
    let components = expected.relative_path.split('\\').collect::<Vec<_>>();
    let mut handles = Vec::with_capacity(components.len());
    let mut relative_path = String::new();
    let mut final_observation = None;
    for (index, component) in components.iter().enumerate() {
        if !relative_path.is_empty() {
            relative_path.push('\\');
        }
        relative_path.push_str(component);
        let is_final = index + 1 == components.len();
        let kind = if is_final {
            expected.kind
        } else {
            FilePathKind::Directory
        };
        let handle = open(&relative_path, kind, ExecutionShare::ReadWrite)?;
        let mut observation = inspect(&relative_path, kind, &handle)?;
        observation.volume_guid_path = normalize_guid(&observation.volume_guid_path)?;
        observation.relative_path = normalize_observed_relative_path(&observation.relative_path)?;
        observation.filesystem_name = observation.filesystem_name.to_uppercase();
        if observation.reparse
            || observation.kind != kind
            || observation.volume_serial != expected.volume_serial
            || !observation
                .volume_guid_path
                .eq_ignore_ascii_case(&expected_volume_guid_path)
            || if require_exact_relative_path {
                observation.relative_path != relative_path
            } else {
                !observation
                    .relative_path
                    .eq_ignore_ascii_case(&relative_path)
            }
            || expected_filesystem_name.is_some_and(|filesystem_name| {
                !observation
                    .filesystem_name
                    .eq_ignore_ascii_case(filesystem_name)
            })
            || (is_final && expected.file_id != [0; 16] && observation.file_id != expected.file_id)
        {
            return Err(FileExecutionError::Stale);
        }
        handles.push(handle);
        if is_final {
            final_observation = Some(observation);
        }
    }
    Ok((handles, final_observation.ok_or(FileExecutionError::Stale)?))
}

pub(crate) struct OwnedHandle(pub(crate) HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn execute_with_components<H>(
    handles: Vec<H>,
    callback: impl FnOnce() -> Result<FileExecutionOutcome, FileExecutionError>,
) -> Result<FileExecutionOutcome, FileExecutionError> {
    let _handles = handles;
    callback()
}

fn expected_path_from_display(
    display_path: &str,
    kind: FilePathKind,
) -> Result<ResolvedPathExpectation, FileExecutionError> {
    let display_wide = to_wide(display_path)?;
    let mut mount = vec![0u16; 32_768];
    unsafe { GetVolumePathNameW(PCWSTR(display_wide.as_ptr()), &mut mount) }
        .map_err(map_windows_open_error)?;
    let mount = from_nul_terminated(&mount)?;
    let mount_wide = to_wide(&mount)?;
    if unsafe { GetDriveTypeW(PCWSTR(mount_wide.as_ptr())) } != 3 {
        return Err(FileExecutionError::Stale);
    }
    let mut volume_guid = vec![0u16; 64];
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount_wide.as_ptr()), &mut volume_guid) }
        .map_err(map_windows_open_error)?;
    let mut volume_serial = 0u32;
    let mut filesystem = vec![0u16; 64];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(mount_wide.as_ptr()),
            None,
            Some(&mut volume_serial),
            None,
            None,
            Some(&mut filesystem),
        )
    }
    .map_err(map_windows_open_error)?;
    let normalized_display = display_path.replace('/', "\\");
    let normalized_mount = mount.replace('/', "\\");
    if normalized_display.len() < normalized_mount.len()
        || !normalized_display[..normalized_mount.len()].eq_ignore_ascii_case(&normalized_mount)
    {
        return Err(FileExecutionError::Stale);
    }
    let relative_path = normalized_display[normalized_mount.len()..]
        .trim_matches('\\')
        .to_owned();
    validate_relative_path(&relative_path)?;
    let filesystem_name = from_nul_terminated(&filesystem)?.to_uppercase();
    Ok(ResolvedPathExpectation {
        identity: AuthenticatedPathIdentity {
            display_path: display_path.into(),
            volume_guid_path: normalize_guid(&from_nul_terminated(&volume_guid)?)?,
            relative_path,
            volume_serial,
            file_id: [0; 16],
            kind,
        },
        filesystem_name,
    })
}

fn normalize_guid(value: &str) -> Result<String, FileExecutionError> {
    if value.is_empty() {
        return Err(FileExecutionError::Stale);
    }
    let mut normalized = value.replace('/', "\\").to_uppercase();
    if !normalized.ends_with('\\') {
        normalized.push('\\');
    }
    Ok(normalized)
}

fn validate_relative_path(value: &str) -> Result<(), FileExecutionError> {
    let components = value.split('\\').collect::<Vec<_>>();
    if components.is_empty()
        || components.iter().any(|component| {
            component.is_empty()
                || *component == "."
                || *component == ".."
                || component.contains('/')
        })
    {
        Err(FileExecutionError::Stale)
    } else {
        Ok(())
    }
}

fn normalize_observed_relative_path(value: &str) -> Result<String, FileExecutionError> {
    let normalized = value.replace('/', "\\").trim_matches('\\').to_owned();
    validate_relative_path(&normalized)?;
    Ok(normalized)
}

fn joined_path(volume_guid_path: &str, relative_path: &str) -> String {
    if relative_path.is_empty() {
        volume_guid_path.into()
    } else {
        format!(
            "{}\\{relative_path}",
            volume_guid_path.trim_end_matches('\\')
        )
    }
}

fn to_wide(value: &str) -> Result<Vec<u16>, FileExecutionError> {
    if value.contains('\0') {
        return Err(FileExecutionError::Stale);
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

fn from_nul_terminated(value: &[u16]) -> Result<String, FileExecutionError> {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16(&value[..end]).map_err(|_| FileExecutionError::Stale)
}

fn map_windows_open_error(error: windows::core::Error) -> FileExecutionError {
    let code = error.code();
    if code == windows::core::HRESULT::from_win32(2)
        || code == windows::core::HRESULT::from_win32(3)
    {
        FileExecutionError::NotFound
    } else {
        FileExecutionError::OpenFailed
    }
}

fn open_execution_component(
    volume_guid_path: &str,
    relative_path: &str,
    kind: FilePathKind,
) -> Result<OwnedHandle, FileExecutionError> {
    let path = joined_path(volume_guid_path, relative_path);
    let wide = to_wide(&path)?;
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    let desired_access = match kind {
        FilePathKind::File => FILE_READ_ATTRIBUTES.0,
        FilePathKind::Directory => {
            flags |= FILE_FLAG_BACKUP_SEMANTICS;
            FILE_LIST_DIRECTORY.0
        }
    };
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .map(OwnedHandle)
    .map_err(map_windows_open_error)
}

fn inspect_execution_component(
    handle: &OwnedHandle,
    expected: &AuthenticatedPathIdentity,
) -> Result<ComponentObservation, FileExecutionError> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>())
                .map_err(|_| FileExecutionError::OpenFailed)?,
        )
    }
    .map_err(map_windows_open_error)?;
    let mut basic = FILE_BASIC_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>())
                .map_err(|_| FileExecutionError::OpenFailed)?,
        )
    }
    .map_err(map_windows_open_error)?;
    let mut standard = FILE_STANDARD_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_STANDARD_INFO>())
                .map_err(|_| FileExecutionError::OpenFailed)?,
        )
    }
    .map_err(map_windows_open_error)?;
    let mut serial = 0u32;
    let mut filesystem = vec![0u16; 64];
    unsafe {
        GetVolumeInformationByHandleW(
            handle.0,
            None,
            Some(&mut serial),
            None,
            None,
            Some(&mut filesystem),
        )
    }
    .map_err(map_windows_open_error)?;
    let final_path = final_path(handle.0)?;
    let expected_volume_guid_path = normalize_guid(&expected.volume_guid_path)?;
    if final_path.len() < expected_volume_guid_path.len()
        || !final_path[..expected_volume_guid_path.len()]
            .eq_ignore_ascii_case(&expected_volume_guid_path)
    {
        return Err(FileExecutionError::Stale);
    }
    let volume_guid_path = normalize_guid(&final_path[..expected_volume_guid_path.len()])?;
    Ok(ComponentObservation {
        reparse: tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
        kind: if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            FilePathKind::Directory
        } else {
            FilePathKind::File
        },
        shell_path: final_shell_path(handle.0)?,
        volume_guid_path,
        volume_serial: serial,
        filesystem_name: from_nul_terminated(&filesystem)?.to_uppercase(),
        relative_path: normalize_observed_relative_path(
            &final_path[expected_volume_guid_path.len()..],
        )?,
        file_id: read_file_id(handle.0)?,
        size_bytes: u64::try_from(standard.EndOfFile).map_err(|_| FileExecutionError::Stale)?,
        modified_filetime: u64::try_from(basic.LastWriteTime)
            .map_err(|_| FileExecutionError::Stale)?,
    })
}

fn final_path(handle: HANDLE) -> Result<String, FileExecutionError> {
    final_path_with_flags(handle, VOLUME_NAME_GUID)
}

fn final_shell_path(handle: HANDLE) -> Result<String, FileExecutionError> {
    let path = final_path_with_flags(handle, VOLUME_NAME_DOS)?;
    path.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .ok_or(FileExecutionError::OpenFailed)
}

fn final_path_with_flags(
    handle: HANDLE,
    flags: GETFINALPATHNAMEBYHANDLE_FLAGS,
) -> Result<String, FileExecutionError> {
    let mut path = vec![0u16; 32_768];
    let written = unsafe { GetFinalPathNameByHandleW(handle, &mut path, flags) };
    let written = usize::try_from(written).map_err(|_| FileExecutionError::OpenFailed)?;
    if written == 0 || written >= path.len() {
        return Err(FileExecutionError::OpenFailed);
    }
    String::from_utf16(&path[..written]).map_err(|_| FileExecutionError::Stale)
}

fn read_file_id(handle: HANDLE) -> Result<[u8; 16], FileExecutionError> {
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>())
                .map_err(|_| FileExecutionError::OpenFailed)?,
        )
    }
    .map_err(map_windows_open_error)?;
    Ok(info.FileId.Identifier)
}

struct OwnedPidl(*mut ITEMIDLIST);

impl Drop for OwnedPidl {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, FileExecutionError> {
        if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
            Err(FileExecutionError::OpenFailed)
        } else {
            Ok(Self)
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn execute_shell(
    path: &str,
    kind: FilePathKind,
) -> Result<FileExecutionOutcome, FileExecutionError> {
    execute_shell_with(path, kind, |path, kind| match kind {
        FilePathKind::File => {
            reveal_file(path)?;
            Ok(FileExecutionOutcome::FileRevealRequested)
        }
        FilePathKind::Directory => {
            open_directory(path)?;
            Ok(FileExecutionOutcome::FolderOpenRequested)
        }
    })
}

fn execute_shell_with<S>(
    path: &str,
    kind: FilePathKind,
    shell: S,
) -> Result<FileExecutionOutcome, FileExecutionError>
where
    S: FnOnce(&str, FilePathKind) -> Result<FileExecutionOutcome, FileExecutionError>,
{
    let _apartment = ComApartment::initialize()?;
    shell(path, kind)
}

fn reveal_file(path: &str) -> Result<(), FileExecutionError> {
    let wide = to_wide(path)?;
    let full = OwnedPidl(unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) });
    if full.0.is_null() {
        return Err(FileExecutionError::OpenFailed);
    }
    let folder = OwnedPidl(unsafe { ILClone(full.0) });
    if folder.0.is_null() || !unsafe { ILRemoveLastID(Some(folder.0)) }.as_bool() {
        return Err(FileExecutionError::OpenFailed);
    }
    let child = unsafe { ILFindLastID(full.0) };
    if child.is_null() {
        return Err(FileExecutionError::OpenFailed);
    }
    unsafe { SHOpenFolderAndSelectItems(folder.0, Some(&[child]), 0) }
        .map_err(|_| FileExecutionError::OpenFailed)
}

fn open_directory(path: &str) -> Result<(), FileExecutionError> {
    open_directory_with(path, |info| unsafe { ShellExecuteExW(info) })
}

fn open_directory_with(
    path: &str,
    shell_execute: impl FnOnce(&mut SHELLEXECUTEINFOW) -> windows::core::Result<()>,
) -> Result<(), FileExecutionError> {
    let wide = to_wide(path)?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())
            .map_err(|_| FileExecutionError::OpenFailed)?,
        fMask: SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(wide.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    shell_execute(&mut info).map_err(|_| FileExecutionError::OpenFailed)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        fs,
        path::{Path, PathBuf},
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    #[test]
    fn component_walk_wrapper_is_test_only() {
        let source = include_str!("path_auth.rs").replace("\r\n", "\n");
        assert!(source.contains("#[cfg(test)]\nfn walk_expected_components_with<"));
    }

    static NEXT_TEMP_TREE_ID: AtomicU64 = AtomicU64::new(0);

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            let id = NEXT_TEMP_TREE_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-path-auth-test-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn child(&self, relative_path: impl AsRef<Path>) -> PathBuf {
            self.0.join(relative_path)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn authenticate_test_path(path: &Path, kind: FilePathKind) -> AuthenticatedPathSnapshot {
        authenticate_path(path.to_str().unwrap(), kind).unwrap()
    }

    fn execute_real_path_with_shell(
        identity: &AuthenticatedPathIdentity,
        shell: impl FnOnce(&str, FilePathKind) -> Result<FileExecutionOutcome, FileExecutionError>,
    ) -> Result<FileExecutionOutcome, FileExecutionError> {
        execute_authenticated_path_with_shell(identity, None, shell)
    }

    fn test_identity(relative_path: &str, kind: FilePathKind) -> AuthenticatedPathIdentity {
        AuthenticatedPathIdentity {
            display_path: format!(r"C:\{relative_path}"),
            volume_guid_path: r"\\?\Volume{PATH-AUTH}\".into(),
            relative_path: relative_path.into(),
            volume_serial: 42,
            file_id: [7; 16],
            kind,
        }
    }

    fn unpublished_test_identity(
        relative_path: &str,
        kind: FilePathKind,
    ) -> AuthenticatedPathIdentity {
        AuthenticatedPathIdentity {
            file_id: [0; 16],
            ..test_identity(relative_path, kind)
        }
    }

    fn test_component_observation(relative_path: &str, kind: FilePathKind) -> ComponentObservation {
        ComponentObservation {
            reparse: false,
            kind,
            shell_path: format!(r"C:\authenticated\{relative_path}"),
            volume_guid_path: r"\\?\VOLUME{PATH-AUTH}\".into(),
            volume_serial: 42,
            filesystem_name: "NTFS".into(),
            relative_path: relative_path.into(),
            file_id: [7; 16],
            size_bytes: 123,
            modified_filetime: 456,
        }
    }

    #[derive(Clone, Copy)]
    enum TestMutation {
        Reparse,
        OtherVolume,
        OtherRelativePath,
        WrongKind,
    }

    fn run_mutated_walk(
        expected: &AuthenticatedPathIdentity,
        mutation: TestMutation,
    ) -> Result<Vec<String>, FileExecutionError> {
        walk_expected_components_with(
            expected,
            |relative, _, _| Ok(relative.to_owned()),
            |relative, expected_kind, _| {
                let mut observation = test_component_observation(relative, expected_kind);
                if relative == expected.relative_path {
                    match mutation {
                        TestMutation::Reparse => observation.reparse = true,
                        TestMutation::OtherVolume => observation.volume_serial = 99,
                        TestMutation::OtherRelativePath => {
                            observation.relative_path = "other".into()
                        }
                        TestMutation::WrongKind => observation.kind = FilePathKind::Directory,
                    }
                }
                Ok(observation)
            },
        )
    }

    #[test]
    fn component_walk_uses_no_delete_share_and_rejects_reparse_or_path_substitution() {
        let opened = RefCell::new(Vec::new());
        let expected = test_identity(r"docs\report.pdf", FilePathKind::File);
        let result = walk_expected_components_with(
            &expected,
            |relative, kind, share| {
                opened.borrow_mut().push((relative.to_owned(), kind, share));
                Ok(relative.to_owned())
            },
            |relative, expected_kind, _handle| {
                Ok(test_component_observation(relative, expected_kind))
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            opened.borrow().as_slice(),
            [
                (
                    "docs".into(),
                    FilePathKind::Directory,
                    ExecutionShare::ReadWrite
                ),
                (
                    "docs\\report.pdf".into(),
                    FilePathKind::File,
                    ExecutionShare::ReadWrite,
                ),
            ]
        );

        for mutation in [
            TestMutation::Reparse,
            TestMutation::OtherVolume,
            TestMutation::OtherRelativePath,
            TestMutation::WrongKind,
        ] {
            assert_eq!(
                run_mutated_walk(&expected, mutation),
                Err(FileExecutionError::Stale)
            );
        }
    }

    #[test]
    fn final_metadata_uses_canonical_identity_and_file_information() {
        let expected = unpublished_test_identity(r"docs\report.pdf", FilePathKind::File);
        let snapshot = authenticate_path_with(
            &expected.display_path,
            &expected,
            Some("NTFS"),
            |relative, _, _| Ok(relative.to_owned()),
            |relative, expected_kind, _| {
                let mut observation = test_component_observation(relative, expected_kind);
                observation.volume_guid_path = r"\\?\Volume{path-auth}".into();
                observation.relative_path = if relative == expected.relative_path {
                    "Docs/Report.pdf".into()
                } else {
                    "Docs".into()
                };
                if relative == expected.relative_path {
                    observation.file_id = [0xA5; 16];
                    observation.size_bytes = 8_192;
                    observation.modified_filetime = 1_337;
                }
                Ok(observation)
            },
        )
        .unwrap();

        assert_eq!(
            snapshot,
            AuthenticatedPathSnapshot {
                identity: AuthenticatedPathIdentity {
                    display_path: r"C:\docs\report.pdf".into(),
                    volume_guid_path: r"\\?\VOLUME{PATH-AUTH}\".into(),
                    relative_path: r"Docs\Report.pdf".into(),
                    volume_serial: 42,
                    file_id: [0xA5; 16],
                    kind: FilePathKind::File,
                },
                size_bytes: Some(8_192),
                modified_filetime: 1_337,
            }
        );
    }

    #[test]
    fn real_file_execution_uses_shell_compatible_path_from_revalidated_handle() {
        let tree = TempTree::new();
        let parent = tree.child("Documents");
        fs::create_dir(&parent).unwrap();
        let file = parent.join("Report.txt");
        fs::write(&file, b"report").unwrap();
        let snapshot = authenticate_test_path(&file, FilePathKind::File);
        let mut identity = snapshot.identity.clone();
        identity.display_path = tree.child("decoy.txt").to_string_lossy().into_owned();
        let shell_target = RefCell::new(None);

        let outcome = execute_real_path_with_shell(&identity, |path, kind| {
            shell_target.replace(Some((path.to_owned(), kind)));
            Ok(FileExecutionOutcome::FileRevealRequested)
        })
        .unwrap();

        assert_eq!(snapshot.size_bytes, Some(6));
        assert_eq!(outcome, FileExecutionOutcome::FileRevealRequested);
        assert_eq!(
            shell_target.into_inner(),
            Some((file.to_string_lossy().into_owned(), FilePathKind::File))
        );
    }

    #[test]
    fn real_directory_execution_uses_shell_compatible_path_from_revalidated_handle() {
        let tree = TempTree::new();
        let directory = tree.child("Documents");
        fs::create_dir(&directory).unwrap();
        let snapshot = authenticate_test_path(&directory, FilePathKind::Directory);
        let mut identity = snapshot.identity.clone();
        identity.display_path = tree.child("decoy").to_string_lossy().into_owned();
        let shell_target = RefCell::new(None);

        let outcome = execute_real_path_with_shell(&identity, |path, kind| {
            shell_target.replace(Some((path.to_owned(), kind)));
            Ok(FileExecutionOutcome::FolderOpenRequested)
        })
        .unwrap();

        assert_eq!(snapshot.size_bytes, None);
        assert_eq!(outcome, FileExecutionOutcome::FolderOpenRequested);
        assert_eq!(
            shell_target.into_inner(),
            Some((
                directory.to_string_lossy().into_owned(),
                FilePathKind::Directory,
            ))
        );
    }

    #[test]
    fn shell_dispatch_initializes_sta_com_on_worker_thread() {
        let outcome = std::thread::spawn(|| {
            execute_shell_with("unused", FilePathKind::File, |_, kind| {
                let nested = unsafe {
                    windows::Win32::System::Com::CoInitializeEx(
                        None,
                        windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                    )
                };
                assert_eq!(nested.0, 1);
                unsafe { windows::Win32::System::Com::CoUninitialize() };
                assert_eq!(kind, FilePathKind::File);
                Ok(FileExecutionOutcome::FileRevealRequested)
            })
        })
        .join()
        .unwrap()
        .unwrap();

        assert_eq!(outcome, FileExecutionOutcome::FileRevealRequested);
    }

    #[test]
    fn directory_shell_configuration_uses_shell_target_and_maps_failure() {
        let target = r"C:\authenticated\Documents\Report".to_owned();

        let result = open_directory_with(&target, |info: &mut SHELLEXECUTEINFOW| {
            assert_eq!(
                info.cbSize,
                u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap()
            );
            assert!(info.lpVerb.is_null());
            assert!(info.lpParameters.is_null());
            assert!(info.lpDirectory.is_null());
            assert_eq!(info.fMask, SEE_MASK_FLAG_NO_UI);
            assert_eq!(info.nShow, SW_SHOWNORMAL.0);
            assert_eq!(unsafe { info.lpFile.to_string() }.unwrap(), target);
            Err(windows::core::Error::from_hresult(
                windows::core::HRESULT::from_win32(5),
            ))
        });

        assert_eq!(result, Err(FileExecutionError::OpenFailed));
    }

    #[test]
    fn real_leaf_replacement_is_stale_before_injected_shell_dispatch() {
        let tree = TempTree::new();
        let file = tree.child("Report.txt");
        let replacement = tree.child("replacement.tmp");
        fs::write(&file, b"original").unwrap();
        fs::write(&replacement, b"replacement").unwrap();
        let snapshot = authenticate_test_path(&file, FilePathKind::File);
        fs::remove_file(&file).unwrap();
        fs::rename(&replacement, &file).unwrap();
        let shell_calls = Cell::new(0);

        let result = execute_real_path_with_shell(&snapshot.identity, |_, _| {
            shell_calls.set(shell_calls.get() + 1);
            Ok(FileExecutionOutcome::FileRevealRequested)
        });

        assert_eq!(result, Err(FileExecutionError::Stale));
        assert_eq!(shell_calls.get(), 0);
    }

    #[derive(Clone, Copy)]
    enum ExecutionMutation {
        LeafReplacement,
        ParentRename,
        ReparseInsertion,
        KindSubstitution,
        FilesystemSubstitution,
    }

    fn execute_mutated_path(
        kind: FilePathKind,
        mutation: ExecutionMutation,
    ) -> (Result<FileExecutionOutcome, FileExecutionError>, usize) {
        let expected = test_identity(r"docs\target", kind);
        let shell_calls = Cell::new(0);
        let result = execute_authenticated_path_with(
            &expected,
            Some("NTFS"),
            |relative, _, _| Ok(relative.to_owned()),
            |relative, expected_kind, _| {
                let mut observation = test_component_observation(relative, expected_kind);
                match mutation {
                    ExecutionMutation::LeafReplacement if relative == expected.relative_path => {
                        observation.file_id = [8; 16];
                    }
                    ExecutionMutation::ParentRename if relative == "docs" => {
                        observation.relative_path = "renamed".into();
                    }
                    ExecutionMutation::ReparseInsertion if relative == "docs" => {
                        observation.reparse = true;
                    }
                    ExecutionMutation::KindSubstitution if relative == expected.relative_path => {
                        observation.kind = match expected_kind {
                            FilePathKind::File => FilePathKind::Directory,
                            FilePathKind::Directory => FilePathKind::File,
                        };
                    }
                    ExecutionMutation::FilesystemSubstitution if relative == "docs" => {
                        observation.filesystem_name = "REFS".into();
                    }
                    _ => {}
                }
                Ok(observation)
            },
            |_, shell_kind| {
                shell_calls.set(shell_calls.get() + 1);
                Ok(match shell_kind {
                    FilePathKind::File => FileExecutionOutcome::FileRevealRequested,
                    FilePathKind::Directory => FileExecutionOutcome::FolderOpenRequested,
                })
            },
        );
        (result, shell_calls.get())
    }

    fn assert_stale_before_shell(kind: FilePathKind, mutation: ExecutionMutation) {
        let (result, shell_calls) = execute_mutated_path(kind, mutation);
        assert_eq!(result, Err(FileExecutionError::Stale));
        assert_eq!(shell_calls, 0);
    }

    #[test]
    fn leaf_replacement_is_stale_before_shell() {
        assert_stale_before_shell(FilePathKind::File, ExecutionMutation::LeafReplacement);
    }

    #[test]
    fn parent_rename_is_stale_before_shell() {
        assert_stale_before_shell(FilePathKind::File, ExecutionMutation::ParentRename);
    }

    #[test]
    fn case_only_parent_rename_is_stale_before_shell() {
        let tree = TempTree::new();
        let original_parent = tree.child("CaseParent");
        fs::create_dir(&original_parent).unwrap();
        let file = original_parent.join("report.txt");
        fs::write(&file, b"report").unwrap();
        let snapshot = authenticate_test_path(&file, FilePathKind::File);
        fs::rename(&original_parent, tree.child("caseparent")).unwrap();
        let shell_calls = Cell::new(0);

        let result = execute_real_path_with_shell(&snapshot.identity, |_, _| {
            shell_calls.set(shell_calls.get() + 1);
            Ok(FileExecutionOutcome::FileRevealRequested)
        });

        assert_eq!(result, Err(FileExecutionError::Stale));
        assert_eq!(shell_calls.get(), 0);
    }

    #[test]
    fn case_only_leaf_rename_is_stale_before_shell() {
        let tree = TempTree::new();
        let original_file = tree.child("Report.txt");
        fs::write(&original_file, b"report").unwrap();
        let snapshot = authenticate_test_path(&original_file, FilePathKind::File);
        fs::rename(&original_file, tree.child("report.txt")).unwrap();
        let shell_calls = Cell::new(0);

        let result = execute_real_path_with_shell(&snapshot.identity, |_, _| {
            shell_calls.set(shell_calls.get() + 1);
            Ok(FileExecutionOutcome::FileRevealRequested)
        });

        assert_eq!(result, Err(FileExecutionError::Stale));
        assert_eq!(shell_calls.get(), 0);
    }

    #[test]
    fn junction_or_reparse_insertion_is_stale_before_shell() {
        assert_stale_before_shell(FilePathKind::File, ExecutionMutation::ReparseInsertion);
    }

    #[test]
    fn file_or_folder_substitution_is_stale_before_shell() {
        for kind in [FilePathKind::File, FilePathKind::Directory] {
            assert_stale_before_shell(kind, ExecutionMutation::KindSubstitution);
        }
    }

    #[test]
    fn legacy_filesystem_substitution_is_stale_before_shell() {
        assert_stale_before_shell(
            FilePathKind::File,
            ExecutionMutation::FilesystemSubstitution,
        );
    }

    #[test]
    fn execution_uses_final_observation_shell_path_instead_of_display_path() {
        let mut expected = test_identity(r"docs\report.pdf", FilePathKind::File);
        expected.display_path = r"C:\decoy\report.pdf".into();
        let shell_target = RefCell::new(None);
        let outcome = execute_authenticated_path_with(
            &expected,
            None,
            |relative, _, _| Ok(relative.to_owned()),
            |relative, expected_kind, _| Ok(test_component_observation(relative, expected_kind)),
            |path, kind| {
                shell_target.replace(Some((path.to_owned(), kind)));
                Ok(FileExecutionOutcome::FileRevealRequested)
            },
        )
        .unwrap();

        assert_eq!(outcome, FileExecutionOutcome::FileRevealRequested);
        assert_eq!(
            shell_target.into_inner(),
            Some((
                r"C:\authenticated\docs\report.pdf".into(),
                FilePathKind::File,
            ))
        );
    }

    #[test]
    fn authenticated_identity_distinguishes_hard_link_paths() {
        let first = test_identity(r"links\first.txt", FilePathKind::File);
        let second = AuthenticatedPathIdentity {
            display_path: r"C:\links\second.txt".into(),
            relative_path: r"links\second.txt".into(),
            ..first.clone()
        };
        assert_eq!(first.file_id, second.file_id);
        assert_ne!(first, second);
    }

    struct TestHandle(Rc<Cell<usize>>);

    impl Drop for TestHandle {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    fn test_handles(drops: Rc<Cell<usize>>) -> Vec<TestHandle> {
        vec![TestHandle(Rc::clone(&drops)), TestHandle(drops)]
    }

    #[test]
    fn component_handles_live_until_shell_callback_returns() {
        let drops = Rc::new(Cell::new(0));
        let outcome = execute_with_components(test_handles(Rc::clone(&drops)), || {
            assert_eq!(drops.get(), 0);
            Ok(FileExecutionOutcome::FileRevealRequested)
        })
        .unwrap();

        assert_eq!(outcome, FileExecutionOutcome::FileRevealRequested);
        assert_eq!(drops.get(), 2);
    }
}
