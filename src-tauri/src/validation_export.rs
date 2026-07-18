use std::{fmt, path::PathBuf, slice};

use windows::{
    core::{HRESULT, PCWSTR, PWSTR},
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND, S_FALSE, S_OK},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        },
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FileSaveDialog, IFileSaveDialog, FILEOPENDIALOGOPTIONS,
            FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, SIGDN,
            SIGDN_FILESYSPATH,
        },
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportError {
    ComUnavailable,
    DialogFailed,
    InvalidDestination,
    MissingResearchId,
    Serialize,
    Write,
}

pub(crate) struct ExportDestination(PathBuf);

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ComUnavailable => "export COM unavailable",
            Self::DialogFailed => "export dialog failed",
            Self::InvalidDestination => "export destination is invalid",
            Self::MissingResearchId => "export research ID is missing",
            Self::Serialize => "export serialization failed",
            Self::Write => "export write failed",
        })
    }
}

impl std::error::Error for ExportError {}

pub(crate) fn choose_export_destination(
    owner: HWND,
) -> Result<Option<ExportDestination>, ExportError> {
    with_com_apartment(
        || unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) },
        || unsafe { CoUninitialize() },
        || {
            let dialog: IFileSaveDialog = unsafe {
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                    .map_err(|_| ExportError::DialogFailed)?
            };
            configure_dialog(&dialog)?;
            dialog_result_with(
                owner,
                |actual_owner| match unsafe { dialog.Show(actual_owner) } {
                    Ok(()) => S_OK,
                    Err(error) => error.code(),
                },
                || {
                    let item =
                        unsafe { dialog.GetResult() }.map_err(|_| ExportError::DialogFailed)?;
                    shell_path_with(
                        |name| {
                            unsafe { item.GetDisplayName(name) }
                                .map_err(|_| ExportError::DialogFailed)
                        },
                        |pointer| unsafe { CoTaskMemFree(Some(pointer.cast())) },
                    )
                },
            )
        },
    )
}

