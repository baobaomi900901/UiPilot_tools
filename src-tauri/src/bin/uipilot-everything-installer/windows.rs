#![allow(dead_code)]

use std::ffi::{c_void, OsStr};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use sha2::{Digest, Sha256};

use crate::model::{
    embedded_lock, RuntimeManifest, ACTIVITY_EVENT_NAME, MANAGED_RELATIVE_ROOT, MUTEX_NAME,
    SERVICE_NAME,
};

type Handle = *mut c_void;
type ScHandle = *mut c_void;
type Dword = u32;
type Bool = i32;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const FALSE: Bool = 0;
const ERROR_INSUFFICIENT_BUFFER: Dword = 122;
const ERROR_SERVICE_DOES_NOT_EXIST: Dword = 1060;
const WAIT_OBJECT_0: Dword = 0;
const WAIT_ABANDONED: Dword = 0x80;
const WAIT_TIMEOUT: Dword = 0x102;
const SYNCHRONIZE: Dword = 0x0010_0000;
const READ_CONTROL: Dword = 0x0002_0000;
const PROCESS_QUERY_LIMITED_INFORMATION: Dword = 0x1000;
const TOKEN_QUERY: Dword = 0x0008;
const FILE_READ_DATA: Dword = 0x0001;
const FILE_READ_ATTRIBUTES: Dword = 0x0080;
const GENERIC_READ_WRITE: Dword = 0xC000_0000;
const FILE_SHARE_READ: Dword = 0x0000_0001;
const CREATE_NEW: Dword = 1;
const OPEN_EXISTING: Dword = 3;
const FILE_ATTRIBUTE_NORMAL: Dword = 0x80;
const FILE_FLAG_BACKUP_SEMANTICS: Dword = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: Dword = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: Dword = 0x400;
const MOVEFILE_REPLACE_EXISTING: Dword = 0x1;
const MOVEFILE_WRITE_THROUGH: Dword = 0x8;
const TOKEN_USER_CLASS: Dword = 1;
const TOKEN_GROUPS_CLASS: Dword = 2;
const TOKEN_ELEVATION_CLASS: Dword = 20;
const TOKEN_INTEGRITY_LEVEL_CLASS: Dword = 25;
const SE_GROUP_ENABLED: Dword = 0x0000_0004;
const SECURITY_MANDATORY_HIGH_RID: u32 = 0x0000_3000;
const SC_MANAGER_CONNECT: Dword = 0x0001;
const SERVICE_QUERY_CONFIG: Dword = 0x0001;
const SERVICE_QUERY_STATUS: Dword = 0x0004;
const SC_STATUS_PROCESS_INFO: Dword = 0;
const SERVICE_STOPPED: Dword = 1;
const SERVICE_RUNNING: Dword = 4;
const OWNER_SECURITY_INFORMATION: Dword = 0x0000_0001;
const GROUP_SECURITY_INFORMATION: Dword = 0x0000_0002;
const DACL_SECURITY_INFORMATION: Dword = 0x0000_0004;
const SDDL_REVISION_1: Dword = 1;
const SE_KERNEL_OBJECT: Dword = 6;
const SE_FILE_OBJECT: Dword = 1;
const FOLDERID_LOCAL_APP_DATA: Guid = Guid::new(
    0xF1B32785,
    0x6FBA,
    0x4FCF,
    [0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91],
);
const FOLDERID_PROGRAM_DATA: Guid = Guid::new(
    0x62AB5D82,
    0xFDC1,
    0x4DC3,
    [0xA9, 0xDD, 0x07, 0x0D, 0x1D, 0x49, 0x5D, 0x97],
);

pub const STAGING_DACL_SDDL: &str = "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)";
pub const STAGING_FILE_DACL_SDDL: &str = "D:AI(A;ID;GA;;;SY)(A;ID;GA;;;BA)";
pub const NAMED_OBJECT_SDDL: &str = "O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)";
pub const MANAGED_ROOT_SDDL: &str = "O:BAG:BAD:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    InvalidInvocation = 10,
    AdmissionDenied = 11,
    Busy = 12,
    ForeignState = 13,
    ValidationFailed = 14,
    RecoveryRequired = 15,
    RollbackFailed = 16,
    NativeFailure = 17,
}

impl ExitCode {
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::Success,
            Self::InvalidInvocation,
            Self::AdmissionDenied,
            Self::Busy,
            Self::ForeignState,
            Self::ValidationFailed,
            Self::RecoveryRequired,
            Self::RollbackFailed,
            Self::NativeFailure,
        ]
        .into_iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionCheck {
    AsInvokerImage,
    ElevatedHighIntegrityAdmin,
    StagingCanonicalOwnerDacl,
    DirectNonReparseFiles,
    ParentProcessIdentity,
    TrustedActivityEvent,
    MutexHeldByParent,
    ReadRequest,
}

pub const ADMISSION_ORDER: [AdmissionCheck; 8] = [
    AdmissionCheck::AsInvokerImage,
    AdmissionCheck::ElevatedHighIntegrityAdmin,
    AdmissionCheck::StagingCanonicalOwnerDacl,
    AdmissionCheck::DirectNonReparseFiles,
    AdmissionCheck::ParentProcessIdentity,
    AdmissionCheck::TrustedActivityEvent,
    AdmissionCheck::MutexHeldByParent,
    AdmissionCheck::ReadRequest,
];

pub fn position(check: AdmissionCheck) -> usize {
    ADMISSION_ORDER
        .iter()
        .position(|candidate| *candidate == check)
        .unwrap()
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }
}

#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
struct TokenElevation {
    elevated: Dword,
}

#[repr(C)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: Dword,
}

#[repr(C)]
struct TokenMandatoryLabel {
    label: SidAndAttributes,
}

#[repr(C)]
struct TokenUser {
    user: SidAndAttributes,
}

#[repr(C)]
struct TokenGroups {
    group_count: Dword,
    groups: [SidAndAttributes; 1],
}

#[repr(C)]
struct FileAttributeTagInfo {
    file_attributes: Dword,
    reparse_tag: Dword,
}

#[repr(C)]
struct QueryServiceConfig {
    service_type: Dword,
    start_type: Dword,
    error_control: Dword,
    binary_path_name: *mut u16,
    load_order_group: *mut u16,
    tag_id: Dword,
    dependencies: *mut u16,
    service_start_name: *mut u16,
    display_name: *mut u16,
}

#[repr(C)]
struct ServiceStatusProcess {
    service_type: Dword,
    current_state: Dword,
    controls_accepted: Dword,
    win32_exit_code: Dword,
    service_specific_exit_code: Dword,
    check_point: Dword,
    wait_hint: Dword,
    process_id: Dword,
    service_flags: Dword,
}

