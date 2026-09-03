use std::{
    ffi::{OsStr, OsString},
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
};

use windows::{
    core::{BOOL, HRESULT, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NOT_FOUND,
            ERROR_NO_MORE_FILES, FILETIME, HANDLE, WAIT_EVENT, WAIT_FAILED, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        Globalization::{CompareStringOrdinal, CSTR_EQUAL},
        Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                GetCurrentProcessId, GetProcessInformation, GetProcessTimes, IsProcessCritical,
                OpenProcess, ProcessProtectionLevelInfo, QueryFullProcessImageNameW,
                WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
                PROCESS_PROTECTION_LEVEL_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, PROTECTION_LEVEL_NONE,
            },
        },
    },
};

use super::core::{
    CoreAuthority, EnumerationFailure, HostInstanceId, Liveness, ObservationAccess,
    ObservationFailure, ProcessAuthorityInitError, ProcessCandidate, ProcessObservation,
    ProcessPlatformAdapter, ProcessProtection, ProcessTarget,
};

const IMAGE_PATH_CAPACITY: usize = 32_768;

pub(super) struct OwnedProcessHandle(OwnedHandle);

struct OwnedSnapshotHandle(OwnedHandle);

pub(super) trait HandleCloser {
    fn close(&mut self, handle: HANDLE);
}

impl<F> HandleCloser for F
where
    F: FnMut(HANDLE),
{
    fn close(&mut self, handle: HANDLE) {
        self(handle);
    }
}

pub(super) struct NativeHandleCloser;

impl HandleCloser for NativeHandleCloser {
    fn close(&mut self, handle: HANDLE) {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}

pub(super) struct OwnedHandle<C: HandleCloser = NativeHandleCloser> {
    handle: HANDLE,
    closer: C,
}

impl OwnedHandle<NativeHandleCloser> {
    fn new(handle: HANDLE) -> Self {
        Self::with_closer(handle, NativeHandleCloser)
    }
}

impl<C: HandleCloser> OwnedHandle<C> {
    pub(super) fn with_closer(handle: HANDLE, closer: C) -> Self {
        Self { handle, closer }
    }