fn configure_dialog(dialog: &IFileSaveDialog) -> Result<(), ExportError> {
    configure_dialog_with(|extension, label, pattern, options| {
        let extension = wide_null(extension);
        let label = wide_null(label);
        let pattern = wide_null(pattern);
        let filter = COMDLG_FILTERSPEC {
            pszName: PCWSTR(label.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        };
        unsafe {
            dialog
                .SetDefaultExtension(PCWSTR(extension.as_ptr()))
                .and_then(|_| dialog.SetFileTypes(&[filter]))
                .and_then(|_| dialog.SetOptions(options))
                .map_err(|_| ExportError::DialogFailed)
        }
    })
}

fn configure_dialog_with<C>(configure: C) -> Result<(), ExportError>
where
    C: FnOnce(&str, &str, &str, FILEOPENDIALOGOPTIONS) -> Result<(), ExportError>,
{
    configure(
        "json",
        "JSON (*.json)",
        "*.json",
        FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT | FOS_NOCHANGEDIR,
    )
}

fn dialog_result_with<S, R>(
    owner: HWND,
    show: S,
    result: R,
) -> Result<Option<ExportDestination>, ExportError>
where
    S: FnOnce(Option<HWND>) -> HRESULT,
    R: FnOnce() -> Result<ExportDestination, ExportError>,
{
    let status = show(Some(owner));
    if status == cancelled_hresult() {
        return Ok(None);
    }
    if status.is_err() {
        return Err(ExportError::DialogFailed);
    }
    result().map(Some)
}

fn cancelled_hresult() -> HRESULT {
    HRESULT((0x8007_0000_u32 | ERROR_CANCELLED.0) as i32)
}

fn with_com_apartment<I, U, O, T>(
    initialize: I,
    uninitialize: U,
    operation: O,
) -> Result<T, ExportError>
where
    I: FnOnce() -> HRESULT,
    U: FnOnce(),
    O: FnOnce() -> Result<T, ExportError>,
{
    let status = initialize();
    if status != S_OK && status != S_FALSE {
        return Err(ExportError::ComUnavailable);
    }
    let _guard = ComApartment {
        uninitialize: Some(uninitialize),
    };
    operation()
}

struct ComApartment<U: FnOnce()> {
    uninitialize: Option<U>,
}

impl<U: FnOnce()> Drop for ComApartment<U> {
    fn drop(&mut self) {
        self.uninitialize.take().expect("COM uninitializer missing")();
    }
}

fn shell_path_with<C, F>(call: C, free: F) -> Result<ExportDestination, ExportError>
where
    C: FnOnce(SIGDN) -> Result<PWSTR, ExportError>,
    F: FnOnce(*mut u16),
{
    let raw = call(SIGDN_FILESYSPATH)?;
    let owned = ShellPath {
        pointer: raw.0,
        free: Some(free),
    };
    strict_path_from_utf16(owned.pointer).map(ExportDestination)
}

struct ShellPath<F: FnOnce(*mut u16)> {
    pointer: *mut u16,
    free: Option<F>,
}

impl<F: FnOnce(*mut u16)> Drop for ShellPath<F> {
    fn drop(&mut self) {
        self.free.take().expect("Shell path deallocator missing")(self.pointer);
    }
}

fn strict_path_from_utf16(pointer: *mut u16) -> Result<PathBuf, ExportError> {
    if pointer.is_null() {
        return Err(ExportError::InvalidDestination);
    }
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    if length == 0 {
        return Err(ExportError::InvalidDestination);
    }
    let value = String::from_utf16(unsafe { slice::from_raw_parts(pointer, length) })
        .map_err(|_| ExportError::InvalidDestination)?;
    Ok(PathBuf::from(value))
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
impl ExportDestination {
    fn test_path(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, ffi::c_void, path::Path};

    use windows::{
        core::{HRESULT, PWSTR},
        Win32::{
            Foundation::{HWND, RPC_E_CHANGED_MODE, S_FALSE, S_OK},
            UI::Shell::{
                FILEOPENDIALOGOPTIONS, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT,
                FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
            },
        },
    };

    use super::*;

    const CANCELLED: HRESULT = HRESULT(0x8007_04c7_u32 as i32);
    const FAILURE: HRESULT = HRESULT(0x8000_4005_u32 as i32);

    #[test]
    fn valid_filesystem_path_is_freed_once_and_wrapped() {
        let mut wide: Vec<u16> = r"C:\Users\Test\validation.json"
            .encode_utf16()
            .chain([0])
            .collect();
        let frees = Cell::new(0);

        let destination = shell_path_with(
            |name| {
                assert_eq!(name, SIGDN_FILESYSPATH);
                Ok(PWSTR(wide.as_mut_ptr()))
            },
            |_| frees.set(frees.get() + 1),
        )
        .unwrap();

        assert_eq!(frees.get(), 1);
        assert_eq!(
            destination.test_path(),
            Path::new(r"C:\Users\Test\validation.json")
        );
    }

    #[test]
    fn null_empty_and_invalid_utf16_paths_are_freed_once() {
        let mut empty = [0_u16];
        let mut invalid = [0xd800_u16, 0];
        let pointers = [
            std::ptr::null_mut(),
            empty.as_mut_ptr(),
            invalid.as_mut_ptr(),
        ];

        for pointer in pointers {
            let frees = Cell::new(0);
            assert!(matches!(
                shell_path_with(|_| Ok(PWSTR(pointer)), |_| frees.set(frees.get() + 1),),
                Err(ExportError::InvalidDestination)
            ));
            assert_eq!(frees.get(), 1);
        }
    }

    #[test]
    fn com_success_and_already_initialized_are_uninitialized_once() {
        for status in [S_OK, S_FALSE] {
            let uninitializes = Cell::new(0);
            let operation_called = Cell::new(false);

            let value = with_com_apartment(
                || status,
                || uninitializes.set(uninitializes.get() + 1),
                || {
                    operation_called.set(true);
                    Ok(17)
                },
            )
            .unwrap();

            assert_eq!(value, 17);
            assert!(operation_called.get());
            assert_eq!(uninitializes.get(), 1);
        }
    }

    #[test]
    fn changed_com_mode_does_not_run_or_uninitialize() {
        let uninitializes = Cell::new(0);
        let operation_called = Cell::new(false);

        assert_eq!(
            with_com_apartment(
                || RPC_E_CHANGED_MODE,
                || uninitializes.set(uninitializes.get() + 1),
                || {
                    operation_called.set(true);
                    Ok(())
                },
            ),
            Err(ExportError::ComUnavailable)
        );
        assert!(!operation_called.get());
        assert_eq!(uninitializes.get(), 0);
    }

    #[test]
    fn dialog_show_uses_owner_and_only_cancel_returns_none() {
        let owner = HWND(7_isize as *mut c_void);
        let result_called = Cell::new(false);

        assert!(matches!(
            dialog_result_with(
                owner,
                |actual| {
                    assert_eq!(actual, Some(owner));
                    CANCELLED
                },
                || {
                    result_called.set(true);
                    panic!("cancel must not read the result")
                },
            ),
            Ok(None)
        ));
        assert!(!result_called.get());

        assert!(matches!(
            dialog_result_with(
                owner,
                |actual| {
                    assert_eq!(actual, Some(owner));
                    FAILURE
                },
                || panic!("failed dialog must not read the result"),
            ),
            Err(ExportError::DialogFailed)
        ));
    }

    #[test]
    fn successful_dialog_reads_one_destination() {
        let owner = HWND(11_isize as *mut c_void);
        let mut wide: Vec<u16> = r"C:\Exports\validation.json"
            .encode_utf16()
            .chain([0])
            .collect();

        let destination = dialog_result_with(
            owner,
            |actual| {
                assert_eq!(actual, Some(owner));
                S_OK
            },
            || shell_path_with(|_| Ok(PWSTR(wide.as_mut_ptr())), |_| {}),
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            destination.test_path(),
            Path::new(r"C:\Exports\validation.json")
        );
    }

    #[test]
    fn dialog_configuration_is_exact() {
        let expected_options =
            FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT | FOS_NOCHANGEDIR;

        configure_dialog_with(|extension, label, pattern, options| {
            assert_eq!(extension, "json");
            assert_eq!(label, "JSON (*.json)");
            assert_eq!(pattern, "*.json");
            assert_eq!(options, expected_options);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn errors_are_fixed_and_path_free() {
        fn assert_std_error<T: std::error::Error>() {}
        assert_std_error::<ExportError>();

        let cases = [
            (ExportError::ComUnavailable, "export COM unavailable"),
            (ExportError::DialogFailed, "export dialog failed"),
            (
                ExportError::InvalidDestination,
                "export destination is invalid",
            ),
            (
                ExportError::MissingResearchId,
                "export research ID is missing",
            ),
            (ExportError::Serialize, "export serialization failed"),
            (ExportError::Write, "export write failed"),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert!(!error.to_string().contains(':'));
            assert!(!error.to_string().contains('\\'));
        }
    }

    #[test]
    fn file_dialog_options_type_is_the_expected_windows_type() {
        let _: FILEOPENDIALOGOPTIONS = FOS_FORCEFILESYSTEM;
    }

    #[test]
    fn native_chooser_exposes_only_owner_and_opaque_destination() {
        let _: fn(HWND) -> Result<Option<ExportDestination>, ExportError> =
            choose_export_destination;
    }
}
