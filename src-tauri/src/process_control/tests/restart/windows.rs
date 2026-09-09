use super::super::super::{
    core::{
        CoreAuthority, EnumerationFailure, HostInstanceId, Liveness, ObservationAccess,
        ObservationFailure, ProcessCandidate, ProcessObservation, ProcessPlatformAdapter,
        ProcessProtection, ProcessTarget,
    },
    operation::{
        ExitWait, OperationAccess, OperationFailure, ProcessOperationAdapter,
        ProcessOperationError, SystemMonotonicClock, WindowCandidate, WindowFacts, WindowFailure,
    },
    restart::{
        windows::{
            created_process_identity_with, filesystem_is_supported, WindowsCreatedProcess,
            WindowsPathLeases,
        },
        CreatedProcessIdentity, ManagedLaunchInput, ManagedLaunchOutcome, NativeLaunchInput,
        ProcessRestartAdapter, ProcessRestartCompletion, ProcessRestartError, ProcessRestartMode,
        ProcessRestartStopError, RestartAdapterError, RestartPathInput,
    },
    windows::{OwnedProcessHandle, WindowsProcessAdapter, WindowsWindowToken},
};
use std::{
    cell::{Cell, RefCell},
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
            GetCurrentProcess, GetCurrentProcessId, GetExitCodeProcess, QueryFullProcessImageNameW,
            TerminateProcess, WaitForSingleObject, PROCESS_NAME_WIN32,
        },
    },
};

const MARKER: &str = "UIPILOT_RESTART_FEASIBILITY_MARKER";
const CONTROL: &str = "UIPILOT_RESTART_FEASIBILITY_CONTROL";
const MODE: &str = "UIPILOT_RESTART_FEASIBILITY_MODE";
const MARKER_VALUE: &str = "explicit value 雪 with spaces";
const WAIT_MS: u32 = 10_000;

