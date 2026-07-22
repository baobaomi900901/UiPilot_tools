use std::{
    collections::{HashMap, VecDeque},
    ffi::c_void,
    path::{Path, PathBuf},
};

#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Globalization::CompareStringOrdinal,
        Storage::FileSystem::{
            CreateFileW, FileAttributeTagInfo, GetDriveTypeW, GetFileInformationByHandleEx,
            GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, GetVolumeInformationW,
            GetVolumeNameForVolumeMountPointW, FILE_ACTION, FILE_ACTION_ADDED,
            FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME,
            FILE_ACTION_RENAMED_OLD_NAME, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_TAG_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FULL_DIR_INFO,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, VOLUME_NAME_GUID,
        },
        System::Com::CoTaskMemFree,
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

#[cfg(not(test))]
use windows::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{
        FileBasicInfo, FileFullDirectoryInfo, FileStandardInfo, GetLogicalDriveStringsW,
        GetTempPathW, GetVolumePathNameW, ReadDirectoryChangesW, FILE_BASIC_INFO,
        FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES,
        FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE,
        FILE_STANDARD_INFO,
    },
    System::{
        SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW},
        Threading::{
            CreateEventW, GetCurrentThread, ResetEvent, SetThreadPriority, WaitForSingleObject,
            THREAD_PRIORITY_BELOW_NORMAL,
        },
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
};

use super::{
    fold_name, FileExecutionOutcome, IndexChangeBatch, IndexEntry, IndexedKind, OpenIndexedPath,
    VolumeIdentity,
};

pub(super) const EVENT_CAPACITY: usize = 65_536;
pub(super) const SCAN_BATCH_SIZE: usize = 512;
pub(super) const DRIVE_FIXED_VALUE: u32 = 3;
const ERROR_NO_MORE_FILES_CODE: u32 = 18;

struct DirectoryStack {
    priority: Vec<String>,
    ordinary: Vec<String>,
}

impl DirectoryStack {
    #[cfg(test)]
    fn root() -> Self {
        Self {
            priority: Vec::new(),
            ordinary: vec![String::new()],
        }
    }

    fn push(&mut self, relative_path: String) -> Result<(), BackendError> {
        if self.priority.len() + self.ordinary.len() == EVENT_CAPACITY {
            return Err(BackendError::Overflow);
        }
        if scan_priority(&relative_path) == 0 {
            self.ordinary.push(relative_path);
        } else {
            self.priority.push(relative_path);
        }
        Ok(())
    }

    fn pop(&mut self) -> Option<String> {
        self.priority.pop().or_else(|| self.ordinary.pop())
    }
}

fn scan_priority(relative_path: &str) -> u8 {
    if relative_path == "Users" || relative_path.starts_with(r"Users\") {
        1
    } else {
        0
    }
}

fn push_denied_prefix(
    denied_prefixes: &mut Vec<String>,
    relative_path: String,
) -> Result<(), BackendError> {
    if denied_prefixes.len() == EVENT_CAPACITY {
        return Err(BackendError::Overflow);
    }
    denied_prefixes.push(relative_path);
    Ok(())
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Clone, Copy)]
struct ExecutionShare;

impl ExecutionShare {
    #[cfg(test)]
    fn allows_write(self) -> bool {
        true
    }

    #[cfg(test)]
    fn allows_delete(self) -> bool {
        false
    }
}

fn pin_indexed_path_components_with<H, O, I>(
    identity: &VolumeIdentity,
    relative_path: &str,
    final_is_directory: bool,
    mut open: O,
    mut inspect: I,
) -> Result<Vec<H>, BackendError>
where
    O: FnMut(&str, bool, ExecutionShare) -> Result<H, BackendError>,
    I: FnMut(&H, &str, bool) -> Result<(bool, bool, VolumeIdentity, String), BackendError>,
{
    let components = relative_path.split('\\').collect::<Vec<_>>();
    if components.is_empty()
        || components.iter().any(|component| {
            component.is_empty()
                || *component == "."
                || *component == ".."
                || component.contains('/')
        })
    {
        return Err(BackendError::InvalidData);
    }
    let mut handles = Vec::with_capacity(components.len());
    let mut cumulative = String::new();
    for (index, component) in components.iter().enumerate() {
        if !cumulative.is_empty() {
            cumulative.push('\\');
        }
        cumulative.push_str(component);
        let expected_directory = index + 1 != components.len() || final_is_directory;
        let handle = open(&cumulative, expected_directory, ExecutionShare)?;
        let (reparse, directory, actual_identity, actual_relative_path) =
            inspect(&handle, &cumulative, expected_directory)?;
        if reparse
            || directory != expected_directory
            || actual_identity != *identity
            || actual_relative_path != cumulative
        {
            return Err(BackendError::InvalidData);
        }
        handles.push(handle);
    }
    Ok(handles)
}

struct DirectoryShellCall<'a> {
    path: &'a str,
}

impl DirectoryShellCall<'_> {
    fn path(&self) -> &str {
        self.path
    }

    #[cfg(test)]
    fn verb(&self) -> Option<&str> {
        None
    }

    #[cfg(test)]
    fn parameters(&self) -> Option<&str> {
        None
    }

    #[cfg(test)]
    fn directory(&self) -> Option<&str> {
        None
    }

    #[cfg(test)]
    fn no_ui(&self) -> bool {
        true
    }

    #[cfg(test)]
    fn show_normal(&self) -> bool {
        true
    }
}

fn directory_shell_execute_ex_with(
    path: &str,
    execute: impl FnOnce(&DirectoryShellCall<'_>) -> bool,
) -> Result<(), BackendError> {
    execute(&DirectoryShellCall { path })
        .then_some(())
        .ok_or(BackendError::Platform)
}

fn native_root(volume: &FixedVolume) -> PathBuf {
    PathBuf::from(&volume.identity.volume_guid_path)
}

fn native_path(volume: &FixedVolume, relative_path: &str) -> PathBuf {
    if relative_path.is_empty() {
        native_root(volume)
    } else {
        native_root(volume).join(relative_path)
    }
}

#[cfg(not(test))]
fn display_path(volume: &FixedVolume, relative_path: &str) -> Result<String, BackendError> {
    volume
        .mount_point
        .join(relative_path)
        .to_str()
        .map(str::to_owned)
        .ok_or(BackendError::InvalidData)
}

#[cfg(not(test))]
fn open_pinned(
    volume: &FixedVolume,
    relative_path: &str,
    expected_directory: Option<bool>,
) -> Result<(OwnedHandle, FILE_ATTRIBUTE_TAG_INFO), BackendError> {
    open_pinned_with_policy(
        volume,
        relative_path,
        expected_directory,
        PinnedPathPolicy::Strict,
    )
}

#[cfg(not(test))]
fn open_pinned_with_policy(
    volume: &FixedVolume,
    relative_path: &str,
    expected_directory: Option<bool>,
    policy: PinnedPathPolicy,
) -> Result<(OwnedHandle, FILE_ATTRIBUTE_TAG_INFO), BackendError> {
    let path = native_path(volume, relative_path);
    let wide = to_wide(path.to_str().ok_or(BackendError::InvalidData)?)?;
    let desired = if expected_directory == Some(true) {
        FILE_LIST_DIRECTORY.0
    } else {
        0
    };
    let handle = OwnedHandle(
        unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                desired,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(|error| match classify_open_failure(error.code()) {
            OpenFailure::Missing => BackendError::Missing,
            OpenFailure::Denied => BackendError::Denied,
            OpenFailure::Failed => BackendError::Platform,
        })?,
    );
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>())
                .map_err(|_| BackendError::Overflow)?,
        )
    }
    .map_err(|_| BackendError::Platform)?;
    validate_pinned_shape(tag.FileAttributes, expected_directory, policy)?;
    let mut final_path = vec![0u16; 32_768];
    let written = unsafe { GetFinalPathNameByHandleW(handle.0, &mut final_path, VOLUME_NAME_GUID) };
    let written = usize::try_from(written).map_err(|_| BackendError::Overflow)?;
    if written == 0 || written >= final_path.len() {
        return Err(BackendError::Platform);
    }
    let final_path =
        String::from_utf16(&final_path[..written]).map_err(|_| BackendError::InvalidData)?;
    if !path_strings_equal_ignore_case(
        final_path.trim_end_matches(['\\', '/']),
        path.to_str()
            .ok_or(BackendError::InvalidData)?
            .trim_end_matches(['\\', '/']),
    )? {
        return Err(BackendError::InvalidData);
    }
    Ok((handle, tag))
}

struct OwnedPidl(*mut ITEMIDLIST);

impl Drop for OwnedPidl {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

fn open_execution_component(path: &str, directory: bool) -> Result<OwnedHandle, BackendError> {
    let wide = to_wide(path)?;
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
    }
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .map(OwnedHandle)
    .map_err(|error| match classify_open_failure(error.code()) {
        OpenFailure::Missing => BackendError::Missing,
        OpenFailure::Denied => BackendError::Denied,
        OpenFailure::Failed => BackendError::Platform,
    })
}

fn inspect_execution_component(
    handle: &OwnedHandle,
    identity: &VolumeIdentity,
) -> Result<(bool, bool, VolumeIdentity, String), BackendError> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>())
                .map_err(|_| BackendError::Overflow)?,
        )
    }
    .map_err(|_| BackendError::Platform)?;
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
    .map_err(|_| BackendError::Platform)?;

    let mut final_path = vec![0u16; 32_768];
    let written = unsafe { GetFinalPathNameByHandleW(handle.0, &mut final_path, VOLUME_NAME_GUID) };
    let written = usize::try_from(written).map_err(|_| BackendError::Overflow)?;
    if written == 0 || written >= final_path.len() {
        return Err(BackendError::Platform);
    }
    let final_path =
        String::from_utf16(&final_path[..written]).map_err(|_| BackendError::InvalidData)?;
    let root = normalize_guid(&identity.volume_guid_path)?;
    if final_path.len() < root.len()
        || !path_strings_equal_ignore_case(&final_path[..root.len()], &root)?
    {
        return Err(BackendError::InvalidData);
    }
    let relative = final_path[root.len()..]
        .trim_matches(['\\', '/'])
        .replace('/', "\\");
    Ok((
        tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
        tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
        VolumeIdentity {
            volume_guid_path: root,
            volume_serial: serial,
            filesystem_name: from_nul_terminated(&filesystem)?.to_uppercase(),
        },
        relative,
    ))
}

fn pin_indexed_path(
    volume: &FixedVolume,
    relative_path: &str,
    final_is_directory: bool,
) -> Result<Vec<OwnedHandle>, BackendError> {
    pin_indexed_path_components_with(
        &volume.identity,
        relative_path,
        final_is_directory,
        |relative, directory, _| {
            let path = native_path(volume, relative);
            open_execution_component(path.to_str().ok_or(BackendError::InvalidData)?, directory)
        },
        |handle, _, _| inspect_execution_component(handle, &volume.identity),
    )
}

fn reveal_file(path: &str) -> Result<(), BackendError> {
    let wide = to_wide(path)?;
    let full = OwnedPidl(unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) });
    if full.0.is_null() {
        return Err(BackendError::Platform);
    }
    let folder = OwnedPidl(unsafe { ILClone(full.0) });
    if folder.0.is_null() || !unsafe { ILRemoveLastID(Some(folder.0)) }.as_bool() {
        return Err(BackendError::Platform);
    }
    let child = unsafe { ILFindLastID(full.0) };
    if child.is_null() {
        return Err(BackendError::Platform);
    }
    unsafe { SHOpenFolderAndSelectItems(folder.0, Some(&[child]), 0) }
        .map_err(|_| BackendError::Platform)
}

