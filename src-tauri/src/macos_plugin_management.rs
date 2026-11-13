//! Main-origin-only management IPC. Never executes installed plugin code.
use crate::manifest::PublicPermission;
pub(crate) use crate::public_plugins::management_macos::ManagementState;
use crate::{
    host_capabilities::require_main,
    public_plugins::management_macos::{
        Error, Management, Preview, Selection, Snapshot, SourceKind,
    },
};
use block2::RcBlock;
use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSWindow};
use objc2_foundation::{NSString, NSURL};
use serde::Deserialize;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;
const FAILED: &str = "pluginManagementUnavailable";
static DIALOG_ACTIVE: AtomicBool = AtomicBool::new(false);
static DIALOG_GENERATION: AtomicU64 = AtomicU64::new(0);
thread_local! {static PANEL:RefCell<Option<(u64,Arc<()>,Retained<NSOpenPanel>)>>=const {RefCell::new(None)};}
pub(crate) fn dialog_active() -> bool {
    DIALOG_ACTIVE.load(Ordering::Acquire)
}

async fn work<T: Send + 'static>(
    window: &WebviewWindow,
    operation: impl FnOnce(&mut Management) -> Result<T, Error> + Send + 'static,
) -> Result<T, &'static str> {
    require_main(window)?;
    let app = window.app_handle();
    let state = app
        .try_state::<Arc<ManagementState>>()
        .ok_or(FAILED)?
        .inner()
        .clone();
    let epoch = state.epoch().map_err(Error::code)?;
    let root = app.path().app_data_dir().map_err(|_| FAILED)?;
    tauri::async_runtime::spawn_blocking(move || {
        state
            .with_epoch(&root, epoch, operation)
            .map_err(Error::code)
    })
    .await
    .map_err(|_| FAILED)?
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PrepareInput {
    kind: SourceKind,
    expected_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommitInput {
    token: String,
    expected_revision: String,
    approved_permissions: Vec<PublicPermission>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TokenInput {
    token: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EnabledInput {
    plugin_id: String,
    enabled: bool,
    expected_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PermissionInput {
    plugin_id: String,
    permission: PublicPermission,
    granted: bool,
    expected_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsInput {
    plugin_id: String,
    updates: BTreeMap<String, Value>,
    expected_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandInput {
    plugin_id: String,
    command_id: String,
    name_override: Option<String>,
    favorite: bool,
    expected_revision: String,
}
#[tauri::command]
pub(crate) async fn save_macos_plugin_command(
    window: WebviewWindow,
    input: CommandInput,
) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.command_preference(
            &input.plugin_id,
            &input.command_id,
            crate::public_plugins::management_macos::CommandPreference {
                name_override: input.name_override,
                favorite: input.favorite,
            },
            &input.expected_revision,
        )
    })
    .await
}
#[tauri::command]
pub(crate) async fn set_plugin_favorite(
    window: WebviewWindow,
    plugin_id: String,
    command_id: String,
    favorite: bool,
) -> Result<(), &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.favorite(&plugin_id, &command_id, favorite)
    })
    .await
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveInput {
    plugin_id: String,
    expected_revision: String,
}

#[tauri::command]
pub(crate) async fn list_macos_plugins(window: WebviewWindow) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, |m| m.list()).await
}
#[tauri::command]
pub(crate) async fn prepare_macos_plugin_install(
    window: WebviewWindow,
    input: PrepareInput,
) -> Result<Option<Preview>, &'static str> {
    require_main(&window)?;
    let selection = work(&window, move |m| {
        m.reserve(input.kind, &input.expected_revision)
    })
    .await?;
    let picked = choose(&window, input.kind, selection.clone()).await;
    match picked {
        Ok(source) => {
            work(&window, move |m| {
                let path = source
                    .as_ref()
                    .map(|s| s.url.to_file_path().ok_or(Error::InvalidPackage))
                    .transpose()?;
                // source retains the URL and any successful sandbox extension until
                // the complete bounded snapshot is captured (including error paths).
                let result = m.selected(selection, path.as_deref());
                drop(source);
                result
            })
            .await
        }
        Err(error) => {
            let _ = work(&window, move |m| m.selected(selection, None)).await;
            Err(error)
        }
    }
}
#[tauri::command]
pub(crate) async fn commit_macos_plugin_install(
    window: WebviewWindow,
    input: CommitInput,
) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.commit(
            &input.token,
            &input.expected_revision,
            input.approved_permissions,
        )
    })
    .await
}
#[tauri::command]
pub(crate) async fn cancel_macos_plugin_install(
    window: WebviewWindow,
    input: TokenInput,
) -> Result<(), &'static str> {
    require_main(&window)?;
    work(&window, move |m| m.cancel(&input.token)).await
}
#[tauri::command]
pub(crate) async fn set_macos_plugin_enabled(
    window: WebviewWindow,
    input: EnabledInput,
) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.enabled(&input.plugin_id, input.enabled, &input.expected_revision)
    })
    .await
}
#[tauri::command]
pub(crate) async fn set_macos_plugin_permission(
    window: WebviewWindow,
    input: PermissionInput,
) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.permission(
            &input.plugin_id,
            input.permission,
            input.granted,
            &input.expected_revision,
        )
    })
    .await
}
#[tauri::command]
pub(crate) async fn save_macos_plugin_settings(
    window: WebviewWindow,
    input: SettingsInput,
) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.settings(&input.plugin_id, &input.updates, &input.expected_revision)
    })
    .await
}
#[tauri::command]
pub(crate) async fn remove_macos_plugin(
    window: WebviewWindow,
    input: RemoveInput,
) -> Result<Snapshot, &'static str> {
    require_main(&window)?;
    work(&window, move |m| {
        m.remove(&input.plugin_id, &input.expected_revision)
    })
    .await
}

