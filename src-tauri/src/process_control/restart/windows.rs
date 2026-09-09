use std::{
    ffi::{c_void, OsStr, OsString},
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    os::windows::io::{AsHandle, BorrowedHandle, RawHandle},
    path::Path,
};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, GENERIC_READ, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FileAttributeTagInfo, GetDriveTypeW, GetFileInformationByHandle,
            GetFileInformationByHandleEx, GetFinalPathNameByHandleW, GetVolumeInformationW,
            GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, BY_HANDLE_FILE_INFORMATION,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, OPEN_EXISTING, VOLUME_NAME_GUID,
        },
        System::Threading::{
            CreateProcessW, GetProcessTimes, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
            STARTUPINFOW,
        },
    },
};

use super::{
    CreatedProcessIdentity, LeasedPathMetadata, NativeLaunchInput, ProcessRestartAdapter,
    RestartAdapterError, RestartPathInput,
};
use crate::process_control::windows::WindowsProcessAdapter;

const PATH_CAPACITY: usize = 32_768;

struct OwnedLaunchHandle(HANDLE);

impl Drop for OwnedLaunchHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    volume_serial: u32,
    file_index: u64,
}

struct LeasedComponent {
    handle: OwnedLaunchHandle,
    expected_path: String,
    expected_identity: FileIdentity,
    directory: bool,
}

pub(crate) struct WindowsPathLeases {
    executable_path: Vec<u16>,
    working_directory_path: Vec<u16>,
    components: Vec<LeasedComponent>,
    metadata: LeasedPathMetadata,
}

pub(crate) struct WindowsCreatedProcess {
    process: OwnedLaunchHandle,
    _thread: OwnedLaunchHandle,
    pid: u32,
}

impl AsHandle for WindowsCreatedProcess {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        // The borrow cannot outlive this owner, which closes the process handle.
        unsafe { BorrowedHandle::borrow_raw(self.process.0 .0 as RawHandle) }
    }
}

struct ResolvedPath {
    guid_root: String,
    relative_components: Vec<String>,
    full: String,
}

impl ProcessRestartAdapter for WindowsProcessAdapter {
    type PathLeases = WindowsPathLeases;
    type CreatedProcess = WindowsCreatedProcess;

    fn acquire_path_leases(
        &self,
        input: &RestartPathInput,
    ) -> Result<Self::PathLeases, RestartAdapterError> {
        let executable = resolve_supported_path(input.executable())?;
        let working_directory = resolve_supported_path(input.working_directory())?;
        let mut components = Vec::new();
        let executable_identity = lease_resolved_path(&executable, false, &mut components)?;
        lease_resolved_path(&working_directory, true, &mut components)?;
        let leases = WindowsPathLeases {
            executable_path: nul_terminated(&executable.full),
            working_directory_path: nul_terminated(&working_directory.full),
            components,
            metadata: LeasedPathMetadata::new(
                executable.full.clone().into(),
                working_directory.full.clone().into(),
                executable_identity.volume_serial,
                executable_identity.file_index,
            ),
        };
        revalidate_components(&leases.components)?;
        Ok(leases)
    }

    fn revalidate_path_leases(&self, leases: &Self::PathLeases) -> Result<(), RestartAdapterError> {
        revalidate_components(&leases.components)
    }

    fn path_lease_metadata<'a>(&self, leases: &'a Self::PathLeases) -> &'a LeasedPathMetadata {
        &leases.metadata
    }

    fn create_process(
        &self,
        leases: &Self::PathLeases,
        input: &NativeLaunchInput,
    ) -> Result<Self::CreatedProcess, RestartAdapterError> {
        validate_native_input(input)?;
        revalidate_components(&leases.components)?;
        let mut command_line = input.command_line().to_vec();
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut information = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                PCWSTR(leases.executable_path.as_ptr()),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_UNICODE_ENVIRONMENT,
                Some(input.environment().as_ptr().cast()),
                PCWSTR(leases.working_directory_path.as_ptr()),
                &startup,
                &mut information,
            )
        }
        .map_err(|_| RestartAdapterError::CreationFailed)?;
        Ok(WindowsCreatedProcess {
            process: OwnedLaunchHandle(information.hProcess),
            _thread: OwnedLaunchHandle(information.hThread),
            pid: information.dwProcessId,
        })
    }

    fn created_process_identity(
        &self,
        process: &Self::CreatedProcess,
    ) -> Result<CreatedProcessIdentity, RestartAdapterError> {
        let mut creation = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        unsafe {
            GetProcessTimes(
                process.process.0,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        }
        .map_err(|_| RestartAdapterError::IdentityUnavailable)?;
        let creation_time =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        created_process_identity_with(process.pid, || Ok(creation_time))
    }
}

