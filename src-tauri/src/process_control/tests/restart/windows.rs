use super::super::super::{
    restart::{
        windows::created_process_identity_with, NativeLaunchInput, ProcessRestartAdapter,
        RestartAdapterError, RestartPathInput,
    },
    windows::WindowsProcessAdapter,
};
use std::{
    ffi::{OsStr, OsString},
    fs,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::symlink_dir,
        io::{AsHandle, AsRawHandle},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, GENERIC_WRITE, HANDLE,
            WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_READ, OPEN_EXISTING,
        },
        System::Threading::{
            GetCurrentProcess, GetExitCodeProcess, QueryFullProcessImageNameW, TerminateProcess,
            WaitForSingleObject, PROCESS_NAME_WIN32,
        },
    },
};

const MARKER: &str = "UIPILOT_RESTART_FEASIBILITY_MARKER";
const CONTROL: &str = "UIPILOT_RESTART_FEASIBILITY_CONTROL";
const MARKER_VALUE: &str = "explicit value 雪 with spaces";
const WAIT_MS: u32 = 10_000;

struct CleanupProcess {
    handle: HANDLE,
    finished: bool,
}
impl CleanupProcess {
    fn duplicate(process: &impl AsHandle) -> Self {
        let mut handle = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(process.as_handle().as_raw_handle()),
                GetCurrentProcess(),
                &mut handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }
        .unwrap();
        Self {
            handle,
            finished: false,
        }
    }
    fn wait(&mut self) {
        match unsafe { WaitForSingleObject(self.handle, WAIT_MS) } {
            WAIT_OBJECT_0 => self.finished = true,
            WAIT_TIMEOUT => panic!("child wait timed out"),
            WAIT_FAILED => panic!("child wait failed"),
            _ => panic!("unexpected child wait result"),
        }
    }
}
impl Drop for CleanupProcess {
    fn drop(&mut self) {
        if !self.finished {
            let _ = unsafe { TerminateProcess(self.handle, 0x72) };
            let _ = unsafe { WaitForSingleObject(self.handle, WAIT_MS) };
        }
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

struct OwnedTree(Option<PathBuf>);
impl OwnedTree {
    fn cleanup_asserted(&mut self) {
        let path = self.0.take().unwrap();
        fs::remove_dir_all(&path).unwrap();
        assert!(
            !path.exists(),
            "owned temporary tree remained after cleanup"
        );
    }
}
impl Drop for OwnedTree {
    fn drop(&mut self) {
        if let Some(path) = self.0.as_ref() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

struct GuardedCompiler(Option<Child>);
impl GuardedCompiler {
    fn finish(mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match self.0.as_mut().unwrap().try_wait().unwrap() {
                Some(status) => {
                    assert!(status.success(), "helper compilation failed: {status}");
                    self.0.take();
                    return;
                }
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                None => panic!("helper compilation timed out"),
            }
        }
    }
}
impl Drop for GuardedCompiler {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

const HELPER_SOURCE: &str = r#"
use std::{env, fs, path::PathBuf};
fn main() {
 let args=env::args().skip(1).collect::<Vec<_>>();
 let control=PathBuf::from(env::var_os("UIPILOT_RESTART_FEASIBILITY_CONTROL").unwrap());
 assert_eq!(PathBuf::from(args.last().unwrap().strip_prefix("control=").unwrap()),control);
 let marker=env::var("UIPILOT_RESTART_FEASIBILITY_MARKER").unwrap();
 let mut report=format!("marker={marker}\ncwd={}\nargc={}\n",env::current_dir().unwrap().display(),args.len());
 for (i,arg) in args.iter().enumerate(){report.push_str(&format!("arg{i}={arg}\n"));}
 fs::write(control.join("child-report.txt"),report).unwrap();
 std::thread::sleep(std::time::Duration::from_millis(500));
}
"#;

fn compile_helper(source: &Path, executable: &Path) {
    fs::write(source, HELPER_SOURCE).unwrap();
    GuardedCompiler(Some(
        Command::new("rustc")
            .arg(source)
            .args(["--crate-name", "uipilot_restart_feasibility_helper"])
            .arg("-o")
            .arg(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    ))
    .finish();
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn quote_arg(value: &OsStr) -> Vec<u16> {
    let units = value.encode_wide().collect::<Vec<_>>();
    if !units.iter().any(|u| matches!(*u, 9 | 32 | 34)) {
        return units;
    }
    let mut out = vec![34];
    let mut slashes = 0;
    for unit in units {
        if unit == 92 {
            slashes += 1;
        } else if unit == 34 {
            out.extend(std::iter::repeat_n(92, slashes * 2 + 1));
            out.push(34);
            slashes = 0;
        } else {
            out.extend(std::iter::repeat_n(92, slashes));
            out.push(unit);
            slashes = 0;
        }
    }
    out.extend(std::iter::repeat_n(92, slashes * 2));
    out.push(34);
    out
}
fn command_line(executable: &Path, args: &[&str]) -> Vec<u16> {
    let mut v = quote_arg(executable.as_os_str());
    for arg in args {
        v.push(32);
        v.extend(quote_arg(OsStr::new(arg)));
    }
    v.push(0);
    v
}
fn environment_block(control: &Path) -> Vec<u16> {
    let mut e = std::env::vars_os().collect::<Vec<_>>();
    e.retain(|(k, _)| k != MARKER && k != CONTROL);
    e.push((MARKER.into(), MARKER_VALUE.into()));
    e.push((CONTROL.into(), control.as_os_str().to_owned()));
    e.sort_by_key(|x| x.0.to_string_lossy().to_uppercase());
    let mut b = vec![];
    for (k, v) in e {
        b.extend(k.encode_wide());
        b.push(61);
        b.extend(v.encode_wide());
        b.push(0);
    }
    b.push(0);
    b
}
fn root(label: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("UiPilot {label} 雪 {} {n}", std::process::id()))
}
fn incompatible_rejected(path: &Path, directory: bool) {
    let p = wide(path.as_os_str());
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            Default::default()
        };
    for access in [GENERIC_WRITE.0, DELETE.0] {
        assert!(unsafe {
            CreateFileW(
                PCWSTR(p.as_ptr()),
                access,
                FILE_SHARE_READ,
                None,
                OPEN_EXISTING,
                flags,
                None,
            )
        }
        .is_err());
    }
}
fn wait_report(path: &Path, process: HANDLE) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
            let mut code = 0;
            unsafe { GetExitCodeProcess(process, &mut code) }.unwrap();
            panic!("child exited before handshake: {code:#x}");
        }
        assert!(Instant::now() < deadline, "handshake timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
fn image(process: HANDLE) -> PathBuf {
    let mut b = vec![0u16; 32768];
    let mut n = b.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(b.as_mut_ptr()), &mut n)
    }
    .unwrap();
    PathBuf::from(OsString::from_wide(&b[..n as usize]))
}

#[test]
fn restart_path_feasibility() {
    let root = root("restart feasibility");
    let mut tree = OwnedTree(Some(root.clone()));
    let bin = root.join("bin parent");
    let cwd = root.join("working directory 雪");
    let control = root.join("control handshake");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let exe = bin.join("owned child 雪.exe");
    compile_helper(&root.join("startup helper.rs"), &exe);
    let adapter = WindowsProcessAdapter;
    for paths in [
        RestartPathInput::new("relative.exe".into(), cwd.clone()),
        RestartPathInput::new(r"\\server\share\child.exe".into(), cwd.clone()),
        RestartPathInput::new(r"C:\owned.exe:stream".into(), cwd.clone()),
    ] {
        assert_eq!(
            adapter.acquire_path_leases(&paths).err(),
            Some(RestartAdapterError::UnsupportedPath)
        );
    }
    let link = root.join("reparse link");
    symlink_dir(&cwd, &link).unwrap();
    assert_eq!(
        adapter
            .acquire_path_leases(&RestartPathInput::new(exe.clone(), link.clone()))
            .err(),
        Some(RestartAdapterError::UnsupportedPath)
    );
    fs::remove_dir(&link).unwrap();
    assert_eq!(
        adapter
            .acquire_path_leases(&RestartPathInput::new(bin.join("missing.exe"), cwd.clone()))
            .err(),
        Some(RestartAdapterError::PathUnavailable)
    );
    let moved = root.join("bin moved");
    fs::rename(&bin, &moved).unwrap();
    fs::rename(&moved, &bin).unwrap();
    let leases = adapter
        .acquire_path_leases(&RestartPathInput::new(exe.clone(), cwd.clone()))
        .unwrap();
    incompatible_rejected(&exe, false);
    incompatible_rejected(&bin, true);
    incompatible_rejected(&cwd, true);
    assert!(fs::rename(&exe, bin.join("replacement.exe")).is_err());
    assert!(fs::rename(&bin, root.join("bin replaced")).is_err());
    assert!(fs::rename(&cwd, root.join("cwd replaced")).is_err());
    adapter.revalidate_path_leases(&leases).unwrap();
    let metadata = adapter.path_lease_metadata(&leases);
    assert!(metadata
        .executable()
        .to_string_lossy()
        .starts_with(r"\\?\Volume{"));
    assert!(metadata
        .working_directory()
        .to_string_lossy()
        .starts_with(r"\\?\Volume{"));
    assert_ne!(metadata.executable_file_identity(), (0, 0));
    let effective_exe = metadata.executable().to_owned();
    let effective_cwd = metadata.working_directory().to_owned();
    let reacquired = adapter
        .acquire_path_leases(&RestartPathInput::new(effective_exe, effective_cwd.clone()))
        .unwrap();
    drop(reacquired);
    let control_arg = format!("control={}", control.display());
    let args = [
        "plain",
        "space value",
        "quote\"value",
        "Unicode-雪",
        &control_arg,
    ];
    let child = adapter
        .create_process(
            &leases,
            &NativeLaunchInput::new(command_line(&exe, &args), environment_block(&control)),
        )
        .unwrap();
    let mut cleanup = CleanupProcess::duplicate(&child);
    let identity = adapter.created_process_identity(&child).unwrap();
    assert_ne!(identity.pid(), 0);
    assert_ne!(identity.creation_time(), 0);
    assert_eq!(image(cleanup.handle), exe);
    drop(child);
    drop(leases);
    let report_path = control.join("child-report.txt");
    wait_report(&report_path, cleanup.handle);
    assert_eq!(
        unsafe { WaitForSingleObject(cleanup.handle, 0) },
        WAIT_TIMEOUT,
        "dropping production CreatedProcess must not terminate a successful child"
    );
    let report = fs::read_to_string(report_path).unwrap();
    assert!(report.contains(&format!("marker={MARKER_VALUE}")));
    assert!(report.contains(&format!("cwd={}", effective_cwd.display())));
    assert!(report.contains("arg1=space value\narg2=quote\"value\narg3=Unicode-雪"));
    cleanup.wait();
    let moved = bin.join("moved.exe");
    fs::rename(&exe, &moved).unwrap();
    fs::rename(&moved, &exe).unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(HANDLE(std::ptr::null_mut()), 0) },
        WAIT_FAILED
    );
    tree.cleanup_asserted();
}

