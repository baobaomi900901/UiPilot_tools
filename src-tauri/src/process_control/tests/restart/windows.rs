use std::{
    ffi::{c_void, OsStr, OsString},
    fs,
    os::windows::{ffi::OsStrExt, fs::symlink_dir},
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx,
            GetFinalPathNameByHandleW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
            GetVolumePathNameW, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, OPEN_EXISTING,
            VOLUME_NAME_GUID,
        },
        System::Threading::{
            CreateProcessW, GetExitCodeProcess, QueryFullProcessImageNameW, TerminateProcess,
            WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
            PROCESS_NAME_WIN32, STARTUPINFOW,
        },
    },
};

const MARKER: &str = "UIPILOT_RESTART_FEASIBILITY_MARKER";
const CONTROL: &str = "UIPILOT_RESTART_FEASIBILITY_CONTROL";
const MARKER_VALUE: &str = "explicit value 雪 with spaces";
const WAIT_MS: u32 = 10_000;

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct OwnedCreatedProcess {
    process: OwnedHandle,
    thread: OwnedHandle,
    finished: bool,
}

impl OwnedCreatedProcess {
    fn from_information(info: PROCESS_INFORMATION) -> Self {
        // Ownership of both returned handles is established before any validation can fail.
        Self {
            process: OwnedHandle(info.hProcess),
            thread: OwnedHandle(info.hThread),
            finished: false,
        }
    }

    fn wait(&mut self) -> Result<(), &'static str> {
        match unsafe { WaitForSingleObject(self.process.0, WAIT_MS) } {
            WAIT_OBJECT_0 => {
                self.finished = true;
                Ok(())
            }
            WAIT_TIMEOUT => Err("child wait timed out"),
            WAIT_FAILED => Err("child wait failed"),
            _ => Err("unexpected child wait result"),
        }
    }
}

impl Drop for OwnedCreatedProcess {
    fn drop(&mut self) {
        if !self.finished {
            let _ = unsafe { TerminateProcess(self.process.0, 0x72) };
            let _ = unsafe { WaitForSingleObject(self.process.0, WAIT_MS) };
        }
        // Fields close the independent process cleanup handle and thread handle.
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
                    assert!(
                        status.success(),
                        "owned helper compilation failed: {status}"
                    );
                    self.0.take();
                    return;
                }
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                None => panic!("owned helper compilation timed out"),
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
    let args = env::args().skip(1).collect::<Vec<_>>();
    let control = PathBuf::from(env::var_os("UIPILOT_RESTART_FEASIBILITY_CONTROL").unwrap());
    let control_arg = args.last().and_then(|arg| arg.strip_prefix("control=")).unwrap();
    assert_eq!(PathBuf::from(control_arg), control);
    let marker = env::var("UIPILOT_RESTART_FEASIBILITY_MARKER").unwrap();
    let cwd = env::current_dir().unwrap();
    let mut report = format!("marker={marker}\ncwd={}\nargc={}\n", cwd.display(), args.len());
    for (index, arg) in args.iter().enumerate() {
        report.push_str(&format!("arg{index}={arg}\n"));
    }
    fs::write(control.join("child-report.txt"), report).unwrap();
}
"#;