#[repr(C)]
struct SecurityAttributes {
    length: Dword,
    security_descriptor: *mut c_void,
    inherit_handle: Bool,
}

#[repr(C)]
struct WintrustFileInfo {
    size: Dword,
    file_path: *const u16,
    file: Handle,
    known_subject: *const Guid,
}

#[repr(C)]
struct WintrustData {
    size: Dword,
    policy_callback_data: *mut c_void,
    sip_client_data: *mut c_void,
    ui_choice: Dword,
    revocation_checks: Dword,
    union_choice: Dword,
    file_info: *mut WintrustFileInfo,
    state_action: Dword,
    state_data: Handle,
    url_reference: *const u16,
    provider_flags: Dword,
    ui_context: Dword,
    signature_settings: *mut c_void,
}

#[repr(C)]
struct CryptIntegerBlob {
    size: Dword,
    data: *mut u8,
}

#[repr(C)]
struct CryptAlgorithmIdentifier {
    object_id: *mut i8,
    parameters: CryptIntegerBlob,
}

#[repr(C)]
struct CertInfo {
    version: Dword,
    serial_number: CryptIntegerBlob,
    signature_algorithm: CryptAlgorithmIdentifier,
    issuer: CryptIntegerBlob,
    not_before: FileTime,
    not_after: FileTime,
    subject: CryptIntegerBlob,
    subject_public_key_info: [usize; 4],
    issuer_unique_id: CryptIntegerBlob,
    subject_unique_id: CryptIntegerBlob,
    extension_count: Dword,
    extensions: *mut c_void,
}

#[repr(C)]
struct CertContext {
    encoding: Dword,
    encoded: *const u8,
    encoded_size: Dword,
    info: *const CertInfo,
    store: Handle,
}

#[repr(C)]
struct VsFixedFileInfo {
    signature: Dword,
    struct_version: Dword,
    file_version_ms: Dword,
    file_version_ls: Dword,
    product_version_ms: Dword,
    product_version_ls: Dword,
    file_flags_mask: Dword,
    file_flags: Dword,
    file_os: Dword,
    file_type: Dword,
    file_subtype: Dword,
    file_date_ms: Dword,
    file_date_ls: Dword,
}

#[link(name = "kernel32")]
extern "system" {
    fn CloseHandle(handle: Handle) -> Bool;
    fn GetCurrentProcess() -> Handle;
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn FindResourceW(module: Handle, name: *const u16, resource_type: *const u16) -> Handle;
    fn LoadResource(module: Handle, resource: Handle) -> Handle;
    fn LockResource(resource: Handle) -> *mut c_void;
    fn SizeofResource(module: Handle, resource: Handle) -> Dword;
    fn OpenProcess(access: Dword, inherit: Bool, pid: Dword) -> Handle;
    fn GetProcessTimes(
        process: Handle,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> Bool;
    fn ProcessIdToSessionId(pid: Dword, session: *mut Dword) -> Bool;
    fn WaitForSingleObject(handle: Handle, milliseconds: Dword) -> Dword;
    fn OpenMutexW(access: Dword, inherit: Bool, name: *const u16) -> Handle;
    fn ReleaseMutex(handle: Handle) -> Bool;
    fn OpenEventW(access: Dword, inherit: Bool, name: *const u16) -> Handle;
    fn CreateFileW(
        name: *const u16,
        access: Dword,
        share: Dword,
        security: *const SecurityAttributes,
        disposition: Dword,
        flags: Dword,
        template: Handle,
    ) -> Handle;
    fn GetFileInformationByHandleEx(
        file: Handle,
        class: Dword,
        info: *mut c_void,
        size: Dword,
    ) -> Bool;
    fn GetFinalPathNameByHandleW(file: Handle, path: *mut u16, size: Dword, flags: Dword) -> Dword;
    fn CreateDirectoryW(path: *const u16, security: *const SecurityAttributes) -> Bool;
    fn FlushFileBuffers(file: Handle) -> Bool;
    fn GetLastError() -> Dword;
    fn LocalFree(memory: Handle) -> Handle;
    fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: Dword) -> Bool;
}

#[link(name = "wintrust")]
extern "system" {
    fn WinVerifyTrust(window: Handle, action: *const Guid, data: *mut WintrustData) -> i32;
}

#[link(name = "crypt32")]
extern "system" {
    fn CryptQueryObject(
        object_type: Dword,
        object: *const c_void,
        expected_content: Dword,
        expected_format: Dword,
        flags: Dword,
        encoding: *mut Dword,
        content: *mut Dword,
        format: *mut Dword,
        store: *mut Handle,
        message: *mut Handle,
        context: *mut *const c_void,
    ) -> Bool;
    fn CryptMsgGetParam(
        message: Handle,
        parameter: Dword,
        index: Dword,
        data: *mut c_void,
        size: *mut Dword,
    ) -> Bool;
    fn CertFindCertificateInStore(
        store: Handle,
        encoding: Dword,
        flags: Dword,
        find_type: Dword,
        find_parameter: *const c_void,
        previous: *const CertContext,
    ) -> *const CertContext;
    fn CertStrToNameW(
        encoding: Dword,
        string: *const u16,
        string_type: Dword,
        reserved: *mut c_void,
        encoded: *mut u8,
        encoded_size: *mut Dword,
        error: *mut *const u16,
    ) -> Bool;
    fn CertFreeCertificateContext(context: *const CertContext) -> Bool;
    fn CertCloseStore(store: Handle, flags: Dword) -> Bool;
    fn CryptMsgClose(message: Handle) -> Bool;
}