fn open_directory(path: &str) -> Result<(), BackendError> {
    directory_shell_execute_ex_with(path, |call| {
        let Ok(wide) = to_wide(call.path()) else {
            return false;
        };
        let mut info = SHELLEXECUTEINFOW {
            cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0),
            fMask: SEE_MASK_FLAG_NO_UI,
            lpFile: PCWSTR(wide.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.is_ok()
    })
}

pub(super) fn execute_indexed_path(
    volume: &FixedVolume,
    action: &OpenIndexedPath,
) -> Result<FileExecutionOutcome, BackendError> {
    let directory = action.kind == IndexedKind::Directory;
    let _handles = pin_indexed_path(volume, &action.relative_path, directory)?;
    let path = native_path(volume, &action.relative_path);
    let path = path.to_str().ok_or(BackendError::InvalidData)?;
    if directory {
        open_directory(path)?;
        Ok(FileExecutionOutcome::FolderOpenRequested)
    } else {
        reveal_file(path)?;
        Ok(FileExecutionOutcome::FileRevealRequested)
    }
}

struct ScanBatcher<F> {
    batch: Vec<IndexEntry>,
    emit: F,
}

impl<F> ScanBatcher<F>
where
    F: FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
{
    fn new(emit: F) -> Self {
        Self {
            batch: Vec::with_capacity(SCAN_BATCH_SIZE),
            emit,
        }
    }

    fn push(&mut self, entry: IndexEntry) -> Result<(), BackendError> {
        self.batch.push(entry);
        if self.batch.len() == SCAN_BATCH_SIZE {
            (self.emit)(std::mem::replace(
                &mut self.batch,
                Vec::with_capacity(SCAN_BATCH_SIZE),
            ))?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(), BackendError> {
        if !self.batch.is_empty() {
            (self.emit)(self.batch)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) enum BackendError {
    Platform,
    Denied,
    InvalidData,
    Overflow,
    Stopped,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FixedVolume {
    pub(super) identity: VolumeIdentity,
    pub(super) mount_point: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExcludedPrefix {
    identity: VolumeIdentity,
    relative_prefix: String,
}

impl ExcludedPrefix {
    #[cfg(test)]
    pub(super) fn new(identity: VolumeIdentity, relative_prefix: &str) -> Self {
        Self {
            identity,
            relative_prefix: relative_prefix.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EnumerationStep {
    Complete,
    Denied,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OpenFailure {
    Missing,
    Denied,
    Failed,
}

#[derive(Clone, Debug)]
pub(super) struct RawVolume {
    pub(super) mount_point: String,
    pub(super) drive_type: u32,
    pub(super) volume_guid_path: String,
    pub(super) volume_serial: u32,
    pub(super) filesystem_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StructuredEvent {
    pub(super) sequence: u64,
    pub(super) action: FILE_ACTION,
    pub(super) relative_path: String,
}

impl StructuredEvent {
    pub(super) fn new(action: FILE_ACTION, relative_path: impl Into<String>) -> Self {
        Self {
            sequence: 0,
            action,
            relative_path: relative_path.into(),
        }
    }
}

pub(super) struct EventBuffer {
    next_sequence: u64,
    last_sequence: Option<u64>,
    events: VecDeque<StructuredEvent>,
    overflowed: bool,
    capacity: usize,
}

impl EventBuffer {
    pub(super) fn new() -> Self {
        Self::with_capacity(EVENT_CAPACITY)
    }

    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            next_sequence: 0,
            last_sequence: None,
            events: VecDeque::new(),
            overflowed: false,
            capacity,
        }
    }

    pub(super) fn push_batch(
        &mut self,
        events: impl IntoIterator<Item = StructuredEvent>,
    ) -> Result<(), BackendError> {
        for mut event in events {
            if self.events.len() == self.capacity {
                self.overflowed = true;
                return Err(BackendError::Overflow);
            }
            event.sequence = self.next_sequence;
            self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
                self.overflowed = true;
                BackendError::Overflow
            })?;
            self.last_sequence = Some(event.sequence);
            self.events.push_back(event);
        }
        Ok(())
    }

    pub(super) fn push_preserved_batch(
        &mut self,
        events: impl IntoIterator<Item = StructuredEvent>,
    ) -> Result<(), BackendError> {
        for event in events {
            if self.events.len() == self.capacity || event.sequence < self.next_sequence {
                self.overflowed = true;
                return Err(BackendError::Overflow);
            }
            self.next_sequence = event.sequence.checked_add(1).ok_or_else(|| {
                self.overflowed = true;
                BackendError::Overflow
            })?;
            self.last_sequence = Some(event.sequence);
            self.events.push_back(event);
        }
        Ok(())
    }

    pub(super) fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    pub(super) fn observe_preserved_sequence(&mut self, sequence: u64) -> Result<(), BackendError> {
        let next = sequence.checked_add(1).ok_or_else(|| {
            self.overflowed = true;
            BackendError::Overflow
        })?;
        if next < self.next_sequence {
            self.overflowed = true;
            return Err(BackendError::Overflow);
        }
        self.next_sequence = next;
        self.last_sequence = Some(sequence);
        Ok(())
    }

    pub(super) fn take_through(&mut self, cutoff: u64) -> Vec<StructuredEvent> {
        let count = self
            .events
            .iter()
            .take_while(|event| event.sequence <= cutoff)
            .count();
        self.events.drain(..count).collect()
    }

    pub(super) fn take_all(&mut self) -> Vec<StructuredEvent> {
        self.events.drain(..).collect()
    }

    #[cfg(test)]
    pub(super) fn events(&self) -> Vec<StructuredEvent> {
        self.events.iter().cloned().collect()
    }
}

#[cfg(not(test))]
const WATCH_BUFFER_BYTES: usize = 64 * 1024;

#[cfg(not(test))]
pub(super) struct Watcher {
    directory: HANDLE,
    event: HANDLE,
    overlapped: Box<OVERLAPPED>,
    user_buffer: Box<[u32]>,
    structured: EventBuffer,
    pending: bool,
}

#[cfg(not(test))]
impl Watcher {
    pub(super) fn arm(volume: &FixedVolume) -> Result<Self, BackendError> {
        reauthenticate_volume(volume)?;
        // The owned structured sink and bounded queue exist before the first native read is armed.
        let structured = EventBuffer::new();
        let wide = to_wide(
            native_root(volume)
                .to_str()
                .ok_or(BackendError::InvalidData)?,
        )?;
        let directory = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|_| BackendError::Platform)?;
        let event = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
            Ok(event) => event,
            Err(_) => {
                let _ = unsafe { CloseHandle(directory) };
                return Err(BackendError::Platform);
            }
        };
        let mut overlapped = Box::<OVERLAPPED>::default();
        overlapped.hEvent = event;
        let user_buffer =
            vec![0u32; WATCH_BUFFER_BYTES / std::mem::size_of::<u32>()].into_boxed_slice();
        let mut watcher = Self {
            directory,
            event,
            overlapped,
            user_buffer,
            structured,
            pending: false,
        };
        watcher.arm_read()?;
        Ok(watcher)
    }

    fn arm_read(&mut self) -> Result<(), BackendError> {
        arm_read_with(
            self.directory,
            self.user_buffer.as_mut(),
            self.overlapped.as_mut(),
        )?;
        self.pending = true;
        Ok(())
    }

    pub(super) fn wait_batch(
        &mut self,
        timeout_ms: u32,
    ) -> Result<Option<Vec<StructuredEvent>>, BackendError> {
        let wait = unsafe { WaitForSingleObject(self.event, timeout_ms) };
        if wait == WAIT_TIMEOUT {
            return Ok(None);
        }
        if wait != WAIT_OBJECT_0 {
            return Err(BackendError::Platform);
        }
        self.pending = false;
        let mut bytes = 0u32;
        unsafe { GetOverlappedResult(self.directory, self.overlapped.as_ref(), &mut bytes, false) }
            .map_err(|_| BackendError::Overflow)?;
        if bytes == 0 {
            return Err(BackendError::Overflow);
        }
        let byte_len = usize::try_from(bytes).map_err(|_| BackendError::Overflow)?;
        if byte_len > WATCH_BUFFER_BYTES {
            return Err(BackendError::InvalidData);
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(self.user_buffer.as_ptr().cast::<u8>(), byte_len) };
        // Parse and copy all returned bytes before the same user buffer is handed to the next read.
        let parsed = parse_notifications(bytes)?;
        let directory = self.directory;
        let event = self.event;
        let user_buffer = self.user_buffer.as_mut();
        let overlapped = self.overlapped.as_mut();
        let pending = &mut self.pending;
        complete_and_rearm_with(&mut self.structured, parsed, || {
            unsafe { ResetEvent(event) }.map_err(|_| BackendError::Platform)?;
            *overlapped = OVERLAPPED::default();
            overlapped.hEvent = event;
            arm_read_with(directory, user_buffer, overlapped)?;
            *pending = true;
            Ok(())
        })?;
        Ok(Some(self.structured.take_all()))
    }
}

#[cfg(not(test))]
fn arm_read_with(
    directory: HANDLE,
    user_buffer: &mut [u32],
    overlapped: &mut OVERLAPPED,
) -> Result<(), BackendError> {
    unsafe {
        ReadDirectoryChangesW(
            directory,
            user_buffer.as_mut_ptr().cast::<c_void>(),
            u32::try_from(WATCH_BUFFER_BYTES).map_err(|_| BackendError::Overflow)?,
            true,
            FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_ATTRIBUTES
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_CREATION,
            None,
            Some(overlapped),
            None,
        )
    }
    .map_err(|_| BackendError::Platform)
}

fn complete_and_rearm_with<R>(
    structured: &mut EventBuffer,
    parsed: Vec<StructuredEvent>,
    rearm: R,
) -> Result<(), BackendError>
where
    R: FnOnce() -> Result<(), BackendError>,
{
    structured.push_batch(parsed)?;
    rearm()?;
    Ok(())
}

#[cfg(not(test))]
impl Drop for Watcher {
    fn drop(&mut self) {
        let completed = shutdown_pending_io_with(
            self.pending,
            || match unsafe { CancelIoEx(self.directory, Some(self.overlapped.as_ref())) } {
                Ok(()) => CancelOutcome::Requested,
                Err(error) if error.code() == windows::core::HRESULT::from_win32(1168) => {
                    CancelOutcome::NotFound
                }
                Err(_) => CancelOutcome::Failed,
            },
            || {
                let mut bytes = 0u32;
                match unsafe {
                    GetOverlappedResult(self.directory, self.overlapped.as_ref(), &mut bytes, true)
                } {
                    Ok(()) => CompletionOutcome::Completed,
                    Err(error) if error.code() == windows::core::HRESULT::from_win32(995) => {
                        CompletionOutcome::Aborted
                    }
                    Err(_) => CompletionOutcome::Failed,
                }
            },
            || {
                let _ = unsafe { CloseHandle(self.directory) };
                let _ = unsafe { CloseHandle(self.event) };
            },
        );
        if !completed {
            let overlapped = std::mem::take(&mut self.overlapped);
            let user_buffer = std::mem::take(&mut self.user_buffer);
            std::mem::forget(overlapped);
            std::mem::forget(user_buffer);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CancelOutcome {
    Requested,
    NotFound,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompletionOutcome {
    Completed,
    Aborted,
    Failed,
}

fn shutdown_pending_io_with<C, W, R>(pending: bool, cancel: C, wait: W, release: R) -> bool
where
    C: FnOnce() -> CancelOutcome,
    W: FnOnce() -> CompletionOutcome,
    R: FnOnce(),
{
    if pending {
        let _ = cancel();
        if !matches!(
            wait(),
            CompletionOutcome::Completed | CompletionOutcome::Aborted
        ) {
            return false;
        }
    }
    release();
    true
}

fn parse_notifications(bytes: &[u8]) -> Result<Vec<StructuredEvent>, BackendError> {
    let mut events = Vec::new();
    let mut offset = 0usize;
    loop {
        let header_end = offset.checked_add(12).ok_or(BackendError::Overflow)?;
        if header_end > bytes.len() {
            return Err(BackendError::InvalidData);
        }
        let next = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let action = FILE_ACTION(u32::from_le_bytes(
            bytes[offset + 4..offset + 8].try_into().unwrap(),
        ));
        let name_bytes = usize::try_from(u32::from_le_bytes(
            bytes[offset + 8..offset + 12].try_into().unwrap(),
        ))
        .map_err(|_| BackendError::Overflow)?;
        if name_bytes == 0 || name_bytes % 2 != 0 {
            return Err(BackendError::InvalidData);
        }
        let name_end = header_end
            .checked_add(name_bytes)
            .ok_or(BackendError::Overflow)?;
        if name_end > bytes.len() {
            return Err(BackendError::InvalidData);
        }
        let name = bytes[header_end..name_end]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let name = String::from_utf16(&name).map_err(|_| BackendError::InvalidData)?;
        events.push(StructuredEvent::new(action, name));
        if next == 0 {
            break;
        }
        let next = usize::try_from(next).map_err(|_| BackendError::Overflow)?;
        if next < 12 || next % 4 != 0 {
            return Err(BackendError::InvalidData);
        }
        let record_end = offset.checked_add(next).ok_or(BackendError::Overflow)?;
        if name_end > record_end {
            return Err(BackendError::InvalidData);
        }
        offset = offset.checked_add(next).ok_or(BackendError::Overflow)?;
        if offset >= bytes.len() {
            return Err(BackendError::InvalidData);
        }
    }
    validate_rename_pairs(&events)?;
    Ok(events)
}

fn validate_rename_pairs(events: &[StructuredEvent]) -> Result<(), BackendError> {
    let mut old_name = false;
    for event in events {
        if event.action == FILE_ACTION_RENAMED_OLD_NAME {
            if old_name {
                return Err(BackendError::InvalidData);
            }
            old_name = true;
        } else if event.action == FILE_ACTION_RENAMED_NEW_NAME {
            if !old_name {
                return Err(BackendError::InvalidData);
            }
            old_name = false;
        } else if old_name {
            return Err(BackendError::InvalidData);
        }
    }
    if old_name {
        return Err(BackendError::InvalidData);
    }
    Ok(())
}

struct EventBatchPlan {
    deleted_prefixes: Vec<String>,
    refresh_paths: Vec<String>,
    volume_dirty: bool,
}

fn parse_event_batch(events: &[StructuredEvent]) -> Result<EventBatchPlan, BackendError> {
    let mut changes = EventBatchPlan {
        deleted_prefixes: Vec::new(),
        refresh_paths: Vec::new(),
        volume_dirty: false,
    };
    let mut renamed_from = None;
    for event in events {
        match event.action {
            FILE_ACTION_RENAMED_OLD_NAME => {
                if renamed_from.replace(event.relative_path.clone()).is_some() {
                    changes.volume_dirty = true;
                }
            }
            FILE_ACTION_RENAMED_NEW_NAME => {
                let Some(old_path) = renamed_from.take() else {
                    changes.volume_dirty = true;
                    continue;
                };
                changes.deleted_prefixes.push(old_path);
                changes.refresh_paths.push(event.relative_path.clone());
            }
            FILE_ACTION_REMOVED => {
                if renamed_from.take().is_some() {
                    changes.volume_dirty = true;
                }
                changes.deleted_prefixes.push(event.relative_path.clone());
            }
            FILE_ACTION_ADDED | FILE_ACTION_MODIFIED => {
                if renamed_from.take().is_some() {
                    changes.volume_dirty = true;
                }
                changes.refresh_paths.push(event.relative_path.clone());
            }
            _ => return Err(BackendError::InvalidData),
        }
    }
    if renamed_from.is_some() {
        changes.volume_dirty = true;
    }
    Ok(changes)
}

enum PathUpdate {
    Delete,
    File(IndexEntry),
    Directory(IndexEntry),
}

#[derive(Clone, Copy)]
enum PinnedPathPolicy {
    Strict,
    EventLeaf,
}

fn validate_pinned_shape(
    attributes: u32,
    expected_directory: Option<bool>,
    policy: PinnedPathPolicy,
) -> Result<(), BackendError> {
    let is_directory = attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    if (attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        && matches!(policy, PinnedPathPolicy::Strict))
        || expected_directory.is_some_and(|expected| is_directory != expected)
    {
        return Err(BackendError::InvalidData);
    }
    Ok(())
}

fn materialize_event_batches_with<S, I, R, E>(
    events: &[StructuredEvent],
    mut stopped: S,
    mut inspect: I,
    mut rescan: R,
    mut emit: E,
) -> Result<(), BackendError>
where
    S: FnMut() -> bool,
    I: FnMut(&str) -> Result<PathUpdate, BackendError>,
    R: FnMut(
        &str,
        &mut dyn FnMut() -> bool,
        &mut dyn FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
    ) -> Result<(), BackendError>,
    E: FnMut(IndexChangeBatch) -> Result<(), BackendError>,
{
    if stopped() {
        return Err(BackendError::Stopped);
    }
    let plan = parse_event_batch(events)?;
    if plan.volume_dirty {
        return Err(BackendError::Platform);
    }
    if !plan.deleted_prefixes.is_empty() {
        if stopped() {
            return Err(BackendError::Stopped);
        }
        emit(IndexChangeBatch {
            deleted_prefixes: plan.deleted_prefixes,
            entries: Vec::new(),
        })?;
    }
    for path in plan.refresh_paths {
        if stopped() {
            return Err(BackendError::Stopped);
        }
        match inspect(&path)? {
            PathUpdate::Delete => emit(IndexChangeBatch {
                deleted_prefixes: vec![path],
                entries: Vec::new(),
            })?,
            PathUpdate::File(entry) => emit(IndexChangeBatch {
                deleted_prefixes: Vec::new(),
                entries: vec![entry],
            })?,
            PathUpdate::Directory(entry) => {
                emit(IndexChangeBatch {
                    deleted_prefixes: vec![path.clone()],
                    entries: vec![entry],
                })?;
                rescan(&path, &mut stopped, &mut |entries| {
                    emit(IndexChangeBatch {
                        deleted_prefixes: Vec::new(),
                        entries,
                    })
                })?;
                if stopped() {
                    return Err(BackendError::Stopped);
                }
            }
        }
    }
    if stopped() {
        return Err(BackendError::Stopped);
    }
    Ok(())
}

#[cfg(not(test))]
pub(super) fn materialize_events(
    volume: &FixedVolume,
    events: &[StructuredEvent],
    exclusions: &[ExcludedPrefix],
    stopped: impl FnMut() -> bool,
    emit: impl FnMut(IndexChangeBatch) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    reauthenticate_volume(volume)?;
    materialize_event_batches_with(
        events,
        stopped,
        |relative_path| read_path_update(volume, relative_path, exclusions),
        |relative_path, stopped, emit_entries| {
            scan_subtree(volume, relative_path, exclusions, stopped, emit_entries)
        },
        emit,
    )?;
    reauthenticate_volume(volume)
}

#[cfg(not(test))]
fn read_path_update(
    volume: &FixedVolume,
    relative_path: &str,
    exclusions: &[ExcludedPrefix],
) -> Result<PathUpdate, BackendError> {
    if is_excluded(&volume.identity, relative_path, 0, exclusions) {
        return Ok(PathUpdate::Delete);
    }
    let (handle, tag) =
        match open_pinned_with_policy(volume, relative_path, None, PinnedPathPolicy::EventLeaf) {
            Ok(opened) => opened,
            Err(BackendError::Missing) => return Ok(PathUpdate::Delete),
            Err(error) => return Err(error),
        };
    if is_excluded(
        &volume.identity,
        relative_path,
        tag.FileAttributes,
        exclusions,
    ) {
        return Ok(PathUpdate::Delete);
    }
    let name = Path::new(relative_path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(BackendError::InvalidData)?
        .to_owned();
    let directory = tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let mut basic = FILE_BASIC_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>())
                .map_err(|_| BackendError::Overflow)?,
        )
    }
    .map_err(|_| BackendError::Platform)?;
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast::<c_void>(),
            u32::try_from(std::mem::size_of::<FILE_STANDARD_INFO>())
                .map_err(|_| BackendError::Overflow)?,
        )
    }
    .map_err(|_| BackendError::Platform)?;
    let display_path = display_path(volume, relative_path)?;
    let entry = IndexEntry {
        relative_path: relative_path.to_owned(),
        display_path: display_path.clone(),
        folded_name: fold_name(&name),
        name,
        kind: if directory {
            IndexedKind::Directory
        } else {
            IndexedKind::File
        },
        category: classify_category(Path::new(&display_path), directory).to_owned(),
        size_bytes: (!directory)
            .then(|| u64::try_from(standard.EndOfFile).map_err(|_| BackendError::InvalidData))
            .transpose()?,
        modified_utc_ms: windows_time_to_unix_ms(basic.LastWriteTime)?,
    };
    Ok(if directory {
        PathUpdate::Directory(entry)
    } else {
        PathUpdate::File(entry)
    })
}

#[cfg(not(test))]
pub(super) struct ScanSummary {
    pub(super) denied_prefixes: Vec<String>,
}

#[cfg(not(test))]
pub(super) fn fixed_volumes() -> Result<Vec<FixedVolume>, BackendError> {
    let required = unsafe { GetLogicalDriveStringsW(None) };
    if required == 0 {
        return Err(BackendError::Platform);
    }
    let mut buffer = vec![0u16; usize::try_from(required).map_err(|_| BackendError::Overflow)?];
    let written = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) };
    if written == 0 || written >= required {
        return Err(BackendError::Platform);
    }
    let mut raw = Vec::new();
    for mount in
        split_multi_sz(&buffer[..usize::try_from(written).map_err(|_| BackendError::Overflow)?])?
    {
        let wide = to_wide(&mount)?;
        if unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } == DRIVE_FIXED_VALUE {
            raw.push(read_raw_volume(&mount)?);
        }
    }
    collect_fixed_volumes_with(raw)
}

fn read_raw_volume(mount: &str) -> Result<RawVolume, BackendError> {
    let wide = to_wide(mount)?;
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
    let mut guid = vec![0u16; 64];
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(wide.as_ptr()), &mut guid) }
        .map_err(|_| BackendError::Platform)?;
    let mut serial = 0u32;
    let mut filesystem = vec![0u16; 64];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            None,
            Some(&mut serial),
            None,
            None,
            Some(&mut filesystem),
        )
    }
    .map_err(|_| BackendError::Platform)?;
    Ok(RawVolume {
        mount_point: mount.into(),
        drive_type,
        volume_guid_path: from_nul_terminated(&guid)?,
        volume_serial: serial,
        filesystem_name: from_nul_terminated(&filesystem)?,
    })
}

