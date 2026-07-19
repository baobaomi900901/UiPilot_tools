use std::{ffi::OsStr, fmt, mem::size_of, os::windows::ffi::OsStrExt, path::Path};

use windows::{
    core::{BOOL, PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, SetLastError, ERROR_NO_MORE_FILES, ERROR_SUCCESS, HANDLE,
            HWND, LPARAM,
        },
        Globalization::{CompareStringOrdinal, CSTR_EQUAL},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
                IsWindowVisible, SetForegroundWindow, GWL_EXSTYLE, GW_OWNER, SW_SHOWNORMAL,
                WS_EX_TOOLWINDOW,
            },
        },
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeActivation {
    Activated,
    Refused,
    Unavailable,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeActionError {
    ApplicationEntryUnavailable,
}

impl fmt::Display for NativeActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("application entry unavailable")
    }
}

impl std::error::Error for NativeActionError {}

enum ProcessStep {
    Entry { pid: u32, basename: Vec<u16> },
    End,
    Failed,
}

enum ProcessLookup<H> {
    Unique { pid: u32, handle: H },
    Unavailable,
    Indeterminate,
}

#[derive(Debug, Eq, PartialEq)]
enum WindowLookup<W> {
    Eligible(W),
    Unavailable,
    Indeterminate,
}

struct WindowProperties {
    pid: Result<u32, ()>,
    visible: bool,
    owned: Result<bool, ()>,
    tool: Result<bool, ()>,
}

#[derive(Default)]
struct WindowEnumeration<W> {
    first: Option<W>,
    indeterminate: bool,
}

fn consider_window<W>(
    state: &mut WindowEnumeration<W>,
    target_pid: u32,
    hwnd: W,
    properties: WindowProperties,
) -> bool {
    let pid = match properties.pid {
        Ok(pid) => pid,
        Err(()) => {
            state.indeterminate = true;
            return true;
        }
    };
    if pid != target_pid || !properties.visible {
        return true;
    }
    let owned = match properties.owned {
        Ok(owned) => owned,
        Err(()) => {
            state.indeterminate = true;
            return true;
        }
    };
    let tool = match properties.tool {
        Ok(tool) => tool,
        Err(()) => {
            state.indeterminate = true;
            return true;
        }
    };
    if !owned && !tool && state.first.is_none() {
        state.first = Some(hwnd);
    }
    true
}

fn finish_window_enumeration<W>(
    state: WindowEnumeration<W>,
    enumeration_succeeded: bool,
) -> WindowLookup<W> {
    if !enumeration_succeeded || state.indeterminate {
        WindowLookup::Indeterminate
    } else if let Some(hwnd) = state.first {
        WindowLookup::Eligible(hwnd)
    } else {
        WindowLookup::Unavailable
    }
}

pub(crate) fn try_activate(executable: &Path) -> NativeActivation {
    try_activate_with(
        || find_unique_process(executable),
        find_window,
        |hwnd| unsafe { SetForegroundWindow(hwnd).as_bool() },
    )
}

fn try_activate_with<H, W, F, E, A>(
    find_process: F,
    find_window: E,
    foreground: A,
) -> NativeActivation
where
    F: FnOnce() -> ProcessLookup<H>,
    E: FnOnce(u32) -> WindowLookup<W>,
    A: FnOnce(W) -> bool,
{
    let (pid, process_handle) = match find_process() {
        ProcessLookup::Unique { pid, handle } => (pid, handle),
        ProcessLookup::Unavailable => return NativeActivation::Unavailable,
        ProcessLookup::Indeterminate => return NativeActivation::Indeterminate,
    };
    let result = match find_window(pid) {
        WindowLookup::Eligible(hwnd) => {
            if foreground(hwnd) {
                NativeActivation::Activated
            } else {
                NativeActivation::Refused
            }
        }
        WindowLookup::Unavailable => NativeActivation::Unavailable,
        WindowLookup::Indeterminate => NativeActivation::Indeterminate,
    };
    drop(process_handle);
    result
}

pub(crate) fn launch_shortcut(shortcut: &Path) -> Result<(), NativeActionError> {
    launch_shortcut_with(shortcut, |path| unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
        .0 as isize
    })
}