fn compile_owned_helper(source: &Path, executable: &Path) {
    fs::write(source, HELPER_SOURCE).unwrap();
    let child = Command::new("rustc")
        .arg(source)
        .args(["--crate-name", "uipilot_restart_feasibility_helper"])
        .arg("-o")
        .arg(executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    GuardedCompiler(Some(child)).finish();
}

#[derive(Clone)]
struct VolumePath {
    guid_root: String,
    relative: String,
    full: String,
    filesystem: String,
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_str(value: &str) -> Vec<u16> {
    wide(OsStr::new(value))
}

fn string_from_nul(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).unwrap()
}

fn resolve_volume(path: &Path) -> VolumePath {
    let display = path.to_string_lossy().replace('/', "\\");
    assert!(
        display.get(2..).is_some_and(|tail| !tail.contains(':')),
        "alternate data streams are unsupported"
    );
    assert!(
        !display.starts_with("\\\\"),
        "network paths are unsupported"
    );
    let display_wide = wide_str(&display);
    let mut mount = vec![0u16; 32_768];
    unsafe { GetVolumePathNameW(PCWSTR(display_wide.as_ptr()), &mut mount) }.unwrap();
    let mount = string_from_nul(&mount);
    let mount_wide = wide_str(&mount);
    let mut guid = vec![0u16; 128];
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount_wide.as_ptr()), &mut guid) }.unwrap();
    let mut filesystem = vec![0u16; 64];
    unsafe {
        GetVolumeInformationW(
            PCWSTR(mount_wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut filesystem),
        )
    }
    .unwrap();
    let guid_root = string_from_nul(&guid);
    assert!(guid_root.starts_with("\\\\?\\Volume{"));
    let relative = display[mount.len()..].trim_matches('\\').to_owned();
    assert!(!relative.is_empty());
    let full = format!("{}{}", guid_root, relative);
    VolumePath {
        guid_root,
        relative,
        full,
        filesystem: string_from_nul(&filesystem),
    }
}

fn open_component(
    path: &str,
    directory: bool,
    final_file: bool,
) -> Result<OwnedHandle, windows::core::Error> {
    let path = wide_str(path);
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    let access = if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
        FILE_LIST_DIRECTORY.0 | FILE_READ_ATTRIBUTES.0
    } else if final_file {
        GENERIC_READ.0
    } else {
        FILE_READ_ATTRIBUTES.0
    };
    unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            access,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .map(OwnedHandle)
}

fn is_directory_and_not_reparse(
    handle: HANDLE,
    expected_directory: bool,
) -> Result<(), &'static str> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast::<c_void>(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    }
    .map_err(|_| "component inspection failed")?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("reparse component rejected");
    }
    let directory = tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    if directory != expected_directory {
        return Err("component kind changed");
    }
    Ok(())
}

fn final_guid_path(handle: HANDLE) -> Result<String, &'static str> {
    let mut buffer = vec![0u16; 32_768];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_GUID) } as usize;
    if written == 0 || written >= buffer.len() {
        return Err("final path query failed");
    }
    String::from_utf16(&buffer[..written]).map_err(|_| "final path was not UTF-16")
}

fn lease_path(
    path: &VolumePath,
    final_directory: bool,
    fail_at: Option<usize>,
    failure_probe: Option<&dyn Fn()>,
) -> Result<Vec<OwnedHandle>, &'static str> {
    let components = path.relative.split('\\').collect::<Vec<_>>();
    let mut leases = Vec::with_capacity(components.len());
    let mut current = path.guid_root.trim_end_matches('\\').to_owned();
    for (index, component) in components.iter().enumerate() {
        if fail_at == Some(index) {
            if let Some(probe) = failure_probe {
                probe();
            }
            return Err("injected partial acquisition failure");
        }
        current.push('\\');
        current.push_str(component);
        let last = index + 1 == components.len();
        let directory = !last || final_directory;
        let handle = open_component(&current, directory, last && !final_directory)
            .map_err(|_| "component open failed")?;
        is_directory_and_not_reparse(handle.0, directory)?;
        let observed = final_guid_path(handle.0)?;
        if !observed.eq_ignore_ascii_case(&current) {
            return Err("component resolved to an unexpected path");
        }
        leases.push(handle);
    }
    Ok(leases)
}

fn assert_incompatible_open_rejected(path: &Path, directory: bool) {
    let path = wide(path.as_os_str());
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            Default::default()
        };
    for access in [GENERIC_WRITE.0, DELETE.0] {
        let opened = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                access,
                FILE_SHARE_READ,
                None,
                OPEN_EXISTING,
                flags,
                None,
            )
        };
        assert!(
            opened.is_err(),
            "incompatible write/delete open unexpectedly succeeded"
        );
    }
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

