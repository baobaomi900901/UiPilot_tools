use std::{
    ffi::{c_void, OsStr, OsString},
    fs,
    os::windows::{ffi::OsStrExt, fs::symlink_dir},
    path::{Path, PathBuf},
    panic::{catch_unwind, AssertUnwindSafe},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, HANDLE, GENERIC_READ, GENERIC_WRITE, WAIT_FAILED,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandleEx, GetFinalPathNameByHandleW, DELETE,
            GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
            FileAttributeTagInfo, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, OPEN_EXISTING,
            VOLUME_NAME_GUID,
        },
        System::Threading::{
            CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
            CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
        },
    },
};

const CHILD_TEST: &str =
    "process_control::tests::restart::windows::restart_path_feasibility_child";
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

struct OwnedTree(PathBuf);

impl Drop for OwnedTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).unwrap()
}

fn resolve_volume(path: &Path) -> VolumePath {
    let display = path.to_string_lossy().replace('/', "\\");
    assert!(
        display.get(2..).is_some_and(|tail| !tail.contains(':')),
        "alternate data streams are unsupported"
    );
    assert!(!display.starts_with("\\\\"), "network paths are unsupported");
    let display_wide = wide_str(&display);
    let mut mount = vec![0u16; 32_768];
    unsafe { GetVolumePathNameW(PCWSTR(display_wide.as_ptr()), &mut mount) }.unwrap();
    let mount = string_from_nul(&mount);
    let mount_wide = wide_str(&mount);
    let mut guid = vec![0u16; 128];
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount_wide.as_ptr()), &mut guid) }.unwrap();
    let mut filesystem = vec![0u16; 64];
    unsafe { GetVolumeInformationW(PCWSTR(mount_wide.as_ptr()), None, None, None, None, Some(&mut filesystem)) }
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

fn open_component(path: &str, directory: bool, final_file: bool) -> Result<OwnedHandle, windows::core::Error> {
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

fn is_directory_and_not_reparse(handle: HANDLE, expected_directory: bool) -> Result<(), &'static str> {
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
    let written = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_GUID) } as usize;
    if written == 0 || written >= buffer.len() {
        return Err("final path query failed");
    }
    String::from_utf16(&buffer[..written]).map_err(|_| "final path was not UTF-16")
}

fn lease_path(path: &VolumePath, final_directory: bool, fail_at: Option<usize>) -> Result<Vec<OwnedHandle>, &'static str> {
    let components = path.relative.split('\\').collect::<Vec<_>>();
    let mut leases = Vec::with_capacity(components.len());
    let mut current = path.guid_root.trim_end_matches('\\').to_owned();
    for (index, component) in components.iter().enumerate() {
        if fail_at == Some(index) {
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
        | if directory { FILE_FLAG_BACKUP_SEMANTICS } else { Default::default() };
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
        assert!(opened.is_err(), "incompatible write/delete open unexpectedly succeeded");
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
    entries.sort_by(|a, b| a.0.to_string_lossy().to_uppercase().cmp(&b.0.to_string_lossy().to_uppercase()));
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
        "--ignored", "--exact", CHILD_TEST, "--test-threads=1",
        "--skip", payloads[0], "--skip", payloads[1], "--skip", payloads[2], "--skip", payloads[3],
        "--skip", control_arg.as_str(),
    ];
    let application = wide_str(&executable.full);
    let cwd_wide = wide_str(&cwd.full);
    let mut command = command_line(&executable.full, &args);
    let environment = environment_block(control);
    let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
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
        assert!(Instant::now() < deadline, "bounded child handshake timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn restart_path_feasibility() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("UiPilot restart feasibility 雪 {} {nonce}", std::process::id()));
    let _tree = OwnedTree(root.clone());
    let bin_parent = root.join("bin parent");
    let cwd = root.join("working directory 雪");
    let control = root.join("control handshake");
    fs::create_dir_all(&bin_parent).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let executable = bin_parent.join("owned child 雪.exe");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();

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
        lease_path(&link_volume, true, None),
        Err("reparse component rejected")
    ));
    fs::remove_dir(&link).unwrap();

    // Partial failure and unwind release every acquired lease and restore cleanup ability.
    assert!(lease_path(&exe_volume, false, Some(2)).is_err());
    let probe = root.join("partial-release-probe");
    fs::create_dir(&probe).unwrap();
    let renamed_probe = root.join("partial-release-probe-renamed");
    fs::rename(&probe, &renamed_probe).unwrap();
    fs::rename(&renamed_probe, &probe).unwrap();
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _leases = lease_path(&cwd_volume, true, None).unwrap();
        panic!("intentional lease unwind");
    }));
    assert!(unwind.is_err());
    fs::rename(&probe, &renamed_probe).unwrap();
    fs::rename(&renamed_probe, &probe).unwrap();

    let exe_leases = lease_path(&exe_volume, false, None).unwrap();
    let cwd_leases = lease_path(&cwd_volume, true, None).unwrap();
    assert_incompatible_open_rejected(&executable, false);
    assert_incompatible_open_rejected(&bin_parent, true);
    assert_incompatible_open_rejected(&cwd, true);
    assert!(fs::rename(&executable, bin_parent.join("replacement.exe")).is_err());
    assert!(fs::rename(&bin_parent, root.join("bin parent replaced")).is_err());
    assert!(fs::rename(&cwd, root.join("working directory replaced")).is_err());

    let mut child = spawn_leased(&exe_volume, &cwd_volume, &control);
    let report_path = control.join("child-report.txt");
    wait_for(&report_path, child.process.0);
    let report = fs::read_to_string(&report_path).unwrap();
    assert!(report.contains(&format!("marker={MARKER_VALUE}")));
    assert!(report.contains("space value"));
    assert!(report.contains("quote\\\"value") || report.contains("quote\"value"));
    assert!(report.contains("Unicode-雪"));
    assert!(report.contains(&format!("cwd={}", cwd_volume.full)));
    child.wait().unwrap();

    drop(cwd_leases);
    drop(exe_leases);
    let moved_executable = bin_parent.join("owned child moved.exe");
    fs::rename(&executable, &moved_executable).unwrap();
    fs::rename(&moved_executable, &executable).unwrap();
    let moved_parent = root.join("bin parent moved");
    fs::rename(&bin_parent, &moved_parent).unwrap();
    fs::rename(&moved_parent, &bin_parent).unwrap();

    assert_eq!(unsafe { WaitForSingleObject(HANDLE(-1isize as *mut c_void), 0) }, WAIT_FAILED);
    println!(
        "supported volume-guid path={} filesystem={} component leases=FILE_SHARE_READ create=CreateProcessW",
        exe_volume.guid_root, exe_volume.filesystem
    );
}

#[test]
#[ignore = "spawned explicitly by restart path feasibility controller"]
fn restart_path_feasibility_child() {
    assert_eq!(std::env::var(MARKER).as_deref(), Ok(MARKER_VALUE));
    let owned_args = std::env::args().collect::<Vec<_>>();
    let control = PathBuf::from(
        owned_args
            .iter()
            .find_map(|arg| arg.strip_prefix("control="))
            .expect("control argument missing"),
    );
    assert_eq!(std::env::var_os(CONTROL).as_deref(), Some(control.as_os_str()));
    let args = owned_args.join("|");
    let cwd = std::env::current_dir().unwrap().to_string_lossy().into_owned();
    fs::write(
        control.join("child-report.txt"),
        format!("marker={MARKER_VALUE}\ncwd={cwd}\nargs={args}\n"),
    )
    .unwrap();
}
