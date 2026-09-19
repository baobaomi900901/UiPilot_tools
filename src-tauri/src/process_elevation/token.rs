//! Primary-token role checks only; not package or session authentication by itself.
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenElevation,
            TokenGroups, TokenIntegrityLevel, TokenSessionId, TokenUser, PSID, SID_AND_ATTRIBUTES,
            TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER,
        },
        System::{
            RemoteDesktop::{
                WTSClientProtocolType, WTSFreeMemory, WTSGetActiveConsoleSessionId,
                WTSQuerySessionInformationW,
            },
            Threading::OpenProcessToken,
        },
    },
};

type TokenResult<T> = Result<T, &'static str>;
#[derive(Clone, Copy)]
pub(crate) enum PeerRole {
    Host,
    Helper,
}

pub(super) struct TokenFacts {
    pub(super) user_sid: String,
    pub(super) session_id: u32,
    elevated: bool,
    administrator: bool,
    integrity: u32,
}

pub(super) fn verify_pair(
    local: &TokenFacts,
    peer: &TokenFacts,
    role: PeerRole,
) -> TokenResult<()> {
    if local.user_sid != peer.user_sid {
        return Err("AccountMismatch");
    }
    if local.session_id == 0 || local.session_id != peer.session_id {
        return Err("SessionMismatch");
    }
    let (host, helper) = match role {
        PeerRole::Host => (peer, local),
        PeerRole::Helper => (local, peer),
    };
    if host.user_sid.is_empty()
        || matches!(host.user_sid.as_str(), "S-1-5-18" | "S-1-5-19" | "S-1-5-20")
        || host.elevated
        || host.administrator
        || host.integrity != 0x2000
        || !helper.elevated
        || !helper.administrator
        || helper.integrity != 0x3000
    {
        return Err("PeerRoleRejected");
    }
    Ok(())
}

struct TokenHandle(HANDLE);
impl Drop for TokenHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct TokenBuffer {
    storage: Vec<usize>,
    len: usize,
}
impl TokenBuffer {
    fn query(token: HANDLE, class: TOKEN_INFORMATION_CLASS, minimum: usize) -> TokenResult<Self> {
        let mut length = 0u32;
        unsafe {
            let _ = GetTokenInformation(token, class, None, 0, &mut length);
        }
        if (length as usize) < minimum || length > 65536 {
            return Err("PeerTokenUnavailable");
        }
        let capacity = length;
        let mut storage = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token,
                class,
                Some(storage.as_mut_ptr().cast()),
                capacity,
                &mut length,
            )
        }
        .map_err(|_| "PeerTokenUnavailable")?;
        if (length as usize) < minimum || length > capacity {
            return Err("PeerTokenUnavailable");
        }
        Ok(Self {
            storage,
            len: length as usize,
        })
    }
    fn sid_bytes(&self, sid: PSID) -> TokenResult<&[u8]> {
        let offset = (sid.0 as usize)
            .checked_sub(self.storage.as_ptr() as usize)
            .ok_or("PeerTokenUnavailable")?;
        if offset.checked_add(8).is_none_or(|end| end > self.len) {
            return Err("PeerTokenUnavailable");
        }
        let start = unsafe { self.storage.as_ptr().cast::<u8>().add(offset) };
        let revision = unsafe { *start };
        let count = unsafe { *start.add(1) };
        let length = 8 + usize::from(count) * 4;
        if revision != 1 || count > 15 || offset + length > self.len {
            return Err("PeerTokenUnavailable");
        }
        Ok(unsafe { std::slice::from_raw_parts(start, length) })
    }
    fn sid_text(&self, sid: PSID) -> TokenResult<String> {
        self.sid_bytes(sid)?;
        let mut text = PWSTR::null();
        unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(|_| "PeerTokenUnavailable")?;
        struct Text(PWSTR);
        impl Drop for Text {
            fn drop(&mut self) {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0 .0.cast())));
                }
            }
        }
        let text = Text(text);
        unsafe { text.0.to_string() }.map_err(|_| "PeerTokenUnavailable")
    }
}

fn dword(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> TokenResult<u32> {
    let mut value = 0u32;
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some((&mut value as *mut u32).cast()),
            4,
            &mut size,
        )
    }
    .map_err(|_| "PeerTokenUnavailable")?;
    if size != 4 {
        return Err("PeerTokenUnavailable");
    }
    Ok(value)
}
fn enabled_admin(sid: &str, attributes: u32) -> bool {
    sid == "S-1-5-32-544" && attributes & 4 != 0 && attributes & 0x10 == 0
}