struct PickedSource {
    url: Retained<NSURL>,
    accessed: bool,
}
impl PickedSource {
    fn new(url: Retained<NSURL>) -> Self {
        // Public Foundation lifetime pairing. A non-scoped URL may return false;
        // ordinary reads still run and report their real success/failure.
        let accessed = unsafe { url.startAccessingSecurityScopedResource() };
        Self { url, accessed }
    }
}
impl Drop for PickedSource {
    fn drop(&mut self) {
        if self.accessed {
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }
}

async fn choose(
    window: &WebviewWindow,
    kind: SourceKind,
    selection: Selection,
) -> Result<Option<PickedSource>, &'static str> {
    require_main(window)?;
    let (send, receive) = oneshot::channel();
    let window_copy = window.clone();
    let generation = DIALOG_GENERATION.load(Ordering::Acquire);
    let state = window
        .app_handle()
        .try_state::<Arc<ManagementState>>()
        .ok_or(FAILED)?
        .inner()
        .clone();
    window
        .run_on_main_thread(move || {
            // Recheck the real origin/window immediately before any AppKit effect.
            if require_main(&window_copy).is_err()
                || DIALOG_GENERATION.load(Ordering::Acquire) != generation
                || !state.selection_current(&selection)
            {
                let _ = send.send(Err(FAILED));
                return;
            }
            let Some(mtm) = MainThreadMarker::new() else {
                let _ = send.send(Err(FAILED));
                return;
            };
            if DIALOG_ACTIVE.swap(true, Ordering::AcqRel) {
                let _ = send.send(Err("pluginManagementBusy"));
                return;
            }
            let parent = window_copy
                .ns_window()
                .ok()
                .and_then(|p| unsafe { p.cast::<NSWindow>().as_ref() });
            let Some(parent) = parent else {
                DIALOG_ACTIVE.store(false, Ordering::Release);
                let _ = send.send(Err(FAILED));
                return;
            };
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseDirectories(matches!(kind, SourceKind::Directory));
            panel.setCanChooseFiles(matches!(kind, SourceKind::Archive));
            panel.setAllowsMultipleSelection(false);
            panel.setCanCreateDirectories(false);
            panel.setResolvesAliases(false);
            panel.setTitle(Some(&NSString::from_str("选择插件")));
            panel.setPrompt(Some(&NSString::from_str("检查插件")));
            panel.setMessage(Some(&NSString::from_str(match kind {
                SourceKind::Archive => {
                    "选择 .uipilot-plugin 文件。检查后确认安装；当前仅保存配置，不运行插件。"
                }
                SourceKind::Directory => {
                    "选择包含 plugin.json 的插件目录。检查后确认安装；当前仅保存配置，不运行插件。"
                }
            })));
            let owner = Arc::new(());
            PANEL.with_borrow_mut(|p| *p = Some((generation, owner.clone(), panel.clone())));
            let send = RefCell::new(Some(send));
            let completion_app = window_copy.app_handle().clone();
            let done = RcBlock::new(move |response| {
                let current = PANEL.with_borrow_mut(|p| take_owner(p, &owner));
                let result = if let Some((_, _, panel)) = current {
                    DIALOG_ACTIVE.store(false, Ordering::Release);
                    crate::macos_host::after_management_dialog(&completion_app);
                    if response == NSModalResponseCancel {
                        Ok(None)
                    } else if response == NSModalResponseOK {
                        panel.URL().map(PickedSource::new).map(Some).ok_or(FAILED)
                    } else {
                        Err(FAILED)
                    }
                } else {
                    Err("pluginManagementStale")
                };
                if let Some(send) = send.borrow_mut().take() {
                    let _ = send.send(result);
                }
            });
            panel.beginSheetModalForWindow_completionHandler(parent, &done);
        })
        .map_err(|_| FAILED)?;
    receive.await.map_err(|_| FAILED)?
}
/// Trusted host close/quit hook: clear pending consent/selection, without loading
/// data. Cancellation belongs to the original panel; late callbacks cannot take
/// a subsequent panel. No installation or filesystem cleanup is performed here.
pub(crate) fn reset(app: &AppHandle) {
    let generation = DIALOG_GENERATION.fetch_add(1, Ordering::AcqRel);
    if let Some(state) = app.try_state::<Arc<ManagementState>>() {
        state.reset();
    }
    let _ = app.run_on_main_thread(move || {
        let panel = PANEL.with_borrow_mut(|p| take_generation(p, generation));
        if let Some((_, _, panel)) = panel {
            DIALOG_ACTIVE.store(false, Ordering::Release);
            unsafe { panel.cancel(None) };
        }
    });
}

