use std::{thread, time::Duration};

use tauri::{App, WebviewUrl, WebviewWindowBuilder};

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
    let app_handle = app.handle().clone();

    thread::spawn(move || {
        for _ in 0..200 {
            let result = probe
                .url()
                .ok()
                .and_then(|url| url.fragment().map(str::to_owned));

            match result.as_deref() {
                Some("rejected") => {
                    app_handle.exit(0);
                    return;
                }
                Some("allowed") => {
                    app_handle.exit(2);
                    return;
                }
                _ => thread::sleep(Duration::from_millis(50)),
            }
        }

        app_handle.exit(3);
    });

    Ok(())
}
