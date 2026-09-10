//! Ordinary-token, owned-child experiment only. Never linked into the installer checker.
use std::{
    ffi::c_void,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    process::{Child, Command, Stdio},
    ptr, thread,
    time::{Duration, Instant},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, LocalFree, FILETIME, HANDLE, HLOCAL},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG},
            GetTokenInformation, RevertToSelf, SecurityIdentification, TokenGroups,
            TokenImpersonationLevel, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            SECURITY_IMPERSONATION_LEVEL, TOKEN_GROUPS, TOKEN_QUERY,
        },
        Storage::FileSystem::{
            CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES,
            FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
            SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
        },
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
                GetNamedPipeServerProcessId, ImpersonateNamedPipeClient, SetNamedPipeHandleState,
                PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
            },
            Threading::{
                GetCurrentProcess, GetCurrentThread, GetProcessTimes, OpenProcess,
                OpenProcessToken, OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    },
};

type Probe<T> = Result<T, &'static str>;
const ACCESS: u32 = 0x00100183; // Data read/write + attributes + synchronize; NOT create-instance.
const TIMEOUT: Duration = Duration::from_secs(15);
struct Owned(HANDLE);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}
struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill(); // Only the child created by this experiment.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        eprintln!("OwnedFixtureCleanupUnconfirmed");
    }
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}
fn created(handle: HANDLE) -> Probe<u64> {
    let mut times = [FILETIME::default(); 4];
    unsafe {
        GetProcessTimes(
            handle,
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        )
    }
    .map_err(|_| "ProcessIdentityUnavailable")?;
    Ok((u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime))
}

fn logon_sid() -> Probe<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|_| "TokenUnavailable")?;
    let token = Owned(token);
    let mut size = 0;
    unsafe {
        let _ = GetTokenInformation(token.0, TokenGroups, None, 0, &mut size);
    }
    if size == 0 || size > 65536 {
        return Err("InvalidTokenSize");
    }
    // TOKEN_GROUPS contains pointers, so its storage must be pointer-aligned.
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenGroups,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
        .map_err(|_| "TokenUnavailable")?;
        let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
        for group in std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        {
            if group.Attributes & 0xc0000000 != 0xc0000000 {
                continue;
            }
            let mut sid = PWSTR::null();
            ConvertSidToStringSidW(group.Sid, &mut sid).map_err(|_| "InvalidLogonSid")?;
            let _allocation = Allocation(sid.0.cast());
            return sid.to_string().map_err(|_| "InvalidLogonSid");
        }
    }
    Err("LogonSidUnavailable")
}

fn server(name: &[u16], sddl: &str) -> Probe<Owned> {
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let sddl = wide(sddl);
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1,
            &mut sd,
            None,
        )
        .map_err(|_| "InvalidPipeSecurity")?;
        let _allocation = Allocation(sd.0);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let pipe = CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            Some(&attributes),
        );
        if pipe.is_invalid() {
            return Err("PipeCreationDenied");
        }
        Ok(Owned(pipe))
    }
}

fn open_client(name: &[u16], deadline: Instant) -> Probe<Owned> {
    loop {
        if let Ok(pipe) = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                ACCESS,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0),
                None,
            )
        } {
            let pipe = Owned(pipe);
            unsafe { SetNamedPipeHandleState(pipe.0, Some(&PIPE_NOWAIT), None, None) }
                .map_err(|_| "NonblockingModeUnavailable")?;
            return Ok(pipe);
        }
        if Instant::now() >= deadline {
            return Err("ConnectDeadline");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn read_frame(pipe: HANDLE, deadline: Instant) -> Probe<[u8; 32]> {
    let mut frame = [0u8; 32];
    let mut offset = 0;
    while offset < frame.len() {
        let mut count = 0;
        let result = unsafe { ReadFile(pipe, Some(&mut frame[offset..]), Some(&mut count), None) };
        if result.is_ok() {
            offset += count as usize;
        }
        if offset == frame.len() {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            return Err("ReadDeadline");
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(frame)
}
fn write_frame(pipe: HANDLE, frame: &[u8; 32]) -> Probe<()> {
    let mut written = 0;
    unsafe { WriteFile(pipe, Some(frame), Some(&mut written), None) }.map_err(|_| "WriteFailed")?;
    if written != 32 {
        return Err("PartialWrite");
    }
    Ok(())
}

fn assert_identification_only(pipe: HANDLE) -> Probe<()> {
    unsafe { ImpersonateNamedPipeClient(pipe) }.map_err(|_| "ImpersonationUnavailable")?;
    // Revert on every return/unwind, before any fixture cleanup or file operation.
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            if unsafe { RevertToSelf() }.is_err() {
                std::process::abort();
            }
        }
    }
    let _revert = Revert;
    let mut token = HANDLE::default();
    unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) }
        .map_err(|_| "ImpersonationTokenUnavailable")?;
    let token = Owned(token);
    let mut level = SECURITY_IMPERSONATION_LEVEL::default();
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenImpersonationLevel,
            Some(ptr::addr_of_mut!(level).cast()),
            std::mem::size_of_val(&level) as u32,
            &mut size,
        )
    }
    .map_err(|_| "ImpersonationLevelUnavailable")?;
    if level != SecurityIdentification {
        return Err("UnsafeImpersonationLevel");
    }
    Ok(())
}

