use std::{thread, time::Duration};

use tauri::{App, WebviewUrl, WebviewWindowBuilder};

const ACL_DENIED_EXIT_CODE: i32 = 73;

#[tauri::command]
pub(crate) fn load_settings() -> &'static str {
    "unexpectedly allowed"
}

pub(crate) fn setup(app: &mut App) -> tauri::Result<()> {
    let probe = WebviewWindowBuilder::new(
        app,
        "security-probe",
        WebviewUrl::App("security-probe.html".into()),
    )
    .title("UiPilot security probe")
    .visible(false)
    .build()?;

    thread::spawn(move || {
        for _ in 0..200 {
            let result = probe
                .url()
                .ok()
                .and_then(|url| url.fragment().map(str::to_owned));

            if result.as_deref() == Some("acl-denied") {
                std::process::exit(ACL_DENIED_EXIT_CODE);
            }

            thread::sleep(Duration::from_millis(50));
        }

        std::process::exit(3);
    });

    Ok(())
}