    fn get(&self) -> HANDLE {
        self.handle
    }
}

impl<C: HandleCloser> Drop for OwnedHandle<C> {
    fn drop(&mut self) {
        self.closer.close(self.handle);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WindowsProcessAdapter;

impl ProcessPlatformAdapter for WindowsProcessAdapter {
    type Handle = OwnedProcessHandle;

    fn enumerate_candidates(
        &self,
    ) -> Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure> {
        Ok(enumerate_native_candidates()?
            .into_iter()
            .map(|candidate| candidate.map(NativeCandidate::into_core))
            .collect())
    }

    fn open_observation(
        &self,
        pid: u32,
        access: ObservationAccess,
    ) -> Result<Self::Handle, ObservationFailure> {
        open_observation_with(pid, access, |access, inherit, pid| {
            debug_assert!(!inherit);
            unsafe { OpenProcess(access, false, pid) }
                .map(|handle| OwnedProcessHandle(OwnedHandle::new(handle)))
                .map_err(classify_observation_error)
        })
    }

    fn creation_time(&self, handle: &Self::Handle) -> Result<u64, ObservationFailure> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetProcessTimes(
                handle.0.get(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        }
        .map_err(classify_observation_error)?;
        Ok(filetime_to_u64(creation))
    }

    fn liveness(&self, handle: &Self::Handle) -> Result<Liveness, ObservationFailure> {
        liveness_from_wait_result(unsafe { WaitForSingleObject(handle.0.get(), 0) })
    }

    fn observation(&self, handle: &Self::Handle) -> Result<ProcessObservation, ObservationFailure> {
        let mut path = vec![0u16; IMAGE_PATH_CAPACITY];
        let mut length =
            u32::try_from(path.len()).map_err(|_| ObservationFailure::IdentityUnavailable)?;
        unsafe {
            QueryFullProcessImageNameW(
                handle.0.get(),
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )
        }
        .map_err(classify_path_query_error)?;
        let length =
            usize::try_from(length).map_err(|_| ObservationFailure::IdentityUnavailable)?;
        if length == 0 || length > path.len() {
            return Err(ObservationFailure::IdentityUnavailable);
        }
        path.truncate(length);
        let executable_path = PathBuf::from(OsString::from_wide(&path));
        let name = executable_path
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|name| !name.is_empty())
            .ok_or(ObservationFailure::IdentityUnavailable)?
            .to_owned();
        Ok(ProcessObservation::new(name, executable_path))
    }

    fn reenumerate_name(&self, pid: u32) -> Result<Option<String>, ObservationFailure> {
        let candidates =
            enumerate_native_candidates().map_err(|_| ObservationFailure::IdentityUnavailable)?;
        find_reenumerated_name(
            candidates
                .into_iter()
                .map(|candidate| candidate.map(|candidate| (candidate.pid, candidate.name)))
                .collect(),
            pid,
        )
    }

    fn process_names_equivalent(
        &self,
        left: &OsStr,
        right: &OsStr,
    ) -> Result<bool, ObservationFailure> {
        let left = left.encode_wide().collect::<Vec<_>>();
        let right = right.encode_wide().collect::<Vec<_>>();
        if left.len() > i32::MAX as usize || right.len() > i32::MAX as usize {
            return Err(ObservationFailure::IdentityUnavailable);
        }
        let comparison = unsafe { CompareStringOrdinal(&left, &right, true) };
        if comparison.0 == 0 {
            return Err(ObservationFailure::IdentityUnavailable);
        }
        Ok(comparison == CSTR_EQUAL)
    }

    fn protection(&self, handle: &Self::Handle) -> Result<ProcessProtection, ObservationFailure> {
        let mut critical = BOOL::default();
        unsafe { IsProcessCritical(handle.0.get(), &mut critical) }
            .map_err(classify_observation_error)?;

        let mut protection = PROCESS_PROTECTION_LEVEL_INFORMATION::default();
        unsafe {
            GetProcessInformation(
                handle.0.get(),
                ProcessProtectionLevelInfo,
                (&mut protection as *mut PROCESS_PROTECTION_LEVEL_INFORMATION).cast(),
                size_of::<PROCESS_PROTECTION_LEVEL_INFORMATION>() as u32,
            )
        }
        .map_err(classify_observation_error)?;
        Ok(ProcessProtection::new(
            critical.as_bool(),
            protection.ProtectionLevel != PROTECTION_LEVEL_NONE,
        ))
    }
}

pub(super) fn create_authority(
) -> Result<CoreAuthority<WindowsProcessAdapter>, ProcessAuthorityInitError> {
    let host_instance_id = generate_host_instance()?;
    let adapter = WindowsProcessAdapter;
    let pid = unsafe { GetCurrentProcessId() };
    let handle = adapter
        .open_observation(pid, ObservationAccess::SNAPSHOT)
        .map_err(|_| ProcessAuthorityInitError::Unavailable)?;
    let creation_time = adapter
        .creation_time(&handle)
        .map_err(|_| ProcessAuthorityInitError::Unavailable)?;
    let self_target = ProcessTarget::new(host_instance_id, pid, creation_time)
        .map_err(|_| ProcessAuthorityInitError::Unavailable)?;
    Ok(CoreAuthority::new(host_instance_id, self_target, adapter))
}

fn generate_host_instance() -> Result<HostInstanceId, ProcessAuthorityInitError> {
    generate_host_instance_with(|bytes| {
        let status = unsafe { BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if status.0 == 0 {
            Ok(())
        } else {
            Err(())
        }
    })
}

pub(super) fn generate_host_instance_with(
    fill: impl FnOnce(&mut [u8]) -> Result<(), ()>,
) -> Result<HostInstanceId, ProcessAuthorityInitError> {
    let mut value = [0; 16];
    fill(&mut value).map_err(|_| ProcessAuthorityInitError::Unavailable)?;
    HostInstanceId::new(value)
}

#[derive(Debug)]
pub(super) enum EnumerationStep<T> {
    Entry(T),
    End,
    Failed,
}

pub(super) fn collect_candidates_with<T>(
    mut next: impl FnMut() -> EnumerationStep<T>,
) -> Vec<Result<T, EnumerationFailure>> {
    let mut candidates = Vec::new();
    loop {
        match next() {
            EnumerationStep::Entry(candidate) => candidates.push(Ok(candidate)),
            EnumerationStep::End => return candidates,
            EnumerationStep::Failed => {
                candidates.push(Err(EnumerationFailure::Unavailable));
                return candidates;
            }
        }
    }
}

pub(super) fn open_observation_with<H>(
    pid: u32,
    access: ObservationAccess,
    open: impl FnOnce(PROCESS_ACCESS_RIGHTS, bool, u32) -> Result<H, ObservationFailure>,
) -> Result<H, ObservationFailure> {
    let mut desired_access = PROCESS_ACCESS_RIGHTS(0);
    if access.query_limited_information() {
        desired_access |= PROCESS_QUERY_LIMITED_INFORMATION;
    }
    if access.synchronize() {
        desired_access |= PROCESS_SYNCHRONIZE;
    }
    open(desired_access, false, pid)
}

pub(super) fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

pub(super) fn find_reenumerated_name(
    candidates: Vec<Result<(u32, String), EnumerationFailure>>,
    pid: u32,
) -> Result<Option<String>, ObservationFailure> {
    let candidates = candidates
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ObservationFailure::IdentityUnavailable)?;
    Ok(candidates
        .into_iter()
        .find_map(|(candidate_pid, name)| (candidate_pid == pid).then_some(name)))
}

pub(super) fn liveness_from_wait_result(
    result: WAIT_EVENT,
) -> Result<Liveness, ObservationFailure> {
    match result {
        WAIT_TIMEOUT => Ok(Liveness::Alive),
        WAIT_OBJECT_0 => Ok(Liveness::Exited),
        WAIT_FAILED => Err(ObservationFailure::AccessDenied),
        _ => Err(ObservationFailure::AccessDenied),
    }
}

struct NativeCandidate {
    pid: u32,
    name: String,
    observed_parent_pid: u32,
}

impl NativeCandidate {
    fn into_core(self) -> ProcessCandidate {
        ProcessCandidate::new(self.pid, self.name, Some(self.observed_parent_pid))
    }
}

fn enumerate_native_candidates(
) -> Result<Vec<Result<NativeCandidate, EnumerationFailure>>, EnumerationFailure> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(|handle| OwnedSnapshotHandle(OwnedHandle::new(handle)))
        .map_err(|_| EnumerationFailure::Unavailable)?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut first = true;

