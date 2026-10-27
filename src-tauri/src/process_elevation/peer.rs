//! OS observations only: this module does not grant an authenticated session or
//! establish package trust from an image path. The process handle pins the observation.
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_TIMEOUT},
        System::{
            Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
            Threading::{
                GetCurrentProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
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

/// Opaque proof owned by native peer verification.
pub(crate) struct AuthenticationProof {
    _private: (),
}

impl AuthenticationProof {
    #[cfg(test)]
    pub(crate) fn test_only() -> Self {
        Self { _private: () }
    }
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
        let token = super::token::observe(process.0)?;
        let user_sid = token.user_sid;
        let session_id = token.session_id;
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
    pub(crate) fn check_account_and_role(
        &self,
        role: super::token::PeerRole,
    ) -> Result<(), &'static str> {
        self.check_live()?;
        let local = super::token::observe(unsafe { GetCurrentProcess() })?;
        let peer = super::token::observe(self.process.0)?;
        if peer.user_sid != self.facts.user_sid || peer.session_id != self.facts.session_id {
            return Err("PeerChanged");
        }
        super::token::verify_pair(&local, &peer, role)?;
        super::token::verify_local_console(local.session_id)?;
        self.check_live()
    }

    pub(crate) fn authenticate_as(
        &self,
        role: super::token::PeerRole,
    ) -> Result<AuthenticationProof, &'static str> {
        self.check_account_and_role(role)?;
        Ok(AuthenticationProof { _private: () })
    }

    pub(crate) fn bind_session(
        &self,
        role: super::token::PeerRole,
    ) -> Result<super::session::BoundSession, &'static str> {
        let proof = self.authenticate_as(role)?;
        Ok(super::session::BoundSession::from_native_peer(proof))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_elevation::pipe::{LocalPipeName, LocalPipeServer, PipeStream};
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};

    fn pipe_pair() -> (LocalPipeServer, PipeStream, u32, u64) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let pid = unsafe { GetCurrentProcessId() };
        let name = LocalPipeName::from_nonce(&format!(
            "{pid:032x}{:032x}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        // This fixture uses owner-only permissions and no external process.
        let server = LocalPipeServer::create(&name, "D:P(A;;GA;;;OW)").unwrap();
        let client = PipeStream::open_client(&name).unwrap();
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
        for (pipe, end) in [
            (server.raw(), PeerEnd::Client),
            (client.raw(), PeerEnd::Server),
        ] {
            let peer = ObservedPeer::from_pipe(pipe, end, pid, created)
                .unwrap_or_else(|code| panic!("{code}"));
            assert_eq!(peer.facts().pid, pid);
            assert_eq!(peer.facts().creation_time, created);
            assert!(peer.facts().user_sid.starts_with("S-1-"));
            assert!(peer.facts().session_id > 0);
            assert_eq!(peer.facts().image_path, std::env::current_exe().unwrap());
            assert_eq!(peer.check_live(), Ok(()));
            // One actual process cannot be both the ordinary host and high helper.
            assert_eq!(
                peer.check_account_and_role(super::super::token::PeerRole::Helper),
                Err("PeerRoleRejected")
            );
            assert_eq!(
                peer.check_account_and_role(super::super::token::PeerRole::Host),
                Err("PeerRoleRejected")
            );
            assert_eq!(
                peer.bind_session(super::super::token::PeerRole::Helper)
                    .map(|_| ()),
                Err("PeerRoleRejected")
            );
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
        let nonce = std::env::var("UIPILOT_PEER_FIXTURE_NONCE").expect("fixture input");
        let name = LocalPipeName::from_nonce(&nonce).expect("fixture nonce");
        let _pipe = PipeStream::open_client(&name).expect("fixture connection");
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
                .env("UIPILOT_PEER_FIXTURE_NONCE", &nonce)
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
