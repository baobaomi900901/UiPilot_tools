use url::Url;

#[cfg(windows)]
pub(crate) fn open_url(url: Url) -> Result<(), ()> {
    use windows::{
        core::PCWSTR,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };

    if !matches!(url.scheme(), "http" | "https") {
        return Err(());
    }
    let wide: Vec<u16> = url.as_str().encode_utf16().chain([0]).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
        .0 as isize
    };
    (result > 32).then_some(()).ok_or(())
}

#[cfg(not(windows))]
pub(crate) fn open_url(_url: Url) -> Result<(), ()> {
    Err(())
}