    Ok(collect_candidates_with(|| {
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let result = unsafe {
            if first {
                first = false;
                Process32FirstW(snapshot.0.get(), &mut entry)
            } else {
                Process32NextW(snapshot.0.get(), &mut entry)
            }
        };
        match result {
            Ok(()) => EnumerationStep::Entry(native_candidate_from_entry(&entry)),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => {
                EnumerationStep::End
            }
            Err(_) => EnumerationStep::Failed,
        }
    }))
}

fn native_candidate_from_entry(entry: &PROCESSENTRY32W) -> NativeCandidate {
    let name_units = nul_terminated_slice(&entry.szExeFile);
    let name = String::from_utf16(name_units).unwrap_or_default();
    NativeCandidate {
        pid: entry.th32ProcessID,
        name,
        observed_parent_pid: entry.th32ParentProcessID,
    }
}

fn nul_terminated_slice(value: &[u16]) -> &[u16] {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    &value[..length]
}

pub(super) fn classify_path_query_error(error: windows::core::Error) -> ObservationFailure {
    match classify_observation_error(error) {
        ObservationFailure::ExitedOrChanged => ObservationFailure::ExitedOrChanged,
        ObservationFailure::AccessDenied | ObservationFailure::IdentityUnavailable => {
            ObservationFailure::IdentityUnavailable
        }
    }
}

pub(super) fn classify_observation_error(error: windows::core::Error) -> ObservationFailure {
    let code = error.code();
    if code == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        ObservationFailure::AccessDenied
    } else if code == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0)
        || code == HRESULT::from_win32(ERROR_NOT_FOUND.0)
    {
        ObservationFailure::ExitedOrChanged
    } else {
        ObservationFailure::IdentityUnavailable
    }
}