#[test]
fn restart_native_creation_failure_and_invalid_buffers_release_leases() {
    let root = root("restart create failure");
    let mut tree = OwnedTree(Some(root.clone()));
    let exe = root.join("not executable.exe");
    let cwd = root.join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(&exe, b"not PE").unwrap();
    let adapter = WindowsProcessAdapter;
    let leases = adapter
        .acquire_path_leases(&RestartPathInput::new(exe.clone(), cwd.clone()))
        .unwrap();
    for input in [
        NativeLaunchInput::new(vec![0], vec![0, 0]),
        NativeLaunchInput::new(vec![65, 0, 66, 0], vec![0, 0]),
        NativeLaunchInput::new(vec![65, 0], vec![65, 0]),
        NativeLaunchInput::new(vec![65, 0], vec![65, 0, 0, 66, 0, 0]),
    ] {
        assert!(matches!(
            adapter.create_process(&leases, &input),
            Err(RestartAdapterError::InvalidInput)
        ));
    }
    assert!(matches!(
        adapter.create_process(
            &leases,
            &NativeLaunchInput::new(wide(exe.as_os_str()), vec![0, 0])
        ),
        Err(RestartAdapterError::CreationFailed)
    ));
    drop(leases);
    fs::rename(&exe, root.join("released.exe")).unwrap();
    fs::rename(&cwd, root.join("released cwd")).unwrap();
    tree.cleanup_asserted();
}

