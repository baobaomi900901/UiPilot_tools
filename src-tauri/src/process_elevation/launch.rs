use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(windows)]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LaunchResult {
    Started { pid: u32, creation_time: u64 },
    UacCancelled,
    LaunchFailed,
    LaunchExpired,
}

#[derive(Clone, Default)]
pub(crate) struct LaunchCoordinator {
    pending: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct LaunchReservation {
    pending: Arc<AtomicBool>,
    returned: bool,
}

impl LaunchCoordinator {
    pub(crate) fn reserve(&self) -> Result<LaunchReservation, &'static str> {
        self.pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "LaunchAlreadyPending")?;
        Ok(LaunchReservation {
            pending: Arc::clone(&self.pending),
            returned: false,
        })
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}
impl LaunchReservation {
    pub(crate) fn cancel_wait(&mut self) {}
    pub(crate) fn complete(mut self, result: LaunchResult) -> LaunchResult {
        self.returned = true;
        result
    }
}
impl Drop for LaunchReservation {
    fn drop(&mut self) {
        if self.returned {
            self.pending.store(false, Ordering::Release);
        }
    }
}

#[cfg(windows)]
pub(crate) struct ElevatedProcess(HANDLE);

#[cfg(windows)]
impl ElevatedProcess {
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

#[cfg(windows)]
impl Drop for ElevatedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn wide(value: &OsStr) -> Result<Vec<u16>, &'static str> {
    let mut result: Vec<u16> = value.encode_wide().collect();
    if result.contains(&0) {
        return Err("InvalidLaunchArgument");
    }
    result.push(0);
    Ok(result)
}

#[cfg(windows)]
pub(crate) fn launch_runas(
    package: &crate::process_elevation::TrustPackage,
    parameters: &str,
) -> Result<ElevatedProcess, &'static str> {
    let executable = package.helper_path();
    let directory = package.root().to_path_buf();
    if !executable.is_absolute()
        || !directory.is_absolute()
        || executable
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || directory
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || parameters.contains('\0')
    {
        return Err("UntrustedInstallation");
    }
    let verb = wide(OsStr::new("runas"))?;
    let file = wide(executable.as_os_str())?;
    let params = wide(OsStr::new(parameters))?;
    let cwd = wide(directory.as_os_str())?;
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
    unsafe {
        ShellExecuteExW(&mut info).map_err(|_| "LaunchFailed")?;
    }
    if info.hProcess.is_invalid() {
        return Err("LaunchHandleMissing");
    }
    Ok(ElevatedProcess(info.hProcess))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_reservation_until_native_call_returns() {
        let coordinator = LaunchCoordinator::default();
        let reservation = coordinator.reserve().unwrap();
        assert!(coordinator.is_pending());
        assert_eq!(coordinator.reserve().unwrap_err(), "LaunchAlreadyPending");
        drop(reservation);
        assert!(coordinator.is_pending());
    }
    #[test]
    fn completion_releases_slot_after_success_or_failure() {
        let coordinator = LaunchCoordinator::default();
        let mut reservation = coordinator.reserve().unwrap();
        reservation.cancel_wait();
        assert_eq!(
            reservation.complete(LaunchResult::LaunchExpired),
            LaunchResult::LaunchExpired
        );
        assert!(!coordinator.is_pending());
        assert!(coordinator.reserve().is_ok());
    }
    #[test]
    fn result_preserves_exact_created_identity() {
        let coordinator = LaunchCoordinator::default();
        let result = coordinator
            .reserve()
            .unwrap()
            .complete(LaunchResult::Started {
                pid: 7,
                creation_time: 11,
            });
        assert_eq!(
            result,
            LaunchResult::Started {
                pid: 7,
                creation_time: 11
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn runas_rejects_workspace_parent_paths_and_nul_parameters_before_shell() {
        use std::path::PathBuf;
        assert!(matches!(
            crate::process_elevation::TrustPackage::new(
                PathBuf::from(r"C:\workspace\..\helper"),
                "a".repeat(64),
                "b".repeat(64)
            ),
            Err("UntrustedInstallation")
        ));
        let package = crate::process_elevation::TrustPackage::new(
            PathBuf::from(r"C:\safe"),
            "a".repeat(64),
            "b".repeat(64),
        )
        .unwrap();
        assert!(matches!(
            launch_runas(&package, "bad\0arg"),
            Err("UntrustedInstallation")
        ));
    }
}
