//! Conservative installation-permission policy, evaluated on held file handles.
use std::{ffi::c_void, fs::File, os::windows::io::AsRawHandle, ptr};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT},
            GetAce, IsValidAcl, IsValidSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        },
    },
};

#[derive(Clone, Copy)]
pub(super) enum ObjectRole {
    Ancestor,
    PackageDirectory,
    Image,
}

// Both pointers must remain valid inside the caller-owned Windows descriptor.
unsafe fn verify_descriptor(
    owner: PSID,
    dacl: *mut ACL,
    role: ObjectRole,
) -> Result<(), &'static str> {
    if !trusted(&sid_text(owner)?) {
        return Err("UntrustedOwner");
    }
    if dacl.is_null() || !IsValidAcl(dacl).as_bool() {
        return Err("UntrustedDacl");
    }
    for index in 0..(*dacl).AceCount {
        let mut ace = ptr::null_mut();
        GetAce(dacl, u32::from(index), &mut ace).map_err(|_| "InvalidInstallationAce")?;
        if ace.is_null() {
            return Err("InvalidInstallationAce");
        }
        let header = &*ace.cast::<ACE_HEADER>();
        if !matches!(header.AceType, 0 | 1) || header.AceFlags & !0x1f != 0 {
            return Err("UnsupportedInstallationAce");
        }
        // Deny entries cannot add authority. Inherit-only entries do not grant
        // access to this object; each actual descendant is checked separately.
        if header.AceType == 1 || header.AceFlags & 0x08 != 0 {
            continue;
        }
        let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        if usize::from(header.AceSize) < sid_offset + 8 {
            return Err("InvalidInstallationAce");
        }
        let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        let sid = PSID(ptr::addr_of!(allowed.SidStart).cast_mut().cast());
        let count = *sid.0.cast::<u8>().add(1);
        if count > 15 || sid_offset + 8 + usize::from(count) * 4 > usize::from(header.AceSize) {
            return Err("InvalidInstallationAce");
        }
        let principal = sid_text(sid)?;
        // READ/EXECUTE/READ_CONTROL/SYNCHRONIZE and generic read/execute only.
        // Ancestors may additionally permit sibling creation, never DELETE_CHILD.
        let safe_rights = 0xa01200a9u32
            | if matches!(role, ObjectRole::Ancestor) {
                6
            } else {
                0
            };
        if !trusted(&principal) && allowed.Mask & !safe_rights != 0 {
            return Err("UntrustedWriteGrant");
        }
    }
    Ok(())
}

struct LocalMemory(*mut c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}
fn trusted(sid: &str) -> bool {
    matches!(
        sid,
        "S-1-5-18"
            | "S-1-5-32-544"
            | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
    )
}
// Caller supplies a SID from a Windows descriptor; ACE SIDs are bounded above.
unsafe fn sid_text(sid: PSID) -> Result<String, &'static str> {
    if sid.0.is_null() || !IsValidSid(sid).as_bool() {
        return Err("InvalidInstallationSid");
    }
    let mut text = PWSTR::null();
    ConvertSidToStringSidW(sid, &mut text).map_err(|_| "InvalidInstallationSid")?;
    let _text = LocalMemory(text.0.cast());
    text.to_string().map_err(|_| "InvalidInstallationSid")
}

