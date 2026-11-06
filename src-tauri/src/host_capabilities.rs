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
}

fn local_main(label: &str, page: &url::Url, origin: &url::Url) -> bool {
    label == "main" && page.scheme() == origin.scheme() && page.host_str() == origin.host_str()
        && page.port_or_known_default() == origin.port_or_known_default()
        && page.username().is_empty() && page.password().is_none()
}

pub(crate) fn trusted_page(label: &str, page: &url::Url, config: &tauri::Config) -> bool {
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
    local_main(label, page, &origin)
}

pub(crate) fn require_main(window: &WebviewWindow) -> Result<(), &'static str> {
    use tauri::Manager;
    let page = window.url().map_err(|_| "invalidCaller")?;
    if trusted_page(window.label(), &page, window.app_handle().config()) {
        Ok(())
    } else {
        Err("invalidCaller")
    }
}

#[tauri::command]
pub(crate) fn get_host_capabilities(window: WebviewWindow) -> Result<HostCapabilities, &'static str> {
    require_main(&window)?;
    let windows = cfg!(windows);
    Ok(HostCapabilities {
        version: 1, platform: if windows { "windows" } else { "macos" },
        plugins: windows, files: windows, screenshot: windows, messages: windows, credentials: windows,
        builtin_features: windows, command_hotkeys: windows, autostart: windows, double_tap: cfg!(any(windows, target_os = "macos")),
    })
}

#[cfg(test)]
mod platform_bootstrap {
    use super::*;
    #[test]
    fn identity_precedes_state_and_port_is_part_of_dev_origin() {
        let url = |s| url::Url::parse(s).unwrap();
        let dev = url("http://127.0.0.1:14321");
        assert!(local_main("main", &dev, &dev));
        assert!(!local_main("find", &dev, &dev));
        assert!(!local_main("main", &url("http://127.0.0.1:14328"), &dev));
        let packaged = url("tauri://localhost");
        assert!(local_main("main", &url("tauri://localhost/index.html"), &packaged));
        assert!(!local_main("main", &dev, &packaged));
        assert!(!local_main("main", &url("tauri://localhost.attacker.test"), &packaged));
    }
}
