use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const CREATE_SUSPENDED_FLAG: u32 = 0x0000_0004;
pub(crate) const EXTENDED_STARTUPINFO_PRESENT_FLAG: u32 = 0x0008_0000;
pub(crate) const PROC_THREAD_ATTRIBUTE_JOB_LIST_VALUE: usize = 0x0002_000d;
const CLEANUP_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessLaunchError {
    InvalidCommand,
    Job,
    AttributeList,
    CreateProcess,
    Resume,
    Wait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitOutcome {
    Exited,
    TimedOut,
}

pub(crate) struct EverythingProcessCommand {
    executable: PathBuf,
    arguments: Vec<OsString>,
}

impl EverythingProcessCommand {
    pub(crate) fn new(
        executable: PathBuf,
        arguments: Vec<OsString>,
    ) -> Result<Self, ProcessLaunchError> {
        if !executable.is_absolute()
            || executable.as_os_str().is_empty()
            || contains_nul(executable.as_os_str())
            || arguments.iter().any(|argument| contains_nul(argument))
        {
            return Err(ProcessLaunchError::InvalidCommand);
        }
        Ok(Self {
            executable,
            arguments,
        })
    }

    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }

    pub(crate) fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
}

#[cfg(windows)]
fn contains_nul(value: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().any(|unit| unit == 0)
}

#[cfg(not(windows))]
fn contains_nul(value: &OsStr) -> bool {
    value.to_string_lossy().contains('\0')
}

pub(crate) struct ProcessHandleSet<H> {
    pub(crate) process: H,
    pub(crate) thread: H,
    pub(crate) pid: u32,
}

pub(crate) trait ProcessLaunchApi: Send + 'static {
    type Handle: Copy + Send + 'static;
    type AttributeList: Send;

    fn create_kill_on_close_job(&mut self) -> Result<Self::Handle, ProcessLaunchError>;
    fn create_job_attribute_list(
        &mut self,
        job: Self::Handle,
        attribute: usize,
    ) -> Result<Self::AttributeList, ProcessLaunchError>;
    fn delete_attribute_list(&mut self, attributes: Self::AttributeList);
    fn create_process_suspended_in_job(
        &mut self,
        command: &EverythingProcessCommand,
        attributes: &mut Self::AttributeList,
        creation_flags: u32,
    ) -> Result<ProcessHandleSet<Self::Handle>, ProcessLaunchError>;
    fn resume_thread(&mut self, thread: Self::Handle) -> Result<(), ProcessLaunchError>;
    fn request_graceful_exit(&mut self, pid: u32);
    fn terminate_process(&mut self, process: Self::Handle);
    fn wait_process(
        &mut self,
        process: Self::Handle,
        timeout: Duration,
    ) -> Result<WaitOutcome, ProcessLaunchError>;
    fn close_handle(&mut self, handle: Self::Handle);
}

pub(crate) struct OwnedEverythingProcess<A: ProcessLaunchApi> {
    api: A,
    job: A::Handle,
    process: A::Handle,
    thread: A::Handle,
    pid: u32,
    closed: bool,
}

impl<A: ProcessLaunchApi> OwnedEverythingProcess<A> {
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn poll_exited(&mut self) -> Result<bool, ProcessLaunchError> {
        self.api
            .wait_process(self.process, Duration::ZERO)
            .map(|outcome| outcome == WaitOutcome::Exited)
    }

    pub(crate) fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.api.request_graceful_exit(self.pid);
        if !matches!(
            self.api.wait_process(self.process, CLEANUP_WAIT),
            Ok(WaitOutcome::Exited)
        ) {
            self.api.terminate_process(self.process);
            let _ = self.api.wait_process(self.process, CLEANUP_WAIT);
        }
        self.api.close_handle(self.thread);
        self.api.close_handle(self.process);
        self.api.close_handle(self.job);
        self.closed = true;
    }
}

