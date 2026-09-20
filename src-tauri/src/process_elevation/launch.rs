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
        Foundation::{CloseHandle, FILETIME, HANDLE},
        System::Threading::{GetProcessId, GetProcessTimes},
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
    cancel_requested: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BootstrapArgs {
    pub(crate) version: String,
    pub(crate) request_id: String,
    pub(crate) pipe_nonce: String,
    pub(crate) host_pid: u32,
    pub(crate) host_creation_time: u64,
}

impl BootstrapArgs {
    pub(crate) fn parse(value: &str) -> Result<Self, &'static str> {
        let mut fields = value.split(';');
        let version = field(&mut fields, "v")?;
        let request_id = field(&mut fields, "id")?;
        let pipe_nonce = field(&mut fields, "nonce")?;
        let host_pid = field(&mut fields, "pid")?
            .parse::<u32>()
            .map_err(|_| "InvalidLaunchArgument")?;
        let host_creation_time = field(&mut fields, "ct")?
            .parse::<u64>()
            .map_err(|_| "InvalidLaunchArgument")?;
        if fields.next().is_some()
            || !valid_token(&version, 64, false)
            || request_id.len() != 64
            || pipe_nonce.len() != 64
            || !valid_token(&request_id, 64, true)
            || !valid_token(&pipe_nonce, 64, true)
            || host_pid == 0
            || host_creation_time == 0
        {
            return Err("InvalidLaunchArgument");
        }
        Ok(Self {
            version,
            request_id,
            pipe_nonce,
            host_pid,
            host_creation_time,
        })
    }
}

fn field<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    expected: &str,
) -> Result<String, &'static str> {
    let value = fields.next().ok_or("InvalidLaunchArgument")?;
    let (key, value) = value.split_once('=').ok_or("InvalidLaunchArgument")?;
    if key != expected || value.is_empty() {
        return Err("InvalidLaunchArgument");
    }
    Ok(value.to_owned())
}

fn valid_token(value: &str, max: usize, hex: bool) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|byte| {
            if hex {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')
            }
        })
}

impl LaunchCoordinator {
    pub(crate) fn reserve(&self) -> Result<LaunchReservation, &'static str> {
        self.pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "LaunchAlreadyPending")?;
        Ok(LaunchReservation {
            pending: Arc::clone(&self.pending),
            cancel_requested: false,
        })
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}
impl LaunchReservation {
    pub(crate) fn cancel_wait(&mut self) {
        self.cancel_requested = true;
    }
    pub(crate) fn cancel_requested(&self) -> bool {
        self.cancel_requested
    }
    pub(crate) fn complete(self, result: LaunchResult) -> LaunchResult {
        result
    }
}
impl Drop for LaunchReservation {
    fn drop(&mut self) {
        // Dropping the reservation means its native launch call is no longer
        // owned by this coordinator. Never strand the single pending slot.
        self.pending.store(false, Ordering::Release);
    }
}

#[cfg(windows)]
pub(crate) struct ElevatedProcess {
    handle: HANDLE,
    image: super::image::ImageLease,
}

#[cfg(windows)]
impl ElevatedProcess {
    pub(crate) fn raw(&self) -> HANDLE {
        self.handle
    }
}

#[cfg(windows)]
impl Drop for ElevatedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
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
    let bootstrap = BootstrapArgs::parse(parameters)?;
    package.verify_version(&bootstrap.version)?;
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
    let image = package.lease_helper_image()?;
    let verb = wide(OsStr::new("runas"))?;
    let file = wide(image.path().as_os_str())?;
    let params = wide(OsStr::new(parameters))?;
    let cwd = wide(
        image
            .path()
            .parent()
            .ok_or("UntrustedInstallation")?
            .as_os_str(),
    )?;
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
    let process = ElevatedProcess {
        handle: info.hProcess,
        image,
    };
    process.image.verify_process_image(process.raw())?;
    Ok(process)
}

#[cfg(windows)]
pub(crate) fn launch_helper(
    coordinator: &LaunchCoordinator,
    package: &crate::process_elevation::TrustPackage,
    parameters: &str,
) -> Result<ElevatedProcess, &'static str> {
    let reservation = coordinator.reserve()?;
    match launch_runas(package, parameters) {
        Ok(process) => {
            let pid = unsafe { GetProcessId(process.raw()) };
            let mut times = [FILETIME::default(); 4];
            if pid == 0
                || unsafe {
                    GetProcessTimes(
                        process.raw(),
                        &mut times[0],
                        &mut times[1],
                        &mut times[2],
                        &mut times[3],
                    )
                }
                .is_err()
            {
                let _ = reservation.complete(LaunchResult::LaunchFailed);
                return Err("PeerIdentityUnavailable");
            }
            let creation_time =
                (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
            if creation_time == 0 {
                let _ = reservation.complete(LaunchResult::LaunchFailed);
                return Err("PeerIdentityUnavailable");
            }
            let _ = reservation.complete(LaunchResult::Started { pid, creation_time });
            Ok(process)
        }
        Err(error) => {
            let result = match error {
                "LaunchHandleMissing" => LaunchResult::LaunchFailed,
                "LaunchFailed" => LaunchResult::LaunchFailed,
                "UntrustedInstallation" | "InvalidLaunchArgument" => LaunchResult::LaunchFailed,
                _ => LaunchResult::LaunchFailed,
            };
            let _ = reservation.complete(result);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_metadata_is_strict_and_exact() {
        let encoded = "v=legacy;id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;nonce=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb;pid=7;ct=11";
        let args = BootstrapArgs::parse(encoded).unwrap();
        assert_eq!(args.version, "legacy");
        assert_eq!(args.host_pid, 7);
        assert_eq!(args.host_creation_time, 11);
        for invalid in [
            "v=legacy;id=a;nonce=b;pid=7;ct=11",
            "v=legacy;id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;nonce=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb;pid=0;ct=11",
            "v=legacy;id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;nonce=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb;pid=7;ct=11;extra=x",
        ] {
            assert!(BootstrapArgs::parse(invalid).is_err());
        }
    }
    #[test]
    fn only_one_reservation_until_native_call_returns() {
        let coordinator = LaunchCoordinator::default();
        let reservation = coordinator.reserve().unwrap();
        assert!(coordinator.is_pending());
        assert_eq!(coordinator.reserve().unwrap_err(), "LaunchAlreadyPending");
        drop(reservation);
        assert!(!coordinator.is_pending());
    }
    #[test]
    fn completion_releases_slot_after_success_or_failure() {
        let coordinator = LaunchCoordinator::default();
        let mut reservation = coordinator.reserve().unwrap();
        reservation.cancel_wait();
        assert!(reservation.cancel_requested());
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
            Err("InvalidLaunchArgument")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn helper_launch_releases_reservation_when_pre_shell_validation_fails() {
        use std::path::PathBuf;
        let coordinator = LaunchCoordinator::default();
        let package = crate::process_elevation::TrustPackage::new(
            PathBuf::from(r"C:\safe"),
            "a".repeat(64),
            "b".repeat(64),
        )
        .unwrap();
        assert!(matches!(
            launch_helper(&coordinator, &package, "bad\0arg"),
            Err("InvalidLaunchArgument")
        ));
        assert!(!coordinator.is_pending());
    }
}