pub(super) fn reauthenticate_volume_with(
    expected: &FixedVolume,
    actual: RawVolume,
) -> Result<(), BackendError> {
    let mut volumes = collect_fixed_volumes_with([actual])?;
    let actual = volumes.pop().ok_or(BackendError::InvalidData)?;
    if actual.identity != expected.identity
        || compare_paths(&actual.mount_point, &expected.mount_point)? != std::cmp::Ordering::Equal
    {
        return Err(BackendError::InvalidData);
    }
    Ok(())
}

pub(super) fn reauthenticate_volume(volume: &FixedVolume) -> Result<(), BackendError> {
    let mount = volume
        .mount_point
        .to_str()
        .ok_or(BackendError::InvalidData)?;
    reauthenticate_volume_with(volume, read_raw_volume(mount)?)
}

pub(super) fn collect_fixed_volumes_with(
    raw: impl IntoIterator<Item = RawVolume>,
) -> Result<Vec<FixedVolume>, BackendError> {
    let mut selected: HashMap<VolumeIdentity, PathBuf> = HashMap::new();
    for volume in raw {
        if volume.drive_type != DRIVE_FIXED_VALUE {
            continue;
        }
        let identity = VolumeIdentity {
            volume_guid_path: normalize_guid(&volume.volume_guid_path)?,
            volume_serial: volume.volume_serial,
            filesystem_name: volume.filesystem_name.to_uppercase(),
        };
        let mount = PathBuf::from(volume.mount_point);
        match selected.get_mut(&identity) {
            Some(current) if compare_paths(&mount, current)? == std::cmp::Ordering::Less => {
                *current = mount;
            }
            Some(_) => {}
            None => {
                selected.insert(identity, mount);
            }
        }
    }
    let mut volumes = selected
        .into_iter()
        .map(|(identity, mount_point)| FixedVolume {
            identity,
            mount_point,
        })
        .collect::<Vec<_>>();
    volumes.sort_by(|left, right| {
        compare_paths(&left.mount_point, &right.mount_point)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.identity
                    .volume_guid_path
                    .cmp(&right.identity.volume_guid_path)
            })
    });
    Ok(volumes)
}

