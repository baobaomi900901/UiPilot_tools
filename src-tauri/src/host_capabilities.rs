use serde::Serialize;
use tauri::WebviewWindow;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostCapabilities {
    version: u8,
    platform: &'static str,
    plugins: bool,
    files: bool,
    screenshot: bool,
    messages: bool,
    credentials: bool,
    builtin_features: bool,
    command_hotkeys: bool,
    autostart: bool,
    double_tap: bool,
    web_search: bool,
    login_items: bool,
}

fn local_webview(label: &str, expected: &str, page: &url::Url, origin: &url::Url) -> bool {
    label == expected && page.scheme() == origin.scheme() && page.host_str() == origin.host_str()
        && page.port_or_known_default() == origin.port_or_known_default()
        && page.username().is_empty() && page.password().is_none()
}

fn trusted_caller(label: &str, expected: &str, page: &url::Url, config: &tauri::Config) -> bool {
    let production = if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" };
    let origin = if tauri::is_dev() {
        let Some(dev_url) = config.build.dev_url.as_ref().filter(|url| {
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1")
        }) else {
            return false;
        };
        dev_url.clone()
    } else {
        url::Url::parse(production).expect("valid packaged origin")
    };
    local_webview(label, expected, page, &origin)
}

pub(crate) fn mac_find_enabled() -> bool {
    #[cfg(all(target_os = "macos", feature = "macos-find-system-qa"))]
    {
        std::env::var_os("UIPILOT_FIND_ISOLATED_QA").as_deref() == Some(std::ffi::OsStr::new("1"))
            && std::env::var_os("UIPILOT_FIND_QA_VERIFIED_ROOTS").is_some_and(|roots| !roots.is_empty())
            && crate::macos_find_results::qa_account_eligible()
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-find-system-qa")))]
    { cfg!(all(target_os = "macos", feature = "macos-find-preview")) }
}

pub(crate) fn trusted_page(label: &str, page: &url::Url, config: &tauri::Config) -> bool {
    trusted_caller(label, "main", page, config) ||
        (mac_find_enabled() &&
            trusted_caller(label, "find", page, config))
}

fn require(window: &WebviewWindow, expected: &str) -> Result<(), &'static str> {
    use tauri::Manager;
    let page = window.url().map_err(|_| "invalidCaller")?;
    trusted_caller(window.label(), expected, &page, window.app_handle().config())
        .then_some(()).ok_or("invalidCaller")
}

pub(crate) fn require_main(window: &WebviewWindow) -> Result<(), &'static str> {
    require(window, "main")
}

#[cfg(all(target_os = "macos", feature = "macos-find-preview"))]
pub(crate) fn require_find(window: &WebviewWindow) -> Result<(), &'static str> {
    if !mac_find_enabled() { return Err("invalidCaller"); }
    require(window, "find")
}

#[tauri::command]
pub(crate) fn get_host_capabilities(window: WebviewWindow) -> Result<HostCapabilities, &'static str> {
    require_main(&window)?;
    let windows = cfg!(windows);
    Ok(HostCapabilities {
        version: 1, platform: if windows { "windows" } else { "macos" },
        plugins: windows, files: windows || mac_find_enabled(), screenshot: windows, messages: windows, credentials: windows,
        login_items: cfg!(all(target_os = "macos", feature = "macos-login-item-preview")),
        web_search: true, builtin_features: windows, command_hotkeys: windows, autostart: windows, double_tap: cfg!(any(windows, target_os = "macos")),
    })
}

#[cfg(test)]
mod platform_bootstrap {
    use super::*;
    #[test]
    fn identity_precedes_state_and_port_is_part_of_dev_origin() {
        let url = |s| url::Url::parse(s).unwrap();
        let dev = url("http://127.0.0.1:14321");
        assert!(local_webview("main", "main", &dev, &dev));
        assert!(!local_webview("find", "main", &dev, &dev));
        assert!(!local_webview("main", "main", &url("http://127.0.0.1:14328"), &dev));
        assert!(!local_webview("main", "find", &dev, &dev));
        assert!(local_webview("find", "find", &dev, &dev));
        let packaged = url("tauri://localhost");
        assert!(local_webview("main", "main", &url("tauri://localhost/index.html"), &packaged));
        assert!(!local_webview("main", "main", &dev, &packaged));
        assert!(!local_webview("main", "main", &url("tauri://localhost.attacker.test"), &packaged));
    }
}