fn command_line(executable: &str, args: &[&str]) -> Vec<u16> {
    let mut result = quote_arg(OsStr::new(executable));
    for arg in args {
        result.push(32);
        result.extend(quote_arg(OsStr::new(arg)));
    }
    result.push(0);
    result
}

fn environment_block(control: &Path) -> Vec<u16> {
    let mut entries = std::env::vars_os().collect::<Vec<_>>();
    entries.retain(|(key, _)| key != MARKER && key != CONTROL);
    entries.push((OsString::from(MARKER), OsString::from(MARKER_VALUE)));
    entries.push((OsString::from(CONTROL), control.as_os_str().to_owned()));
    entries.sort_by(|a, b| {
        a.0.to_string_lossy()
            .to_uppercase()
            .cmp(&b.0.to_string_lossy().to_uppercase())
    });
    let mut block = Vec::new();
    for (key, value) in entries {
        block.extend(key.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn spawn_leased(executable: &VolumePath, cwd: &VolumePath, control: &Path) -> OwnedCreatedProcess {
    let payloads = ["plain", "space value", "quote\"value", "Unicode-雪"];
    let control_arg = format!("control={}", control.display());
    let args = vec![
        payloads[0],
        payloads[1],
        payloads[2],
        payloads[3],
        control_arg.as_str(),
    ];
    let application = wide_str(&executable.full);
    let cwd_wide = wide_str(&cwd.full);
    let mut command = command_line(&executable.full, &args);
    let environment = environment_block(control);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT,
            Some(environment.as_ptr().cast()),
            PCWSTR(cwd_wide.as_ptr()),
            &startup,
            &mut info,
        )
    }
    .unwrap();
    OwnedCreatedProcess::from_information(info)
}

fn wait_for(path: &Path, process: HANDLE) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
            let mut exit_code = 0;
            unsafe { GetExitCodeProcess(process, &mut exit_code) }.unwrap();
            panic!("child exited before handshake with code {exit_code:#x}");
        }
        assert!(
            Instant::now() < deadline,
            "bounded child handshake timed out"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn process_image(process: HANDLE) -> String {
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    }
    .unwrap();
    String::from_utf16(&buffer[..size as usize]).unwrap()
}

fn assert_cwd_identity(observed: &str, expected: &VolumePath) {
    let handle = open_component(observed, true, false).unwrap();
    is_directory_and_not_reparse(handle.0, true).unwrap();
    assert!(
        final_guid_path(handle.0)
            .unwrap()
            .eq_ignore_ascii_case(&expected.full),
        "child effective CWD resolved to a different directory"
    );
}