#[cfg(not(test))]
pub(super) fn system_exclusions(
    app_data_root: &Path,
    volumes: &[FixedVolume],
) -> Result<Vec<ExcludedPrefix>, BackendError> {
    let mut roots = vec![app_data_root.to_path_buf()];
    for read in [
        GetWindowsDirectoryW as unsafe fn(Option<&mut [u16]>) -> u32,
        GetSystemDirectoryW,
        GetTempPathW,
    ] {
        let mut buffer = vec![0u16; 32_768];
        let written = unsafe { read(Some(&mut buffer)) };
        if written == 0
            || usize::try_from(written).map_err(|_| BackendError::Overflow)? >= buffer.len()
        {
            return Err(BackendError::Platform);
        }
        roots.push(PathBuf::from(
            String::from_utf16(
                &buffer[..usize::try_from(written).map_err(|_| BackendError::Overflow)?],
            )
            .map_err(|_| BackendError::InvalidData)?,
        ));
    }
    roots
        .into_iter()
        .filter_map(|root| {
            match excluded_prefix_for_resolved_path_with(volumes, &root, |path| {
                resolve_fixed_volume_for_path(path)
            }) {
                Ok(Some(prefix)) => Some(Ok(prefix)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

fn excluded_prefix_for_resolved_path_with<F>(
    volumes: &[FixedVolume],
    path: &Path,
    resolve: F,
) -> Result<Option<ExcludedPrefix>, BackendError>
where
    F: FnOnce(&Path) -> Result<Option<FixedVolume>, BackendError>,
{
    let Some(actual) = resolve(path)? else {
        return Ok(None);
    };
    let Some(volume) = volumes
        .iter()
        .find(|volume| volume.identity == actual.identity)
    else {
        return Ok(None);
    };
    Ok(Some(ExcludedPrefix {
        identity: volume.identity.clone(),
        relative_prefix: relative_to_volume(&actual, path)?,
    }))
}

#[cfg(not(test))]
fn resolve_fixed_volume_for_path(path: &Path) -> Result<Option<FixedVolume>, BackendError> {
    let wide = to_wide(path.to_str().ok_or(BackendError::InvalidData)?)?;
    let mut mount = vec![0u16; 32_768];
    unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut mount) }
        .map_err(|_| BackendError::Platform)?;
    let mount = from_nul_terminated(&mount)?;
    let mount_wide = to_wide(&mount)?;
    if unsafe { GetDriveTypeW(PCWSTR(mount_wide.as_ptr())) } != DRIVE_FIXED_VALUE {
        return Ok(None);
    }
    let raw = read_raw_volume(&mount)?;
    Ok(collect_fixed_volumes_with([raw])?.pop())
}

#[cfg(not(test))]
pub(super) fn scan_volume(
    volume: &FixedVolume,
    exclusions: &[ExcludedPrefix],
    stop: &AtomicBool,
    emit: impl FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
) -> Result<ScanSummary, BackendError> {
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) }
        .map_err(|_| BackendError::Platform)?;
    reauthenticate_volume(volume)?;
    let mut batcher = ScanBatcher::new(emit);
    let mut denied_prefixes = Vec::new();
    scan_directories(volume, exclusions, stop, &mut batcher, &mut denied_prefixes)?;
    batcher.finish()?;
    reauthenticate_volume(volume)?;
    Ok(ScanSummary { denied_prefixes })
}

#[cfg(not(test))]
fn scan_directories<F>(
    volume: &FixedVolume,
    exclusions: &[ExcludedPrefix],
    stop: &AtomicBool,
    batcher: &mut ScanBatcher<F>,
    denied_prefixes: &mut Vec<String>,
) -> Result<(), BackendError>
where
    F: FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
{
    scan_directories_from_with(
        volume,
        String::new(),
        exclusions,
        || stop.load(Ordering::Acquire),
        batcher,
        denied_prefixes,
        |relative_directory, visit| {
            let (handle, _) = open_pinned(volume, relative_directory, Some(true))?;
            enumerate_pinned_directory(&handle, visit)
        },
    )
}

#[cfg(not(test))]
fn scan_subtree(
    volume: &FixedVolume,
    relative_directory: &str,
    exclusions: &[ExcludedPrefix],
    stopped: &mut dyn FnMut() -> bool,
    emit: &mut dyn FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    let mut batcher = ScanBatcher::new(emit);
    let mut denied_prefixes = Vec::new();
    scan_directories_from_with(
        volume,
        relative_directory.to_owned(),
        exclusions,
        &mut *stopped,
        &mut batcher,
        &mut denied_prefixes,
        |relative_directory, visit| {
            let (handle, _) = open_pinned(volume, relative_directory, Some(true))?;
            enumerate_pinned_directory(&handle, visit)
        },
    )?;
    if !denied_prefixes.is_empty() {
        return Err(BackendError::Denied);
    }
    if stopped() {
        return Err(BackendError::Stopped);
    }
    batcher.finish()?;
    if stopped() {
        return Err(BackendError::Stopped);
    }
    Ok(())
}

#[cfg(test)]
fn scan_directories_with<F, S, O>(
    volume: &FixedVolume,
    exclusions: &[ExcludedPrefix],
    stopped: S,
    batcher: &mut ScanBatcher<F>,
    denied_prefixes: &mut Vec<String>,
    enumerate: O,
) -> Result<(), BackendError>
where
    F: FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
    S: FnMut() -> bool,
    O: FnMut(
        &str,
        &mut dyn FnMut(DirectoryRecord) -> Result<(), BackendError>,
    ) -> Result<(), BackendError>,
{
    scan_directories_from_with(
        volume,
        String::new(),
        exclusions,
        stopped,
        batcher,
        denied_prefixes,
        enumerate,
    )
}

fn scan_directories_from_with<F, S, O>(
    volume: &FixedVolume,
    relative_root: String,
    exclusions: &[ExcludedPrefix],
    mut stopped: S,
    batcher: &mut ScanBatcher<F>,
    denied_prefixes: &mut Vec<String>,
    mut enumerate: O,
) -> Result<(), BackendError>
where
    F: FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
    S: FnMut() -> bool,
    O: FnMut(
        &str,
        &mut dyn FnMut(DirectoryRecord) -> Result<(), BackendError>,
    ) -> Result<(), BackendError>,
{
    let mut pending = DirectoryStack {
        priority: Vec::new(),
        ordinary: vec![relative_root],
    };
    while let Some(relative_directory) = pending.pop() {
        if stopped() {
            return Err(BackendError::Stopped);
        }
        let result = enumerate(&relative_directory, &mut |record| {
            if stopped() {
                return Err(BackendError::Stopped);
            }
            if record.name == "." || record.name == ".." {
                return Ok(());
            }
            let relative_path = if relative_directory.is_empty() {
                record.name.clone()
            } else {
                format!("{relative_directory}\\{}", record.name)
            };
            if is_excluded(
                &volume.identity,
                &relative_path,
                record.attributes,
                exclusions,
            ) {
                return Ok(());
            }
            let directory = record.attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
            let display = volume
                .mount_point
                .join(&relative_path)
                .to_str()
                .map(str::to_owned)
                .ok_or(BackendError::InvalidData)?;
            batcher.push(IndexEntry {
                relative_path: relative_path.clone(),
                display_path: display.clone(),
                name: record.name.clone(),
                folded_name: fold_name(&record.name),
                kind: if directory {
                    IndexedKind::Directory
                } else {
                    IndexedKind::File
                },
                category: classify_category(Path::new(&display), directory).to_owned(),
                size_bytes: (!directory).then_some(record.size),
                modified_utc_ms: windows_time_to_unix_ms(record.modified)?,
            })?;
            if directory {
                pending.push(relative_path)?;
            }
            Ok(())
        });
        match result {
            Ok(()) | Err(BackendError::Missing) => {}
            Err(BackendError::Denied) => {
                push_denied_prefix(denied_prefixes, relative_directory)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[derive(Clone)]
struct DirectoryRecord {
    name: String,
    attributes: u32,
    size: u64,
    modified: i64,
}

#[cfg(not(test))]
fn enumerate_pinned_directory(
    handle: &OwnedHandle,
    mut visit: impl FnMut(DirectoryRecord) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    let mut buffer = vec![0u64; (64 * 1024) / std::mem::size_of::<u64>()];
    loop {
        let result = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileFullDirectoryInfo,
                buffer.as_mut_ptr().cast::<c_void>(),
                u32::try_from(buffer.len() * std::mem::size_of::<u64>())
                    .map_err(|_| BackendError::Overflow)?,
            )
        };
        if let Err(error) = result {
            return match classify_enumeration_error(error.code()) {
                EnumerationStep::Complete => Ok(()),
                EnumerationStep::Denied => Err(BackendError::Denied),
                EnumerationStep::Failed => Err(BackendError::Platform),
            };
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>(),
                buffer.len() * std::mem::size_of::<u64>(),
            )
        };
        parse_directory_records(bytes, &mut visit)?;
    }
}

fn parse_directory_records(
    bytes: &[u8],
    visit: &mut dyn FnMut(DirectoryRecord) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    let mut offset = 0usize;
    loop {
        let record_end = offset
            .checked_add(std::mem::size_of::<FILE_FULL_DIR_INFO>())
            .ok_or(BackendError::Overflow)?;
        if record_end > bytes.len() {
            return Err(BackendError::InvalidData);
        }
        let header_end = offset
            .checked_add(std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName))
            .ok_or(BackendError::Overflow)?;
        let info = unsafe {
            std::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<FILE_FULL_DIR_INFO>())
        };
        let name_bytes =
            usize::try_from(info.FileNameLength).map_err(|_| BackendError::Overflow)?;
        if name_bytes == 0 || name_bytes % 2 != 0 {
            return Err(BackendError::InvalidData);
        }
        let name_end = header_end
            .checked_add(name_bytes)
            .ok_or(BackendError::Overflow)?;
        if name_end > bytes.len() {
            return Err(BackendError::InvalidData);
        }
        let name = bytes[header_end..name_end]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        visit(DirectoryRecord {
            name: String::from_utf16(&name).map_err(|_| BackendError::InvalidData)?,
            attributes: info.FileAttributes,
            size: u64::try_from(info.EndOfFile).map_err(|_| BackendError::InvalidData)?,
            modified: info.LastWriteTime,
        })?;
        if info.NextEntryOffset == 0 {
            break;
        }
        let next = usize::try_from(info.NextEntryOffset).map_err(|_| BackendError::Overflow)?;
        if next < std::mem::size_of::<FILE_FULL_DIR_INFO>()
            || next % std::mem::align_of::<FILE_FULL_DIR_INFO>() != 0
            || name_end > offset.checked_add(next).ok_or(BackendError::Overflow)?
        {
            return Err(BackendError::InvalidData);
        }
        offset = offset.checked_add(next).ok_or(BackendError::Overflow)?;
    }
    Ok(())
}