#[test]
#[ignore = "Owned child entrypoint; launched explicitly by the pipe experiment"]
fn child_fixture() {
    fn run() -> Probe<()> {
        let name =
            wide(&std::env::var("UIPILOT_PIPE_PROBE_NAME").map_err(|_| "FixtureInputMissing")?);
        let expected_pid: u32 = std::env::var("UIPILOT_PIPE_PROBE_PID")
            .map_err(|_| "FixtureInputMissing")?
            .parse()
            .map_err(|_| "FixtureInputInvalid")?;
        let expected_time: u64 = std::env::var("UIPILOT_PIPE_PROBE_TIME")
            .map_err(|_| "FixtureInputMissing")?
            .parse()
            .map_err(|_| "FixtureInputInvalid")?;
        let deadline = Instant::now() + TIMEOUT;
        let pipe = open_client(&name, deadline)?;
        let mut pid = 0;
        unsafe { GetNamedPipeServerProcessId(pipe.0, &mut pid) }
            .map_err(|_| "ServerIdentityUnavailable")?;
        if pid != expected_pid {
            return Err("ServerMismatch");
        }
        let parent = Owned(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .map_err(|_| "ServerIdentityUnavailable")?,
        );
        if created(parent.0)? != expected_time {
            return Err("ServerChanged");
        }
        let challenge = read_frame(pipe.0, deadline)?;
        write_frame(pipe.0, &challenge)?;
        if read_frame(pipe.0, deadline)? != challenge {
            return Err("AckMismatch");
        }
        Ok(())
    }
    assert_eq!(run(), Ok(()));
}

#[test]
fn owned_cross_process_pipe_binds_peers_and_limits_impersonation() {
    fn run() -> Probe<()> {
        let mut nonce = [0u8; 32];
        unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
            .ok()
            .map_err(|_| "RandomUnavailable")?;
        let suffix: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let name_text = format!(r"\\.\pipe\uipilot-owned-probe-{suffix}");
        let name = wide(&name_text);
        let sddl = format!("D:P(A;;0x{ACCESS:08x};;;{})", logon_sid()?);
        let pipe = server(&name, &sddl)?;
        if server(&name, &sddl).is_ok() {
            return Err("DuplicateServerAllowed");
        }
        // Establish the NOWAIT listening state; no blocking Win32 wait.
        unsafe {
            let _ = ConnectNamedPipe(pipe.0, None);
        }
        let deadline = Instant::now() + TIMEOUT;
        let child =
            Command::new(std::env::current_exe().map_err(|_| "FixtureExecutableUnavailable")?)
                .args(["--exact", "pipe_probe::child_fixture", "--ignored"])
                .env("UIPILOT_PIPE_PROBE_NAME", name_text)
                .env("UIPILOT_PIPE_PROBE_PID", std::process::id().to_string())
                .env(
                    "UIPILOT_PIPE_PROBE_TIME",
                    created(unsafe { GetCurrentProcess() })?.to_string(),
                )
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "FixtureSpawnFailed")?;
        let mut child = Fixture(child);
        loop {
            let mut pid = 0;
            if unsafe { GetNamedPipeClientProcessId(pipe.0, &mut pid) }.is_ok() {
                if pid != child.0.id() {
                    return Err("ClientMismatch");
                }
                let peer = Owned(
                    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                        .map_err(|_| "ClientIdentityUnavailable")?,
                );
                if created(peer.0)? != created(HANDLE(child.0.as_raw_handle()))? {
                    return Err("ClientChanged");
                }
                break;
            }
            if Instant::now() >= deadline {
                return Err("ClientConnectDeadline");
            }
            thread::sleep(Duration::from_millis(5));
        }
        write_frame(pipe.0, &nonce)?;
        if read_frame(pipe.0, deadline)? != nonce {
            return Err("ChallengeMismatch");
        }
        assert_identification_only(pipe.0)?;
        write_frame(pipe.0, &nonce)?;
        loop {
            if let Some(status) = child.0.try_wait().map_err(|_| "FixtureWaitFailed")? {
                if !status.success() {
                    return Err("FixtureRejected");
                }
                break;
            }
            if Instant::now() >= deadline {
                return Err("FixtureExitDeadline");
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
    assert_eq!(run(), Ok(()));
}