#[link(name = "version")]
extern "system" {
    fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut Dword) -> Dword;
    fn GetFileVersionInfoW(
        path: *const u16,
        handle: Dword,
        length: Dword,
        data: *mut c_void,
    ) -> Bool;
    fn VerQueryValueW(
        block: *const c_void,
        sub_block: *const u16,
        buffer: *mut *mut c_void,
        length: *mut u32,
    ) -> Bool;
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(process: Handle, access: Dword, token: *mut Handle) -> Bool;
    fn GetTokenInformation(
        token: Handle,
        class: Dword,
        information: *mut c_void,
        length: Dword,
        return_length: *mut Dword,
    ) -> Bool;
    fn ConvertSidToStringSidW(sid: *mut c_void, string_sid: *mut *mut u16) -> Bool;
    fn ConvertStringSidToSidW(string_sid: *const u16, sid: *mut *mut c_void) -> Bool;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: Dword,
        descriptor: *mut *mut c_void,
        size: *mut Dword,
    ) -> Bool;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
        descriptor: *mut c_void,
        revision: Dword,
        information: Dword,
        sddl: *mut *mut u16,
        length: *mut Dword,
    ) -> Bool;
    fn GetSecurityInfo(
        handle: Handle,
        object_type: Dword,
        information: Dword,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> Dword;
    fn GetNamedSecurityInfoW(
        name: *mut u16,
        object_type: Dword,
        information: Dword,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> Dword;
    fn GetSidSubAuthorityCount(sid: *mut c_void) -> *mut u8;
    fn GetSidSubAuthority(sid: *mut c_void, index: Dword) -> *mut Dword;
    fn EqualSid(first: *mut c_void, second: *mut c_void) -> Bool;
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: Dword) -> ScHandle;
    fn OpenServiceW(manager: ScHandle, name: *const u16, access: Dword) -> ScHandle;
    fn QueryServiceConfigW(
        service: ScHandle,
        config: *mut QueryServiceConfig,
        size: Dword,
        needed: *mut Dword,
    ) -> Bool;
    fn QueryServiceStatusEx(
        service: ScHandle,
        level: Dword,
        buffer: *mut u8,
        size: Dword,
        needed: *mut Dword,
    ) -> Bool;
    fn CloseServiceHandle(handle: ScHandle) -> Bool;
}

#[link(name = "user32")]
extern "system" {
    fn GetShellWindow() -> Handle;
    fn GetWindowThreadProcessId(window: Handle, pid: *mut Dword) -> Dword;
}

#[link(name = "shell32")]
extern "system" {
    fn SHGetKnownFolderPath(
        folder: *const Guid,
        flags: Dword,
        token: Handle,
        path: *mut *mut u16,
    ) -> i32;
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct ServiceHandle(ScHandle);
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn manifest_text(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words).ok()
    } else {
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    }
}

fn xml_attribute<'a>(tag: &'a str, wanted: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut index = tag.find(char::is_whitespace)?;
    let mut found = None;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || matches!(bytes[index], b'/' | b'>') {
            break;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        let name = &tag[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            return None;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || !matches!(bytes[index], b'\'' | b'"') {
            return None;
        }
        let quote = bytes[index];
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let value = &tag[value_start..index];
        index += 1;
        if name.eq_ignore_ascii_case(wanted) {
            if found.replace(value).is_some() {
                return None;
            }
        }
    }
    found
}

pub fn embedded_manifest_is_as_invoker(bytes: &[u8]) -> bool {
    let Some(text) = manifest_text(bytes) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    let needle = "<requestedexecutionlevel";
    let Some(start) = lower.find(needle) else {
        return false;
    };
    if lower[start + needle.len()..].contains(needle) {
        return false;
    }
    let Some(relative_end) = lower[start..].find('>') else {
        return false;
    };
    let tag = &text[start + 1..start + relative_end];
    xml_attribute(tag, "level") == Some("asInvoker")
        && xml_attribute(tag, "uiAccess") == Some("false")
}

pub fn verify_current_image_as_invoker() -> Result<(), String> {
    const MANIFEST_RESOURCE_ID: usize = 1;
    const RT_MANIFEST: usize = 24;
    let module = unsafe { GetModuleHandleW(null()) };
    if module.is_null() {
        return Err("image-module-unavailable".into());
    }
    let resource = unsafe {
        FindResourceW(
            module,
            MANIFEST_RESOURCE_ID as *const u16,
            RT_MANIFEST as *const u16,
        )
    };
    if resource.is_null() {
        return Err("image-manifest-missing".into());
    }
    let size = unsafe { SizeofResource(module, resource) };
    let loaded = unsafe { LoadResource(module, resource) };
    if size == 0 || loaded.is_null() {
        return Err("image-manifest-load-failed".into());
    }
    let bytes = unsafe { LockResource(loaded) }.cast::<u8>();
    if bytes.is_null() {
        return Err("image-manifest-lock-failed".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, size as usize) };
    embedded_manifest_is_as_invoker(bytes)
        .then_some(())
        .ok_or_else(|| "image-manifest-not-as-invoker".into())
}

unsafe fn wide_ptr_to_string(mut value: *const u16) -> String {
    let start = value;
    while *value != 0 {
        value = value.add(1);
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(
        start,
        value.offset_from(start) as usize,
    ))
}

fn known_folder(folder: &Guid) -> Result<PathBuf, String> {
    let mut value = null_mut();
    let result = unsafe { SHGetKnownFolderPath(folder, 0, null_mut(), &mut value) };
    if result < 0 || value.is_null() {
        return Err("known-folder-unavailable".into());
    }
    let path = unsafe { wide_ptr_to_string(value) };
    unsafe {
        LocalFree(value.cast());
    }
    Ok(PathBuf::from(path))
}

pub fn local_app_data() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_LOCAL_APP_DATA)
}

pub fn managed_root() -> Result<PathBuf, String> {
    Ok(known_folder(&FOLDERID_PROGRAM_DATA)?.join(MANAGED_RELATIVE_ROOT))
}

fn token_information(token: Handle, class: Dword) -> Result<Vec<u8>, String> {
    let mut length = 0;
    unsafe {
        GetTokenInformation(token, class, null_mut(), 0, &mut length);
    }
    if length == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err("token-query-size-failed".into());
    }
    let mut buffer = vec![0u8; length as usize];
    if unsafe {
        GetTokenInformation(
            token,
            class,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == FALSE
    {
        return Err("token-query-failed".into());
    }
    Ok(buffer)
}

fn current_process_token() -> Result<OwnedHandle, String> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == FALSE {
        return Err("token-open-failed".into());
    }
    Ok(OwnedHandle(token))
}

fn enabled_ba(token: Handle) -> Result<bool, String> {
    let ba_text = wide("S-1-5-32-544");
    let mut ba = null_mut();
    if unsafe { ConvertStringSidToSidW(ba_text.as_ptr(), &mut ba) } == FALSE {
        return Err("ba-sid-failed".into());
    }
    let groups = token_information(token, TOKEN_GROUPS_CLASS)?;
    let header = groups.as_ptr().cast::<TokenGroups>();
    let count = unsafe { (*header).group_count } as usize;
    let first = unsafe { &(*header).groups as *const SidAndAttributes };
    let found = (0..count).any(|index| {
        let group = unsafe { &*first.add(index) };
        group.attributes & SE_GROUP_ENABLED != 0 && unsafe { EqualSid(group.sid, ba) } != FALSE
    });
    unsafe {
        LocalFree(ba.cast());
    }
    Ok(found)
}