fn launch_shortcut_with<S>(shortcut: &Path, shell_execute: S) -> Result<(), NativeActionError>
where
    S: FnOnce(&[u16]) -> isize,
{
    let path = wide_null(shortcut.as_os_str());
    if shell_execute(&path) > 32 {
        Ok(())
    } else {
        Err(NativeActionError::ApplicationEntryUnavailable)
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn ordinal_eq(left: &[u16], right: &[u16]) -> bool {
    unsafe { CompareStringOrdinal(left, right, true) == CSTR_EQUAL }
}

fn select_unique_process_with<H, N, Q>(
    target_basename: &[u16],
    target_path: &[u16],
    mut next: N,
    mut query: Q,
) -> ProcessLookup<H>
where
    N: FnMut() -> ProcessStep,
    Q: FnMut(u32) -> Result<(H, Vec<u16>), ()>,
{
    let mut matched = None;
    let mut multiple = false;
    let mut indeterminate = false;

    loop {
        match next() {
            ProcessStep::Entry { pid, basename } => {
                if !ordinal_eq(&basename, target_basename) {
                    continue;
                }
                match query(pid) {
                    Ok((handle, path)) if ordinal_eq(&path, target_path) => {
                        if matched.is_some() {
                            multiple = true;
                        } else {
                            matched = Some((pid, handle));
                        }
                    }
                    Ok(_) => {}
                    Err(()) => indeterminate = true,
                }
            }
            ProcessStep::End => break,
            ProcessStep::Failed => return ProcessLookup::Indeterminate,
        }
    }

    if indeterminate {
        ProcessLookup::Indeterminate
    } else if multiple {
        ProcessLookup::Unavailable
    } else if let Some((pid, handle)) = matched {
        ProcessLookup::Unique { pid, handle }
    } else {
        ProcessLookup::Unavailable
    }
}

fn find_unique_process(executable: &Path) -> ProcessLookup<OwnedHandle> {
    let Some(basename) = executable.file_name() else {
        return ProcessLookup::Indeterminate;
    };
    let target_basename = wide(basename);
    let target_path = wide(executable.as_os_str());
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(handle) if !handle.is_invalid() => OwnedHandle(handle),
        _ => return ProcessLookup::Indeterminate,
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut first = true;

    select_unique_process_with(
        &target_basename,
        &target_path,
        || {
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let result = unsafe {
                if first {
                    first = false;
                    Process32FirstW(snapshot.0, &mut entry)
                } else {
                    Process32NextW(snapshot.0, &mut entry)
                }
            };
            match result {
                Ok(()) => ProcessStep::Entry {
                    pid: entry.th32ProcessID,
                    basename: nul_terminated_slice(&entry.szExeFile).to_vec(),
                },
                Err(_) if unsafe { GetLastError() } == ERROR_NO_MORE_FILES => ProcessStep::End,
                Err(_) => ProcessStep::Failed,
            }
        },
        query_process_path,
    )
}

struct NativeWindowEnumeration {
    pid: u32,
    state: WindowEnumeration<HWND>,
}

fn find_window(pid: u32) -> WindowLookup<HWND> {
    let mut context = NativeWindowEnumeration {
        pid,
        state: WindowEnumeration::default(),
    };
    let result = unsafe {
        EnumWindows(
            Some(enum_window),
            LPARAM((&mut context as *mut NativeWindowEnumeration) as isize),
        )
    };
    finish_window_enumeration(context.state, result.is_ok())
}

unsafe extern "system" fn enum_window(hwnd: HWND, context: LPARAM) -> BOOL {
    let context = unsafe { &mut *(context.0 as *mut NativeWindowEnumeration) };
    let mut pid = 0_u32;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if thread_id == 0 {
        context.state.indeterminate = true;
        return true.into();
    }
    if pid != context.pid {
        return true.into();
    }

    let properties = WindowProperties {
        pid: Ok(pid),
        visible: unsafe { IsWindowVisible(hwnd).as_bool() },
        owned: window_is_owned(hwnd),
        tool: window_is_tool(hwnd),
    };
    consider_window(&mut context.state, context.pid, hwnd, properties).into()
}

fn window_is_owned(hwnd: HWND) -> Result<bool, ()> {
    unsafe {
        SetLastError(ERROR_SUCCESS);
        match GetWindow(hwnd, GW_OWNER) {
            Ok(_) => Ok(true),
            Err(_) if GetLastError() == ERROR_SUCCESS => Ok(false),
            Err(_) => Err(()),
        }
    }
}

fn window_is_tool(hwnd: HWND) -> Result<bool, ()> {
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if style == 0 && GetLastError() != ERROR_SUCCESS {
            return Err(());
        }
        Ok(style & WS_EX_TOOLWINDOW.0 as isize != 0)
    }
}

fn query_process_path(pid: u32) -> Result<(OwnedHandle, Vec<u16>), ()> {
    let raw =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|_| ())?;
    if raw.is_invalid() {
        return Err(());
    }
    let handle = OwnedHandle(raw);
    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|_| ())?;
    let length = usize::try_from(length).map_err(|_| ())?;
    if length == 0 || length > path.len() {
        return Err(());
    }
    path.truncate(length);
    Ok((handle, path))
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().collect()
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

