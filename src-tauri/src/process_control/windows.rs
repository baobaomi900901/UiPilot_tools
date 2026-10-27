use std::{
    ffi::{OsStr, OsString},
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    time::Duration,
};

use windows::{
    core::{BOOL, HRESULT, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, SetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER,
            ERROR_INVALID_WINDOW_HANDLE, ERROR_NOT_FOUND, ERROR_NO_MORE_FILES, FILETIME, HANDLE,
            HWND, LPARAM, NO_ERROR, WAIT_EVENT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
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
                TerminateProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
                PROCESS_PROTECTION_LEVEL_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, PROTECTION_LEVEL_NONE,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
            GW_OWNER, WM_CLOSE,
        },
    },
};

use super::core::{
    CoreAuthority, EnumerationFailure, HostInstanceId, Liveness, ObservationAccess,
    ObservationFailure, ProcessAuthorityInitError, ProcessCandidate, ProcessObservation,
    ProcessPlatformAdapter, ProcessProtection, ProcessTarget,
};
use super::operation::{
    ExitWait, OperationAccess, OperationFailure, ProcessOperationAdapter, WindowCandidate,
    WindowFacts, WindowFailure,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WindowsWindowToken(HWND);

impl WindowsWindowToken {
    pub(super) fn from_raw(window: HWND) -> Self {
        Self(window)
    }

    pub(super) fn raw(self) -> HWND {
        self.0
    }
}

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

impl ProcessOperationAdapter for WindowsProcessAdapter {
    type WindowToken = WindowsWindowToken;

    fn open_operation(
        &self,
        pid: u32,
        access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure> {
        open_operation_with(access, pid, |access, inherit, pid| {
            debug_assert!(!inherit);
            unsafe { OpenProcess(access, false, pid) }
                .map(|handle| OwnedProcessHandle(OwnedHandle::new(handle)))
                .map_err(classify_operation_error)
        })
    }

    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure> {
        collect_window_candidates_with(enumerate_native_top_level_windows()?, |window| {
            self.window_facts(window)
        })
    }

    fn window_facts(&self, window: Self::WindowToken) -> Result<WindowFacts, WindowFailure> {
        window_facts_with(window, read_native_window_facts)
    }

    fn post_close(&self, window: Self::WindowToken) -> Result<(), WindowFailure> {
        unsafe {
            PostMessageW(
                Some(window.raw()),
                WM_CLOSE,
                Default::default(),
                Default::default(),
            )
        }
        .map_err(classify_window_error)
    }

    fn terminate_process(
        &self,
        handle: &Self::Handle,
        exit_code: u32,
    ) -> Result<(), OperationFailure> {
        unsafe { TerminateProcess(handle.0.get(), exit_code) }.map_err(classify_operation_error)
    }

    fn wait_for_exit(
        &self,
        handle: &Self::Handle,
        timeout: Duration,
    ) -> Result<ExitWait, OperationFailure> {
        wait_result_with(
            timeout,
            |timeout| unsafe { WaitForSingleObject(handle.0.get(), timeout) },
            windows::core::Error::from_win32,
        )
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

pub(super) fn open_operation_with<H>(
    access: OperationAccess,
    pid: u32,
    open: impl FnOnce(PROCESS_ACCESS_RIGHTS, bool, u32) -> Result<H, OperationFailure>,
) -> Result<H, OperationFailure> {
    let mut desired_access = PROCESS_ACCESS_RIGHTS(0);
    if access.query_limited_information() {
        desired_access |= PROCESS_QUERY_LIMITED_INFORMATION;
    }
    if access.synchronize() {
        desired_access |= PROCESS_SYNCHRONIZE;
    }
    if access.terminate() {
        desired_access |= PROCESS_TERMINATE;
    }
    open(desired_access, false, pid)
}

pub(super) fn collect_window_candidates_with<W: Copy>(
    windows: Vec<W>,
    mut facts: impl FnMut(W) -> Result<WindowFacts, WindowFailure>,
) -> Result<Vec<WindowCandidate<W>>, OperationFailure> {
    let mut candidates = Vec::with_capacity(windows.len());
    for window in windows {
        match facts(window) {
            Ok(facts) => candidates.push(WindowCandidate::new(
                window,
                facts.pid(),
                facts.visible(),
                facts.has_owner(),
            )),
            Err(WindowFailure::Gone) => {}
            Err(WindowFailure::AccessDenied) => return Err(OperationFailure::AccessDenied),
            Err(WindowFailure::Unavailable) => return Err(OperationFailure::Unavailable),
        }
    }
    Ok(candidates)
}

pub(super) fn window_facts_with<W: Copy>(
    window: W,
    read: impl FnOnce(W) -> Result<(u32, bool, bool), WindowFailure>,
) -> Result<WindowFacts, WindowFailure> {
    read(window).map(|(pid, visible, has_owner)| WindowFacts::new(pid, visible, has_owner))
}

pub(super) fn finite_timeout_millis(timeout: Duration) -> u32 {
    if timeout.is_zero() {
        return 0;
    }
    let whole_millis = timeout.as_millis();
    let rounded_millis =
        whole_millis + u128::from(!timeout.subsec_nanos().is_multiple_of(1_000_000));
    rounded_millis.min(u128::from(u32::MAX - 1)) as u32
}

pub(super) fn wait_result_with(
    timeout: Duration,
    wait: impl FnOnce(u32) -> WAIT_EVENT,
    last_error: impl FnOnce() -> windows::core::Error,
) -> Result<ExitWait, OperationFailure> {
    match wait(finite_timeout_millis(timeout)) {
        WAIT_OBJECT_0 => Ok(ExitWait::Exited),
        WAIT_TIMEOUT => Ok(ExitWait::TimedOut),
        WAIT_FAILED => Err(classify_wait_error(last_error())),
        _ => Err(OperationFailure::Unavailable),
    }
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

fn enumerate_native_top_level_windows() -> Result<Vec<WindowsWindowToken>, OperationFailure> {
    let mut windows = Vec::new();
    unsafe {
        EnumWindows(
            Some(collect_native_top_level_window),
            LPARAM((&mut windows as *mut Vec<WindowsWindowToken>) as isize),
        )
    }
    .map_err(classify_enumeration_error)?;
    Ok(windows)
}

unsafe extern "system" fn collect_native_top_level_window(window: HWND, state: LPARAM) -> BOOL {
    let windows = unsafe { &mut *(state.0 as *mut Vec<WindowsWindowToken>) };
    windows.push(WindowsWindowToken::from_raw(window));
    BOOL(1)
}

fn read_native_window_facts(
    window: WindowsWindowToken,
) -> Result<(u32, bool, bool), WindowFailure> {
    unsafe { SetLastError(NO_ERROR) };
    let mut pid = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(window.raw(), Some(&mut pid)) };
    if thread_id == 0 || pid == 0 {
        return Err(classify_window_error(windows::core::Error::from_win32()));
    }

    let visible = unsafe { IsWindowVisible(window.raw()) }.as_bool();
    unsafe { SetLastError(NO_ERROR) };
    let has_owner = match unsafe { GetWindow(window.raw(), GW_OWNER) } {
        Ok(_) => true,
        Err(error) if error.code() == HRESULT::from_win32(NO_ERROR.0) => false,
        Err(error) => return Err(classify_window_error(error)),
    };
    Ok((pid, visible, has_owner))
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

pub(super) fn classify_operation_error(error: windows::core::Error) -> OperationFailure {
    let code = error.code();
    if code == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        OperationFailure::AccessDenied
    } else if code == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0)
        || code == HRESULT::from_win32(ERROR_NOT_FOUND.0)
    {
        OperationFailure::ExitedOrChanged
    } else {
        OperationFailure::Unavailable
    }
}

pub(super) fn classify_window_error(error: windows::core::Error) -> WindowFailure {
    let code = error.code();
    if code == HRESULT::from_win32(ERROR_INVALID_WINDOW_HANDLE.0) {
        WindowFailure::Gone
    } else if code == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        WindowFailure::AccessDenied
    } else {
        WindowFailure::Unavailable
    }
}

fn classify_enumeration_error(error: windows::core::Error) -> OperationFailure {
    if error.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        OperationFailure::AccessDenied
    } else {
        OperationFailure::Unavailable
    }
}

fn classify_wait_error(error: windows::core::Error) -> OperationFailure {
    if error.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        OperationFailure::AccessDenied
    } else {
        OperationFailure::Unavailable
    }
}