pub fn verify_elevated_high_integrity_admin() -> Result<(), String> {
    let token = current_process_token()?;
    let elevation = token_information(token.0, TOKEN_ELEVATION_CLASS)?;
    if unsafe { (*(elevation.as_ptr().cast::<TokenElevation>())).elevated } == 0
        || !enabled_ba(token.0)?
    {
        return Err("not-fully-elevated-admin".into());
    }
    let integrity = token_information(token.0, TOKEN_INTEGRITY_LEVEL_CLASS)?;
    let sid = unsafe {
        (*(integrity.as_ptr().cast::<TokenMandatoryLabel>()))
            .label
            .sid
    };
    let count = unsafe { *GetSidSubAuthorityCount(sid) } as u32;
    if count == 0 {
        return Err("missing-integrity-rid".into());
    }
    let rid = unsafe { *GetSidSubAuthority(sid, count - 1) };
    if rid < SECURITY_MANDATORY_HIGH_RID {
        return Err("integrity-below-high".into());
    }
    Ok(())
}

fn process_creation_time(process: Handle) -> Result<u64, String> {
    let mut creation = FileTime { low: 0, high: 0 };
    let mut exit = FileTime { low: 0, high: 0 };
    let mut kernel = FileTime { low: 0, high: 0 };
    let mut user = FileTime { low: 0, high: 0 };
    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
        == FALSE
    {
        return Err("parent-time-query-failed".into());
    }
    Ok(((creation.high as u64) << 32) | creation.low as u64)
}

pub struct AdmissionLease {
    parent: OwnedHandle,
    _activity: OwnedHandle,
    _mutex: OwnedHandle,
}

impl AdmissionLease {
    pub fn ensure_parent_alive(&self) -> Result<(), String> {
        if unsafe { WaitForSingleObject(self.parent.0, 0) } == WAIT_TIMEOUT {
            Ok(())
        } else {
            Err("parent-setup-exited".into())
        }
    }
}

pub fn parent_identity_is_current(
    actual_creation: u64,
    expected_creation: u64,
    wait_result: Dword,
) -> bool {
    actual_creation == expected_creation && wait_result == WAIT_TIMEOUT
}

fn descriptor_sddl_for(handle: Handle, object_type: Dword) -> Result<String, String> {
    let mut descriptor = null_mut();
    let information =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let status = unsafe {
        GetSecurityInfo(
            handle,
            object_type,
            information,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || descriptor.is_null() {
        return Err("security-info-failed".into());
    }
    let mut string = null_mut();
    let ok = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            information,
            &mut string,
            null_mut(),
        )
    };
    let result = if ok == FALSE || string.is_null() {
        Err("security-descriptor-format-failed".into())
    } else {
        Ok(unsafe { wide_ptr_to_string(string) })
    };
    unsafe {
        if !string.is_null() {
            LocalFree(string.cast());
        }
        LocalFree(descriptor.cast());
    }
    result
}

fn verify_named_object_descriptor(handle: Handle) -> Result<(), String> {
    let actual = descriptor_sddl_for(handle, SE_KERNEL_OBJECT)?;
    if actual != NAMED_OBJECT_SDDL {
        return Err("named-object-descriptor-mismatch".into());
    }
    Ok(())
}

pub fn verify_managed_directory(path: &Path) -> Result<(), String> {
    let directory = open_direct(path, FILE_READ_ATTRIBUTES | READ_CONTROL, FILE_SHARE_READ)?;
    if final_path(directory.0)?
        != fs::canonicalize(path).map_err(|_| "managed-directory-canonicalize-failed")?
    {
        return Err("managed-directory-identity-mismatch".into());
    }
    if descriptor_sddl_for(directory.0, SE_FILE_OBJECT)? != MANAGED_ROOT_SDDL {
        return Err("managed-directory-owner-dacl-mismatch".into());
    }
    Ok(())
}

fn canonical_program_data() -> Result<PathBuf, String> {
    fs::canonicalize(known_folder(&FOLDERID_PROGRAM_DATA)?)
        .map_err(|_| "program-data-canonicalize-failed".into())
}

pub fn verify_managed_parent(path: &Path) -> Result<(), String> {
    let directory = open_direct(path, FILE_READ_ATTRIBUTES | READ_CONTROL, FILE_SHARE_READ)?;
    let expected = canonical_program_data()?.join("UiPilot_tools");
    if final_path(directory.0)? != expected {
        return Err("managed-parent-reparse-or-location-mismatch".into());
    }
    let descriptor = descriptor_sddl_for(directory.0, SE_FILE_OBJECT)?;
    if !managed_parent_descriptor_is_exact(&descriptor) {
        return Err("managed-parent-owner-dacl-mismatch".into());
    }
    Ok(())
}

pub fn managed_parent_descriptor_is_exact(descriptor: &str) -> bool {
    descriptor == MANAGED_ROOT_SDDL
}

pub fn verify_managed_root(path: &Path) -> Result<(), String> {
    let directory = open_direct(path, FILE_READ_ATTRIBUTES | READ_CONTROL, FILE_SHARE_READ)?;
    let expected = canonical_program_data()?.join(MANAGED_RELATIVE_ROOT);
    if final_path(directory.0)? != expected {
        return Err("managed-root-reparse-or-location-mismatch".into());
    }
    if descriptor_sddl_for(directory.0, SE_FILE_OBJECT)? != MANAGED_ROOT_SDDL {
        return Err("managed-root-owner-dacl-mismatch".into());
    }
    Ok(())
}

pub fn verify_managed_file(path: &Path) -> Result<(), String> {
    let file = open_direct(path, FILE_READ_ATTRIBUTES | READ_CONTROL, FILE_SHARE_READ)?;
    if final_path(file.0)?
        != fs::canonicalize(path).map_err(|_| "managed-file-canonicalize-failed")?
    {
        return Err("managed-file-identity-mismatch".into());
    }
    if descriptor_sddl_for(file.0, SE_FILE_OBJECT)? != MANAGED_ROOT_SDDL {
        return Err("managed-file-owner-dacl-mismatch".into());
    }
    Ok(())
}