impl<A: ProcessLaunchApi> Drop for OwnedEverythingProcess<A> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) fn launch_suspended_in_job_with<A: ProcessLaunchApi>(
    mut api: A,
    command: &EverythingProcessCommand,
) -> Result<OwnedEverythingProcess<A>, ProcessLaunchError> {
    let job = api.create_kill_on_close_job()?;
    let mut attributes =
        match api.create_job_attribute_list(job, PROC_THREAD_ATTRIBUTE_JOB_LIST_VALUE) {
            Ok(attributes) => attributes,
            Err(error) => {
                api.close_handle(job);
                return Err(error);
            }
        };
    let created = api.create_process_suspended_in_job(
        command,
        &mut attributes,
        CREATE_SUSPENDED_FLAG | EXTENDED_STARTUPINFO_PRESENT_FLAG,
    );
    api.delete_attribute_list(attributes);
    let handles = match created {
        Ok(handles) => handles,
        Err(error) => {
            api.close_handle(job);
            return Err(error);
        }
    };
    if let Err(error) = api.resume_thread(handles.thread) {
        api.terminate_process(handles.process);
        let _ = api.wait_process(handles.process, CLEANUP_WAIT);
        api.close_handle(handles.thread);
        api.close_handle(handles.process);
        api.close_handle(job);
        return Err(error);
    }
    Ok(OwnedEverythingProcess {
        api,
        job,
        process: handles.process,
        thread: handles.thread,
        pid: handles.pid,
        closed: false,
    })
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    #[cfg(test)]
    use windows::Win32::System::JobObjects::IsProcessInJob;
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
        ResumeThread, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOEXW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowThreadProcessId, SendMessageTimeoutW, SMTO_ABORTIFHUNG,
        SMTO_ERRORONEXIT, WM_USER,
    };

    use crate::file_search::everything_runtime::PRIVATE_EVERYTHING_WINDOW_CLASS;

    use super::*;

    pub(crate) struct WindowsProcessApi;

    #[derive(Clone, Copy)]
    pub(crate) struct WindowsHandle(HANDLE);

    unsafe impl Send for WindowsHandle {}

    pub(crate) struct WindowsAttributeList {
        _storage: Vec<usize>,
        _job_handles: Box<[HANDLE; 1]>,
        list: LPPROC_THREAD_ATTRIBUTE_LIST,
    }

    unsafe impl Send for WindowsAttributeList {}

    impl ProcessLaunchApi for WindowsProcessApi {
        type Handle = WindowsHandle;
        type AttributeList = WindowsAttributeList;

        fn create_kill_on_close_job(&mut self) -> Result<WindowsHandle, ProcessLaunchError> {
            let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
                .map_err(|_| ProcessLaunchError::Job)?;
            let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    ptr::from_ref(&information).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            }
            .is_err()
            {
                let _ = unsafe { CloseHandle(job) };
                return Err(ProcessLaunchError::Job);
            }
            Ok(WindowsHandle(job))
        }

        fn create_job_attribute_list(
            &mut self,
            job: WindowsHandle,
            attribute: usize,
        ) -> Result<WindowsAttributeList, ProcessLaunchError> {
            let mut byte_count = 0usize;
            let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut byte_count) };
            if byte_count == 0 {
                return Err(ProcessLaunchError::AttributeList);
            }
            let words = byte_count
                .checked_add(size_of::<usize>() - 1)
                .and_then(|value| value.checked_div(size_of::<usize>()))
                .ok_or(ProcessLaunchError::AttributeList)?;
            let mut storage = vec![0usize; words];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
            unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut byte_count) }
                .map_err(|_| ProcessLaunchError::AttributeList)?;
            let job_handles = Box::new([job.0]);
            if unsafe {
                UpdateProcThreadAttribute(
                    list,
                    0,
                    attribute,
                    Some(job_handles.as_ptr().cast()),
                    size_of::<HANDLE>(),
                    None,
                    None,
                )
            }
            .is_err()
            {
                unsafe { DeleteProcThreadAttributeList(list) };
                return Err(ProcessLaunchError::AttributeList);
            }
            Ok(WindowsAttributeList {
                _storage: storage,
                _job_handles: job_handles,
                list,
            })
        }

        fn delete_attribute_list(&mut self, attributes: WindowsAttributeList) {
            unsafe { DeleteProcThreadAttributeList(attributes.list) };
        }

        fn create_process_suspended_in_job(
            &mut self,
            command: &EverythingProcessCommand,
            attributes: &mut WindowsAttributeList,
            creation_flags: u32,
        ) -> Result<ProcessHandleSet<WindowsHandle>, ProcessLaunchError> {
            let application = wide(command.executable().as_os_str());
            let mut command_line = command_line(command);
            let working_directory = command
                .executable()
                .parent()
                .ok_or(ProcessLaunchError::InvalidCommand)?;
            let working_directory = wide(working_directory.as_os_str());
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = attributes.list;
            let mut information = PROCESS_INFORMATION::default();
            unsafe {
                CreateProcessW(
                    PCWSTR(application.as_ptr()),
                    Some(PWSTR(command_line.as_mut_ptr())),
                    None,
                    None,
                    false,
                    PROCESS_CREATION_FLAGS(creation_flags),
                    None,
                    PCWSTR(working_directory.as_ptr()),
                    ptr::from_ref(&startup.StartupInfo),
                    &mut information,
                )
            }
            .map_err(|_| ProcessLaunchError::CreateProcess)?;
            Ok(ProcessHandleSet {
                process: WindowsHandle(information.hProcess),
                thread: WindowsHandle(information.hThread),
                pid: information.dwProcessId,
            })
        }

        fn resume_thread(&mut self, thread: WindowsHandle) -> Result<(), ProcessLaunchError> {
            (unsafe { ResumeThread(thread.0) } != u32::MAX)
                .then_some(())
                .ok_or(ProcessLaunchError::Resume)
        }

        fn request_graceful_exit(&mut self, pid: u32) {
            let class = wide(OsStr::new(PRIVATE_EVERYTHING_WINDOW_CLASS));
            let Ok(window) = (unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) })
            else {
                return;
            };
            let mut actual_pid = 0;
            if unsafe { GetWindowThreadProcessId(window, Some(&mut actual_pid)) } == 0
                || actual_pid != pid
            {
                return;
            }
            let _ = unsafe {
                SendMessageTimeoutW(
                    window,
                    WM_USER,
                    windows::Win32::Foundation::WPARAM(4),
                    windows::Win32::Foundation::LPARAM(0),
                    SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                    1_000,
                    None,
                )
            };
        }

        fn terminate_process(&mut self, process: WindowsHandle) {
            let _ = unsafe { TerminateProcess(process.0, 1) };
        }

        fn wait_process(
            &mut self,
            process: WindowsHandle,
            timeout: Duration,
        ) -> Result<WaitOutcome, ProcessLaunchError> {
            let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;
            match unsafe { WaitForSingleObject(process.0, timeout_ms) } {
                WAIT_OBJECT_0 => Ok(WaitOutcome::Exited),
                WAIT_TIMEOUT => Ok(WaitOutcome::TimedOut),
                WAIT_FAILED => Err(ProcessLaunchError::Wait),
                _ => Err(ProcessLaunchError::Wait),
            }
        }

        fn close_handle(&mut self, handle: WindowsHandle) {
            let _ = unsafe { CloseHandle(handle.0) };
        }
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    fn command_line(command: &EverythingProcessCommand) -> Vec<u16> {
        let mut encoded = quote_argument(command.executable().as_os_str());
        for argument in command.arguments() {
            encoded.push(' ');
            encoded.push_str(&quote_argument(argument));
        }
        encoded.encode_utf16().chain(Some(0)).collect()
    }

    fn quote_argument(value: &OsStr) -> String {
        let value = value.to_string_lossy();
        if !value.is_empty()
            && !value
                .chars()
                .any(|character| matches!(character, ' ' | '\t' | '"'))
        {
            return value.into_owned();
        }
        let mut quoted = String::from('"');
        let mut backslashes = 0usize;
        for character in value.chars() {
            if character == '\\' {
                backslashes += 1;
            } else if character == '"' {
                quoted.push_str(&"\\".repeat(backslashes.saturating_mul(2).saturating_add(1)));
                quoted.push('"');
                backslashes = 0;
            } else {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(character);
            }
        }
        quoted.push_str(&"\\".repeat(backslashes.saturating_mul(2)));
        quoted.push('"');
        quoted
    }

    pub(crate) type EverythingProcess = OwnedEverythingProcess<WindowsProcessApi>;

    pub(crate) fn launch(
        command: &EverythingProcessCommand,
    ) -> Result<EverythingProcess, ProcessLaunchError> {
        launch_suspended_in_job_with(WindowsProcessApi, command)
    }

    #[cfg(test)]
    pub(super) fn process_is_in_job(process: WindowsHandle, job: WindowsHandle) -> bool {
        let mut assigned = windows::core::BOOL::default();
        unsafe { IsProcessInJob(process.0, Some(job.0), &mut assigned) }.is_ok()
            && assigned.as_bool()
    }
}