pub(crate) fn created_process_identity_with(
    pid: u32,
    query_creation_time: impl FnOnce() -> Result<u64, ()>,
) -> Result<CreatedProcessIdentity, RestartAdapterError> {
    let creation_time =
        query_creation_time().map_err(|()| RestartAdapterError::IdentityUnavailable)?;
    CreatedProcessIdentity::new(pid, creation_time)
}

fn validate_native_input(input: &NativeLaunchInput) -> Result<(), RestartAdapterError> {
    let command = input.command_line();
    if command.len() < 2 || command.last() != Some(&0) || command[..command.len() - 1].contains(&0)
    {
        return Err(RestartAdapterError::InvalidInput);
    }
    let environment = input.environment();
    if environment.len() < 2
        || environment[environment.len() - 2..] != [0, 0]
        || environment[..environment.len() - 2]
            .windows(2)
            .any(|pair| pair == [0, 0])
    {
        return Err(RestartAdapterError::InvalidInput);
    }
    Ok(())
}

fn resolve_supported_path(path: &Path) -> Result<ResolvedPath, RestartAdapterError> {
    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(RestartAdapterError::UnsupportedPath);
    }
    let display = String::from_utf16(&units)
        .map_err(|_| RestartAdapterError::UnsupportedPath)?
        .replace('/', "\\");
    let (guid_root, relative) = if display.starts_with("\\\\?\\Volume{") {
        let close = display
            .find("}\\")
            .ok_or(RestartAdapterError::UnsupportedPath)?;
        let guid = &display[11..close];
        if !valid_guid(guid) {
            return Err(RestartAdapterError::UnsupportedPath);
        }
        let root_end = close + 2;
        (
            display[..root_end].to_owned(),
            display[root_end..].trim_matches('\\').to_owned(),
        )
    } else {
        if !path.is_absolute()
            || display.starts_with("\\\\")
            || display.get(2..).is_none_or(|tail| tail.contains(':'))
            || display.as_bytes().get(1).is_none_or(|unit| *unit != b':')
        {
            return Err(RestartAdapterError::UnsupportedPath);
        }
        let display_wide = nul_terminated(&display);
        let mut mount = vec![0u16; PATH_CAPACITY];
        unsafe { GetVolumePathNameW(PCWSTR(display_wide.as_ptr()), &mut mount) }
            .map_err(|_| RestartAdapterError::PathUnavailable)?;
        let mount = from_nul(&mount)?;
        let mut guid = vec![0u16; 128];
        unsafe {
            GetVolumeNameForVolumeMountPointW(PCWSTR(nul_terminated(&mount).as_ptr()), &mut guid)
        }
        .map_err(|_| RestartAdapterError::UnsupportedPath)?;
        let guid_root = from_nul(&guid)?;
        let relative = display
            .get(mount.len()..)
            .ok_or(RestartAdapterError::UnsupportedPath)?
            .trim_matches('\\')
            .to_owned();
        (guid_root, relative)
    };
    // GetDriveTypeW's documented DRIVE_FIXED value is 3.
    if unsafe { GetDriveTypeW(PCWSTR(nul_terminated(&guid_root).as_ptr())) } != 3 {
        return Err(RestartAdapterError::UnsupportedPath);
    }
    let mut filesystem = vec![0u16; 64];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(nul_terminated(&guid_root).as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut filesystem),
        )
    }
    .map_err(|_| RestartAdapterError::UnsupportedPath)?;
    if !filesystem_is_supported(&from_nul(&filesystem)?) {
        return Err(RestartAdapterError::UnsupportedPath);
    }
    let relative_components = relative
        .split('\\')
        .filter(|component| !component.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if relative_components.is_empty()
        || relative_components
            .iter()
            .any(|component| component == "." || component == ".." || component.contains(':'))
    {
        return Err(RestartAdapterError::UnsupportedPath);
    }
    let full = format!("{}{}", guid_root, relative_components.join("\\"));
    Ok(ResolvedPath {
        guid_root,
        relative_components,
        full,
    })
}