pub fn acquire_admission_lease(
    parent_pid: u32,
    expected_creation: u64,
) -> Result<AdmissionLease, String> {
    verify_elevated_high_integrity_admin()?;
    let parent = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            FALSE,
            parent_pid,
        )
    };
    if parent.is_null() || parent == INVALID_HANDLE_VALUE {
        return Err("parent-open-failed".into());
    }
    let parent = OwnedHandle(parent);
    if !parent_identity_is_current(
        process_creation_time(parent.0)?,
        expected_creation,
        unsafe { WaitForSingleObject(parent.0, 0) },
    ) {
        return Err("parent-identity-mismatch".into());
    }
    let event_name = wide(ACTIVITY_EVENT_NAME);
    let activity = unsafe { OpenEventW(SYNCHRONIZE | READ_CONTROL, FALSE, event_name.as_ptr()) };
    if activity.is_null() {
        return Err("activity-event-missing".into());
    }
    let activity = OwnedHandle(activity);
    verify_named_object_descriptor(activity.0)?;
    let mutex_name = wide(MUTEX_NAME);
    let mutex = unsafe { OpenMutexW(SYNCHRONIZE | READ_CONTROL, FALSE, mutex_name.as_ptr()) };
    if mutex.is_null() {
        return Err("installer-mutex-missing".into());
    }
    let mutex = OwnedHandle(mutex);
    verify_named_object_descriptor(mutex.0)?;
    match unsafe { WaitForSingleObject(mutex.0, 0) } {
        WAIT_TIMEOUT => {}
        WAIT_OBJECT_0 | WAIT_ABANDONED => {
            unsafe {
                ReleaseMutex(mutex.0);
            }
            return Err("installer-mutex-not-owned-by-parent".into());
        }
        _ => return Err("installer-mutex-wait-failed".into()),
    }
    Ok(AdmissionLease {
        parent,
        _activity: activity,
        _mutex: mutex,
    })
}

fn final_path(handle: Handle) -> Result<PathBuf, String> {
    let size = unsafe { GetFinalPathNameByHandleW(handle, null_mut(), 0, 0) };
    if size == 0 {
        return Err("final-path-size-failed".into());
    }
    let mut buffer = vec![0u16; size as usize + 1];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err("final-path-query-failed".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..written as usize],
    )))
}

pub fn verify_staging_layout(
    staging: &Path,
    executable: &Path,
    request: &Path,
    journal: &Path,
) -> Result<(), String> {
    let expected_parent = local_app_data()?.join("Temp");
    let canonical_parent =
        fs::canonicalize(expected_parent).map_err(|_| "temp-root-canonicalize-failed")?;
    let canonical_staging = fs::canonicalize(staging).map_err(|_| "staging-canonicalize-failed")?;
    if canonical_staging.parent() != Some(canonical_parent.as_path()) {
        return Err("staging-outside-elevated-temp".into());
    }
    let directory = open_direct(
        &canonical_staging,
        FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_SHARE_READ,
    )?;
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            directory.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || owner.is_null() || descriptor.is_null() {
        return Err("staging-security-query-failed".into());
    }
    let token = current_process_token()?;
    let user = token_information(token.0, TOKEN_USER_CLASS)?;
    let token_sid = unsafe { (*(user.as_ptr().cast::<TokenUser>())).user.sid };
    let owner_matches = unsafe { EqualSid(owner, token_sid) } != FALSE;
    let mut string = null_mut();
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut string,
            null_mut(),
        )
    };
    let dacl = if converted != FALSE && !string.is_null() {
        Some(unsafe { wide_ptr_to_string(string) })
    } else {
        None
    };
    unsafe {
        if !string.is_null() {
            LocalFree(string.cast());
        }
        LocalFree(descriptor.cast());
    }
    if !owner_matches || dacl.as_deref() != Some(STAGING_DACL_SDDL) {
        return Err("staging-owner-dacl-mismatch".into());
    }
    for path in [executable, request, journal] {
        if path.parent() != Some(staging) {
            return Err("staging-file-not-direct-child".into());
        }
        let file = open_direct(path, FILE_READ_ATTRIBUTES | READ_CONTROL, FILE_SHARE_READ)?;
        if final_path(file.0)?
            != fs::canonicalize(path).map_err(|_| "staging-file-canonicalize-failed")?
        {
            return Err("staging-file-identity-mismatch".into());
        }
        verify_file_owner_and_acl(file.0)?;
    }
    Ok(())
}

fn verify_file_owner_and_acl(handle: Handle) -> Result<(), String> {
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || owner.is_null() || descriptor.is_null() {
        return Err("staging-file-security-query-failed".into());
    }
    let token = current_process_token()?;
    let user = token_information(token.0, TOKEN_USER_CLASS)?;
    let token_sid = unsafe { (*(user.as_ptr().cast::<TokenUser>())).user.sid };
    let owner_matches = unsafe { EqualSid(owner, token_sid) } != FALSE;
    let mut string = null_mut();
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut string,
            null_mut(),
        )
    };
    let dacl = if converted != FALSE && !string.is_null() {
        unsafe { wide_ptr_to_string(string) }
    } else {
        String::new()
    };
    unsafe {
        if !string.is_null() {
            LocalFree(string.cast());
        }
        LocalFree(descriptor.cast());
    }
    if !owner_matches || !staging_file_dacl_is_exact(&dacl) {
        return Err("staging-file-owner-dacl-mismatch".into());
    }
    Ok(())
}

pub fn staging_file_dacl_is_exact(dacl: &str) -> bool {
    dacl == STAGING_FILE_DACL_SDDL
}

pub fn atomic_replace(existing: &Path, replacement: &Path) -> Result<(), String> {
    let source = wide(existing.as_os_str());
    let target = wide(replacement.as_os_str());
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == FALSE
    {
        return Err("atomic-replace-failed".into());
    }
    Ok(())
}

pub fn create_protected_file(path: &Path) -> Result<File, String> {
    let sddl_wide = wide(MANAGED_ROOT_SDDL);
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == FALSE
    {
        return Err("protected-file-descriptor-parse-failed".into());
    }
    let attributes = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor,
        inherit_handle: FALSE,
    };
    let path_wide = wide(path.as_os_str());
    let handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            GENERIC_READ_WRITE,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    if handle == INVALID_HANDLE_VALUE {
        return Err("protected-file-create-failed".into());
    }
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

fn open_direct(path: &Path, access: Dword, share: Dword) -> Result<OwnedHandle, String> {
    let name = wide(path.as_os_str());
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("direct-file-open-failed".into());
    }
    let handle = OwnedHandle(handle);
    let mut info = FileAttributeTagInfo {
        file_attributes: 0,
        reparse_tag: 0,
    };
    if unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            9,
            (&mut info as *mut FileAttributeTagInfo).cast(),
            std::mem::size_of::<FileAttributeTagInfo>() as u32,
        )
    } == FALSE
        || info.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err("reparse-file-rejected".into());
    }
    Ok(handle)
}