#[cfg(windows)]
pub(crate) use platform::{launch, EverythingProcess};

#[cfg(not(windows))]
pub(crate) struct EverythingProcess;

#[cfg(not(windows))]
impl EverythingProcess {
    pub(crate) fn pid(&self) -> u32 {
        0
    }

    pub(crate) fn poll_exited(&mut self) -> Result<bool, ProcessLaunchError> {
        Err(ProcessLaunchError::Wait)
    }

    pub(crate) fn shutdown(&mut self) {}
}

#[cfg(not(windows))]
pub(crate) fn launch(
    _command: &EverythingProcessCommand,
) -> Result<EverythingProcess, ProcessLaunchError> {
    Err(ProcessLaunchError::CreateProcess)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{
        launch_suspended_in_job_with, EverythingProcessCommand, ProcessHandleSet, ProcessLaunchApi,
        ProcessLaunchError, WaitOutcome, CREATE_SUSPENDED_FLAG, EXTENDED_STARTUPINFO_PRESENT_FLAG,
        PROC_THREAD_ATTRIBUTE_JOB_LIST_VALUE,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Failure {
        Job,
        Attributes,
        Create,
        Resume,
        Wait,
    }

    #[derive(Default)]
    struct FakeState {
        events: Vec<String>,
        child_created: bool,
        child_owned_at_creation: bool,
        child_resumed: bool,
        child_alive: bool,
    }

    struct FakeApi {
        state: Arc<Mutex<FakeState>>,
        failure: Option<Failure>,
    }

    impl FakeApi {
        fn new(failure: Option<Failure>) -> (Self, Arc<Mutex<FakeState>>) {
            let state = Arc::new(Mutex::new(FakeState::default()));
            (
                Self {
                    state: Arc::clone(&state),
                    failure,
                },
                state,
            )
        }

        fn event(&self, value: impl Into<String>) {
            self.state.lock().unwrap().events.push(value.into());
        }
    }

    impl ProcessLaunchApi for FakeApi {
        type Handle = usize;
        type AttributeList = usize;

        fn create_kill_on_close_job(&mut self) -> Result<Self::Handle, ProcessLaunchError> {
            self.event("create-job");
            (self.failure != Some(Failure::Job))
                .then_some(10)
                .ok_or(ProcessLaunchError::Job)
        }

        fn create_job_attribute_list(
            &mut self,
            job: Self::Handle,
            attribute: usize,
        ) -> Result<Self::AttributeList, ProcessLaunchError> {
            self.event(format!("create-attributes:{job}:{attribute:#x}"));
            (self.failure != Some(Failure::Attributes))
                .then_some(20)
                .ok_or(ProcessLaunchError::AttributeList)
        }

        fn delete_attribute_list(&mut self, attributes: Self::AttributeList) {
            self.event(format!("delete-attributes:{attributes}"));
        }

        fn create_process_suspended_in_job(
            &mut self,
            command: &EverythingProcessCommand,
            attributes: &mut Self::AttributeList,
            creation_flags: u32,
        ) -> Result<ProcessHandleSet<Self::Handle>, ProcessLaunchError> {
            self.event(format!(
                "create-process:{}:{attributes}:{creation_flags:#x}",
                command.arguments().len()
            ));
            if self.failure == Some(Failure::Create) {
                return Err(ProcessLaunchError::CreateProcess);
            }
            let mut state = self.state.lock().unwrap();
            state.child_created = true;
            state.child_owned_at_creation = *attributes == 20
                && creation_flags == CREATE_SUSPENDED_FLAG | EXTENDED_STARTUPINFO_PRESENT_FLAG;
            state.child_alive = true;
            Ok(ProcessHandleSet {
                process: 30,
                thread: 31,
                pid: 32,
            })
        }

        fn resume_thread(&mut self, thread: Self::Handle) -> Result<(), ProcessLaunchError> {
            self.event(format!("resume:{thread}"));
            if self.failure == Some(Failure::Resume) {
                return Err(ProcessLaunchError::Resume);
            }
            self.state.lock().unwrap().child_resumed = true;
            Ok(())
        }

        fn request_graceful_exit(&mut self, pid: u32) {
            self.event(format!("graceful:{pid}"));
        }

        fn terminate_process(&mut self, process: Self::Handle) {
            self.event(format!("terminate:{process}"));
            self.state.lock().unwrap().child_alive = false;
        }

        fn wait_process(
            &mut self,
            process: Self::Handle,
            timeout: Duration,
        ) -> Result<WaitOutcome, ProcessLaunchError> {
            self.event(format!("wait:{process}:{}", timeout.as_millis()));
            if self.failure == Some(Failure::Wait) {
                return Err(ProcessLaunchError::Wait);
            }
            Ok(if self.state.lock().unwrap().child_alive {
                WaitOutcome::TimedOut
            } else {
                WaitOutcome::Exited
            })
        }

        fn close_handle(&mut self, handle: Self::Handle) {
            self.event(format!("close:{handle}"));
        }
    }

    fn command() -> EverythingProcessCommand {
        EverythingProcessCommand::new(
            PathBuf::from(r"C:\UiPilot\resources\everything\Everything.exe"),
            vec![
                OsString::from("-instance"),
                OsString::from("UiPilot"),
                OsString::from("-startup"),
            ],
        )
        .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_launch_keeps_attribute_values_valid_through_process_creation() {
        let command = EverythingProcessCommand::new(
            PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec must be set on Windows")),
            vec![
                OsString::from("/D"),
                OsString::from("/C"),
                OsString::from("exit 0"),
            ],
        )
        .unwrap();

        let mut process = super::launch(&command).expect("Windows job launch must succeed");
        assert!(super::platform::process_is_in_job(
            process.process,
            process.job
        ));
        process.shutdown();
    }

    #[test]
    fn job_list_ownership_exists_before_the_suspended_process_can_run() {
        let (api, state) = FakeApi::new(None);
        let mut process = launch_suspended_in_job_with(api, &command()).unwrap();
        let snapshot = state.lock().unwrap();
        assert!(snapshot.child_created);
        assert!(snapshot.child_owned_at_creation);
        assert!(snapshot.child_resumed);
        assert_eq!(
            snapshot.events,
            [
                "create-job",
                &format!("create-attributes:10:{PROC_THREAD_ATTRIBUTE_JOB_LIST_VALUE:#x}"),
                &format!(
                    "create-process:3:20:{:#x}",
                    CREATE_SUSPENDED_FLAG | EXTENDED_STARTUPINFO_PRESENT_FLAG
                ),
                "delete-attributes:20",
                "resume:31",
            ]
        );
        drop(snapshot);
        process.shutdown();
    }

    #[test]
    fn launch_failures_leave_no_running_or_unowned_child_and_cleanup_in_reverse() {
        for failure in [
            Failure::Job,
            Failure::Attributes,
            Failure::Create,
            Failure::Resume,
        ] {
            let (api, state) = FakeApi::new(Some(failure));
            assert_eq!(
                launch_suspended_in_job_with(api, &command()).map(|_| ()),
                Err(match failure {
                    Failure::Job => ProcessLaunchError::Job,
                    Failure::Attributes => ProcessLaunchError::AttributeList,
                    Failure::Create => ProcessLaunchError::CreateProcess,
                    Failure::Resume => ProcessLaunchError::Resume,
                    Failure::Wait => unreachable!(),
                })
            );
            let snapshot = state.lock().unwrap();
            assert!(!snapshot.child_alive, "{failure:?}");
            assert!(
                !snapshot.child_created || snapshot.child_owned_at_creation,
                "{failure:?}"
            );
            if failure == Failure::Resume {
                assert!(snapshot.events.ends_with(&[
                    "terminate:30".into(),
                    "wait:30:2000".into(),
                    "close:31".into(),
                    "close:30".into(),
                    "close:10".into(),
                ]));
            }
        }
    }

    #[test]
    fn wait_failure_still_closes_the_job_and_cannot_leave_an_unowned_child() {
        let (api, state) = FakeApi::new(Some(Failure::Wait));
        let mut process = launch_suspended_in_job_with(api, &command()).unwrap();
        process.shutdown();
        process.shutdown();

        let snapshot = state.lock().unwrap();
        assert!(snapshot.child_owned_at_creation);
        assert!(snapshot.events.ends_with(&[
            "graceful:32".into(),
            "wait:30:2000".into(),
            "terminate:30".into(),
            "wait:30:2000".into(),
            "close:31".into(),
            "close:30".into(),
            "close:10".into(),
        ]));
    }

    #[test]
    fn production_source_never_uses_post_create_job_assignment() {
        let source = include_str!("everything_process.rs").replace("\r\n", "\n");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(!production.contains("AssignProcessToJobObject"));
        assert!(production.contains("PROC_THREAD_ATTRIBUTE_JOB_LIST"));
        assert!(production.contains("CREATE_SUSPENDED"));
        assert!(production.contains("EXTENDED_STARTUPINFO_PRESENT"));
    }

    #[test]
    fn private_window_class_identity_is_defined_once_by_the_runtime() {
        let process = include_str!("everything_process.rs").replace("\r\n", "\n");
        let production = process.split("#[cfg(test)]\nmod tests").next().unwrap();
        let runtime = include_str!("../everything_runtime.rs").replace("\r\n", "\n");
        assert!(production.contains("PRIVATE_EVERYTHING_WINDOW_CLASS"));
        assert!(!production.contains("EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)"));
        assert_eq!(
            runtime
                .matches("EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)")
                .count(),
            1
        );
    }
}