pub(super) fn verify_held_file(file: &File, role: ObjectRole) -> Result<(), &'static str> {
    let mut owner = PSID::default();
    let mut dacl = ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor),
        )
        .ok()
        .map_err(|_| "InstallationPermissionsUnavailable")?;
        let _descriptor = LocalMemory(descriptor.0);
        if descriptor.0.is_null() {
            return Err("InstallationPermissionsUnavailable");
        }
        verify_descriptor(owner, dacl, role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        core::{BOOL, PCWSTR},
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, PSECURITY_DESCRIPTOR,
            },
        },
    };
    fn evaluate(sddl: &str, role: ObjectRole) -> Result<(), &'static str> {
        let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                1,
                &mut descriptor,
                None,
            )
            .unwrap();
            struct Owned(PSECURITY_DESCRIPTOR);
            impl Drop for Owned {
                fn drop(&mut self) {
                    unsafe {
                        let _ = LocalFree(Some(HLOCAL(self.0 .0)));
                    }
                }
            }
            let descriptor = Owned(descriptor);
            let mut owner = PSID::default();
            let mut defaulted = BOOL::default();
            GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut defaulted).unwrap();
            let mut dacl = std::ptr::null_mut();
            let mut present = BOOL::default();
            GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted)
                .unwrap();
            verify_descriptor(owner, dacl, role)
        }
    }
    #[test]
    fn trusted_owner_and_system_admin_writers_allow_users_read_execute() {
        for role in [
            ObjectRole::Ancestor,
            ObjectRole::PackageDirectory,
            ObjectRole::Image,
        ] {
            assert_eq!(
                evaluate("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)", role),
                Ok(())
            );
        }
    }
    #[test]
    fn untrusted_owner_and_null_or_absent_dacl_are_rejected() {
        assert_eq!(
            evaluate("O:S-1-5-21-1-2-3-1001G:BAD:(A;;FA;;;BA)", ObjectRole::Image),
            Err("UntrustedOwner")
        );
        for sddl in ["O:BAG:BA", "O:BAG:BAD:NO_ACCESS_CONTROL"] {
            assert_eq!(evaluate(sddl, ObjectRole::Image), Err("UntrustedDacl"));
        }
    }
    #[test]
    fn package_and_image_deny_every_untrusted_mutation_grant() {
        for role in [ObjectRole::PackageDirectory, ObjectRole::Image] {
            for mask in [
                2, 4, 0x10, 0x40, 0x100, 0x10000, 0x40000, 0x80000, 0x40000000, 0x10000000,
            ] {
                assert_eq!(
                    evaluate(&format!("O:BAG:BAD:(A;;0x{mask:x};;;BU)"), role),
                    Err("UntrustedWriteGrant")
                );
            }
        }
    }
    #[test]
    fn ancestors_allow_sibling_creation_but_not_child_replacement() {
        assert_eq!(
            evaluate("O:BAG:BAD:(A;;0x6;;;BU)", ObjectRole::Ancestor),
            Ok(())
        );
        assert_eq!(
            evaluate("O:BAG:BAD:(A;;0x40;;;BU)", ObjectRole::Ancestor),
            Err("UntrustedWriteGrant")
        );
        assert_eq!(
            evaluate("O:BAG:BAD:(A;;0x6;;;BU)", ObjectRole::PackageDirectory),
            Err("UntrustedWriteGrant")
        );
    }
    #[test]
    fn deny_rules_do_not_hide_unsafe_allow_and_unknown_ace_forms_reject() {
        assert_eq!(
            evaluate("O:BAG:BAD:(D;;GW;;;BU)(A;;GW;;;BU)", ObjectRole::Image),
            Err("UntrustedWriteGrant")
        );
        assert_eq!(
            evaluate("O:BAG:BAD:(D;;GA;;;BU)", ObjectRole::Image),
            Ok(())
        );
        assert_eq!(
            // Without an object GUID Windows normalizes OA into a standard ACE.
            evaluate(
                "O:BAG:BAD:(OA;;GR;00112233-4455-6677-8899-aabbccddeeff;;BU)",
                ObjectRole::Image
            ),
            Err("UnsupportedInstallationAce")
        );
        assert_eq!(
            evaluate("O:BAG:BAD:(A;;0x02000000;;;BU)", ObjectRole::Image),
            Err("UntrustedWriteGrant")
        );
    }
    #[test]
    fn native_protected_directory_security_is_read_from_its_handle() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        let program_files =
            std::env::var_os("ProgramFiles").expect("Windows ProgramFiles test location");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(0x00020080)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(program_files)
            .expect("read-only directory inspection");
        assert_eq!(
            verify_held_file(&file, ObjectRole::PackageDirectory),
            Ok(())
        );
    }
}