fn nul_terminated_slice(value: &[u16]) -> &[u16] {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    &value[..length]
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::{
        consider_window, finish_window_enumeration, launch_shortcut_with, ordinal_eq,
        select_unique_process_with, try_activate_with, NativeActionError, NativeActivation,
        ProcessLookup, ProcessStep, WindowEnumeration, WindowLookup, WindowProperties,
    };

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    struct FakeHandle(Rc<Cell<usize>>);

    impl Drop for FakeHandle {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn process_comparison_is_utf16_ordinal_ignore_case() {
        assert!(ordinal_eq(&wide("CALC.EXE"), &wide("calc.exe")));
        assert!(ordinal_eq(&wide("CAFÉ.EXE"), &wide("café.exe")));
        assert!(!ordinal_eq(&wide("cafe.exe"), &wide("café.exe")));
    }

    #[test]
    fn only_one_exact_match_after_complete_enumeration_is_unique() {
        let closes = Rc::new(Cell::new(0));
        let mut steps = vec![
            ProcessStep::Entry {
                pid: 17,
                basename: wide("APP.EXE"),
            },
            ProcessStep::End,
        ]
        .into_iter();
        let mut queried = Vec::new();

        let lookup = select_unique_process_with(
            &wide("app.exe"),
            &wide(r"C:\Apps\App.exe"),
            || steps.next().unwrap(),
            |pid| {
                queried.push(pid);
                Ok((FakeHandle(Rc::clone(&closes)), wide(r"c:\apps\APP.EXE")))
            },
        );

        assert!(matches!(lookup, ProcessLookup::Unique { pid: 17, .. }));
        assert_eq!(queried, [17]);
        assert_eq!(closes.get(), 0, "unique process handle must remain owned");
        drop(lookup);
        assert_eq!(closes.get(), 1);
    }

    #[test]
    fn zero_multiple_and_indeterminate_processes_are_not_unique() {
        let target_basename = wide("app.exe");
        let target_path = wide(r"C:\Apps\App.exe");

        let mut empty = vec![ProcessStep::End].into_iter();
        let empty_lookup = select_unique_process_with::<(), _, _>(
            &target_basename,
            &target_path,
            || empty.next().unwrap(),
            |_| panic!("empty enumeration must not query a process"),
        );
        assert!(matches!(empty_lookup, ProcessLookup::Unavailable));

        let closes = Rc::new(Cell::new(0));
        let mut multiple = vec![
            ProcessStep::Entry {
                pid: 1,
                basename: wide("app.exe"),
            },
            ProcessStep::Entry {
                pid: 2,
                basename: wide("APP.EXE"),
            },
            ProcessStep::End,
        ]
        .into_iter();
        let multiple_lookup = select_unique_process_with(
            &target_basename,
            &target_path,
            || multiple.next().unwrap(),
            |_| Ok((FakeHandle(Rc::clone(&closes)), target_path.clone())),
        );
        assert!(matches!(multiple_lookup, ProcessLookup::Unavailable));
        assert_eq!(closes.get(), 2);

        let mut query_failure = vec![
            ProcessStep::Entry {
                pid: 3,
                basename: wide("app.exe"),
            },
            ProcessStep::End,
        ]
        .into_iter();
        let failed_lookup = select_unique_process_with::<(), _, _>(
            &target_basename,
            &target_path,
            || query_failure.next().unwrap(),
            |_| Err(()),
        );
        assert!(matches!(failed_lookup, ProcessLookup::Indeterminate));
    }

    #[test]
    fn tail_failure_overrides_an_exact_match_and_other_basenames_are_not_opened() {
        let target_basename = wide("app.exe");
        let target_path = wide(r"C:\Apps\App.exe");
        let closes = Rc::new(Cell::new(0));
        let mut steps = vec![
            ProcessStep::Entry {
                pid: 5,
                basename: wide("other.exe"),
            },
            ProcessStep::Entry {
                pid: 7,
                basename: wide("APP.EXE"),
            },
            ProcessStep::Failed,
        ]
        .into_iter();
        let mut queried = Vec::new();

        let lookup = select_unique_process_with(
            &target_basename,
            &target_path,
            || steps.next().unwrap(),
            |pid| {
                queried.push(pid);
                Ok((FakeHandle(Rc::clone(&closes)), target_path.clone()))
            },
        );

        assert!(matches!(lookup, ProcessLookup::Indeterminate));
        assert_eq!(queried, [7]);
        assert_eq!(closes.get(), 1);
    }

    #[test]
    fn window_enumeration_keeps_first_eligible_window_and_always_continues() {
        let mut state = WindowEnumeration::default();
        let fixtures = [
            (
                10,
                WindowProperties {
                    pid: Ok(7),
                    visible: true,
                    owned: Ok(false),
                    tool: Ok(false),
                },
            ),
            (
                11,
                WindowProperties {
                    pid: Ok(7),
                    visible: true,
                    owned: Ok(false),
                    tool: Ok(false),
                },
            ),
            (
                12,
                WindowProperties {
                    pid: Ok(99),
                    visible: true,
                    owned: Ok(false),
                    tool: Ok(false),
                },
            ),
        ];

        let callback_results: Vec<_> = fixtures
            .into_iter()
            .map(|(hwnd, properties)| consider_window(&mut state, 7, hwnd, properties))
            .collect();

        assert_eq!(callback_results, [true, true, true]);
        assert_eq!(
            finish_window_enumeration(state, true),
            WindowLookup::Eligible(10)
        );
    }

    #[test]
    fn window_ineligible_or_uncertain_candidates_never_produce_an_activation_target() {
        for properties in [
            WindowProperties {
                pid: Ok(7),
                visible: false,
                owned: Ok(false),
                tool: Ok(false),
            },
            WindowProperties {
                pid: Ok(7),
                visible: true,
                owned: Ok(true),
                tool: Ok(false),
            },
            WindowProperties {
                pid: Ok(7),
                visible: true,
                owned: Ok(false),
                tool: Ok(true),
            },
        ] {
            let mut state = WindowEnumeration::default();
            assert!(consider_window(&mut state, 7, 10, properties));
            assert_eq!(
                finish_window_enumeration(state, true),
                WindowLookup::Unavailable
            );
        }

        for properties in [
            WindowProperties {
                pid: Err(()),
                visible: true,
                owned: Ok(false),
                tool: Ok(false),
            },
            WindowProperties {
                pid: Ok(7),
                visible: true,
                owned: Err(()),
                tool: Ok(false),
            },
            WindowProperties {
                pid: Ok(7),
                visible: true,
                owned: Ok(false),
                tool: Err(()),
            },
        ] {
            let mut state = WindowEnumeration::default();
            assert!(consider_window(&mut state, 7, 10, properties));
            assert_eq!(
                finish_window_enumeration(state, true),
                WindowLookup::Indeterminate
            );
        }

        assert_eq!(
            finish_window_enumeration(WindowEnumeration::<usize>::default(), false),
            WindowLookup::Indeterminate
        );
    }

    #[test]
    fn primitive_try_activate_maps_native_decisions_without_launching() {
        let unavailable = try_activate_with(
            || ProcessLookup::<()>::Unavailable,
            |_| panic!("unavailable process must not enumerate windows"),
            |_: usize| panic!("unavailable process must not request foreground"),
        );
        assert_eq!(unavailable, NativeActivation::Unavailable);

        let indeterminate = try_activate_with(
            || ProcessLookup::<()>::Indeterminate,
            |_| panic!("indeterminate process must not enumerate windows"),
            |_: usize| panic!("indeterminate process must not request foreground"),
        );
        assert_eq!(indeterminate, NativeActivation::Indeterminate);

        let no_window = try_activate_with(
            || ProcessLookup::Unique { pid: 7, handle: () },
            |pid| {
                assert_eq!(pid, 7);
                WindowLookup::<usize>::Unavailable
            },
            |_| panic!("missing window must not request foreground"),
        );
        assert_eq!(no_window, NativeActivation::Unavailable);

        let uncertain_window = try_activate_with(
            || ProcessLookup::Unique { pid: 7, handle: () },
            |_| WindowLookup::<usize>::Indeterminate,
            |_| panic!("indeterminate window must not request foreground"),
        );
        assert_eq!(uncertain_window, NativeActivation::Indeterminate);
    }

    #[test]
    fn primitive_try_activate_retains_process_handle_through_foreground_result() {
        for (foreground, expected) in [
            (true, NativeActivation::Activated),
            (false, NativeActivation::Refused),
        ] {
            let closes = Rc::new(Cell::new(0));
            let actual = try_activate_with(
                || ProcessLookup::Unique {
                    pid: 7,
                    handle: FakeHandle(Rc::clone(&closes)),
                },
                |_| WindowLookup::Eligible(11_usize),
                |hwnd| {
                    assert_eq!(hwnd, 11);
                    assert_eq!(closes.get(), 0);
                    foreground
                },
            );
            assert_eq!(actual, expected);
            assert_eq!(closes.get(), 1);
        }
    }

    #[test]
    fn primitive_launch_shortcut_calls_shell_once_and_uses_greater_than_thirty_two() {
        let calls = Cell::new(0);
        let shortcut = std::path::Path::new(r"C:\Menu\App.lnk");

        let success = launch_shortcut_with(shortcut, |path| {
            calls.set(calls.get() + 1);
            assert_eq!(path.last(), Some(&0));
            assert_eq!(&path[..path.len() - 1], wide(r"C:\Menu\App.lnk"));
            33
        });
        assert_eq!(success, Ok(()));
        assert_eq!(calls.get(), 1);

        assert_eq!(
            launch_shortcut_with(shortcut, |_| 32),
            Err(NativeActionError::ApplicationEntryUnavailable)
        );
    }
}
