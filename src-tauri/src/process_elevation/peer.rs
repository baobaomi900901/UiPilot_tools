//! OS observations only: this module does not grant an authenticated session or
//! establish package trust from an image path. The process handle pins the observation.
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, LocalFree, FILETIME, HANDLE, HLOCAL, WAIT_TIMEOUT},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenSessionId, TokenUser,
            TOKEN_QUERY, TOKEN_USER,
        },
        System::{
            Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
            Threading::{
                GetProcessTimes, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
                WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE,
            },
        },
    },
};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub(crate) struct PeerFacts {
    pub(crate) pid: u32,
    pub(crate) creation_time: u64,
    pub(crate) user_sid: String,
    pub(crate) session_id: u32,
    pub(crate) image_path: PathBuf,
}

pub(crate) struct ObservedPeer {
    process: OwnedHandle,
    facts: PeerFacts,
}

#[derive(Clone, Copy)]
pub(crate) enum PeerEnd {
    Client,
    Server,
}

impl ObservedPeer {
    pub(crate) fn from_pipe(
        pipe: HANDLE,
        end: PeerEnd,
        expected_pid: u32,
        expected_creation: u64,
    ) -> Result<Self, &'static str> {
        if expected_pid == 0 || expected_creation == 0 {
            return Err("InvalidPeerIdentity");
        }
        let pid = pipe_pid(pipe, end)?;
        if pid != expected_pid {
            return Err("PeerMismatch");
        }
        let process = OwnedHandle(
            unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
            }
            .map_err(|_| "PeerInspectionDenied")?,
        );
        let mut times = [FILETIME::default(); 4];
        unsafe {
            GetProcessTimes(
                process.0,
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        }
        .map_err(|_| "PeerInspectionDenied")?;
        let creation_time =
            (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
        if creation_time == 0 || creation_time != expected_creation {
            return Err("PeerChanged");
        }
        ensure_live(process.0)?;
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }
            .map_err(|_| "PeerTokenUnavailable")?;
        let token = OwnedHandle(token);
        let user_sid = token_user(token.0)?;
        let mut session_id = 0u32;
        let mut size = 0;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenSessionId,
                Some((&mut session_id as *mut u32).cast()),
                std::mem::size_of::<u32>() as u32,
                &mut size,
            )
        }
        .map_err(|_| "PeerTokenUnavailable")?;
        if size != std::mem::size_of::<u32>() as u32 {
            return Err("PeerTokenUnavailable");
        }
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )
        }
        .map_err(|_| "PeerImageUnavailable")?;
        if length == 0 || length as usize > path.len() {
            return Err("PeerImageUnavailable");
        }
        let image_path = PathBuf::from(OsString::from_wide(&path[..length as usize]));
        if !image_path.is_absolute() {
            return Err("PeerImageUnavailable");
        }
        ensure_live(process.0)?;
        if pipe_pid(pipe, end)? != pid {
            return Err("PeerChanged");
        }
        Ok(Self {
            process,
            facts: PeerFacts {
                pid,
                creation_time,
                user_sid,
                session_id,
                image_path,
            },
        })
    }
    pub(crate) fn facts(&self) -> &PeerFacts {
        &self.facts
    }
    pub(crate) fn check_live(&self) -> Result<(), &'static str> {
        ensure_live(self.process.0)
    }
}

fn ensure_live(process: HANDLE) -> Result<(), &'static str> {
    if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
        return Err("PeerNotAlive");
    }
    Ok(())
}

fn pipe_pid(pipe: HANDLE, end: PeerEnd) -> Result<u32, &'static str> {
    let mut pid = 0;
    match end {
        PeerEnd::Client => unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) },
        PeerEnd::Server => unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) },
    }
    .map_err(|_| "PeerUnavailable")?;
    if pid == 0 {
        return Err("PeerUnavailable");
    }
    Ok(pid)
}