pub(super) fn is_excluded(
    identity: &VolumeIdentity,
    relative_path: &str,
    attributes: u32,
    roots: &[ExcludedPrefix],
) -> bool {
    if attributes
        & (FILE_ATTRIBUTE_HIDDEN.0 | FILE_ATTRIBUTE_SYSTEM.0 | FILE_ATTRIBUTE_REPARSE_POINT.0)
        != 0
    {
        return true;
    }
    roots.iter().any(|root| {
        root.identity == *identity
            && path_is_same_or_descendant(
                Path::new(relative_path),
                Path::new(&root.relative_prefix),
            )
            .unwrap_or(true)
    })
}

pub(super) fn filter_replay_events(
    identity: &VolumeIdentity,
    events: &[StructuredEvent],
    roots: &[ExcludedPrefix],
) -> Vec<StructuredEvent> {
    events
        .iter()
        .filter(|event| !is_excluded(identity, &event.relative_path, 0, roots))
        .cloned()
        .collect()
}

pub(super) fn path_is_same_or_descendant(path: &Path, root: &Path) -> Result<bool, BackendError> {
    let path = path
        .to_str()
        .ok_or(BackendError::InvalidData)?
        .replace('/', "\\");
    let root = root
        .to_str()
        .ok_or(BackendError::InvalidData)?
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\");
    let path_units = path.encode_utf16().collect::<Vec<_>>();
    let root_units = root.encode_utf16().collect::<Vec<_>>();
    if root_units.is_empty() {
        return Ok(true);
    }
    if path_units.len() < root_units.len() {
        return Ok(false);
    }
    let equal =
        unsafe { CompareStringOrdinal(&path_units[..root_units.len()], &root_units, true) }.0 == 2;
    Ok(equal
        && (path_units.len() == root_units.len()
            || path_units.get(root_units.len()).copied() == Some(u16::from(b'\\'))))
}

#[cfg(test)]
pub(super) fn drive_relative_path(path: &Path) -> Result<String, BackendError> {
    let relative = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value),
            _ => None,
        })
        .collect::<PathBuf>();
    relative
        .to_str()
        .map(str::to_owned)
        .ok_or(BackendError::InvalidData)
}

fn relative_to_volume(volume: &FixedVolume, path: &Path) -> Result<String, BackendError> {
    let path_components = path.components().collect::<Vec<_>>();
    let mount_components = volume.mount_point.components().collect::<Vec<_>>();
    if path_components.len() < mount_components.len() {
        return Err(BackendError::InvalidData);
    }
    for (path_component, mount_component) in path_components.iter().zip(&mount_components) {
        let left = path_component
            .as_os_str()
            .to_str()
            .ok_or(BackendError::InvalidData)?;
        let right = mount_component
            .as_os_str()
            .to_str()
            .ok_or(BackendError::InvalidData)?;
        let left = left.encode_utf16().collect::<Vec<_>>();
        let right = right.encode_utf16().collect::<Vec<_>>();
        if unsafe { CompareStringOrdinal(&left, &right, true) }.0 != 2 {
            return Err(BackendError::InvalidData);
        }
    }
    path_components[mount_components.len()..]
        .iter()
        .map(|component| component.as_os_str())
        .collect::<PathBuf>()
        .to_str()
        .map(str::to_owned)
        .ok_or(BackendError::InvalidData)
}

fn classify_enumeration_error(error: windows::core::HRESULT) -> EnumerationStep {
    if error == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES_CODE) {
        EnumerationStep::Complete
    } else if error == windows::core::HRESULT::from_win32(5) {
        EnumerationStep::Denied
    } else {
        EnumerationStep::Failed
    }
}

#[cfg(test)]
pub(super) fn classify_enumeration_error_for_test(error: u32) -> EnumerationStep {
    classify_enumeration_error(windows::core::HRESULT::from_win32(error))
}

fn classify_open_failure(error: windows::core::HRESULT) -> OpenFailure {
    if error == windows::core::HRESULT::from_win32(2)
        || error == windows::core::HRESULT::from_win32(3)
    {
        OpenFailure::Missing
    } else if error == windows::core::HRESULT::from_win32(5) {
        OpenFailure::Denied
    } else {
        OpenFailure::Failed
    }
}

#[cfg(test)]
pub(super) fn classify_open_failure_for_test(error: u32) -> OpenFailure {
    classify_open_failure(windows::core::HRESULT::from_win32(error))
}

pub(super) fn classify_category(path: &Path, directory: bool) -> &'static str {
    if directory {
        return "folder";
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "xls" | "xlsx" | "xlsm" | "xlsb" | "csv" => "excel",
        "doc" | "docx" | "docm" | "rtf" => "word",
        "ppt" | "pptx" | "pptm" => "ppt",
        "pdf" => "pdf",
        "bmp" | "gif" | "heic" | "jpeg" | "jpg" | "png" | "svg" | "tif" | "tiff" | "webp" => {
            "image"
        }
        "avi" | "m4v" | "mkv" | "mov" | "mp4" | "webm" | "wmv" => "video",
        "aac" | "flac" | "m4a" | "mp3" | "ogg" | "wav" | "wma" => "audio",
        "7z" | "bz2" | "gz" | "rar" | "tar" | "tgz" | "zip" => "archive",
        _ => "other",
    }
}

fn normalize_guid(value: &str) -> Result<String, BackendError> {
    if value.is_empty() {
        return Err(BackendError::InvalidData);
    }
    let mut normalized = value.replace('/', "\\").to_uppercase();
    if !normalized.ends_with('\\') {
        normalized.push('\\');
    }
    Ok(normalized)
}

fn compare_paths(left: &Path, right: &Path) -> Result<std::cmp::Ordering, BackendError> {
    let left = left.to_str().ok_or(BackendError::InvalidData)?;
    let right = right.to_str().ok_or(BackendError::InvalidData)?;
    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    Ok(
        match unsafe { CompareStringOrdinal(&left, &right, false) }.0 {
            1 => std::cmp::Ordering::Less,
            2 => std::cmp::Ordering::Equal,
            3 => std::cmp::Ordering::Greater,
            _ => return Err(BackendError::Platform),
        },
    )
}

fn path_strings_equal_ignore_case(left: &str, right: &str) -> Result<bool, BackendError> {
    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    Ok(unsafe { CompareStringOrdinal(&left, &right, true) }.0 == 2)
}

#[cfg(not(test))]
fn split_multi_sz(value: &[u16]) -> Result<Vec<String>, BackendError> {
    value
        .split(|unit| *unit == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf16(part).map_err(|_| BackendError::InvalidData))
        .collect()
}