struct CleanupProcess {
    handle: HANDLE,
    finished: bool,
}
impl CleanupProcess {
    fn try_duplicate(process: &impl AsHandle) -> windows::core::Result<Self> {
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
        }?;
        Ok(Self {
            handle,
            finished: false,
        })
    }
    fn duplicate(process: &impl AsHandle) -> Self {
        Self::try_duplicate(process).unwrap()
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
use std::{env, ffi::c_void, fs, path::PathBuf};
type Hwnd=*mut c_void; type Hinstance=*mut c_void; type Hicon=*mut c_void; type Hcursor=*mut c_void; type Hbrush=*mut c_void; type Lpcwstr=*const u16;
#[repr(C)] struct WndClass { style:u32, proc:Option<unsafe extern "system" fn(Hwnd,u32,usize,isize)->isize>, cls_extra:i32, wnd_extra:i32, instance:Hinstance, icon:Hicon, cursor:Hcursor, background:Hbrush, menu:Lpcwstr, class:Lpcwstr }
#[repr(C)] struct Point { x:i32,y:i32 }
#[repr(C)] struct Msg { hwnd:Hwnd,message:u32,wparam:usize,lparam:isize,time:u32,pt:Point,lprivate:u32 }
#[link(name="user32")] unsafe extern "system" { fn RegisterClassW(c:*const WndClass)->u16; fn CreateWindowExW(ex:u32,class:Lpcwstr,name:Lpcwstr,style:u32,x:i32,y:i32,w:i32,h:i32,parent:Hwnd,menu:*mut c_void,instance:Hinstance,param:*mut c_void)->Hwnd; fn DefWindowProcW(h:Hwnd,m:u32,w:usize,l:isize)->isize; fn DestroyWindow(h:Hwnd)->i32; fn PostQuitMessage(code:i32); fn GetMessageW(m:*mut Msg,h:Hwnd,min:u32,max:u32)->i32; fn TranslateMessage(m:*const Msg)->i32; fn DispatchMessageW(m:*const Msg)->isize; }
static mut IGNORE:bool=false;
unsafe extern "system" fn proc(h:Hwnd,m:u32,w:usize,l:isize)->isize { if m==0x10 { if IGNORE { return 0 } DestroyWindow(h); return 0 } if m==2 { PostQuitMessage(0); return 0 } DefWindowProcW(h,m,w,l) }
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
fn main() {
 let args=env::args().skip(1).collect::<Vec<_>>();
 let control=PathBuf::from(env::var_os("UIPILOT_RESTART_FEASIBILITY_CONTROL").unwrap());
 let marker=env::var("UIPILOT_RESTART_FEASIBILITY_MARKER").unwrap();
 let pid=std::process::id();
 let mut report=format!("pid={pid}\nmarker={marker}\ncwd={}\nargc={}\n",env::current_dir().unwrap().display(),args.len());
 for (i,arg) in args.iter().enumerate(){report.push_str(&format!("arg{i}={arg}\n"));}
 fs::write(control.join("child-report.txt"),&report).unwrap();
 fs::write(control.join(format!("child-report-{pid}.txt")),report).unwrap();
 let mode=env::var("UIPILOT_RESTART_FEASIBILITY_MODE").unwrap_or_else(|_|"short".into());
 if mode=="short" { std::thread::sleep(std::time::Duration::from_millis(500)); return }
 unsafe { IGNORE=mode=="ignore"; let class=wide(&format!("UiPilotOwnedRestart{pid}")); let wc=WndClass{style:0,proc:Some(proc),cls_extra:0,wnd_extra:0,instance:std::ptr::null_mut(),icon:std::ptr::null_mut(),cursor:std::ptr::null_mut(),background:std::ptr::null_mut(),menu:std::ptr::null(),class:class.as_ptr()}; assert_ne!(RegisterClassW(&wc),0); let title=wide("UiPilot owned restart fixture"); assert!(!CreateWindowExW(0,class.as_ptr(),title.as_ptr(),0x10CF0000,0,0,120,80,std::ptr::null_mut(),std::ptr::null_mut(),std::ptr::null_mut(),std::ptr::null_mut()).is_null()); fs::write(control.join(format!("ready-{pid}")),b"ready").unwrap(); let mut msg:Msg=std::mem::zeroed(); while GetMessageW(&mut msg,std::ptr::null_mut(),0,0)>0 { TranslateMessage(&msg); DispatchMessageW(&msg); } }
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

struct OwnedRestartAdapter {
    inner: WindowsProcessAdapter,
    cleanup: RefCell<Vec<CleanupProcess>>,
    create_calls: Cell<usize>,
    fail_create_on: Cell<Option<usize>>,
    fail_identity_on: Cell<Option<usize>>,
}

impl OwnedRestartAdapter {
    fn new() -> Self {
        Self {
            inner: WindowsProcessAdapter,
            cleanup: RefCell::new(Vec::new()),
            create_calls: Cell::new(0),
            fail_create_on: Cell::new(None),
            fail_identity_on: Cell::new(None),
        }
    }
    fn created_count(&self) -> usize {
        self.create_calls.get()
    }
    fn cleanup_all(&self) {
        for process in self.cleanup.borrow_mut().iter_mut() {
            if !process.finished {
                let _ = unsafe { TerminateProcess(process.handle, 0x73) };
                process.wait();
            }
        }
    }
}

impl ProcessPlatformAdapter for OwnedRestartAdapter {
    type Handle = OwnedProcessHandle;
    fn enumerate_candidates(
        &self,
    ) -> Result<Vec<Result<ProcessCandidate, EnumerationFailure>>, EnumerationFailure> {
        self.inner.enumerate_candidates()
    }
    fn open_observation(
        &self,
        pid: u32,
        access: ObservationAccess,
    ) -> Result<Self::Handle, ObservationFailure> {
        self.inner.open_observation(pid, access)
    }
    fn creation_time(&self, handle: &Self::Handle) -> Result<u64, ObservationFailure> {
        self.inner.creation_time(handle)
    }
    fn liveness(&self, handle: &Self::Handle) -> Result<Liveness, ObservationFailure> {
        self.inner.liveness(handle)
    }
    fn observation(&self, handle: &Self::Handle) -> Result<ProcessObservation, ObservationFailure> {
        self.inner.observation(handle)
    }
    fn reenumerate_name(&self, pid: u32) -> Result<Option<String>, ObservationFailure> {
        self.inner.reenumerate_name(pid)
    }
    fn process_names_equivalent(
        &self,
        left: &OsStr,
        right: &OsStr,
    ) -> Result<bool, ObservationFailure> {
        self.inner.process_names_equivalent(left, right)
    }
    fn protection(&self, handle: &Self::Handle) -> Result<ProcessProtection, ObservationFailure> {
        self.inner.protection(handle)
    }
}
impl ProcessOperationAdapter for OwnedRestartAdapter {
    type WindowToken = WindowsWindowToken;
    fn open_operation(
        &self,
        pid: u32,
        access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure> {
        self.inner.open_operation(pid, access)
    }
    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure> {
        self.inner.enumerate_top_level_windows()
    }
    fn window_facts(&self, w: Self::WindowToken) -> Result<WindowFacts, WindowFailure> {
        self.inner.window_facts(w)
    }
    fn post_close(&self, w: Self::WindowToken) -> Result<(), WindowFailure> {
        self.inner.post_close(w)
    }
    fn terminate_process(&self, h: &Self::Handle, c: u32) -> Result<(), OperationFailure> {
        self.inner.terminate_process(h, c)
    }
    fn wait_for_exit(&self, h: &Self::Handle, t: Duration) -> Result<ExitWait, OperationFailure> {
        self.inner.wait_for_exit(h, t)
    }
}
impl ProcessRestartAdapter for OwnedRestartAdapter {
    type PathLeases = WindowsPathLeases;
    type CreatedProcess = WindowsCreatedProcess;
    fn acquire_path_leases(
        &self,
        input: &RestartPathInput,
    ) -> Result<Self::PathLeases, RestartAdapterError> {
        self.inner.acquire_path_leases(input)
    }
    fn revalidate_path_leases(&self, l: &Self::PathLeases) -> Result<(), RestartAdapterError> {
        self.inner.revalidate_path_leases(l)
    }
    fn path_lease_metadata<'a>(
        &self,
        l: &'a Self::PathLeases,
    ) -> &'a super::super::super::restart::LeasedPathMetadata {
        self.inner.path_lease_metadata(l)
    }
    fn create_process(
        &self,
        l: &Self::PathLeases,
        input: &NativeLaunchInput,
    ) -> Result<Self::CreatedProcess, RestartAdapterError> {
        let call = self.create_calls.get() + 1;
        self.create_calls.set(call);
        if self.fail_create_on.get() == Some(call) {
            return Err(RestartAdapterError::CreationFailed);
        }
        let process = self.inner.create_process(l, input)?;
        let cleanup = match CleanupProcess::try_duplicate(&process) {
            Ok(cleanup) => cleanup,
            Err(error) => {
                let original = HANDLE(process.as_handle().as_raw_handle());
                let _ = unsafe { TerminateProcess(original, 0x74) };
                let _ = unsafe { WaitForSingleObject(original, WAIT_MS) };
                panic!("test cleanup handle duplication failed after bounded cleanup: {error}")
            }
        };
        self.cleanup.borrow_mut().push(cleanup);
        Ok(process)
    }
    fn created_process_identity(
        &self,
        p: &Self::CreatedProcess,
    ) -> Result<CreatedProcessIdentity, RestartAdapterError> {
        if self.fail_identity_on.get() == Some(self.create_calls.get()) {
            return Err(RestartAdapterError::IdentityUnavailable);
        }
        self.inner.created_process_identity(p)
    }
}

fn owned_authority(adapter: OwnedRestartAdapter) -> CoreAuthority<OwnedRestartAdapter> {
    let host = HostInstanceId::new([0x5a; 16]).unwrap();
    let pid = unsafe { GetCurrentProcessId() };
    let handle = adapter
        .open_observation(pid, ObservationAccess::SNAPSHOT)
        .unwrap();
    let target = ProcessTarget::new(host, pid, adapter.creation_time(&handle).unwrap()).unwrap();
    CoreAuthority::new(host, target, adapter)
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn quote_arg(value: &OsStr) -> Vec<u16> {
    let units = value.encode_wide().collect::<Vec<_>>();
    if units.is_empty() {
        return vec![34, 34];
    }
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
    e.retain(|(k, _)| k != MARKER && k != CONTROL && k != MODE);
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
fn managed_input(executable: &Path, cwd: &Path, control: &Path, mode: &str) -> ManagedLaunchInput {
    let args = ["plain", "space value", "quote\"value", "Unicode-雪", ""];
    let mut command = command_line(executable, &args);
    command.pop();
    ManagedLaunchInput::new(
        Some(executable.to_owned()),
        Some(command),
        Some(cwd.to_owned()),
        Some(vec![
            (
                MARKER.encode_utf16().collect(),
                MARKER_VALUE.encode_utf16().collect(),
            ),
            (
                CONTROL.encode_utf16().collect(),
                control.as_os_str().encode_wide().collect(),
            ),
            (MODE.encode_utf16().collect(), mode.encode_utf16().collect()),
        ]),
    )
}
fn managed_input_without_arguments(
    executable: &Path,
    cwd: &Path,
    control: &Path,
) -> ManagedLaunchInput {
    ManagedLaunchInput::new(
        Some(executable.to_owned()),
        Some(quote_arg(executable.as_os_str())),
        Some(cwd.to_owned()),
        Some(vec![
            (
                MARKER.encode_utf16().collect(),
                MARKER_VALUE.encode_utf16().collect(),
            ),
            (
                CONTROL.encode_utf16().collect(),
                control.as_os_str().encode_wide().collect(),
            ),
            (
                MODE.encode_utf16().collect(),
                "ignore".encode_utf16().collect(),
            ),
        ]),
    )
}
fn wait_for_file(path: &Path, process: HANDLE) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert_ne!(
            unsafe { WaitForSingleObject(process, 0) },
            WAIT_OBJECT_0,
            "fixture exited before {}",
            path.display()
        );
        assert!(
            Instant::now() < deadline,
            "fixture handshake timed out: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn assert_report(control: &Path, pid: u32, cwd: &Path) {
    let report = fs::read_to_string(control.join(format!("child-report-{pid}.txt"))).unwrap();
    assert!(report.contains(&format!(
        "pid={pid}\nmarker={MARKER_VALUE}\ncwd={}",
        cwd.display()
    )));
    assert!(report.contains(
        "argc=5\narg0=plain\narg1=space value\narg2=quote\"value\narg3=Unicode-雪\narg4=\n"
    ));
}
fn clear_generation_handshake(control: &Path, pid: u32) {
    fs::remove_file(control.join(format!("ready-{pid}"))).unwrap();
    fs::remove_file(control.join(format!("child-report-{pid}.txt"))).unwrap();
}
fn root(label: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("UiPilot {label} 雪 {} {n}", std::process::id()))
}

fn root_under(base: &Path, label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    base.join(format!("UiPilot {label} 雪 {} {nonce}", std::process::id()))
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

#[test]
fn restart_filesystem_admission_is_ntfs_only() {
    assert!(filesystem_is_supported("NTFS"));
    assert!(filesystem_is_supported("ntfs"));
    for unsupported in ["FAT", "FAT32", "exFAT", "ReFS", "", "unknown"] {
        assert!(!filesystem_is_supported(unsupported));
    }
}

#[test]
fn restart_distinct_fixed_ntfs_volumes_are_composed_when_available() {
    let executable_base = std::env::current_dir()
        .unwrap()
        .join("src-tauri")
        .join("target");
    let cwd_base = std::env::temp_dir();
    let executable_drive = executable_base
        .to_string_lossy()
        .get(..2)
        .map(str::to_owned);
    let cwd_drive = cwd_base.to_string_lossy().get(..2).map(str::to_owned);
    if executable_drive == cwd_drive {
        eprintln!("native cross-volume case unavailable: target and temp use the same volume");
        return;
    }

    let executable_root = root_under(&executable_base, "restart cross volume executable");
    let cwd_root = root_under(&cwd_base, "restart cross volume cwd");
    let mut executable_tree = OwnedTree(Some(executable_root.clone()));
    let mut cwd_tree = OwnedTree(Some(cwd_root.clone()));
    fs::create_dir_all(&executable_root).unwrap();
    fs::create_dir_all(&cwd_root).unwrap();
    let executable = executable_root.join("cross volume child.exe");
    compile_helper(&executable_root.join("cross volume helper.rs"), &executable);
    let control = cwd_root.join("control");
    fs::create_dir_all(&control).unwrap();

    let adapter = WindowsProcessAdapter;
    let leases = match adapter
        .acquire_path_leases(&RestartPathInput::new(executable.clone(), cwd_root.clone()))
    {
        Ok(leases) => leases,
        Err(RestartAdapterError::UnsupportedPath) => {
            eprintln!("native cross-volume case unavailable: second fixed NTFS volume not present");
            cwd_tree.cleanup_asserted();
            executable_tree.cleanup_asserted();
            return;
        }
        Err(error) => panic!("cross-volume acquisition failed unexpectedly: {error}"),
    };
    let metadata = adapter.path_lease_metadata(&leases);
    let executable_root_name = metadata.executable().to_string_lossy()[..48].to_owned();
    let cwd_root_name = metadata.working_directory().to_string_lossy()[..48].to_owned();
    assert_ne!(executable_root_name, cwd_root_name);

    let control_arg = format!("control={}", control.display());
    let child = adapter
        .create_process(
            &leases,
            &NativeLaunchInput::new(
                command_line(&executable, &[&control_arg]),
                environment_block(&control),
            ),
        )
        .unwrap();
    let mut cleanup = CleanupProcess::duplicate(&child);
    drop(child);
    drop(leases);
    wait_report(&control.join("child-report.txt"), cleanup.handle);
    cleanup.wait();
    cwd_tree.cleanup_asserted();
    executable_tree.cleanup_asserted();
}

#[test]
fn restart_windows_owned_graceful_force_and_replacement_receipt_roundtrip() {
    let root = root("restart owned roundtrip");
    let mut tree = OwnedTree(Some(root.clone()));
    let bin = root.join("bin");
    let cwd = root.join("working directory 雪");
    let control = root.join("control");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let exe = bin.join("owned restart child 雪.exe");
    compile_helper(&root.join("helper.rs"), &exe);
    let authority = owned_authority(OwnedRestartAdapter::new());
    let receipt = match authority
        .launch_managed(managed_input(&exe, &cwd, &control, "graceful"))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(r) => r,
        _ => panic!("initial identity unavailable"),
    };
    let old = *receipt.target();
    let first = authority
        .plan_restart(&old, Some(&receipt), ProcessRestartMode::Graceful)
        .unwrap();
    let effective_cwd = first.preview().working_directory().to_owned();
    let consumed_plan = authority
        .plan_restart(&old, Some(&receipt), ProcessRestartMode::Graceful)
        .unwrap();
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", old.pid())),
            guards[0].handle,
        );
    }
    assert_report(&control, old.pid(), &effective_cwd);
    clear_generation_handshake(&control, old.pid());
    let report = authority
        .execute_restart_with_clock(first, &SystemMonotonicClock)
        .unwrap();
    {
        let mut guards = authority.adapter().cleanup.borrow_mut();
        guards[0].wait();
    }
    assert_eq!(
        authority
            .execute_restart_with_clock(consumed_plan, &SystemMonotonicClock)
            .unwrap_err(),
        ProcessRestartError::ReceiptConsumed
    );
    let replacement = match report.completion() {
        ProcessRestartCompletion::OldProcessExitedNewProcessCreated(r) => r,
        _ => panic!("graceful restart did not create replacement"),
    };
    let second = *replacement.target();
    assert!(second.pid() != old.pid() || second.creation_time() != old.creation_time());
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", second.pid())),
            guards[1].handle,
        );
    }
    assert_report(&control, second.pid(), &effective_cwd);
    clear_generation_handshake(&control, second.pid());
    let force = authority
        .plan_restart(&second, Some(replacement), ProcessRestartMode::Force)
        .unwrap();
    let force_report = authority
        .execute_restart_with_clock(force, &SystemMonotonicClock)
        .unwrap();
    {
        let mut guards = authority.adapter().cleanup.borrow_mut();
        guards[1].wait();
    }
    let third = match force_report.completion() {
        ProcessRestartCompletion::OldProcessExitedNewProcessCreated(r) => *r.target(),
        _ => panic!("force restart did not create replacement"),
    };
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", third.pid())),
            guards[2].handle,
        );
    }
    assert_report(&control, third.pid(), &effective_cwd);
    authority.adapter().cleanup_all();
    drop(authority);

    let adapter = OwnedRestartAdapter::new();
    adapter.fail_identity_on.set(Some(2));
    let authority = owned_authority(adapter);
    let receipt = match authority
        .launch_managed(managed_input(&exe, &cwd, &control, "graceful"))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(receipt) => receipt,
        _ => panic!("initial identity unavailable"),
    };
    let target = *receipt.target();
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", target.pid())),
            guards[0].handle,
        );
    }
    let plan = authority
        .plan_restart(&target, Some(&receipt), ProcessRestartMode::Graceful)
        .unwrap();
    let report = authority
        .execute_restart_with_clock(plan, &SystemMonotonicClock)
        .unwrap();
    assert!(matches!(
        report.completion(),
        ProcessRestartCompletion::OldProcessExitedLaunchIdentityUnknown
    ));
    assert_eq!(authority.adapter().cleanup.borrow().len(), 2);
    assert_eq!(
        unsafe { WaitForSingleObject(authority.adapter().cleanup.borrow()[1].handle, 0) },
        WAIT_TIMEOUT,
        "identity observation failure must not auto-kill the created child"
    );
    authority.adapter().cleanup_all();
    drop(authority);
    tree.cleanup_asserted();
}