fn valid_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn filesystem_is_supported(name: &str) -> bool {
    name.eq_ignore_ascii_case("NTFS")
}

fn lease_resolved_path(
    path: &ResolvedPath,
    final_directory: bool,
    leases: &mut Vec<LeasedComponent>,
) -> Result<FileIdentity, RestartAdapterError> {
    let mut final_identity = None;
    let mut current = path.guid_root.trim_end_matches('\\').to_owned();
    for (index, component) in path.relative_components.iter().enumerate() {
        current.push('\\');
        current.push_str(component);
        let last = index + 1 == path.relative_components.len();
        let component = open_component(&current, !last || final_directory)?;
        if last {
            final_identity = Some(component.expected_identity);
        }
        leases.push(component);
    }
    final_identity.ok_or(RestartAdapterError::UnsupportedPath)
}

fn open_component(path: &str, directory: bool) -> Result<LeasedComponent, RestartAdapterError> {
    let path_wide = nul_terminated(path);
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    let access = if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
        FILE_LIST_DIRECTORY.0 | FILE_READ_ATTRIBUTES.0
    } else {
        GENERIC_READ.0
    };
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path_wide.as_ptr()),
            access,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .map(OwnedLaunchHandle)
    .map_err(|_| RestartAdapterError::PathUnavailable)?;
    validate_kind_and_reparse(handle.0, directory)?;
    let expected_identity = file_identity(handle.0)?;
    let observed = final_guid_path(handle.0)?;
    if !observed.eq_ignore_ascii_case(path) {
        return Err(RestartAdapterError::PathChanged);
    }
    Ok(LeasedComponent {
        handle,
        expected_path: path.to_owned(),
        expected_identity,
        directory,
    })
}

fn revalidate_components(components: &[LeasedComponent]) -> Result<(), RestartAdapterError> {
    for component in components {
        validate_kind_and_reparse(component.handle.0, component.directory)?;
        if file_identity(component.handle.0)? != component.expected_identity
            || !final_guid_path(component.handle.0)?.eq_ignore_ascii_case(&component.expected_path)
        {
            return Err(RestartAdapterError::PathChanged);
        }
    }
    Ok(())
}

fn validate_kind_and_reparse(handle: HANDLE, directory: bool) -> Result<(), RestartAdapterError> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast::<c_void>(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    }
    .map_err(|_| RestartAdapterError::PathUnavailable)?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(RestartAdapterError::UnsupportedPath);
    }
    if (tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory {
        return Err(RestartAdapterError::PathChanged);
    }
    Ok(())
}

fn file_identity(handle: HANDLE) -> Result<FileIdentity, RestartAdapterError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut information) }
        .map_err(|_| RestartAdapterError::PathUnavailable)?;
    Ok(FileIdentity {
        volume_serial: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

fn final_guid_path(handle: HANDLE) -> Result<String, RestartAdapterError> {
    let mut buffer = vec![0u16; PATH_CAPACITY];
    let written = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_GUID) };
    let written = usize::try_from(written).map_err(|_| RestartAdapterError::PathUnavailable)?;
    if written == 0 || written >= buffer.len() {
        return Err(RestartAdapterError::PathUnavailable);
    }
    String::from_utf16(&buffer[..written]).map_err(|_| RestartAdapterError::PathUnavailable)
}

fn nul_terminated(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn from_nul(value: &[u16]) -> Result<String, RestartAdapterError> {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(RestartAdapterError::PathUnavailable)?;
    OsString::from_wide(&value[..end])
        .into_string()
        .map_err(|_| RestartAdapterError::UnsupportedPath)
}
