//! Opt-in, non-destructive UAC experiment. No process operation or plugin entry.
#[path = "../native.rs"]
mod native;
#[path = "../policy.rs"]
mod policy;
use policy::{trusted_writer, unsafe_grant};
#[path = "../pipe_probe.rs"]
mod pipe_probe;
#[path = "../uac_identity.rs"]
mod uac_identity;

use pipe_probe::{created, wide, Owned, Probe};
use std::{
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{ERROR_CANCELLED, HANDLE},
        Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG},
        System::{
            Pipes::{ConnectNamedPipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
            Threading::{
                GetCurrentProcess, GetExitCodeProcess, GetProcessId, OpenProcess,
                QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            },
        },
        UI::Shell::{
            ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        },
        UI::WindowsAndMessaging::SW_HIDE,
    },
};

fn verify_image(handle: HANDLE, expected: &Path) -> Probe<()> {
    let mut text = vec![0u16; 32768];
    let mut len = text.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(text.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(|_| "PeerImageUnavailable")?;
    let actual = String::from_utf16(&text[..len as usize]).map_err(|_| "PeerImageInvalid")?;
    if Path::new(&actual) != expected {
        return Err("PeerImageMismatch");
    }
    Ok(())
}

fn launch(executable: std::path::PathBuf, parameters: String) -> Probe<Owned> {
    let (sender, receiver) = mpsc::sync_channel(0);
    thread::spawn(move || {
        // COM apartment is local to this launch worker, not the host UI thread.
        use windows::Win32::System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
        };
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        if !initialized {
            let _ = sender.send(Err("ComUnavailable"));
            return;
        }
        let file = wide(&executable.to_string_lossy());
        let cwd = wide(&executable.parent().expect("fixed parent").to_string_lossy());
        let params = wide(&parameters);
        let verb = wide("runas");
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            lpDirectory: PCWSTR(cwd.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        let result = unsafe { ShellExecuteExW(&mut info) };
        unsafe {
            CoUninitialize();
        }
        match result {
            Ok(()) if !info.hProcess.is_invalid() => {
                let process = Owned(info.hProcess);
                // Win32 process handles are process-wide. Transfer ownership via
                // an integer; on expired receiver the worker closes its own handle.
                if sender.send(Ok(process.0 .0 as usize)).is_ok() {
                    std::mem::forget(process);
                }
            }
            Ok(()) => {
                let _ = sender.send(Err("LaunchHandleMissing"));
            }
            Err(error) => {
                let code = if error.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0)
                {
                    "UacCancelled"
                } else {
                    "LaunchFailed"
                };
                let _ = sender.send(Err(code));
            }
        }
    });
    match receiver.recv_timeout(Duration::from_secs(120)) {
        Ok(Ok(raw)) => Ok(Owned(HANDLE(raw as *mut _))),
        Ok(Err(code)) => Err(code),
        Err(_) => Err("LaunchExpired"),
    }
}

fn helper(executable: &Path, args: &[String]) -> Probe<()> {
    if args.len() != 4 || args[1].len() != 64 || !args[1].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("InvalidBootstrap");
    }
    let expected_pid: u32 = args[2].parse().map_err(|_| "InvalidBootstrap")?;
    let expected_time: u64 = args[3].parse().map_err(|_| "InvalidBootstrap")?;
    if expected_pid == 0 || expected_time == 0 {
        return Err("InvalidBootstrap");
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let name = wide(&format!(r"\\.\pipe\uipilot-uac-probe-{}", args[1]));
    let pipe = pipe_probe::open_client(&name, deadline)?;
    let mut pid = 0;
    unsafe { GetNamedPipeServerProcessId(pipe.0, &mut pid) }.map_err(|_| "PeerUnavailable")?;
    if pid != expected_pid {
        return Err("PeerMismatch");
    }
    let host = Owned(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map_err(|_| "PeerUnavailable")?,
    );
    if created(host.0)? != expected_time {
        return Err("PeerChanged");
    }
    verify_image(host.0, executable)?;
    uac_identity::verify_pair(host.0, unsafe { GetCurrentProcess() })?;
    let challenge = pipe_probe::read_frame(pipe.0, deadline)?;
    if unsafe { WaitForSingleObject(host.0, 0) }.0 != 258 {
        return Err("HostNotAlive");
    }
    pipe_probe::write_frame(pipe.0, &challenge)?;
    if pipe_probe::read_frame(pipe.0, deadline)? != challenge {
        return Err("AckMismatch");
    }
    Ok(())
}

fn host(executable: &Path) -> Probe<()> {
    uac_identity::verify_host(unsafe { GetCurrentProcess() })?;
    let mut nonce = [0u8; 32];
    unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .map_err(|_| "RandomUnavailable")?;
    let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let name = wide(&format!(r"\\.\pipe\uipilot-uac-probe-{suffix}"));
    let sddl = format!("D:P(A;;0x00100183;;;{})", pipe_probe::logon_sid()?);
    let pipe = pipe_probe::server(&name, &sddl)?;
    unsafe {
        let _ = ConnectNamedPipe(pipe.0, None);
    }
    let params = format!(
        "helper {suffix} {} {}",
        std::process::id(),
        created(unsafe { GetCurrentProcess() })?
    );
    let child = launch(executable.to_owned(), params)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut pid = 0;
        if unsafe { GetNamedPipeClientProcessId(pipe.0, &mut pid) }.is_ok() {
            if pid != unsafe { GetProcessId(child.0) } {
                return Err("PeerMismatch");
            }
            let peer = Owned(
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                    .map_err(|_| "PeerUnavailable")?,
            );
            if created(peer.0)? != created(child.0)? {
                return Err("PeerChanged");
            }
            verify_image(child.0, executable)?;
            uac_identity::verify_pair(unsafe { GetCurrentProcess() }, child.0)?;
            break;
        }
        if Instant::now() >= deadline {
            return Err("HandshakeExpired");
        }
        thread::sleep(Duration::from_millis(5));
    }
    pipe_probe::write_frame(pipe.0, &nonce)?;
    if pipe_probe::read_frame(pipe.0, deadline)? != nonce {
        return Err("ChallengeMismatch");
    }
    pipe_probe::assert_identification_only(pipe.0)?;
    pipe_probe::write_frame(pipe.0, &nonce)?;
    if unsafe { WaitForSingleObject(child.0, 5000) }.0 != 0 {
        return Err("HelperExitUnconfirmed");
    }
    let mut exit_code = 1;
    unsafe { GetExitCodeProcess(child.0, &mut exit_code) }.map_err(|_| "HelperExitUnconfirmed")?;
    if exit_code != 0 {
        return Err("HelperFailed");
    }
    Ok(())
}

fn run() -> Probe<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.as_slice() == ["self-token-check"] {
        return uac_identity::verify_host(unsafe { GetCurrentProcess() });
    }
    // No UAC path is reachable from a user-writable/development location.
    let (executable, _leases) = native::probe_installation()?;
    match args.first().map(String::as_str) {
        Some("host") if args.len() == 1 => host(&executable),
        Some("helper") => helper(&executable, &args),
        Some("inspect") if args.len() == 1 => Ok(()),
        _ => Err("InvalidArguments"),
    }
}

fn main() {
    match run() {
        Ok(()) => println!("UAC_PROBE_COMMAND_PASS"),
        Err(code) => {
            eprintln!("{code}");
            std::process::exit(1);
        }
    }
}