fn token_user(token: HANDLE) -> Result<String, &'static str> {
    let mut size = 0u32;
    unsafe {
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
    }
    if (size as usize) < std::mem::size_of::<TOKEN_USER>() || size > 65536 {
        return Err("PeerTokenUnavailable");
    }
    let capacity = size;
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            capacity,
            &mut size,
        )
    }
    .map_err(|_| "PeerTokenUnavailable")?;
    if (size as usize) < std::mem::size_of::<TOKEN_USER>() || size > capacity {
        return Err("PeerTokenUnavailable");
    }
    // Validate that the variable SID lies within this aligned token buffer before
    // passing it to Win32. A SID header is 8 bytes plus four per subauthority.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let offset = (sid.0 as usize)
        .checked_sub(buffer.as_ptr() as usize)
        .ok_or("PeerTokenUnavailable")?;
    if offset.checked_add(8).is_none_or(|end| end > size as usize) {
        return Err("PeerTokenUnavailable");
    }
    let sid_bytes = sid.0.cast::<u8>();
    let revision = unsafe { *sid_bytes };
    let count = unsafe { *sid_bytes.add(1) };
    if revision != 1 || count > 15 || offset + 8 + usize::from(count) * 4 > size as usize {
        return Err("PeerTokenUnavailable");
    }
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(|_| "PeerTokenUnavailable")?;
    struct SidText(PWSTR);
    impl Drop for SidText {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0.cast())));
            }
        }
    }
    let text = SidText(text);
    unsafe { text.0.to_string() }.map_err(|_| "PeerTokenUnavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_elevation::pipe::{LocalPipeName, LocalPipeServer};
    use std::{
        ffi::OsStr,
        os::windows::ffi::OsStrExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    use windows::{
        core::PCWSTR,
        Win32::{
            Storage::FileSystem::{
                CreateFileW, FILE_SHARE_MODE, OPEN_EXISTING, SECURITY_IDENTIFICATION,
                SECURITY_SQOS_PRESENT,
            },
            System::Threading::{GetCurrentProcess, GetCurrentProcessId},
        },
    };

    fn pipe_pair() -> (LocalPipeServer, OwnedHandle, u32, u64) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let pid = unsafe { GetCurrentProcessId() };
        let name = LocalPipeName::from_nonce(&format!(
            "{pid:032x}{:032x}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        // This fixture uses owner-only permissions and no external process.
        let server = LocalPipeServer::create(&name, "D:P(A;;GA;;;OW)").unwrap();
        let path: Vec<u16> = OsStr::new(name.as_str())
            .encode_wide()
            .chain(Some(0))
            .collect();
        let client = OwnedHandle(
            unsafe {
                CreateFileW(
                    PCWSTR(path.as_ptr()),
                    0x00100183,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    None,
                )
            }
            .unwrap(),
        );
        let mut times = [FILETIME::default(); 4];
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        }
        .unwrap();
        let created =
            (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
        (server, client, pid, created)
    }

    #[test]
    fn actual_pipe_peers_are_observed_through_pinned_process_handles() {
        let (server, client, pid, created) = pipe_pair();
        for (pipe, end) in [(server.raw(), PeerEnd::Client), (client.0, PeerEnd::Server)] {
            let peer = ObservedPeer::from_pipe(pipe, end, pid, created)
                .unwrap_or_else(|code| panic!("{code}"));
            assert_eq!(peer.facts().pid, pid);
            assert_eq!(peer.facts().creation_time, created);
            assert!(peer.facts().user_sid.starts_with("S-1-"));
            assert!(peer.facts().session_id > 0);
            assert_eq!(peer.facts().image_path, std::env::current_exe().unwrap());
            assert_eq!(peer.check_live(), Ok(()));
        }
    }

    #[test]
    fn pipe_expected_identity_mismatch_is_rejected() {
        let (server, _client, pid, created) = pipe_pair();
        assert!(matches!(
            ObservedPeer::from_pipe(server.raw(), PeerEnd::Client, 0, created),
            Err("InvalidPeerIdentity")
        ));
        assert!(matches!(
            ObservedPeer::from_pipe(server.raw(), PeerEnd::Client, pid, 0),
            Err("InvalidPeerIdentity")
        ));
        assert!(matches!(
            ObservedPeer::from_pipe(server.raw(), PeerEnd::Client, pid ^ 0x40000000, created),
            Err("PeerMismatch")
        ));
        assert!(matches!(
            ObservedPeer::from_pipe(server.raw(), PeerEnd::Client, pid, created ^ 1),
            Err("PeerChanged")
        ));
        assert!(matches!(
            ObservedPeer::from_pipe(HANDLE::default(), PeerEnd::Client, pid, created),
            Err("PeerUnavailable")
        ));
    }

    #[test]
    #[ignore = "Child entrypoint launched explicitly by the owned peer test"]
    fn owned_peer_fixture() {
        use std::io::Read;
        let name = std::env::var("UIPILOT_PEER_FIXTURE_PIPE").expect("fixture input");
        let path: Vec<u16> = OsStr::new(&name).encode_wide().chain(Some(0)).collect();
        let _pipe = OwnedHandle(
            unsafe {
                CreateFileW(
                    PCWSTR(path.as_ptr()),
                    0x00100183,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    None,
                )
            }
            .expect("fixture connection"),
        );
        let mut release = [0];
        std::io::stdin()
            .read_exact(&mut release)
            .expect("fixture release");
    }

    #[test]
    fn child_peer_exit_is_detected_through_the_retained_handle() {
        use std::{
            io::Write,
            os::windows::{io::AsRawHandle, process::CommandExt},
            process::{Child, Command, Stdio},
            thread,
            time::{Duration, Instant},
        };
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                if !matches!(self.0.try_wait(), Ok(Some(_))) {
                    let _ = self.0.kill();
                    assert_eq!(
                        unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 5000) }.0,
                        0,
                        "owned fixture cleanup unconfirmed"
                    );
                }
            }
        }
        let nonce = format!(
            "{:064x}",
            u64::from(unsafe { GetCurrentProcessId() }) << 32 | 0x1234
        );
        let name = LocalPipeName::from_nonce(&nonce).unwrap();
        let server = LocalPipeServer::create(&name, "D:P(A;;GA;;;OW)").unwrap();
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "process_elevation::peer::tests::owned_peer_fixture",
                    "--ignored",
                ])
                .env("UIPILOT_PEER_FIXTURE_PIPE", name.as_str())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW.0)
                .spawn()
                .unwrap(),
        );
        let mut times = [FILETIME::default(); 4];
        unsafe {
            GetProcessTimes(
                HANDLE(child.0.as_raw_handle()),
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        }
        .unwrap();
        let created =
            (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
        let deadline = Instant::now() + Duration::from_secs(10);
        while server.client_pid().is_err() {
            assert!(
                Instant::now() < deadline,
                "owned fixture connection deadline"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let peer = ObservedPeer::from_pipe(server.raw(), PeerEnd::Client, child.0.id(), created)
            .unwrap_or_else(|code| panic!("{code}"));
        assert_eq!(peer.facts().pid, child.0.id());
        assert_eq!(peer.facts().creation_time, created);
        assert_eq!(peer.check_live(), Ok(()));
        child.0.stdin.take().unwrap().write_all(&[1]).unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(HANDLE(child.0.as_raw_handle()), 5000) }.0,
            0
        );
        assert!(child.0.try_wait().unwrap().unwrap().success());
        assert_eq!(peer.check_live(), Err("PeerNotAlive"));
    }
}