fn take_owner<T>(
    slot: &mut Option<(u64, Arc<()>, T)>,
    owner: &Arc<()>,
) -> Option<(u64, Arc<()>, T)> {
    if slot
        .as_ref()
        .is_some_and(|(_, id, _)| Arc::ptr_eq(id, owner))
    {
        slot.take()
    } else {
        None
    }
}
fn take_generation<T>(
    slot: &mut Option<(u64, Arc<()>, T)>,
    generation: u64,
) -> Option<(u64, Arc<()>, T)> {
    if slot.as_ref().is_some_and(|(g, _, _)| *g == generation) {
        slot.take()
    } else {
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipc_typed_inputs_never_accept_javascript_source_paths_or_extra_fields() {
        assert!(serde_json::from_value::<PrepareInput>(
            serde_json::json!({"kind":"directory","expectedRevision":"0"})
        )
        .is_ok());
        for input in [
            serde_json::json!({"kind":"directory","expectedRevision":"0","path":"/private/user"}),
            serde_json::json!({"kind":"other","expectedRevision":"0"}),
            serde_json::json!({"kind":"archive","expectedRevision":0}),
        ] {
            assert!(serde_json::from_value::<PrepareInput>(input).is_err());
        }
        assert!(serde_json::from_value::<RemoveInput>(
            serde_json::json!({"pluginId":"a.b","expectedRevision":"0","deleteData":true})
        )
        .is_err());
    }
    #[test]
    fn old_completion_and_delayed_reset_never_take_a_new_native_panel() {
        let old = Arc::new(());
        let new = Arc::new(());
        let mut slot = Some((1, new.clone(), "new panel"));
        assert!(take_owner(&mut slot, &old).is_none());
        assert!(take_generation(&mut slot, 0).is_none());
        assert_eq!(slot.as_ref().unwrap().2, "new panel");
        assert_eq!(take_owner(&mut slot, &new).unwrap().2, "new panel");
        assert!(take_owner(&mut slot, &new).is_none());
    }
}