pub fn read_direct_file(path: &Path) -> Result<Vec<u8>, String> {
    let handle = open_direct(path, FILE_READ_DATA | FILE_READ_ATTRIBUTES, FILE_SHARE_READ)?;
    let raw = handle.0;
    std::mem::forget(handle);
    let mut file = unsafe { File::from_raw_handle(raw as RawHandle) };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| "direct-file-read-failed")?;
    Ok(bytes)
}

pub struct HeldSource {
    file: File,
    path: PathBuf,
}

pub fn held_source_share_mode() -> Dword {
    FILE_SHARE_READ
}

impl HeldSource {
    fn open(source: &Path) -> Result<Self, String> {
        let handle = open_direct(
            source,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES,
            held_source_share_mode(),
        )?;
        if final_path(handle.0)?
            != fs::canonicalize(source).map_err(|_| "source-canonicalize-failed")?
        {
            return Err("source-file-identity-mismatch".into());
        }
        let raw = handle.0;
        std::mem::forget(handle);
        Ok(Self {
            file: unsafe { File::from_raw_handle(raw as RawHandle) },
            path: source.to_path_buf(),
        })
    }

    fn sha256(&mut self) -> Result<String, String> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| "source-seek-failed")?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = self
                .file
                .read(&mut buffer)
                .map_err(|_| "source-read-failed")?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        Ok(format!("{:x}", hash.finalize()))
    }
}

pub fn copy_from_held_source(
    source: &mut HeldSource,
    destination: &Path,
) -> Result<String, String> {
    source
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "source-seek-failed")?;
    let mut output =
        create_protected_file(destination).map_err(|_| "protected-destination-create-failed")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source
            .file
            .read(&mut buffer)
            .map_err(|_| "source-read-failed")?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|_| "destination-write-failed")?;
    }
    output.flush().map_err(|_| "destination-flush-failed")?;
    output.sync_all().map_err(|_| "destination-sync-failed")?;
    Ok(format!("{:x}", hash.finalize()))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut input = File::open(path).map_err(|_| "hash-open-failed")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|_| "hash-read-failed")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn validate_bundle_sources(
    install_dir: &Path,
) -> Result<(HeldSource, crate::model::ReviewedLock), String> {
    let root = fs::canonicalize(install_dir).map_err(|_| "install-root-canonicalize-failed")?;
    let resource = root.join(r"resources\everything");
    let executable = resource.join("Everything.exe");
    let license = resource.join("LICENSE.txt");
    let lock = resource.join("everything.lock.json");
    let ini = resource.join("Everything-UiPilot.ini");
    if read_direct_file(&lock)? != crate::model::EMBEDDED_LOCK
        || read_direct_file(&license)? != crate::model::EMBEDDED_LICENSE
        || read_direct_file(&ini)? != crate::model::EMBEDDED_INI
    {
        return Err("embedded-resource-mismatch".into());
    }
    let reviewed = embedded_lock()?;
    let mut held = HeldSource::open(&executable)?;
    if reviewed.schema_version != 1
        || reviewed.architecture != "x64"
        || held.sha256()? != reviewed.everything_exe_sha256
    {
        return Err("everything-identity-mismatch".into());
    }
    verify_file_version(&held.path, &reviewed.version)?;
    verify_authenticode_publisher(&held.path, &reviewed.authenticode_publisher)?;
    Ok((held, reviewed))
}

pub fn validate_managed_binary(path: &Path, manifest: &RuntimeManifest) -> Result<(), String> {
    if sha256_file(path)? != manifest.sha256 {
        return Err("managed-binary-hash-mismatch".into());
    }
    verify_file_version(path, &manifest.version)?;
    verify_authenticode_publisher(path, &manifest.publisher)?;
    verify_managed_file(path)
}

pub fn verify_authenticode_publisher(path: &Path, expected: &str) -> Result<(), String> {
    win_verify_trust(path)?;
    signer_subject_matches(path, expected)
}

fn win_verify_trust(path: &Path) -> Result<(), String> {
    const ACTION: Guid = Guid::new(
        0x00AAC56B,
        0xCD44,
        0x11D0,
        [0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
    );
    let path = wide(path.as_os_str());
    let mut file = WintrustFileInfo {
        size: std::mem::size_of::<WintrustFileInfo>() as u32,
        file_path: path.as_ptr(),
        file: null_mut(),
        known_subject: null(),
    };
    let mut data = WintrustData {
        size: std::mem::size_of::<WintrustData>() as u32,
        policy_callback_data: null_mut(),
        sip_client_data: null_mut(),
        ui_choice: 2,
        revocation_checks: 1,
        union_choice: 1,
        file_info: &mut file,
        state_action: 0,
        state_data: null_mut(),
        url_reference: null(),
        provider_flags: 0x80,
        ui_context: 0,
        signature_settings: null_mut(),
    };
    let status = unsafe { WinVerifyTrust(INVALID_HANDLE_VALUE, &ACTION, &mut data) };
    if status != 0 {
        return Err("winverifytrust-failed".into());
    }
    Ok(())
}

fn signer_subject_matches(path: &Path, expected: &str) -> Result<(), String> {
    const X509_PKCS7_ENCODING: Dword = 0x0001_0001;
    const CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED: Dword = 1 << 10;
    const CERT_QUERY_FORMAT_FLAG_BINARY: Dword = 2;
    const CMSG_SIGNER_CERT_INFO_PARAM: Dword = 7;
    const CERT_FIND_SUBJECT_CERT: Dword = 0x000B_0000;
    const CERT_X500_NAME_STR: Dword = 3;
    let path = wide(path.as_os_str());
    let mut store = null_mut();
    let mut message = null_mut();
    let mut context = null();
    if unsafe {
        CryptQueryObject(
            1,
            path.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut store,
            &mut message,
            &mut context,
        )
    } == FALSE
    {
        return Err("signature-query-failed".into());
    }
    let result = (|| {
        let mut size = 0;
        unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_CERT_INFO_PARAM,
                0,
                null_mut(),
                &mut size,
            );
        }
        if size < std::mem::size_of::<CertInfo>() as u32 {
            return Err("signer-info-size-failed".into());
        }
        let mut signer = vec![0u8; size as usize];
        if unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_CERT_INFO_PARAM,
                0,
                signer.as_mut_ptr().cast(),
                &mut size,
            )
        } == FALSE
        {
            return Err("signer-info-failed".into());
        }
        let certificate = unsafe {
            CertFindCertificateInStore(
                store,
                X509_PKCS7_ENCODING,
                0,
                CERT_FIND_SUBJECT_CERT,
                signer.as_ptr().cast(),
                null(),
            )
        };
        if certificate.is_null() {
            return Err("signer-certificate-missing".into());
        }
        let result = (|| {
            let expected = wide(expected);
            let mut encoded_size = 0;
            unsafe {
                CertStrToNameW(
                    X509_PKCS7_ENCODING,
                    expected.as_ptr(),
                    CERT_X500_NAME_STR,
                    null_mut(),
                    null_mut(),
                    &mut encoded_size,
                    null_mut(),
                );
            }
            if encoded_size == 0 {
                return Err("expected-publisher-invalid".into());
            }
            let mut encoded = vec![0u8; encoded_size as usize];
            if unsafe {
                CertStrToNameW(
                    X509_PKCS7_ENCODING,
                    expected.as_ptr(),
                    CERT_X500_NAME_STR,
                    null_mut(),
                    encoded.as_mut_ptr(),
                    &mut encoded_size,
                    null_mut(),
                )
            } == FALSE
            {
                return Err("expected-publisher-invalid".into());
            }
            let subject = unsafe { &(*(*certificate).info).subject };
            let actual = unsafe { std::slice::from_raw_parts(subject.data, subject.size as usize) };
            (actual == &encoded[..encoded_size as usize])
                .then_some(())
                .ok_or_else(|| "authenticode-publisher-mismatch".into())
        })();
        unsafe {
            CertFreeCertificateContext(certificate);
        }
        result
    })();
    unsafe {
        if !message.is_null() {
            CryptMsgClose(message);
        }
        if !store.is_null() {
            CertCloseStore(store, 0);
        }
    }
    result
}

