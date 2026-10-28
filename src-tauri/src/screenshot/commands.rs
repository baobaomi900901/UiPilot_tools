use super::{
    model::ScreenshotMode,
    service::{OutputKind, Owner},
    window::ScreenshotHost,
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

#[derive(serde::Serialize)]
pub(crate) struct CommandError {
    code: String,
}
impl From<String> for CommandError {
    fn from(code: String) -> Self {
        Self {
            code: match code.as_str() {
                "Expired" => "expired",
                "Busy" => "busy",
                "Unavailable" => "unavailable",
                "Exhausted" | "ResourceLimit" | "AllocationFailed" => "resourceLimit",
                "SaveFailed" => "saveFailed",
                "EncodingFailed" => "encodingFailed",
                other => other,
            }
            .to_owned(),
        }
    }
}
impl From<&str> for CommandError {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
#[derive(serde::Serialize)]
pub(crate) struct OutputResult {
    status: &'static str,
}

fn check_caller(label: &str, actual: isize, registered: isize) -> Result<(), String> {
    if label == "main" && actual != 0 && actual == registered {
        Ok(())
    } else {
        Err("unauthorized".into())
    }
}
fn number(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n != 0 && n.to_string() == value)
        .ok_or_else(|| "expired".into())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnerRequest {
    session_id: String,
    generation: String,
    window_instance: String,
}
impl OwnerRequest {
    fn parse(&self) -> Result<Owner, String> {
        Ok(Owner {
            session_id: number(&self.session_id)?,
            generation: number(&self.generation)?,
            window_instance: number(&self.window_instance)?,
        })
    }
}
fn authorize_main(window: &WebviewWindow, app: &AppHandle) -> Result<(), String> {
    let actual = window.hwnd().map_err(|_| "unauthorized")?.0 as isize;
    let registered = app
        .get_webview_window("main")
        .ok_or("unauthorized")?
        .hwnd()
        .map_err(|_| "unauthorized")?
        .0 as isize;
    check_caller(window.label(), actual, registered)
}
fn authorize_owner(
    window: &WebviewWindow,
    host: &ScreenshotHost,
    request: &OwnerRequest,
) -> Result<Owner, String> {
    let owner = request.parse()?;
    host.authorize(window, owner.window_instance)?;
    let current = host
        .service
        .owner(owner.window_instance)
        .map_err(|_| "expired")?;
    if current != owner {
        return Err("expired".into());
    }
    Ok(owner)
}
#[tauri::command]
pub(crate) fn start_screenshot(
    window: WebviewWindow,
    app: AppHandle,
    host: State<'_, Arc<ScreenshotHost>>,
    mode: ScreenshotMode,
    invocation_id: String,
    query_sequence: u64,
) -> Result<(), CommandError> {
    authorize_main(&window, &app)?;
    if app
        .state::<crate::result_registry::ResultRegistries>()
        .main()
        .prepare_application_query_retirement(&invocation_id, query_sequence)
        .map_err(|_| CommandError::from("unavailable"))?
        .is_none()
    {
        return Err("expired".into());
    }
    host.start_from_launcher(&app, mode, invocation_id, query_sequence)
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) fn get_screenshot_state(
    window: WebviewWindow,
    app: AppHandle,
    host: State<'_, Arc<ScreenshotHost>>,
    window_instance: String,
) -> Result<super::window::WireSnapshot, CommandError> {
    let instance = number(&window_instance)?;
    host.authorize(&window, instance)?;
    host.ready(&app, instance).map_err(Into::into)
}
#[tauri::command]
pub(crate) fn read_screenshot_image(
    window: WebviewWindow,
    host: State<'_, Arc<ScreenshotHost>>,
    owner: OwnerRequest,
    transfer_id: String,
) -> Result<tauri::ipc::Response, CommandError> {
    let owner = authorize_owner(&window, &host, &owner)?;
    host.read_image(owner, number(&transfer_id)?)
        .map(super::window::ImageTransfer::into_response)
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) fn release_screenshot_image(
    window: WebviewWindow,
    host: State<'_, Arc<ScreenshotHost>>,
    owner: OwnerRequest,
    transfer_id: String,
) -> Result<(), CommandError> {
    let owner = owner.parse()?;
    // Unlike reads, ACKs can refer to a completed previous generation. Native
    // caller HWND + current preview instance and the exact stored token bind it.
    host.authorize(&window, owner.window_instance)?;
    host.release_image(owner, number(&transfer_id)?)
        .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn copy_screenshot(
    window: WebviewWindow,
    app: AppHandle,
    host: State<'_, Arc<ScreenshotHost>>,
    owner: OwnerRequest,
) -> Result<OutputResult, CommandError> {
    let owner = authorize_owner(&window, &host, &owner)?;
    let host = host.inner().clone();
    let lease = host
        .service
        .begin_output(owner, OutputKind::Copy)
        .map_err(|e| format!("{e:?}"))?;
    host.emit(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let bytes = crate::clipboard_image::clipboard_workspace_bytes(
                lease.image.png().len(),
                lease.image.width,
                lease.image.height,
            )
            .ok_or("resourceLimit")?;
            let _workspace = host
                .service
                .budget
                .reserve(bytes)
                .map_err(|_| "resourceLimit")?;
            host.service.image(lease.owner).map_err(|_| "expired")?;
            crate::clipboard_image::copy_png(lease.image.png()).map_err(|e| match e {
                crate::clipboard_image::ClipboardImageError::Busy => "clipboardBusy",
                _ => "clipboardWriteFailed",
            })
        })();
        let finish = host
            .service
            .finish_output(&lease, result.as_ref().err().map(|e| e.to_string()));
        host.after_output(&app, finish, owner.window_instance);
        result
            .map(|()| OutputResult { status: "copied" })
            .map_err(str::to_owned)
    })
    .await
    .map_err(|_| "unavailable".to_owned())?
    .map_err(Into::into)
}
#[tauri::command]
pub(crate) async fn save_screenshot(
    window: WebviewWindow,
    app: AppHandle,
    host: State<'_, Arc<ScreenshotHost>>,
    owner: OwnerRequest,
) -> Result<OutputResult, CommandError> {
    let owner = authorize_owner(&window, &host, &owner)?;
    let host = host.inner().clone();
    let lease = host
        .service
        .begin_output(owner, OutputKind::Save)
        .map_err(|e| format!("{e:?}"))?;
    host.emit(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let name = format!(
                "截图_{}.png",
                chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
            );
            let path = app
                .dialog()
                .file()
                .set_title("保存截图")
                .add_filter("PNG", &["png"])
                .set_file_name(name)
                .set_parent(&window)
                .blocking_save_file();
            let Some(path) = path else {
                return Ok(OutputResult {
                    status: "cancelled",
                });
            };
            host.service
                .image(lease.owner)
                .map_err(|_| "expired".to_owned())?;
            let path = path.into_path().map_err(|_| "saveFailed".to_owned())?;
            let _workspace = host
                .service
                .budget
                .reserve(
                    super::output::encoding_workspace_bytes(lease.image.width, lease.image.height)
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            super::output::save_png(&path, lease.image.png())
                .map(|()| OutputResult { status: "saved" })
                .map_err(|e| e.to_string())
        })();
        let finish = host
            .service
            .finish_output(&lease, result.as_ref().err().cloned());
        host.after_output(&app, finish, owner.window_instance);
        result
    })
    .await
    .map_err(|_| "unavailable".to_owned())?
    .map_err(Into::into)
}
#[tauri::command]
pub(crate) fn retake_screenshot(
    window: WebviewWindow,
    app: AppHandle,
    host: State<'_, Arc<ScreenshotHost>>,
    owner: OwnerRequest,
) -> Result<(), CommandError> {
    let owner = authorize_owner(&window, &host, &owner)?;
    let ticket = host.service.retake(owner).map_err(|e| format!("{e:?}"))?;
    host.capture(&app, ticket);
    Ok(())
}
#[tauri::command]
pub(crate) fn close_screenshot(
    window: WebviewWindow,
    app: AppHandle,
    host: State<'_, Arc<ScreenshotHost>>,
    owner: OwnerRequest,
) -> Result<(), CommandError> {
    let owner = authorize_owner(&window, &host, &owner)?;
    if host.service.close(owner).map_err(|e| format!("{e:?}"))? {
        host.destroy(&app, owner.window_instance);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authority_rejects_forged_labels_and_other_native_windows() {
        assert!(check_caller("main", 1, 1).is_ok());
        assert!(check_caller("main", 2, 1).is_err());
        assert!(check_caller("plugin-runtime", 1, 1).is_err());
        assert!(check_caller("find", 1, 1).is_err());
        assert!(check_caller("main", 0, 0).is_err());
    }
    #[test]
    fn owner_numbers_are_canonical_nonzero_decimal_strings() {
        for bad in ["", "0", "01", "+1", "-1", " 1", "18446744073709551616"] {
            assert!(number(bad).is_err(), "{bad}");
        }
        assert_eq!(number("42").unwrap(), 42);
    }
}
