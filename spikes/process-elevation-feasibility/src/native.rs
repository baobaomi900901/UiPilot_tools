use std::{
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT},
            GetAce, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION,
            OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        },
        Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, GetVolumeInformationW,
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramFilesX64, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    },
};

type Check<T> = Result<T, &'static str>;
pub(super) struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct LocalAllocation(*mut std::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}

fn wide(value: &OsStr) -> Check<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err("InvalidPath");
    }
    value.push(0);
    Ok(value)
}

fn sid_text(sid: PSID) -> Check<String> {
    if sid.0.is_null() {
        return Err("InvalidSecurityDescriptor");
    }
    let mut text = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut text).map_err(|_| "InvalidSid")?;
        let _allocation = LocalAllocation(text.0.cast());
        text.to_string().map_err(|_| "InvalidSid")
    }
}

// This deliberately rejects unknown ACE forms and grants even if another ACE
// denies them. It is a conservative allowlist, not an approximate AccessCheck.
fn check_acl(handle: HANDLE, ancestor: bool) -> Check<()> {
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor),
        )
        .ok()
        .map_err(|_| "SecurityInspectionDenied")?;
        let _allocation = LocalAllocation(descriptor.0);
        if !super::trusted_writer(&sid_text(owner)?) || dacl.is_null() {
            return Err("UntrustedOwnerOrDacl");
        }
        for index in 0..(*dacl).AceCount {
            let mut ace = ptr::null_mut();
            GetAce(dacl, index.into(), &mut ace).map_err(|_| "InvalidAce")?;
            let header = &*ace.cast::<ACE_HEADER>();
            if header.AceFlags & 0x08 != 0 {
                continue;
            } // INHERIT_ONLY_ACE
            match header.AceType {
                1 => continue, // Denies cannot add authority.
                0 => {
                    if usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
                        return Err("InvalidAce");
                    }
                    let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                    let sid = PSID(ptr::addr_of!(allowed.SidStart).cast_mut().cast());
                    if super::unsafe_grant(allowed.Mask, &sid_text(sid)?, ancestor) {
                        return Err("UntrustedWritePermission");
                    }
                }
                _ => return Err("UnsupportedAce"),
            }
        }
    }
    Ok(())
}

fn inspect(path: &Path, ancestor: bool) -> Check<Handle> {
    let path = wide(path.as_os_str())?;
    // READ_CONTROL | FILE_READ_ATTRIBUTES; sharing denies replacement while held.
    let handle = Handle(unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0x20080,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
        .map_err(|_| "PathInspectionDenied")?
    });
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle.0, &mut info) }
        .map_err(|_| "PathInspectionDenied")?;
    if info.dwFileAttributes & 0x400 != 0 {
        return Err("ReparsePointRejected");
    }
    check_acl(handle.0, ancestor)?;
    Ok(handle)
}

fn program_files() -> Check<PathBuf> {
    unsafe {
        let value = SHGetKnownFolderPath(&FOLDERID_ProgramFilesX64, KF_FLAG_DEFAULT, None)
            .map_err(|_| "KnownFolderUnavailable")?;
        let text = value.to_string();
        CoTaskMemFree(Some(value.0.cast()));
        Ok(PathBuf::from(text.map_err(|_| "InvalidPath")?))
    }
}

fn inspect_tree(path: &Path, leases: &mut Vec<Handle>) -> Check<()> {
    if leases.len() >= 8192 {
        return Err("PackageTooLarge");
    }
    leases.push(inspect(path, false)?);
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| "DirectoryInspectionDenied")? {
            inspect_tree(
                &entry.map_err(|_| "DirectoryInspectionDenied")?.path(),
                leases,
            )?;
        }
    }
    Ok(())
}

#[allow(dead_code, reason = "Shared with the opt-in probe binary")]
pub(super) fn run() -> Check<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2
        || !matches!(
            args[0].to_str(),
            Some("preinstall" | "postinstall" | "preuninstall")
        )
    {
        return Err("InvalidArguments");
    }
    let expected = program_files()?.join("UiPilot_tools Protected");
    // Exact fixed destination prevents /D, relative, alternate stream and dot-path
    // variants. It intentionally does not accept arbitrary user-selected roots.
    if args[1] != expected.as_os_str() {
        return Err("UnexpectedInstallDirectory");
    }
    verify_directory(&expected, args[0] == "preinstall")?;
    Ok(())
}

fn verify_directory(expected: &Path, allow_missing: bool) -> Check<Vec<Handle>> {
    let mut ancestors: Vec<_> = expected.ancestors().skip(1).collect();
    ancestors.reverse();
    let root = ancestors.first().ok_or("InvalidPath")?;
    let root_wide = wide(root.as_os_str())?;
    let mut filesystem = [0u16; 32];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(root_wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut filesystem),
        )
    }
    .map_err(|_| "VolumeInspectionDenied")?;
    if String::from_utf16_lossy(
        &filesystem[..filesystem.iter().position(|c| *c == 0).unwrap_or(32)],
    ) != "NTFS"
    {
        return Err("UnsupportedFilesystem");
    }
    let mut leases = Vec::new();
    for path in ancestors {
        leases.push(inspect(path, true)?);
    }
    match fs::symlink_metadata(expected) {
        Ok(_) => inspect_tree(expected, &mut leases)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => (),
        Err(_) => return Err("InstallDirectoryUnavailable"),
    }
    Ok(leases)
}

#[cfg(feature = "uac-probe")]
#[allow(dead_code, reason = "Used only by the opt-in probe binary")]
pub(super) fn probe_installation() -> Check<(PathBuf, Vec<Handle>)> {
    let directory = program_files()?.join("UiPilot Elevation Probe");
    let executable = directory.join("uipilot-uac-probe.exe");
    if std::env::current_exe().map_err(|_| "ExecutableUnavailable")? != executable {
        return Err("UntrustedProbeLocation");
    }
    let mut leases = verify_directory(&directory, false)?;
    let path = wide(executable.as_os_str())?;
    // Hold the exact executable against write/delete through the whole handshake.
    leases.push(Handle(
        unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                0x20080,
                FILE_SHARE_READ,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(|_| "ExecutableLeaseDenied")?,
    ));
    Ok((executable, leases))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_user_writable_directory_is_rejected() {
        let path = std::env::temp_dir().join(format!("uipilot-install-acl-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let result = inspect(&path, false);
        assert!(result.is_err());
        drop(result);
        fs::remove_dir(&path).unwrap();
    }
}
