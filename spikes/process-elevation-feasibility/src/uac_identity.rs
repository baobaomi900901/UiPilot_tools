use super::pipe_probe::{Owned, Probe};
use std::{ffi::c_void, ptr};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::ConvertSidToStringSidW, GetSidSubAuthority, GetSidSubAuthorityCount,
            GetTokenInformation, TokenElevation, TokenGroups, TokenIntegrityLevel, TokenSessionId,
            TokenUser, PSID, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL,
            TOKEN_QUERY, TOKEN_USER,
        },
        System::{RemoteDesktop::WTSGetActiveConsoleSessionId, Threading::OpenProcessToken},
    },
};

fn query(token: HANDLE, kind: TOKEN_INFORMATION_CLASS) -> Probe<Vec<usize>> {
    let mut size = 0;
    unsafe {
        let _ = GetTokenInformation(token, kind, None, 0, &mut size);
    }
    if size == 0 || size > 65536 {
        return Err("InvalidTokenSize");
    }
    let mut bytes = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token,
            kind,
            Some(bytes.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(|_| "TokenInspectionDenied")?;
    Ok(bytes)
}

fn sid_text(sid: PSID) -> Probe<String> {
    let mut text = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut text).map_err(|_| "InvalidTokenSid")?;
        let value = text.to_string();
        let _ = LocalFree(Some(HLOCAL(text.0.cast::<c_void>())));
        value.map_err(|_| "InvalidTokenSid")
    }
}

struct Identity {
    user: String,
    session: u32,
    elevated: bool,
    admin: bool,
    integrity: u32,
}
fn inspect(process: HANDLE) -> Probe<Identity> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|_| "TokenInspectionDenied")?;
    let token = Owned(token);
    let user_buffer = query(token.0, TokenUser)?;
    let groups_buffer = query(token.0, TokenGroups)?;
    let label_buffer = query(token.0, TokenIntegrityLevel)?;
    unsafe {
        let user = sid_text((*user_buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid)?;
        let groups = &*groups_buffer.as_ptr().cast::<TOKEN_GROUPS>();
        let mut admin = false;
        for group in std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        {
            if group.Attributes & 4 != 0
                && group.Attributes & 0x10 == 0
                && sid_text(group.Sid)? == "S-1-5-32-544"
            {
                admin = true;
            }
        }
        let label = (*label_buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
            .Label
            .Sid;
        let count = *GetSidSubAuthorityCount(label);
        if count == 0 {
            return Err("InvalidIntegritySid");
        }
        let integrity = *GetSidSubAuthority(label, u32::from(count - 1));
        let session = ptr::read(query(token.0, TokenSessionId)?.as_ptr().cast::<u32>());
        let elevated = ptr::read(query(token.0, TokenElevation)?.as_ptr().cast::<u32>()) != 0;
        Ok(Identity {
            user,
            session,
            elevated,
            admin,
            integrity,
        })
    }
}

fn require_host(host: &Identity) -> Probe<()> {
    if host.elevated || host.integrity != 0x2000 || host.user == "S-1-5-18" {
        return Err("HostMustBeMediumIntegrity");
    }
    if host.session == 0 || host.session != unsafe { WTSGetActiveConsoleSessionId() } {
        return Err("LocalConsoleSessionRequired");
    }
    Ok(())
}

pub(super) fn verify_host(process: HANDLE) -> Probe<()> {
    require_host(&inspect(process)?)
}

pub(super) fn verify_pair(host: HANDLE, helper: HANDLE) -> Probe<()> {
    let host = inspect(host)?;
    let helper = inspect(helper)?;
    require_pair(&host, &helper)
}

fn require_pair(host: &Identity, helper: &Identity) -> Probe<()> {
    require_host(host)?;
    if helper.user != host.user {
        return Err("AccountMismatch");
    }
    if helper.session != host.session {
        return Err("SessionMismatch");
    }
    if !helper.elevated || !helper.admin || helper.integrity != 0x3000 {
        return Err("ElevatedAdministratorRequired");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identities() -> (Identity, Identity) {
        let session = unsafe { WTSGetActiveConsoleSessionId() };
        let host = Identity {
            user: "S-1-5-21-1-2-3-1001".into(),
            session,
            elevated: false,
            admin: false,
            integrity: 0x2000,
        };
        let helper = Identity {
            user: host.user.clone(),
            session,
            elevated: true,
            admin: true,
            integrity: 0x3000,
        };
        (host, helper)
    }
    #[test]
    fn same_account_high_admin_pair_is_required() {
        let (host, mut helper) = identities();
        assert_eq!(require_pair(&host, &helper), Ok(()));
        helper.admin = false;
        assert_eq!(
            require_pair(&host, &helper),
            Err("ElevatedAdministratorRequired")
        );
        helper.admin = true;
        helper.elevated = false;
        assert_eq!(
            require_pair(&host, &helper),
            Err("ElevatedAdministratorRequired")
        );
        helper.elevated = true;
        helper.integrity = 0x2000;
        assert_eq!(
            require_pair(&host, &helper),
            Err("ElevatedAdministratorRequired")
        );
    }
    #[test]
    fn alternate_account_or_session_is_rejected() {
        let (host, mut helper) = identities();
        helper.user.push('2');
        assert_eq!(require_pair(&host, &helper), Err("AccountMismatch"));
        helper.user = host.user.clone();
        helper.session ^= 1;
        assert_eq!(require_pair(&host, &helper), Err("SessionMismatch"));
    }
    #[test]
    fn elevated_or_system_host_is_rejected() {
        let (mut host, helper) = identities();
        host.elevated = true;
        assert_eq!(
            require_pair(&host, &helper),
            Err("HostMustBeMediumIntegrity")
        );
        host.elevated = false;
        host.user = "S-1-5-18".into();
        assert_eq!(
            require_pair(&host, &helper),
            Err("HostMustBeMediumIntegrity")
        );
    }
    #[test]
    fn current_medium_token_cannot_also_be_the_elevated_helper() {
        let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        assert_eq!(
            verify_host(process),
            Ok(()),
            "Run this probe from a normal local-console token"
        );
        assert_eq!(
            verify_pair(process, process),
            Err("ElevatedAdministratorRequired")
        );
    }
}