fn verify_file_version(path: &Path, expected: &str) -> Result<(), String> {
    let path = wide(path.as_os_str());
    let mut ignored = 0;
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
    if size == 0 {
        return Err("file-version-size-failed".into());
    }
    let mut data = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == FALSE {
        return Err("file-version-read-failed".into());
    }
    let root = wide("\\");
    let mut info = null_mut();
    let mut length = 0;
    if unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut info, &mut length) }
        == FALSE
        || length < std::mem::size_of::<VsFixedFileInfo>() as u32
    {
        return Err("file-version-query-failed".into());
    }
    let info = unsafe { &*(info.cast::<VsFixedFileInfo>()) };
    let actual = format!(
        "{}.{}.{}.{}",
        info.file_version_ms >> 16,
        info.file_version_ms & 0xffff,
        info.file_version_ls >> 16,
        info.file_version_ls & 0xffff
    );
    if actual != expected {
        return Err("file-version-mismatch".into());
    }
    Ok(())
}

pub fn create_protected_directory(path: &Path, sddl: &str) -> Result<(), String> {
    let sddl_wide = wide(sddl);
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == FALSE
    {
        return Err("security-descriptor-parse-failed".into());
    }
    let attributes = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor,
        inherit_handle: FALSE,
    };
    let path_wide = wide(path.as_os_str());
    let created = unsafe { CreateDirectoryW(path_wide.as_ptr(), &attributes) };
    unsafe {
        LocalFree(descriptor);
    }
    if created == FALSE {
        return Err("protected-directory-create-failed".into());
    }
    Ok(())
}

pub fn desktop_user_sid(parent_pid: u32) -> Result<String, String> {
    let shell = unsafe { GetShellWindow() };
    if shell.is_null() {
        return Err("same-session-explorer-missing".into());
    }
    let mut shell_pid = 0;
    unsafe {
        GetWindowThreadProcessId(shell, &mut shell_pid);
    }
    let mut parent_session = 0;
    let mut shell_session = 0;
    if shell_pid == 0
        || unsafe { ProcessIdToSessionId(parent_pid, &mut parent_session) } == FALSE
        || unsafe { ProcessIdToSessionId(shell_pid, &mut shell_session) } == FALSE
        || parent_session != shell_session
    {
        return Err("same-session-explorer-mismatch".into());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, shell_pid) };
    if process.is_null() {
        return Err("shell-process-open-failed".into());
    }
    let process = OwnedHandle(process);
    let mut token = null_mut();
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == FALSE {
        return Err("shell-token-open-failed".into());
    }
    let token = OwnedHandle(token);
    let user = token_information(token.0, TOKEN_USER_CLASS)?;
    let sid = unsafe { (*(user.as_ptr().cast::<TokenUser>())).user.sid };
    let mut sid_string = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut sid_string) } == FALSE {
        return Err("shell-sid-format-failed".into());
    }
    let result = unsafe { wide_ptr_to_string(sid_string) };
    unsafe {
        LocalFree(sid_string.cast());
    }
    Ok(result)
}

#[derive(Debug)]
pub enum ServiceState {
    Absent,
    Running(String),
    Stopped(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatusClass {
    Running,
    Stopped,
}

pub fn classify_service_status(state: Dword) -> Result<ServiceStatusClass, &'static str> {
    match state {
        SERVICE_RUNNING => Ok(ServiceStatusClass::Running),
        SERVICE_STOPPED => Ok(ServiceStatusClass::Stopped),
        _ => Err("service-state-transitional-or-unsupported"),
    }
}

pub fn query_service() -> Result<ServiceState, String> {
    let manager = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err("scm-open-failed".into());
    }
    let manager = ServiceHandle(manager);
    let name = wide(SERVICE_NAME);
    let service = unsafe {
        OpenServiceW(
            manager.0,
            name.as_ptr(),
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS,
        )
    };
    if service.is_null() {
        return if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST {
            Ok(ServiceState::Absent)
        } else {
            Err("service-open-failed".into())
        };
    }
    let service = ServiceHandle(service);
    let mut needed = 0;
    unsafe {
        QueryServiceConfigW(service.0, null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err("service-config-size-failed".into());
    }
    let mut config = vec![0u8; needed as usize];
    if unsafe { QueryServiceConfigW(service.0, config.as_mut_ptr().cast(), needed, &mut needed) }
        == FALSE
    {
        return Err("service-config-query-failed".into());
    }
    let command = unsafe {
        wide_ptr_to_string((*(config.as_ptr().cast::<QueryServiceConfig>())).binary_path_name)
    };
    let mut status = ServiceStatusProcess {
        service_type: 0,
        current_state: 0,
        controls_accepted: 0,
        win32_exit_code: 0,
        service_specific_exit_code: 0,
        check_point: 0,
        wait_hint: 0,
        process_id: 0,
        service_flags: 0,
    };
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut ServiceStatusProcess).cast(),
            std::mem::size_of::<ServiceStatusProcess>() as u32,
            &mut needed,
        )
    } == FALSE
    {
        return Err("service-status-query-failed".into());
    }
    match classify_service_status(status.current_state)? {
        ServiceStatusClass::Running => Ok(ServiceState::Running(command)),
        ServiceStatusClass::Stopped => Ok(ServiceState::Stopped(command)),
    }
}