pub(super) fn observe(process: HANDLE) -> TokenResult<TokenFacts> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|_| "PeerTokenUnavailable")?;
    let token = TokenHandle(token);
    let user = TokenBuffer::query(token.0, TokenUser, std::mem::size_of::<TOKEN_USER>())?;
    let user_sid =
        user.sid_text(unsafe { (*user.storage.as_ptr().cast::<TOKEN_USER>()).User.Sid })?;
    let groups_offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
    let groups = TokenBuffer::query(token.0, TokenGroups, groups_offset)?;
    let count = unsafe { *groups.storage.as_ptr().cast::<u32>() } as usize;
    if count
        .checked_mul(std::mem::size_of::<SID_AND_ATTRIBUTES>())
        .and_then(|bytes| bytes.checked_add(groups_offset))
        .is_none_or(|end| end > groups.len)
    {
        return Err("PeerTokenUnavailable");
    }
    let entries = unsafe {
        std::slice::from_raw_parts(
            groups
                .storage
                .as_ptr()
                .cast::<u8>()
                .add(groups_offset)
                .cast::<SID_AND_ATTRIBUTES>(),
            count,
        )
    };
    let mut administrator = false;
    for entry in entries {
        if enabled_admin(&groups.sid_text(entry.Sid)?, entry.Attributes) {
            administrator = true;
        }
    }
    let label = TokenBuffer::query(
        token.0,
        TokenIntegrityLevel,
        std::mem::size_of::<TOKEN_MANDATORY_LABEL>(),
    )?;
    let sid = unsafe {
        (*label.storage.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
            .Label
            .Sid
    };
    let bytes = label.sid_bytes(sid)?;
    if bytes[1] != 1 || bytes[2..8] != [0, 0, 0, 0, 0, 16] {
        return Err("PeerTokenUnavailable");
    }
    let integrity = u32::from_le_bytes(
        bytes[8..12]
            .try_into()
            .map_err(|_| "PeerTokenUnavailable")?,
    );
    let elevated = match dword(token.0, TokenElevation)? {
        0 => false,
        1 => true,
        _ => return Err("PeerTokenUnavailable"),
    };
    Ok(TokenFacts {
        user_sid,
        session_id: dword(token.0, TokenSessionId)?,
        elevated,
        administrator,
        integrity,
    })
}