#[test]
fn restart_path_input_rejects_malformed_native_spelling() {
    let adapter = WindowsProcessAdapter;
    let cwd = std::env::temp_dir();
    let invalid_utf16 = PathBuf::from(OsString::from_wide(&[
        b'C' as u16,
        b':' as u16,
        b'\\' as u16,
        0xD800,
    ]));
    let embedded_nul = PathBuf::from(OsString::from_wide(&[
        b'C' as u16,
        b':' as u16,
        b'\\' as u16,
        b'a' as u16,
        0,
        b'b' as u16,
    ]));
    for executable in [
        invalid_utf16,
        embedded_nul,
        PathBuf::from(r"\\?\C:\device.exe"),
        PathBuf::from(r"\\?\Volume{not-a-guid}\x.exe"),
    ] {
        assert_eq!(
            adapter
                .acquire_path_leases(&RestartPathInput::new(executable, cwd.clone()))
                .err(),
            Some(RestartAdapterError::UnsupportedPath)
        );
    }
}

#[test]
fn restart_returned_handle_identity_failures_are_stable() {
    assert_eq!(
        created_process_identity_with(41, || Err(())),
        Err(RestartAdapterError::IdentityUnavailable)
    );
    assert_eq!(
        created_process_identity_with(41, || Ok(0)),
        Err(RestartAdapterError::IdentityUnavailable)
    );
}