pub fn invoke_service_command(
    executable: &Path,
    verb: &str,
    sddl: Option<&str>,
) -> Result<(), String> {
    let mut command = std::process::Command::new(executable);
    command
        .arg("-instance")
        .arg(crate::model::INSTANCE_NAME)
        .arg(verb);
    if verb == "-install-service" {
        let sddl = sddl.ok_or_else(|| "missing-service-sddl".to_string())?;
        command
            .arg("-install-service-pipe-name")
            .arg(crate::model::PIPE_NAME)
            .arg("-install-service-security-descriptor")
            .arg(sddl);
    } else if sddl.is_some() {
        return Err("unexpected-service-sddl".into());
    }
    let status = command
        .status()
        .map_err(|_| "everything-service-command-start-failed")?;
    if !status.success() {
        return Err("everything-service-command-failed".into());
    }
    Ok(())
}

pub fn validate_service_command(command: &str, manifest: &RuntimeManifest) -> bool {
    let Some(end) = command
        .strip_prefix('"')
        .and_then(|value| value.find('"').map(|index| index + 1))
    else {
        return false;
    };
    let executable = &command[1..end];
    let arguments = command[end + 1..].trim();
    Path::new(executable) == manifest.service_binary_path
        && crate::model::service_arguments_match(arguments, manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_order_fails_before_input_reads() {
        assert_eq!(ADMISSION_ORDER[0], AdmissionCheck::AsInvokerImage);
        assert_eq!(
            ADMISSION_ORDER[1],
            AdmissionCheck::ElevatedHighIntegrityAdmin
        );
        assert!(
            position(AdmissionCheck::ParentProcessIdentity) < position(AdmissionCheck::ReadRequest)
        );
        assert!(
            position(AdmissionCheck::TrustedActivityEvent) < position(AdmissionCheck::ReadRequest)
        );
        assert!(
            position(AdmissionCheck::MutexHeldByParent) < position(AdmissionCheck::ReadRequest)
        );
        assert!(
            position(AdmissionCheck::DirectNonReparseFiles) < position(AdmissionCheck::ReadRequest)
        );
    }

    #[test]
    fn runtime_manifest_admission_requires_exact_as_invoker_policy() {
        let valid = br#"<assembly><trustInfo><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false" /></requestedPrivileges></security></trustInfo></assembly>"#;
        assert!(embedded_manifest_is_as_invoker(valid));
        assert!(!embedded_manifest_is_as_invoker(
            br#"<requestedExecutionLevel level="requireAdministrator" uiAccess="false" />"#
        ));
        assert!(!embedded_manifest_is_as_invoker(
            br#"<requestedExecutionLevel level="asInvoker" uiAccess="true" />"#
        ));
        assert!(!embedded_manifest_is_as_invoker(
            br#"<requestedExecutionLevel level="asInvoker" />"#
        ));
        let duplicate = [valid.as_slice(), valid.as_slice()].concat();
        assert!(!embedded_manifest_is_as_invoker(&duplicate));
    }

    #[cfg(feature = "installer-helper-build")]
    #[test]
    fn compiled_helper_image_is_as_invoker() {
        assert!(verify_current_image_as_invoker().is_ok());
    }

    #[test]
    fn security_descriptors_are_protected_and_exact() {
        assert_eq!(STAGING_DACL_SDDL, "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)");
        assert_eq!(NAMED_OBJECT_SDDL, "O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)");
        assert_eq!(
            MANAGED_ROOT_SDDL,
            "O:BAG:BAD:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)"
        );
    }

    #[test]
    fn managed_parent_and_staging_file_descriptors_are_compared_exactly() {
        assert!(managed_parent_descriptor_is_exact(MANAGED_ROOT_SDDL));
        assert!(!managed_parent_descriptor_is_exact(
            "O:BAG:BAD:(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)"
        ));
        assert!(!managed_parent_descriptor_is_exact(
            "O:BAG:BAD:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GR;;;BU)"
        ));

        assert!(staging_file_dacl_is_exact(STAGING_FILE_DACL_SDDL));
        assert!(!staging_file_dacl_is_exact(
            "D:(A;ID;GA;;;SY)(A;ID;GA;;;BA)"
        ));
        assert!(!staging_file_dacl_is_exact(
            "D:AI(A;;GA;;;SY)(A;ID;GA;;;BA)"
        ));
        assert!(!staging_file_dacl_is_exact(
            "D:AI(A;ID;GA;;;SY)(A;ID;GA;;;BA)(A;ID;GR;;;BU)"
        ));
    }

    #[test]
    fn scm_transitional_and_paused_states_are_rejected() {
        assert_eq!(classify_service_status(1), Ok(ServiceStatusClass::Stopped));
        assert_eq!(classify_service_status(4), Ok(ServiceStatusClass::Running));
        for state in [2, 3, 5, 6, 7, 0, 8] {
            assert_eq!(
                classify_service_status(state),
                Err("service-state-transitional-or-unsupported")
            );
        }
    }

    #[test]
    fn parent_identity_requires_creation_time_match_and_live_process() {
        assert!(parent_identity_is_current(42, 42, WAIT_TIMEOUT));
        assert!(!parent_identity_is_current(41, 42, WAIT_TIMEOUT));
        assert!(!parent_identity_is_current(42, 42, WAIT_OBJECT_0));
    }

    #[test]
    fn held_source_native_handle_denies_concurrent_write() {
        use std::fs::OpenOptions;
        use std::time::{SystemTime, UNIX_EPOCH};

        assert_eq!(held_source_share_mode(), FILE_SHARE_READ);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uipilot-held-source-{}-{unique}.bin",
            std::process::id()
        ));
        fs::write(&path, b"trusted-source").unwrap();
        let held = HeldSource::open(&path).unwrap();
        let error = OpenOptions::new().write(true).open(&path).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        drop(held);
        assert!(OpenOptions::new().write(true).open(&path).is_ok());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn stable_exit_codes_do_not_alias() {
        let values = ExitCode::all()
            .map(|code| code as i32)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(values.len(), ExitCode::all().count());
        assert_eq!(ExitCode::Success as i32, 0);
        assert_eq!(ExitCode::RecoveryRequired as i32, 15);
    }
}