#[test]
fn restart_windows_ignored_close_times_out_without_relaunch() {
    let root = root("restart ignored close");
    let mut tree = OwnedTree(Some(root.clone()));
    let cwd = root.join("cwd");
    let control = root.join("control");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let exe = root.join("ignored child.exe");
    compile_helper(&root.join("helper.rs"), &exe);
    let authority = owned_authority(OwnedRestartAdapter::new());
    let receipt = match authority
        .launch_managed(managed_input(&exe, &cwd, &control, "ignore"))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(r) => r,
        _ => panic!(),
    };
    let target = *receipt.target();
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", target.pid())),
            guards[0].handle,
        );
    }
    let plan = authority
        .plan_restart(&target, Some(&receipt), ProcessRestartMode::Graceful)
        .unwrap();
    let report = authority
        .execute_restart_with_clock(plan, &SystemMonotonicClock)
        .unwrap();
    assert!(matches!(
        report.completion(),
        ProcessRestartCompletion::StopFailedLaunchNotAttempted(ProcessRestartStopError::Operation(
            ProcessOperationError::GracefulCloseTimedOut
        ))
    ));
    assert_eq!(authority.adapter().created_count(), 1);
    assert_eq!(
        unsafe { WaitForSingleObject(authority.adapter().cleanup.borrow()[0].handle, 0) },
        WAIT_TIMEOUT
    );
    authority.adapter().cleanup_all();
    drop(authority);
    tree.cleanup_asserted();
}