#[test]
fn restart_path_feasibility() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "UiPilot restart feasibility 雪 {} {nonce}",
        std::process::id()
    ));
    let mut tree = OwnedTree(Some(root.clone()));
    let bin_parent = root.join("bin parent");
    let cwd = root.join("working directory 雪");
    let control = root.join("control handshake");
    fs::create_dir_all(&bin_parent).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let executable = bin_parent.join("owned child 雪.exe");
    compile_owned_helper(&root.join("startup helper.rs"), &executable);

    let exe_volume = resolve_volume(&executable);
    let cwd_volume = resolve_volume(&cwd);
    assert_eq!(exe_volume.guid_root, cwd_volume.guid_root);
    assert!(!exe_volume.filesystem.is_empty());

    // Unsupported aliases and ADS syntax are rejected before any creation attempt.
    assert!(catch_unwind(|| resolve_volume(Path::new(r"\\server\share\child.exe"))).is_err());
    assert!(catch_unwind(|| resolve_volume(Path::new(r"C:\owned.exe:stream"))).is_err());

    let link = root.join("reparse link");
    symlink_dir(&cwd, &link).expect("owned reparse fixture must be creatable for this gate");
    let link_volume = resolve_volume(&link);
    assert!(matches!(
        lease_path(&link_volume, true, None, None),
        Err("reparse component rejected")
    ));
    fs::remove_dir(&link).unwrap();

    // Partial failure and unwind release every acquired lease and restore cleanup ability.
    let moved_bin_parent = root.join("bin parent partial-failure moved");
    let fail_before_final_executable = exe_volume.relative.split('\\').count() - 1;
    let probe_partial = || {
        assert_incompatible_open_rejected(&bin_parent, true);
        assert!(fs::rename(&bin_parent, &moved_bin_parent).is_err());
    };
    assert!(lease_path(
        &exe_volume,
        false,
        Some(fail_before_final_executable),
        Some(&probe_partial),
    )
    .is_err());
    fs::rename(&bin_parent, &moved_bin_parent).unwrap();
    fs::rename(&moved_bin_parent, &bin_parent).unwrap();
    let moved_cwd = root.join("working directory unwind moved");
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _leases = lease_path(&cwd_volume, true, None, None).unwrap();
        assert_incompatible_open_rejected(&cwd, true);
        assert!(fs::rename(&cwd, &moved_cwd).is_err());
        panic!("intentional lease unwind");
    }));
    assert!(unwind.is_err());
    fs::rename(&cwd, &moved_cwd).unwrap();
    fs::rename(&moved_cwd, &cwd).unwrap();

    let exe_leases = lease_path(&exe_volume, false, None, None).unwrap();
    let cwd_leases = lease_path(&cwd_volume, true, None, None).unwrap();
    assert_incompatible_open_rejected(&executable, false);
    assert_incompatible_open_rejected(&bin_parent, true);
    assert_incompatible_open_rejected(&cwd, true);
    assert!(fs::rename(&executable, bin_parent.join("replacement.exe")).is_err());
    assert!(fs::rename(&bin_parent, root.join("bin parent replaced")).is_err());
    assert!(fs::rename(&cwd, root.join("working directory replaced")).is_err());

    let mut child = spawn_leased(&exe_volume, &cwd_volume, &control);
    let observed_image = process_image(child.process.0);
    assert!(
        observed_image.eq_ignore_ascii_case(executable.to_string_lossy().as_ref()),
        "created process image differed from owned executable"
    );
    drop(cwd_leases);
    drop(exe_leases);
    let report_path = control.join("child-report.txt");
    wait_for(&report_path, child.process.0);
    let report = fs::read_to_string(&report_path).unwrap();
    let lines = report.lines().collect::<Vec<_>>();
    let control_arg = format!("control={}", control.display());
    assert_eq!(lines[0], format!("marker={MARKER_VALUE}"));
    assert_eq!(lines[2], "argc=5");
    assert_eq!(
        &lines[3..],
        [
            "arg0=plain",
            "arg1=space value",
            "arg2=quote\"value",
            "arg3=Unicode-雪",
            format!("arg4={control_arg}").as_str(),
        ]
    );
    let observed_cwd = lines[1].strip_prefix("cwd=").unwrap();
    assert_cwd_identity(observed_cwd, &cwd_volume);
    child.wait().unwrap();

    let moved_executable = bin_parent.join("owned child moved.exe");
    fs::rename(&executable, &moved_executable).unwrap();
    fs::rename(&moved_executable, &executable).unwrap();
    let moved_parent = root.join("bin parent moved");
    fs::rename(&bin_parent, &moved_parent).unwrap();
    fs::rename(&moved_parent, &bin_parent).unwrap();

    assert_eq!(
        unsafe { WaitForSingleObject(HANDLE(std::ptr::null_mut()), 0) },
        WAIT_FAILED
    );
    println!(
        "supported volume-guid path={} filesystem={} component leases=FILE_SHARE_READ create=CreateProcessW",
        exe_volume.guid_root, exe_volume.filesystem
    );
    tree.cleanup_asserted();
}
