//! Single-use, cancellable output jobs. Execute `perform` on a background worker.
use super::{
    geometry::{BudgetedImage, PixelBudget},
    output::{encode_png, encoding_workspace_bytes},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputKind {
    SaveDesktop,
    Copy,
}
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum OutputSuccess {
    Saved(PathBuf),
    Copied,
}
#[derive(Default)]
pub(crate) struct OutputGate {
    state: AtomicU8,
}
const READY: u8 = 0;
const RUNNING: u8 = 1;
const CANCELLED: u8 = 2;
const COMMITTING: u8 = 3;
const FINISHED: u8 = 4;
impl OutputGate {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    /// Nonblocking: false means commit authorization or completion already won.
    /// Once authorization wins the worker reports the actual publish result.
    pub(crate) fn cancel(&self) -> bool {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            match state {
                READY | RUNNING => match self.state.compare_exchange_weak(
                    state,
                    CANCELLED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return true,
                    Err(current) => state = current,
                },
                CANCELLED => return true,
                _ => return false,
            }
        }
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLED
    }
    fn begin(&self) -> Result<(), String> {
        self.state
            .compare_exchange(READY, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|state| {
                if state == CANCELLED {
                    "Cancelled"
                } else {
                    "OutputAlreadyStarted"
                }
                .into()
            })
    }
    fn check_cancelled(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("Cancelled".into())
        } else {
            Ok(())
        }
    }
    /// The single linearization point between cancellation and external output.
    fn authorize_commit(&self) -> Result<(), String> {
        self.state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                matches!(state, READY | RUNNING).then_some(COMMITTING)
            })
            .map(|_| ())
            .map_err(|state| {
                if state == CANCELLED {
                    "Cancelled"
                } else {
                    "OutputAlreadyStarted"
                }
                .into()
            })
    }
}
struct FinishJob<'a>(&'a OutputGate);
impl Drop for FinishJob<'_> {
    fn drop(&mut self) {
        let _ = self
            .0
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                matches!(state, RUNNING | COMMITTING).then_some(FINISHED)
            });
    }
}
/// Owns pixels and leases through completion. Never call on the overlay thread.
/// Each invocation needs a fresh gate, including after a failed output.
pub(crate) fn perform(
    kind: OutputKind,
    image: BudgetedImage,
    budget: &PixelBudget,
    desktop: Result<PathBuf, String>,
    gate: Arc<OutputGate>,
) -> Result<OutputSuccess, String> {
    perform_with(kind, image, budget, desktop, gate, |png| {
        crate::clipboard_image::copy_png(png).map_err(|error| {
            match error {
                crate::clipboard_image::ClipboardImageError::Busy => "ClipboardBusy",
                _ => "ClipboardWriteFailed",
            }
            .into()
        })
    })
}
fn perform_with(
    kind: OutputKind,
    image: BudgetedImage,
    budget: &PixelBudget,
    desktop: Result<PathBuf, String>,
    gate: Arc<OutputGate>,
    copy: impl FnOnce(&[u8]) -> Result<(), String>,
) -> Result<OutputSuccess, String> {
    gate.begin()?;
    let _finish = FinishJob(&gate);
    let directory = match kind {
        OutputKind::SaveDesktop => {
            let path = desktop.map_err(|_| "DesktopUnavailable".to_owned())?;
            validate_directory(&path)?;
            Some(path)
        }
        OutputKind::Copy => None,
    };
    let encoding_bytes =
        encoding_workspace_bytes(image.width(), image.height()).map_err(|e| e.to_string())?;
    // This lease covers both encoding scratch and the PNG Vec. Keep it until
    // after PNG use, including clipboard preparation and publication.
    let _encoding = budget.reserve(encoding_bytes).map_err(|e| e.to_string())?;
    gate.check_cancelled()?;
    let png = encode_png(image.width(), image.height(), &image.image().pixels)
        .map_err(|e| e.to_string())?;
    gate.check_cancelled()?;
    match kind {
        OutputKind::SaveDesktop => {
            let stamp = chrono::Local::now()
                .format("%Y-%m-%d_%H-%M-%S-%3f")
                .to_string();
            save_with_io(
                directory.as_deref().expect("save directory validated"),
                &png,
                &stamp,
                &gate,
                |file, bytes| file.write_all(bytes).and_then(|_| file.sync_all()),
                publish,
            )
            .map(OutputSuccess::Saved)
        }
        OutputKind::Copy => {
            let clipboard_bytes = crate::clipboard_image::clipboard_workspace_bytes(
                png.len(),
                image.width(),
                image.height(),
            )
            .ok_or_else(|| "ResourceLimit".to_owned())?;
            let _clipboard = budget.reserve(clipboard_bytes).map_err(|e| e.to_string())?;
            gate.authorize_commit()?;
            copy(&png)?;
            Ok(OutputSuccess::Copied)
        }
    }
}
fn validate_directory(directory: &Path) -> Result<(), String> {
    if directory.as_os_str().is_empty()
        || !fs::metadata(directory).is_ok_and(|metadata| metadata.is_dir())
    {
        return Err("DesktopUnavailable".into());
    }
    Ok(())
}
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn save_with_io(
    directory: &Path,
    png: &[u8],
    stamp: &str,
    gate: &OutputGate,
    mut write: impl FnMut(&mut File, &[u8]) -> io::Result<()>,
    mut publish: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<PathBuf, String> {
    gate.check_cancelled()?;
    validate_directory(directory)?;
    let (mut file, temp) = loop {
        gate.check_cancelled()?;
        let path = directory.join(format!(
            ".uipilot-screenshot-desktop-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (file, TempFile(path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("SaveFailed".into()),
        }
    };
    let result = write(&mut file, png);
    // Windows requires closing the handle before both moving and cleanup.
    drop(file);
    result.map_err(|_| "SaveFailed".to_owned())?;
    gate.authorize_commit()?;
    let mut suffix = 0u64;
    loop {
        let name = if suffix == 0 {
            format!("截图_{stamp}.png")
        } else {
            format!("截图_{stamp}_{suffix}.png")
        };
        let path = directory.join(name);
        // No existence check: the publication itself must reject collisions.
        match publish(&temp.0, &path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                suffix = suffix
                    .checked_add(1)
                    .ok_or_else(|| "SaveFailed".to_owned())?;
            }
            Err(_) => return Err("SaveFailed".into()),
        }
    }
}
#[cfg(windows)]
fn publish(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
    };
    let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Deliberately omit MOVEFILE_REPLACE_EXISTING. Preserve the OS error code
    // so ERROR_FILE_EXISTS / ERROR_ALREADY_EXISTS can select another name.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xffff))
}
#[cfg(not(windows))]
fn publish(from: &Path, to: &Path) -> io::Result<()> {
    // Same-directory hard-link publication atomically rejects any old target.
    // The temporary guard unlinks our original name after publication.
    fs::hard_link(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "uipilot-desktop-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn entries(&self) -> Vec<PathBuf> {
            fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect()
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn write(file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    }
    fn save(dir: &Path, gate: &OutputGate) -> Result<PathBuf, String> {
        save_with_io(
            dir,
            b"complete png",
            "2026-10-29_12-34-56-789",
            gate,
            write,
            publish,
        )
    }
    #[test]
    fn collision_keeps_existing_file_and_uses_suffix() {
        let dir = TestDirectory::new();
        let existing = dir.0.join("截图_2026-10-29_12-34-56-789.png");
        fs::write(&existing, b"original").unwrap();
        let path = save(&dir.0, &OutputGate::new()).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "截图_2026-10-29_12-34-56-789_1.png"
        );
        assert_eq!(fs::read(existing).unwrap(), b"original");
        assert_eq!(fs::read(path).unwrap(), b"complete png");
        assert_eq!(dir.entries().len(), 2);
    }
    #[test]
    fn concurrent_same_names_publish_distinct_complete_files() {
        let dir = TestDirectory::new();
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                let path = dir.0.clone();
                std::thread::spawn(move || save(&path, &OutputGate::new()).unwrap())
            })
            .collect();
        let mut paths: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 8);
        assert_eq!(dir.entries().len(), 8);
        for path in paths {
            assert_eq!(fs::read(path).unwrap(), b"complete png");
        }
    }
    #[test]
    fn cancellation_before_commit_cleans_complete_temporary_file() {
        let dir = TestDirectory::new();
        let gate = OutputGate::new();
        let result = save_with_io(
            &dir.0,
            b"png",
            "fixed",
            &gate,
            |file, bytes| {
                write(file, bytes)?;
                assert!(gate.cancel());
                Ok(())
            },
            |_, _| panic!("cancelled output must not publish"),
        );
        assert_eq!(result, Err("Cancelled".into()));
        assert!(dir.entries().is_empty());
    }
    #[test]
    fn cancellation_after_commit_authorization_reports_saved_truthfully() {
        let dir = TestDirectory::new();
        let gate = OutputGate::new();
        let path = save_with_io(&dir.0, b"png", "fixed", &gate, write, |from, to| {
            assert!(!gate.cancel());
            publish(from, to)
        })
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), b"png");
        assert_eq!(dir.entries().len(), 1);
    }
    #[test]
    fn partial_write_and_failed_publish_clean_only_owned_temporary_file() {
        let dir = TestDirectory::new();
        fs::write(dir.0.join("unrelated.tmp"), b"keep").unwrap();
        assert_eq!(
            save_with_io(
                &dir.0,
                b"png",
                "fixed",
                &OutputGate::new(),
                |file, _| {
                    file.write_all(b"partial")?;
                    Err(io::Error::other("write failure"))
                },
                |_, _| panic!("must not publish partial file")
            ),
            Err("SaveFailed".into())
        );
        assert_eq!(
            save_with_io(
                &dir.0,
                b"png",
                "fixed",
                &OutputGate::new(),
                write,
                |_, _| Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            ),
            Err("SaveFailed".into())
        );
        assert_eq!(dir.entries(), vec![dir.0.join("unrelated.tmp")]);
    }
    #[test]
    fn missing_directory_and_file_in_place_of_directory_never_fall_back() {
        let dir = TestDirectory::new();
        let missing = dir.0.join("missing");
        assert_eq!(
            save(&missing, &OutputGate::new()),
            Err("DesktopUnavailable".into())
        );
        assert!(!missing.exists());
        let file = dir.0.join("file");
        fs::write(&file, b"keep").unwrap();
        assert_eq!(
            save(&file, &OutputGate::new()),
            Err("DesktopUnavailable".into())
        );
        assert_eq!(dir.entries(), vec![file]);
    }
    #[test]
    fn copy_keeps_all_budget_leases_until_callback_returns_and_releases_them() {
        let budget = PixelBudget::default();
        let image = BudgetedImage::allocate(2, 1, &budget).unwrap();
        let result = perform_with(
            OutputKind::Copy,
            image,
            &budget,
            Err("unused".into()),
            Arc::new(OutputGate::new()),
            |png| {
                let encoding = super::super::output::encoding_workspace_bytes(2, 1).unwrap();
                let clipboard =
                    crate::clipboard_image::clipboard_workspace_bytes(png.len(), 2, 1).unwrap();
                assert!(budget.used() >= 8 + encoding + clipboard);
                assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
                Ok(())
            },
        );
        assert_eq!(result, Ok(OutputSuccess::Copied));
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn jobs_use_fresh_gates_and_cancelled_copy_never_calls_clipboard() {
        let budget = PixelBudget::default();
        let gate = Arc::new(OutputGate::new());
        assert!(gate.cancel());
        assert!(gate.is_cancelled());
        assert_eq!(
            perform_with(
                OutputKind::Copy,
                BudgetedImage::allocate(1, 1, &budget).unwrap(),
                &budget,
                Err("unused".into()),
                gate,
                |_| panic!("cancelled copy")
            ),
            Err("Cancelled".into())
        );
        let used = Arc::new(OutputGate::new());
        assert_eq!(
            perform_with(
                OutputKind::Copy,
                BudgetedImage::allocate(1, 1, &budget).unwrap(),
                &budget,
                Err("unused".into()),
                used.clone(),
                |_| Ok(())
            ),
            Ok(OutputSuccess::Copied)
        );
        assert_eq!(
            perform_with(
                OutputKind::Copy,
                BudgetedImage::allocate(1, 1, &budget).unwrap(),
                &budget,
                Err("unused".into()),
                used,
                |_| panic!("gate cannot be reused")
            ),
            Err("OutputAlreadyStarted".into())
        );
    }
    #[test]
    fn resource_failure_releases_pixels_before_any_clipboard_access() {
        let budget = PixelBudget::new(4);
        assert_eq!(
            perform_with(
                OutputKind::Copy,
                BudgetedImage::allocate(1, 1, &budget).unwrap(),
                &budget,
                Err("unused".into()),
                Arc::new(OutputGate::new()),
                |_| panic!("no workspace")
            ),
            Err("ResourceLimit".into())
        );
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn save_job_encodes_actual_pixels_in_injected_desktop_directory() {
        let dir = TestDirectory::new();
        let budget = PixelBudget::default();
        let mut image = BudgetedImage::allocate(1, 1, &budget).unwrap();
        image.pixels_mut().copy_from_slice(&[12, 34, 56, 255]);
        let OutputSuccess::Saved(path) = perform(
            OutputKind::SaveDesktop,
            image,
            &budget,
            Ok(dir.0.clone()),
            Arc::new(OutputGate::new()),
        )
        .unwrap() else {
            panic!("expected saved path")
        };
        assert_eq!(path.parent(), Some(dir.0.as_path()));
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("截图_"));
        assert!(name.ends_with(".png"));
        let mut reader = png::Decoder::new(io::BufReader::new(File::open(path).unwrap()))
            .read_info()
            .unwrap();
        let mut rgba = [0; 4];
        reader.next_frame(&mut rgba).unwrap();
        assert_eq!(rgba, [12, 34, 56, 255]);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn desktop_resolution_and_clipboard_failures_have_stable_retry_codes() {
        let budget = PixelBudget::default();
        assert_eq!(
            perform(
                OutputKind::SaveDesktop,
                BudgetedImage::allocate(1, 1, &budget).unwrap(),
                &budget,
                Err("platform-specific details".into()),
                Arc::new(OutputGate::new())
            ),
            Err("DesktopUnavailable".into())
        );
        assert_eq!(
            perform_with(
                OutputKind::Copy,
                BudgetedImage::allocate(1, 1, &budget).unwrap(),
                &budget,
                Err("unused".into()),
                Arc::new(OutputGate::new()),
                |_| Err("ClipboardBusy".into())
            ),
            Err("ClipboardBusy".into())
        );
        assert_eq!(budget.used(), 0);
    }
}