#[test]
fn restart_windows_changed_context_rejects_before_stop() {
    let root = root("restart changed context");
    let mut tree = OwnedTree(Some(root.clone()));
    let bin = root.join("bin");
    let moved = root.join("bin moved");
    let cwd = root.join("cwd");
    let control = root.join("control");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let exe = bin.join("child.exe");
    compile_helper(&root.join("helper.rs"), &exe);
    let authority = owned_authority(OwnedRestartAdapter::new());
    let receipt = match authority
        .launch_managed(managed_input(&exe, &cwd, &control, "ignore"))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(r) => r,
        _ => panic!(),
    };
    let target = *receipt.target();
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", target.pid())),
            guards[0].handle,
        );
    }
    let plan = authority
        .plan_restart(&target, Some(&receipt), ProcessRestartMode::Force)
        .unwrap();
    fs::rename(&bin, &moved).unwrap();
    let result = authority.execute_restart_with_clock(plan, &SystemMonotonicClock);
    assert!(matches!(
        result,
        Err(ProcessRestartError::PathUnavailable) | Err(ProcessRestartError::PathChanged)
    ));
    assert_eq!(authority.adapter().created_count(), 1);
    assert_eq!(
        unsafe { WaitForSingleObject(authority.adapter().cleanup.borrow()[0].handle, 0) },
        WAIT_TIMEOUT
    );
    authority.adapter().cleanup_all();
    fs::rename(&moved, &bin).unwrap();
    drop(authority);
    tree.cleanup_asserted();
}