fn verify_session_facts(session: u32, active_console: u32, protocol: u16) -> TokenResult<()> {
    if session == 0 || active_console == u32::MAX || session != active_console || protocol != 0 {
        return Err("PeerSessionRejected");
    }
    Ok(())
}
pub(super) fn verify_local_console(session: u32) -> TokenResult<()> {
    let mut buffer = PWSTR::null();
    let mut size = 0;
    unsafe {
        WTSQuerySessionInformationW(None, session, WTSClientProtocolType, &mut buffer, &mut size)
    }
    .map_err(|_| "PeerSessionUnavailable")?;
    struct Allocation(PWSTR);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                WTSFreeMemory(self.0 .0.cast());
            }
        }
    }
    let buffer = Allocation(buffer);
    if buffer.0.is_null() || size != 2 {
        return Err("PeerSessionUnavailable");
    }
    let protocol = unsafe { buffer.0 .0.read_unaligned() };
    verify_session_facts(session, unsafe { WTSGetActiveConsoleSessionId() }, protocol)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host() -> TokenFacts {
        TokenFacts {
            user_sid: "S-1-5-21-1-2-3-1001".into(),
            session_id: 1,
            elevated: false,
            administrator: false,
            integrity: 0x2000,
        }
    }
    fn helper() -> TokenFacts {
        TokenFacts {
            elevated: true,
            administrator: true,
            integrity: 0x3000,
            ..host()
        }
    }

    #[test]
    fn same_account_opposite_roles_are_required_in_both_directions() {
        assert_eq!(verify_pair(&host(), &helper(), PeerRole::Helper), Ok(()));
        assert_eq!(verify_pair(&helper(), &host(), PeerRole::Host), Ok(()));
        assert_eq!(
            verify_pair(&host(), &host(), PeerRole::Helper),
            Err("PeerRoleRejected")
        );
        assert_eq!(
            verify_pair(&helper(), &helper(), PeerRole::Host),
            Err("PeerRoleRejected")
        );
    }
    #[test]
    fn account_and_session_mismatch_reject() {
        let mut peer = helper();
        peer.user_sid.push('2');
        assert_eq!(
            verify_pair(&host(), &peer, PeerRole::Helper),
            Err("AccountMismatch")
        );
        let mut peer = helper();
        peer.session_id = 2;
        assert_eq!(
            verify_pair(&host(), &peer, PeerRole::Helper),
            Err("SessionMismatch")
        );
        let mut local = host();
        local.session_id = 0;
        peer.session_id = 0;
        assert_eq!(
            verify_pair(&local, &peer, PeerRole::Helper),
            Err("SessionMismatch")
        );
    }
    #[test]
    fn unexpected_integrity_and_administrator_states_reject() {
        for level in [0, 0x1000, 0x2100, 0x3000, 0x4000] {
            let local = TokenFacts {
                integrity: level,
                ..host()
            };
            assert_eq!(
                verify_pair(&local, &helper(), PeerRole::Helper),
                Err("PeerRoleRejected")
            );
        }
        for level in [0, 0x1000, 0x2000, 0x4000] {
            let peer = TokenFacts {
                integrity: level,
                ..helper()
            };
            assert_eq!(
                verify_pair(&host(), &peer, PeerRole::Helper),
                Err("PeerRoleRejected")
            );
        }
        let peer = TokenFacts {
            administrator: false,
            ..helper()
        };
        assert_eq!(
            verify_pair(&host(), &peer, PeerRole::Helper),
            Err("PeerRoleRejected")
        );
        let peer = TokenFacts {
            elevated: false,
            ..helper()
        };
        assert_eq!(
            verify_pair(&host(), &peer, PeerRole::Helper),
            Err("PeerRoleRejected")
        );
        let local = TokenFacts {
            administrator: true,
            ..host()
        };
        assert_eq!(
            verify_pair(&local, &helper(), PeerRole::Helper),
            Err("PeerRoleRejected")
        );
    }

    #[test]
    fn disabled_or_deny_only_administrator_groups_do_not_grant_the_role() {
        assert!(enabled_admin("S-1-5-32-544", 4));
        assert!(!enabled_admin("S-1-5-32-544", 4 | 0x10));
        assert!(!enabled_admin("S-1-5-32-544", 0));
        assert!(!enabled_admin("S-1-5-32-545", 4));
    }

    #[test]
    fn service_accounts_and_nonconsole_sessions_are_rejected() {
        for sid in ["", "S-1-5-18", "S-1-5-19", "S-1-5-20"] {
            let local = TokenFacts {
                user_sid: sid.into(),
                ..host()
            };
            let peer = TokenFacts {
                user_sid: sid.into(),
                ..helper()
            };
            assert_eq!(
                verify_pair(&local, &peer, PeerRole::Helper),
                Err("PeerRoleRejected")
            );
        }
        assert_eq!(verify_session_facts(1, 1, 0), Ok(()));
        for (session, active, protocol) in
            [(0, 0, 0), (1, u32::MAX, 0), (2, 1, 0), (1, 1, 1), (1, 1, 2)]
        {
            assert_eq!(
                verify_session_facts(session, active, protocol),
                Err("PeerSessionRejected")
            );
        }
    }

    #[test]
    fn token_sid_range_checks_reject_outside_truncated_or_oversized_data() {
        let mut buffer = TokenBuffer {
            storage: vec![0; 16usize.div_ceil(std::mem::size_of::<usize>())],
            len: 16,
        };
        buffer.storage[0] = 0x0101;
        let sid = PSID(buffer.storage.as_ptr().cast_mut().cast());
        assert_eq!(buffer.sid_bytes(sid).unwrap().len(), 12);
        assert!(buffer.sid_bytes(PSID::default()).is_err());
        assert!(buffer
            .sid_bytes(PSID(std::ptr::without_provenance_mut(usize::MAX)))
            .is_err());
        buffer.len = 8;
        assert!(buffer.sid_bytes(sid).is_err());
        buffer.len = 16;
        buffer.storage[0] = 0x1001; // 16 subauthorities is outside the Windows SID bound.
        assert!(buffer.sid_bytes(sid).is_err());
    }

    #[test]
    fn current_process_token_and_local_session_are_read_natively() {
        let facts = observe(unsafe { windows::Win32::System::Threading::GetCurrentProcess() })
            .unwrap_or_else(|code| panic!("{code}"));
        assert!(facts.user_sid.starts_with("S-1-"));
        assert!(facts.integrity > 0);
        assert_eq!(
            verify_local_console(facts.session_id),
            Ok(()),
            "requires the supported local-console environment"
        );
    }
}