fn to_wide(value: &str) -> Result<Vec<u16>, BackendError> {
    if value.contains('\0') {
        return Err(BackendError::InvalidData);
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

fn from_nul_terminated(value: &[u16]) -> Result<String, BackendError> {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16(&value[..end]).map_err(|_| BackendError::InvalidData)
}

fn windows_time_to_unix_ms(ticks: i64) -> Result<i64, BackendError> {
    const WINDOWS_TO_UNIX_MS: i128 = 11_644_473_600_000;
    if ticks < 0 {
        return Err(BackendError::InvalidData);
    }
    let unix = i128::from(ticks) / 10_000 - WINDOWS_TO_UNIX_MS;
    i64::try_from(unix).map_err(|_| BackendError::Overflow)
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct NativeEntry {
    path: String,
    attributes: u32,
    size: Option<u64>,
    modified_utc_ms: i64,
    denied: bool,
    stop: bool,
}

#[cfg(test)]
impl NativeEntry {
    pub(super) fn directory(path: &str, attributes: u32) -> Self {
        Self {
            path: path.into(),
            attributes,
            size: None,
            modified_utc_ms: 0,
            denied: false,
            stop: false,
        }
    }

    pub(super) fn file(path: &str, size: u64, modified_utc_ms: i64) -> Self {
        Self {
            path: path.into(),
            attributes: 0,
            size: Some(size),
            modified_utc_ms,
            denied: false,
            stop: false,
        }
    }

    pub(super) fn denied(path: &str) -> Self {
        Self {
            path: path.into(),
            attributes: 0,
            size: None,
            modified_utc_ms: 0,
            denied: true,
            stop: false,
        }
    }

    pub(super) fn stop() -> Self {
        Self {
            path: String::new(),
            attributes: 0,
            size: None,
            modified_utc_ms: 0,
            denied: false,
            stop: true,
        }
    }
}

#[cfg(test)]
pub(super) fn run_scanner_with(
    root: &Path,
    native: VecDeque<NativeEntry>,
) -> Result<Vec<IndexEntry>, BackendError> {
    let mut entries = Vec::new();
    run_scanner_batches_with(root, native, |batch| {
        entries.extend(batch);
        Ok(())
    })?;
    Ok(entries)
}

#[cfg(test)]
pub(super) fn run_scanner_batches_with<E>(
    root: &Path,
    native: VecDeque<NativeEntry>,
    emit: E,
) -> Result<(), BackendError>
where
    E: FnMut(Vec<IndexEntry>) -> Result<(), BackendError>,
{
    let volume = FixedVolume {
        identity: VolumeIdentity {
            volume_guid_path: r"\\?\VOLUME{SCANNER-SEAM}\".into(),
            volume_serial: 1,
            filesystem_name: "NTFS".into(),
        },
        mount_point: root.to_path_buf(),
    };
    let stopped = native.iter().any(|entry| entry.stop);
    let denied = native
        .iter()
        .filter(|entry| entry.denied)
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let records = native
        .into_iter()
        .filter(|entry| !entry.denied && !entry.stop)
        .map(|entry| {
            let windows_ms = entry
                .modified_utc_ms
                .checked_add(11_644_473_600_000)
                .ok_or(BackendError::Overflow)?;
            Ok(DirectoryRecord {
                name: entry.path,
                attributes: entry.attributes,
                size: entry.size.unwrap_or(0),
                modified: windows_ms
                    .checked_mul(10_000)
                    .ok_or(BackendError::Overflow)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut batcher = ScanBatcher::new(emit);
    let mut denied_prefixes = Vec::new();
    scan_directories_with(
        &volume,
        &[],
        || stopped,
        &mut batcher,
        &mut denied_prefixes,
        |relative_directory, visit| {
            if denied.iter().any(|path| path == relative_directory) {
                return Err(BackendError::Denied);
            }
            if relative_directory.is_empty() {
                for record in records.iter().cloned() {
                    visit(record)?;
                }
            }
            Ok(())
        },
    )?;
    batcher.finish()?;
    Ok(())
}

#[cfg(test)]
pub(super) struct WatchCompletion(Result<Vec<StructuredEvent>, BackendError>);

#[cfg(test)]
impl WatchCompletion {
    pub(super) fn events(events: impl IntoIterator<Item = (FILE_ACTION, &'static str)>) -> Self {
        Self(Ok(events
            .into_iter()
            .map(|(action, path)| StructuredEvent::new(action, path))
            .collect()))
    }

    pub(super) fn overflow() -> Self {
        Self(Err(BackendError::Overflow))
    }

    pub(super) fn zero_bytes() -> Self {
        Self(Err(BackendError::Overflow))
    }

    pub(super) fn notify_enum_dir() -> Self {
        Self(Err(BackendError::Overflow))
    }

    pub(super) fn malformed_rename() -> Self {
        Self(Ok(vec![StructuredEvent::new(
            windows::Win32::Storage::FileSystem::FILE_ACTION_RENAMED_OLD_NAME,
            "old",
        )]))
    }

    pub(super) fn parse(self) -> Result<Vec<StructuredEvent>, BackendError> {
        let events = self.0?;
        let mut rename_pending = false;
        for event in &events {
            if event.action == windows::Win32::Storage::FileSystem::FILE_ACTION_RENAMED_OLD_NAME {
                if rename_pending {
                    return Err(BackendError::InvalidData);
                }
                rename_pending = true;
            } else if event.action
                == windows::Win32::Storage::FileSystem::FILE_ACTION_RENAMED_NEW_NAME
            {
                if !rename_pending {
                    return Err(BackendError::InvalidData);
                }
                rename_pending = false;
            }
        }
        if rename_pending {
            return Err(BackendError::InvalidData);
        }
        Ok(events)
    }
}

#[cfg(test)]
pub(super) fn watcher_cycle_with<A, P, R>(
    buffer: &mut EventBuffer,
    arm: A,
    mut parsed: P,
    rearm: R,
) -> Result<(), BackendError>
where
    A: FnOnce() -> WatchCompletion,
    P: FnMut(&[StructuredEvent]),
    R: FnOnce(),
{
    let events = arm().parse()?;
    parsed(&events);
    complete_and_rearm_with(buffer, events, || {
        rearm();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        path::{Path, PathBuf},
    };

    use windows::Win32::Storage::FileSystem::{
        FILE_ACTION_ADDED, FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_SYSTEM,
    };

    use super::{
        classify_category, classify_enumeration_error_for_test, classify_open_failure_for_test,
        collect_fixed_volumes_with, directory_shell_execute_ex_with, drive_relative_path,
        excluded_prefix_for_resolved_path_with, filter_replay_events, is_excluded,
        materialize_event_batches_with, parse_directory_records, parse_event_batch,
        parse_notifications, path_is_same_or_descendant, pin_indexed_path_components_with,
        push_denied_prefix, reauthenticate_volume_with, run_scanner_batches_with, run_scanner_with,
        scan_directories_with, shutdown_pending_io_with, validate_pinned_shape, watcher_cycle_with,
        windows_time_to_unix_ms, BackendError, CancelOutcome, CompletionOutcome, DirectoryRecord,
        DirectoryStack, EnumerationStep, EventBuffer, ExcludedPrefix, FixedVolume, IndexEntry,
        IndexedKind, NativeEntry, OpenFailure, PathUpdate, PinnedPathPolicy, RawVolume,
        ScanBatcher, StructuredEvent, VolumeIdentity, WatchCompletion, DRIVE_FIXED_VALUE,
        EVENT_CAPACITY, SCAN_BATCH_SIZE,
    };

    fn raw_volume(mount: &str, guid: &str, serial: u32, drive_type: u32) -> RawVolume {
        RawVolume {
            mount_point: mount.into(),
            drive_type,
            volume_guid_path: guid.into(),
            volume_serial: serial,
            filesystem_name: "ntfs".into(),
        }
    }

    fn execution_identity() -> VolumeIdentity {
        VolumeIdentity {
            volume_guid_path: r"\\?\Volume{PIN}\".into(),
            volume_serial: 1,
            filesystem_name: "NTFS".into(),
        }
    }

    #[test]
    fn directory_events_update_or_remove_the_complete_subtree() {
        let events = [
            StructuredEvent::new(FILE_ACTION_RENAMED_OLD_NAME, r"old\tree"),
            StructuredEvent::new(FILE_ACTION_RENAMED_NEW_NAME, r"new\tree"),
        ];

        let changes = parse_event_batch(&events).unwrap();

        assert_eq!(changes.deleted_prefixes, [r"old\tree"]);
        assert_eq!(changes.refresh_paths, [r"new\tree"]);
        assert!(!changes.volume_dirty);

        let entry = |path: String, kind| IndexEntry {
            display_path: format!(r"C:\{path}"),
            name: path.clone(),
            folded_name: path.clone(),
            relative_path: path,
            kind,
            category: "other".into(),
            size_bytes: Some(1),
            modified_utc_ms: 1,
        };
        let emitted = RefCell::new(Vec::new());
        materialize_event_batches_with(
            &events,
            || false,
            |path| {
                Ok(PathUpdate::Directory(entry(
                    path.into(),
                    IndexedKind::Directory,
                )))
            },
            |path, _, emit| {
                let mut batcher = ScanBatcher::new(emit);
                for index in 0..1025 {
                    batcher.push(entry(
                        format!(r"{path}\child-{index}.txt"),
                        IndexedKind::File,
                    ))?;
                }
                batcher.finish()
            },
            |batch| {
                emitted.borrow_mut().push(batch);
                Ok(())
            },
        )
        .unwrap();
        let emitted = emitted.into_inner();
        assert_eq!(emitted[0].deleted_prefixes, [r"old\tree"]);
        assert_eq!(emitted[1].deleted_prefixes, [r"new\tree"]);
        assert_eq!(emitted[1].entries.len(), 1);
        assert!(emitted.iter().all(|batch| batch.entries.len() <= 512));
        assert_eq!(
            emitted
                .iter()
                .map(|batch| batch.entries.len())
                .sum::<usize>(),
            1026
        );

        let uncertain = [StructuredEvent::new(FILE_ACTION_RENAMED_OLD_NAME, "orphan")];
        assert!(materialize_event_batches_with(
            &uncertain,
            || false,
            |_| unreachable!(),
            |_, _, _| unreachable!(),
            |_| unreachable!(),
        )
        .is_err());

        let added = [StructuredEvent::new(FILE_ACTION_ADDED, "fresh.txt")];
        let files = RefCell::new(Vec::new());
        materialize_event_batches_with(
            &added,
            || false,
            |path| Ok(PathUpdate::File(entry(path.into(), IndexedKind::File))),
            |_, _, _| unreachable!(),
            |batch| {
                files.borrow_mut().extend(batch.entries);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(files.borrow().len(), 1);
        assert!(materialize_event_batches_with(
            &added,
            || false,
            |_| Err(BackendError::Missing),
            |_, _, _| unreachable!(),
            |_| unreachable!(),
        )
        .is_err());
        let deleted = RefCell::new(Vec::new());
        materialize_event_batches_with(
            &added,
            || false,
            |_| Ok(PathUpdate::Delete),
            |_, _, _| unreachable!(),
            |batch| {
                deleted.borrow_mut().extend(batch.deleted_prefixes);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*deleted.borrow(), ["fresh.txt"]);

        let stop = Cell::new(false);
        let emitted_batches = Cell::new(0);
        let stopped = materialize_event_batches_with(
            &events,
            || stop.get(),
            |path| {
                Ok(PathUpdate::Directory(entry(
                    path.into(),
                    IndexedKind::Directory,
                )))
            },
            |path, stopped, emit| {
                let mut batcher = ScanBatcher::new(|batch| {
                    if stopped() {
                        return Err(BackendError::Stopped);
                    }
                    emit(batch)
                });
                for index in 0..1025 {
                    batcher.push(entry(
                        format!(r"{path}\child-{index}.txt"),
                        IndexedKind::File,
                    ))?;
                }
                batcher.finish()
            },
            |_| {
                let count = emitted_batches.get() + 1;
                emitted_batches.set(count);
                if count == 3 {
                    stop.set(true);
                }
                Ok(())
            },
        );
        assert!(matches!(stopped, Err(BackendError::Stopped)));

        let final_stop = Cell::new(false);
        let final_emits = Cell::new(0);
        let stopped_after_partial_flush = materialize_event_batches_with(
            &events,
            || final_stop.get(),
            |path| {
                Ok(PathUpdate::Directory(entry(
                    path.into(),
                    IndexedKind::Directory,
                )))
            },
            |path, _, emit| {
                let mut batcher = ScanBatcher::new(emit);
                batcher.push(entry(format!(r"{path}\only-child.txt"), IndexedKind::File))?;
                batcher.finish()
            },
            |_| {
                let count = final_emits.get() + 1;
                final_emits.set(count);
                if count == 2 {
                    final_stop.set(true);
                }
                Ok(())
            },
        );
        assert!(matches!(
            stopped_after_partial_flush,
            Err(BackendError::Stopped)
        ));
    }

    #[test]
    fn fixed_volume_identity_is_guid_serial_and_filesystem() {
        let source = include_str!("windows_backend.rs");
        let share_mode = ["FILE_SHARE_READ", "FILE_SHARE_WRITE", "FILE_SHARE_DELETE"].join(" | ");
        assert_eq!(source.matches(&share_mode).count(), 2);
        for required in [
            "FileFullDirectoryInfo",
            "FILE_FLAG_OPEN_REPARSE_POINT",
            "native_root(volume)",
            "scan_directories_with(",
            "complete_and_rearm_with(&mut self.structured",
            "GetVolumePathNameW",
        ] {
            assert!(source.contains(required));
        }
        for forbidden in [["#[", "allow("].concat(), ["#[", "expect("].concat()] {
            assert!(!source.contains(&forbidden));
        }
        let volumes = collect_fixed_volumes_with([
            raw_volume("D:\\", r"\\?\Volume{A}\", 7, DRIVE_FIXED_VALUE),
            raw_volume("C:\\", r"\\?\Volume{A}\", 7, DRIVE_FIXED_VALUE),
            raw_volume("C:\\", r"\\?\Volume{B}\", 8, DRIVE_FIXED_VALUE),
            raw_volume("E:\\", r"\\?\Volume{USB}\", 9, 2),
        ])
        .unwrap();
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].mount_point, PathBuf::from(r"C:\"));
        assert_eq!(volumes[0].identity.volume_guid_path, r"\\?\VOLUME{A}\");
        assert_eq!(volumes[0].identity.volume_serial, 7);
        assert_eq!(volumes[0].identity.filesystem_name, "NTFS");
        assert_ne!(volumes[0].identity, volumes[1].identity);
        assert!(reauthenticate_volume_with(
            &volumes[0],
            raw_volume("C:\\", r"\\?\Volume{A}\", 7, DRIVE_FIXED_VALUE),
        )
        .is_ok());
        assert!(reauthenticate_volume_with(
            &volumes[0],
            raw_volume("C:\\", r"\\?\Volume{REUSED}\", 70, DRIVE_FIXED_VALUE),
        )
        .is_err());
    }

    #[test]
    fn event_leaf_reparse_is_deleted_while_scanner_policy_rejects_it() {
        let attributes = FILE_ATTRIBUTE_DIRECTORY.0 | FILE_ATTRIBUTE_REPARSE_POINT.0;
        assert!(matches!(
            validate_pinned_shape(attributes, None, PinnedPathPolicy::Strict),
            Err(BackendError::InvalidData)
        ));
        assert!(validate_pinned_shape(attributes, None, PinnedPathPolicy::EventLeaf).is_ok());
        let source = include_str!("windows_backend.rs");
        let test_module_marker = ["mod", "tests {"].join(" ");
        assert_eq!(source.matches(&test_module_marker).count(), 1);
        let production = source.split_once(&test_module_marker).unwrap().0;
        assert_eq!(production.matches("PinnedPathPolicy::EventLeaf").count(), 1);
        let read_path_update = production.split_once("fn read_path_update(").unwrap().1;
        let read_path_update = read_path_update
            .split_once("pub(super) struct ScanSummary")
            .unwrap()
            .0;
        assert!(read_path_update.contains("open_pinned_with_policy"));
        assert!(read_path_update.contains("PinnedPathPolicy::EventLeaf"));
    }

    #[test]
    fn windows_filetime_accepts_pre_epoch_values_and_rejects_negative_ticks() {
        const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

        assert_eq!(windows_time_to_unix_ms(0).unwrap(), -11_644_473_600_000);
        assert_eq!(windows_time_to_unix_ms(UNIX_EPOCH_TICKS).unwrap(), 0);
        assert_eq!(
            windows_time_to_unix_ms(UNIX_EPOCH_TICKS + 12_340_000).unwrap(),
            1_234
        );
        assert!(matches!(
            windows_time_to_unix_ms(-1),
            Err(BackendError::InvalidData)
        ));
    }

    #[test]
    fn scanner_excludes_attributes_system_roots_temp_and_app_data() {
        let volume = collect_fixed_volumes_with([raw_volume(
            "C:\\",
            r"\\?\Volume{A}\",
            7,
            DRIVE_FIXED_VALUE,
        )])
        .unwrap()
        .remove(0);
        let excluded = [
            ExcludedPrefix::new(volume.identity.clone(), r"Windows"),
            ExcludedPrefix::new(volume.identity.clone(), r"Temp"),
            ExcludedPrefix::new(volume.identity.clone(), r"Users\me\AppData\UiPilot"),
        ];
        for attributes in [
            FILE_ATTRIBUTE_HIDDEN.0,
            FILE_ATTRIBUTE_SYSTEM.0,
            FILE_ATTRIBUTE_REPARSE_POINT.0,
        ] {
            assert!(is_excluded(
                &volume.identity,
                r"data\blocked",
                attributes,
                &excluded,
            ));
        }
        for path in [
            r"C:\Windows\System32\kernel32.dll",
            r"C:\Temp\scratch.txt",
            r"C:\Users\me\AppData\UiPilot\file-index.sqlite3",
            r"C:\Users\me\AppData\UiPilot\file-index.sqlite3-wal",
            r"C:\Users\me\AppData\UiPilot\file-index.sqlite3-shm",
        ] {
            assert!(is_excluded(
                &volume.identity,
                drive_relative_path(Path::new(path)).unwrap().as_str(),
                0,
                &excluded,
            ));
        }
        assert!(!is_excluded(
            &volume.identity,
            r"data\visible.txt",
            0,
            &excluded,
        ));
        let other = super::VolumeIdentity {
            volume_guid_path: r"\\?\VOLUME{OTHER}\".into(),
            volume_serial: 8,
            filesystem_name: "NTFS".into(),
        };
        assert!(!is_excluded(&other, r"Windows\System32", 0, &excluded));
        assert!(excluded_prefix_for_resolved_path_with(
            std::slice::from_ref(&volume),
            Path::new(r"D:\Temp"),
            |_| Ok(None),
        )
        .unwrap()
        .is_none());
        let selected_alias = FixedVolume {
            identity: volume.identity.clone(),
            mount_point: PathBuf::from(r"C:\mnt\"),
        };
        let resolved = excluded_prefix_for_resolved_path_with(
            std::slice::from_ref(&selected_alias),
            Path::new(r"D:\Windows\System32"),
            |_| {
                Ok(Some(FixedVolume {
                    identity: volume.identity.clone(),
                    mount_point: PathBuf::from(r"D:\"),
                }))
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.identity, volume.identity);
        assert_eq!(resolved.relative_prefix, r"Windows\System32");
        let feedback = filter_replay_events(
            &volume.identity,
            &[
                StructuredEvent::new(
                    windows::Win32::Storage::FileSystem::FILE_ACTION_REMOVED,
                    r"Users\me\AppData\UiPilot\file-index.sqlite3",
                ),
                StructuredEvent::new(
                    windows::Win32::Storage::FileSystem::FILE_ACTION_MODIFIED,
                    r"Users\me\AppData\UiPilot\file-index.sqlite3-wal",
                ),
                StructuredEvent::new(
                    windows::Win32::Storage::FileSystem::FILE_ACTION_ADDED,
                    r"Users\me\AppData\UiPilot\file-index.sqlite3-shm",
                ),
            ],
            &excluded,
        );
        assert!(feedback.is_empty());
        assert!(path_is_same_or_descendant(
            Path::new(r"c:\WINDOWS\System32"),
            Path::new(r"C:\Windows"),
        )
        .unwrap());
        assert!(!path_is_same_or_descendant(
            Path::new(r"C:\Windows-old"),
            Path::new(r"C:\Windows"),
        )
        .unwrap());
    }

    #[test]
    fn scanner_reads_metadata_without_file_content() {
        let entries = run_scanner_with(
            Path::new(r"C:\"),
            VecDeque::from([
                NativeEntry::directory("docs", FILE_ATTRIBUTE_DIRECTORY.0),
                NativeEntry::file("docs\\report.xlsx", 12, 1_725_120_000_000),
            ]),
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].category, "excel");
        assert_eq!(
            drive_relative_path(Path::new(r"C:\docs\report.xlsx")).unwrap(),
            r"docs\report.xlsx"
        );
        assert_eq!(
            classify_category(Path::new("archive.tar.gz"), false),
            "archive"
        );
        assert_eq!(classify_category(Path::new("sheet.XLSX"), false), "excel");
        assert_eq!(classify_category(Path::new("letter.docm"), false), "word");
        assert_eq!(classify_category(Path::new("slides.pptx"), false), "ppt");
        assert_eq!(classify_category(Path::new("paper.pdf"), false), "pdf");
        assert_eq!(classify_category(Path::new("photo.webp"), false), "image");
        assert_eq!(classify_category(Path::new("movie.mkv"), false), "video");
        assert_eq!(classify_category(Path::new("sound.flac"), false), "audio");
        assert_eq!(classify_category(Path::new("unknown.bin"), false), "other");
        assert_eq!(classify_category(Path::new("folder"), true), "folder");

        let batch_sizes = RefCell::new(Vec::new());
        run_scanner_batches_with(
            Path::new(r"C:\"),
            (0..1025)
                .map(|index| NativeEntry::file(&format!("file-{index}.txt"), 1, 0))
                .collect(),
            |batch| {
                batch_sizes.borrow_mut().push(batch.len());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*batch_sizes.borrow(), [512, 512, 1]);
        let volume = collect_fixed_volumes_with([raw_volume(
            "C:\\",
            r"\\?\Volume{A}\",
            7,
            DRIVE_FIXED_VALUE,
        )])
        .unwrap()
        .remove(0);
        let deep_batches = RefCell::new(Vec::new());
        let mut deep_batcher = ScanBatcher::new(|batch| {
            deep_batches.borrow_mut().push(batch.len());
            Ok(())
        });
        let mut deep_denied = Vec::new();
        scan_directories_with(
            &volume,
            &[],
            || false,
            &mut deep_batcher,
            &mut deep_denied,
            |relative, visit| {
                let depth = relative.split('\\').filter(|part| !part.is_empty()).count();
                if depth < 300 {
                    visit(DirectoryRecord {
                        name: "child".into(),
                        attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                        size: 0,
                        modified: 116_444_736_000_000_000,
                    })?;
                }
                Ok(())
            },
        )
        .unwrap();
        deep_batcher.finish().unwrap();
        assert_eq!(deep_batches.borrow().iter().sum::<usize>(), 300);

        let mut denied_batcher = ScanBatcher::new(|_| Ok(()));
        let mut denied_prefixes = Vec::new();
        scan_directories_with(
            &volume,
            &[],
            || false,
            &mut denied_batcher,
            &mut denied_prefixes,
            |relative, visit| {
                if relative.is_empty() {
                    visit(DirectoryRecord {
                        name: "private".into(),
                        attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                        size: 0,
                        modified: 116_444_736_000_000_000,
                    })?;
                    Ok(())
                } else {
                    Err(BackendError::Denied)
                }
            },
        )
        .unwrap();
        assert_eq!(denied_prefixes, ["private"]);

        let mut stack = DirectoryStack::root();
        for index in 1..EVENT_CAPACITY {
            stack.push(index.to_string()).unwrap();
        }
        assert!(stack.push("overflow".into()).is_err());
        let mut denied_bound = Vec::new();
        for index in 0..EVENT_CAPACITY {
            push_denied_prefix(&mut denied_bound, index.to_string()).unwrap();
        }
        assert!(push_denied_prefix(&mut denied_bound, "overflow".into()).is_err());

        let denied = run_scanner_with(
            Path::new(r"C:\"),
            VecDeque::from([
                NativeEntry::denied("private"),
                NativeEntry::file("visible.txt", 1, 1_725_120_000_000),
            ]),
        )
        .unwrap();
        assert_eq!(denied.len(), 1);
        assert!(matches!(
            run_scanner_with(Path::new(r"C:\"), VecDeque::from([NativeEntry::stop()]),),
            Err(super::BackendError::Stopped)
        ));
        assert_eq!(
            classify_enumeration_error_for_test(windows::Win32::Foundation::ERROR_NO_MORE_FILES.0),
            EnumerationStep::Complete
        );
        assert_eq!(
            classify_enumeration_error_for_test(windows::Win32::Foundation::ERROR_ACCESS_DENIED.0),
            EnumerationStep::Denied
        );
        assert_eq!(
            classify_enumeration_error_for_test(87),
            EnumerationStep::Failed
        );
        assert_eq!(
            classify_open_failure_for_test(windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.0),
            OpenFailure::Missing
        );
        assert_eq!(
            classify_open_failure_for_test(windows::Win32::Foundation::ERROR_PATH_NOT_FOUND.0),
            OpenFailure::Missing
        );
        assert_eq!(
            classify_open_failure_for_test(windows::Win32::Foundation::ERROR_ACCESS_DENIED.0),
            OpenFailure::Denied
        );
        assert_eq!(classify_open_failure_for_test(87), OpenFailure::Failed);

        let record_size =
            std::mem::size_of::<windows::Win32::Storage::FileSystem::FILE_FULL_DIR_INFO>();
        assert!(parse_directory_records(&vec![0u8; record_size - 1], &mut |_| Ok(()),).is_err());
        let mut truncated_tail = vec![0u8; record_size * 2 - 1];
        truncated_tail[..4].copy_from_slice(&(record_size as u32).to_le_bytes());
        let name_length = std::mem::offset_of!(
            windows::Win32::Storage::FileSystem::FILE_FULL_DIR_INFO,
            FileNameLength
        );
        truncated_tail[name_length..name_length + 4].copy_from_slice(&2u32.to_le_bytes());
        let name = std::mem::offset_of!(
            windows::Win32::Storage::FileSystem::FILE_FULL_DIR_INFO,
            FileName
        );
        truncated_tail[name..name + 2].copy_from_slice(&(b'a' as u16).to_le_bytes());
        let visited = Cell::new(0);
        assert!(parse_directory_records(&truncated_tail, &mut |_| {
            visited.set(visited.get() + 1);
            Ok(())
        })
        .is_err());
        assert_eq!(visited.get(), 1);
    }

    #[test]
    fn c_drive_scan_prioritizes_user_desktop_before_large_system_branches() {
        let volume = collect_fixed_volumes_with([raw_volume(
            "C:\\",
            r"\\?\Volume{C}\",
            7,
            DRIVE_FIXED_VALUE,
        )])
        .unwrap()
        .remove(0);
        let first_batch = RefCell::new(None::<Vec<String>>);
        let mut batcher = ScanBatcher::new(|batch: Vec<IndexEntry>| {
            if first_batch.borrow().is_none() {
                *first_batch.borrow_mut() =
                    Some(batch.into_iter().map(|entry| entry.relative_path).collect());
            }
            Ok(())
        });
        let mut denied_prefixes = Vec::new();

        scan_directories_with(
            &volume,
            &[],
            || false,
            &mut batcher,
            &mut denied_prefixes,
            |relative, visit| {
                match relative {
                    "" => {
                        visit(DirectoryRecord {
                            name: "Users".into(),
                            attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                            size: 0,
                            modified: 116_444_736_000_000_000,
                        })?;
                        visit(DirectoryRecord {
                            name: "Windows".into(),
                            attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                            size: 0,
                            modified: 116_444_736_000_000_000,
                        })?;
                    }
                    "Users" => visit(DirectoryRecord {
                        name: "moby".into(),
                        attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                        size: 0,
                        modified: 116_444_736_000_000_000,
                    })?,
                    r"Users\moby" => visit(DirectoryRecord {
                        name: "Desktop".into(),
                        attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                        size: 0,
                        modified: 116_444_736_000_000_000,
                    })?,
                    r"Users\moby\Desktop" => visit(DirectoryRecord {
                        name: "云图".into(),
                        attributes: FILE_ATTRIBUTE_DIRECTORY.0,
                        size: 0,
                        modified: 116_444_736_000_000_000,
                    })?,
                    "Windows" => {
                        for index in 0..SCAN_BATCH_SIZE {
                            visit(DirectoryRecord {
                                name: format!("system-{index}.dll"),
                                attributes: 0,
                                size: 1,
                                modified: 116_444_736_000_000_000,
                            })?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            },
        )
        .unwrap();

        let first_batch = first_batch.into_inner().unwrap();
        assert!(first_batch
            .iter()
            .any(|path| path == r"Users\moby\Desktop\云图"));
    }

    #[test]
    fn watcher_sink_and_buffer_exist_before_first_arm() {
        let mut buffer = EventBuffer::new();
        let order = RefCell::new(Vec::new());
        watcher_cycle_with(
            &mut buffer,
            || {
                order.borrow_mut().push("arm");
                WatchCompletion::events([(FILE_ACTION_ADDED, "instant.txt")])
            },
            |_| order.borrow_mut().push("parse"),
            || order.borrow_mut().push("rearm"),
        )
        .unwrap();
        assert_eq!(*order.borrow(), ["arm", "parse", "rearm"]);
        assert_eq!(buffer.events().len(), 1);
        assert_eq!(buffer.events()[0].sequence, 0);
    }

    #[test]
    fn completed_user_buffer_is_parsed_before_rearm() {
        let source = include_str!("windows_backend.rs");
        let parsed = source
            .find("let parsed = parse_notifications(bytes)?;")
            .unwrap();
        let staged = source
            .find("complete_and_rearm_with(&mut self.structured")
            .unwrap();
        assert!(parsed < staged);
        let mut buffer = EventBuffer::new();
        let parsing = Cell::new(false);
        watcher_cycle_with(
            &mut buffer,
            || WatchCompletion::events([(FILE_ACTION_ADDED, "first.txt")]),
            |_| parsing.set(true),
            || {
                assert!(
                    parsing.get(),
                    "rearm happened before completed bytes were parsed"
                )
            },
        )
        .unwrap();
        buffer
            .push_batch([
                StructuredEvent::new(FILE_ACTION_RENAMED_OLD_NAME, "old"),
                StructuredEvent::new(FILE_ACTION_RENAMED_NEW_NAME, "new"),
            ])
            .unwrap();
        assert_eq!(buffer.events().len(), 3);
        let first = buffer.take_all();
        let mut replay = EventBuffer::new();
        replay.push_preserved_batch(first).unwrap();
        buffer
            .push_batch([StructuredEvent::new(FILE_ACTION_ADDED, "later")])
            .unwrap();
        replay.push_preserved_batch(buffer.take_all()).unwrap();
        assert_eq!(
            replay
                .events()
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        let cutoff = replay.last_sequence().unwrap();
        assert_eq!(cutoff, 3);
        assert_eq!(replay.take_through(cutoff).len(), 4);
        assert!(replay
            .push_preserved_batch([StructuredEvent {
                sequence: 3,
                action: FILE_ACTION_ADDED,
                relative_path: "duplicate".into(),
            }])
            .is_err());
        assert!(EventBuffer::with_capacity(EVENT_CAPACITY)
            .push_batch((0..=EVENT_CAPACITY).map(|_| StructuredEvent::new(FILE_ACTION_ADDED, "x")))
            .is_err());
        assert!(WatchCompletion::overflow().parse().is_err());
        assert!(WatchCompletion::zero_bytes().parse().is_err());
        assert!(WatchCompletion::notify_enum_dir().parse().is_err());
        assert!(WatchCompletion::malformed_rename().parse().is_err());
        let mut malformed_record = vec![0u8; 20];
        malformed_record[0..4].copy_from_slice(&16u32.to_le_bytes());
        malformed_record[4..8].copy_from_slice(&FILE_ACTION_ADDED.0.to_le_bytes());
        malformed_record[8..12].copy_from_slice(&8u32.to_le_bytes());
        assert!(parse_notifications(&malformed_record).is_err());
        let order = RefCell::new(Vec::new());
        assert!(shutdown_pending_io_with(
            true,
            || {
                order.borrow_mut().push("cancel");
                CancelOutcome::Requested
            },
            || {
                order.borrow_mut().push("wait");
                CompletionOutcome::Aborted
            },
            || order.borrow_mut().push("release"),
        ));
        assert_eq!(*order.borrow(), ["cancel", "wait", "release"]);

        let failed = RefCell::new(Vec::new());
        assert!(!shutdown_pending_io_with(
            true,
            || {
                failed.borrow_mut().push("cancel");
                CancelOutcome::Requested
            },
            || {
                failed.borrow_mut().push("wait");
                CompletionOutcome::Failed
            },
            || failed.borrow_mut().push("release"),
        ));
        assert_eq!(*failed.borrow(), ["cancel", "wait"]);

        let no_pending = RefCell::new(Vec::new());
        assert!(shutdown_pending_io_with(
            false,
            || {
                no_pending.borrow_mut().push("cancel");
                CancelOutcome::Failed
            },
            || {
                no_pending.borrow_mut().push("wait");
                CompletionOutcome::Failed
            },
            || no_pending.borrow_mut().push("release"),
        ));
        assert_eq!(*no_pending.borrow(), ["release"]);

        let cancel_not_found = RefCell::new(Vec::new());
        assert!(shutdown_pending_io_with(
            true,
            || {
                cancel_not_found.borrow_mut().push("not-found");
                CancelOutcome::NotFound
            },
            || {
                cancel_not_found.borrow_mut().push("complete");
                CompletionOutcome::Completed
            },
            || cancel_not_found.borrow_mut().push("release"),
        ));
        assert_eq!(
            *cancel_not_found.borrow(),
            ["not-found", "complete", "release"]
        );

        let cancel_failed = RefCell::new(Vec::new());
        assert!(shutdown_pending_io_with(
            true,
            || {
                cancel_failed.borrow_mut().push("failed");
                CancelOutcome::Failed
            },
            || {
                cancel_failed.borrow_mut().push("complete");
                CompletionOutcome::Completed
            },
            || cancel_failed.borrow_mut().push("release"),
        ));
        assert_eq!(*cancel_failed.borrow(), ["failed", "complete", "release"]);
    }

    #[test]
    fn pinned_path_rejects_reparse_type_volume_and_prefix_races() {
        let opened = RefCell::new(Vec::new());
        let identity = execution_identity();
        let handles = pin_indexed_path_components_with(
            &identity,
            r"docs\report.pdf",
            false,
            |component, expected_directory, share| {
                opened
                    .borrow_mut()
                    .push((component.to_owned(), expected_directory, share));
                Ok(component.to_owned())
            },
            |_, component, expected_directory| {
                Ok((
                    false,
                    expected_directory,
                    identity.clone(),
                    component.to_owned(),
                ))
            },
        )
        .unwrap();
        assert_eq!(handles.len(), 2);
        assert_eq!(opened.borrow().len(), 2);

        for (reparse, directory, volume, prefix) in [
            (true, false, r"\\?\Volume{PIN}\", r"docs\report.pdf"),
            (false, true, r"\\?\Volume{PIN}\", r"docs\report.pdf"),
            (false, false, r"\\?\Volume{OTHER}\", r"docs\report.pdf"),
            (false, false, r"\\?\Volume{PIN}\", r"other\report.pdf"),
        ] {
            let mut actual_identity = execution_identity();
            actual_identity.volume_guid_path = volume.to_owned();
            assert!(pin_indexed_path_components_with(
                &identity,
                r"docs\report.pdf",
                false,
                |component, _, _| Ok(component.to_owned()),
                |_, component, expected_directory| Ok((
                    reparse,
                    if component.ends_with("report.pdf") {
                        directory
                    } else {
                        expected_directory
                    },
                    actual_identity.clone(),
                    if component.ends_with("report.pdf") {
                        prefix.to_owned()
                    } else {
                        component.to_owned()
                    },
                )),
            )
            .is_err());
        }
    }

    #[test]
    fn pinned_path_allows_existing_shared_writers() {
        let shares = RefCell::new(Vec::new());
        let identity = execution_identity();
        assert!(pin_indexed_path_components_with(
            &identity,
            r"docs\report.pdf",
            false,
            |component, _, share| {
                shares.borrow_mut().push(share);
                Ok(component.to_owned())
            },
            |_, component, expected_directory| Ok((
                false,
                expected_directory,
                identity.clone(),
                component.to_owned(),
            )),
        )
        .is_ok());
        assert!(shares
            .borrow()
            .iter()
            .all(|share| share.allows_write() && !share.allows_delete()));
    }

    #[test]
    fn directory_shell_execute_ex_uses_null_verb_args_and_cwd() {
        let calls = Cell::new(0);
        assert!(
            directory_shell_execute_ex_with(r"\\?\Volume{PIN}\docs", |call| {
                calls.set(calls.get() + 1);
                assert_eq!(call.path(), r"\\?\Volume{PIN}\docs");
                assert!(call.verb().is_none());
                assert!(call.parameters().is_none());
                assert!(call.directory().is_none());
                assert!(call.no_ui());
                assert!(call.show_normal());
                true
            },)
            .is_ok()
        );
        assert_eq!(calls.get(), 1);
    }
}