#[test]
fn restart_windows_post_stop_creation_and_identity_faults_are_controlled() {
    let root = root("restart controlled faults");
    let mut tree = OwnedTree(Some(root.clone()));
    let cwd = root.join("cwd");
    let control = root.join("control");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&control).unwrap();
    let exe = root.join("child.exe");
    compile_helper(&root.join("helper.rs"), &exe);
    let adapter = OwnedRestartAdapter::new();
    adapter.fail_create_on.set(Some(2));
    let authority = owned_authority(adapter);
    let receipt = match authority
        .launch_managed(managed_input(&exe, &cwd, &control, "graceful"))
        .unwrap()
    {
        ManagedLaunchOutcome::Created(r) => r,
        _ => panic!(),
    };
    let target = *receipt.target();
    {
        let guards = authority.adapter().cleanup.borrow();
        wait_for_file(
            &control.join(format!("ready-{}", target.pid())),
            guards[0].handle,
        );
    }
    let plan = authority
        .plan_restart(&target, Some(&receipt), ProcessRestartMode::Graceful)
        .unwrap();
    let second_plan = authority
        .plan_restart(&target, Some(&receipt), ProcessRestartMode::Graceful)
        .unwrap();
    let report = authority
        .execute_restart_with_clock(plan, &SystemMonotonicClock)
        .unwrap();
    assert!(matches!(
        report.completion(),
        ProcessRestartCompletion::OldProcessExitedLaunchFailed
    ));
    assert_eq!(authority.adapter().created_count(), 2);
    assert_eq!(
        authority
            .execute_restart_with_clock(second_plan, &SystemMonotonicClock)
            .unwrap_err(),
        ProcessRestartError::ReceiptConsumed
    );
    {
        authority.adapter().cleanup.borrow_mut()[0].wait();
    }
    drop(authority);
    fs::remove_file(control.join("child-report.txt")).unwrap();
    let adapter = OwnedRestartAdapter::new();
    adapter.fail_identity_on.set(Some(1));
    let authority = owned_authority(adapter);
    assert!(matches!(
        authority
            .launch_managed(managed_input_without_arguments(&exe, &cwd, &control))
            .unwrap(),
        ManagedLaunchOutcome::CreatedIdentityUnknown
    ));
    assert_eq!(authority.adapter().cleanup.borrow().len(), 1);
    wait_report(
        &control.join("child-report.txt"),
        authority.adapter().cleanup.borrow()[0].handle,
    );
    assert!(
        fs::read_to_string(control.join("child-report.txt"))
            .unwrap()
            .contains("argc=0\n"),
        "executable-only command line must produce an empty user-argument tail"
    );
    assert_eq!(
        unsafe { WaitForSingleObject(authority.adapter().cleanup.borrow()[0].handle, 0) },
        WAIT_TIMEOUT
    );
    authority.adapter().cleanup_all();
    drop(authority);
    tree.cleanup_asserted();
}
