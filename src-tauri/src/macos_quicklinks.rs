//! Trusted Mac Quicklinks integration. No browser calls during query or mutation.
use crate::{
    host_capabilities::require_main,
    model::{LauncherResultActivation, ResultFavorite, ResultIconKind, ResultItem},
    public_plugins::management_macos::{Error, Management, ManagementState},
    quicklinks::{
        QuicklinkErrorCode as QError, QuicklinkIconCandidate, QuicklinkListResponse,
        QuicklinkSaveInput, QuicklinkView, QuicklinksStore,
    },
    result_registry::ResultAction,
    settings::{BuiltinFeature, ManagedBuiltinFeature, SettingsStore},
};
use std::sync::Arc;
use tauri::{Manager, WebviewWindow};
type QResult<T> = Result<T, QError>;
fn failure(code: impl serde::Serialize) -> serde_json::Value {
    serde_json::json!({"code": code})
}

pub(crate) fn query_url(template: &str, query: &str) -> QResult<String> {
    if template.len() > 4096 || query.is_empty() || query.len() > 4096 || query.contains('\0') {
        return Err(QError::InvalidTemplate);
    }
    let url = crate::quicklinks::expand_template(template, query)?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > crate::macos_browser_open::MAX_URL_BYTES
    {
        return Err(QError::InvalidTemplate);
    }
    Ok(url.into())
}
pub(crate) fn validate_input(input: &QuicklinkSaveInput) -> QResult<()> {
    if input.name.len() > 256
        || input.id.as_ref().is_some_and(|id| {
            id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return Err(QError::SaveFailed);
    }
    query_url(&input.template, "x")?;
    Ok(())
}
pub(crate) fn save_validated(
    store: &QuicklinksStore,
    input: QuicklinkSaveInput,
    conflicts: &[String],
) -> QResult<QuicklinkView> {
    validate_input(&input)?;
    let current = store.list();
    if current.load_error.is_some()
        || input
            .id
            .as_ref()
            .is_some_and(|id| !current.items.iter().any(|q| &q.id == id))
    {
        return Err(QError::SaveFailed);
    }
    store.save_with_external_conflicts(input, conflicts)
}

pub(crate) fn entries(
    settings: &crate::settings::Settings,
    query: &str,
    links: &[QuicklinkView],
) -> Vec<(ResultItem, Option<ResultAction>)> {
    if !settings.builtin_feature_enabled(ManagedBuiltinFeature::Quicklinks) {
        return Vec::new();
    }
    let query = query.trim();
    let mut rows = Vec::new();
    if query.is_empty() || (query.starts_with('/') && "/quicklinks".starts_with(query)) {
        rows.push((
            ResultItem {
                result_id: String::new(),
                title: "/quicklinks".into(),
                subtitle: Some("管理快速链接".into()),
                activation: LauncherResultActivation::OpenQuicklinks,
                result_actions: None,
                icon: None,
                plugin_icon_url: None,
                icon_kind: Some(ResultIconKind::Quicklinks),
                detail: None,
                has_default_action: false,
                favorite: Some(ResultFavorite::builtin(
                    BuiltinFeature::Quicklinks,
                    settings
                        .favorite_builtin_features
                        .contains(&BuiltinFeature::Quicklinks),
                )),
            },
            None,
        ));
    }
    let Some(command) = query.strip_prefix('/') else {
        return rows;
    };
    let (prefix, argument) = command
        .split_once(char::is_whitespace)
        .map(|(a, b)| (a, Some(b.trim())))
        .unwrap_or((command, None));
    for link in links
        .iter()
        .filter(|q| {
            if argument.is_some() {
                q.command == prefix
            } else {
                q.command.starts_with(prefix)
            }
        })
        .take(100)
    {
        let argument = argument.filter(|s| !s.is_empty());
        let url = match argument {
            Some(arg) => match query_url(&link.template, arg) {
                Ok(url) => Some(url),
                Err(_) => continue,
            },
            None => None,
        };
        let title = format!("/{}", link.command);
        let action = argument.map(|arg| ResultAction::MacOpenQuicklink {
            id: link.id.clone(),
            command: link.command.clone(),
            template: link.template.clone(),
            query: arg.into(),
        });
        rows.push((
            ResultItem {
                result_id: String::new(),
                title: title.clone(),
                subtitle: Some(link.name.clone()),
                activation: if argument.is_some() {
                    LauncherResultActivation::ExecuteResult
                } else {
                    LauncherResultActivation::Completion {
                        completion_text: format!("{title} "),
                    }
                },
                result_actions: None,
                icon: link.icon_data_url.clone(),
                plugin_icon_url: None,
                icon_kind: None,
                detail: url,
                has_default_action: argument.is_some(),
                favorite: None,
            },
            action,
        ));
    }
    rows
}

async fn work<T: Send + 'static>(
    window: WebviewWindow,
    operation: impl FnOnce(&mut Management) -> Result<QResult<T>, Error> + Send + 'static,
) -> Result<T, serde_json::Value> {
    require_main(&window).map_err(failure)?;
    let app = window.app_handle().clone();
    let state = app.state::<Arc<ManagementState>>().inner().clone();
    let epoch = state.epoch().map_err(|e| failure(e.code()))?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| failure("quicklinkLoadFailed"))?;
    tauri::async_runtime::spawn_blocking(move || {
        state
            .with_epoch(&root, epoch, |manager| {
                app.state::<SettingsStore>()
                    .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || operation(manager))
                    .map_err(|_| Error::Unsupported)?
            })
            .map_err(|e| failure(e.code()))?
            .map_err(failure)
    })
    .await
    .map_err(|_| failure("quicklinkLoadFailed"))?
}
#[tauri::command]
pub(crate) async fn list_quicklinks(
    window: WebviewWindow,
) -> Result<QuicklinkListResponse, serde_json::Value> {
    require_main(&window).map_err(failure)?;
    let app = window.app_handle().clone();
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| failure("quicklinkLoadFailed"))?;
    let store = app
        .state::<Arc<ManagementState>>()
        .quicklinks(&root)
        .map_err(|e| failure(e.code()))?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<SettingsStore>()
            .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || store.list())
            .map_err(|_| failure("quicklinkLoadFailed"))
    })
    .await
    .map_err(|_| failure("quicklinkLoadFailed"))?
}
#[tauri::command]
pub(crate) async fn save_quicklink(
    window: WebviewWindow,
    input: QuicklinkSaveInput,
) -> Result<QuicklinkView, serde_json::Value> {
    work(window, move |manager| manager.save_quicklink(input)).await
}
#[tauri::command]
pub(crate) async fn delete_quicklink(
    window: WebviewWindow,
    id: String,
) -> Result<(), serde_json::Value> {
    work(window, move |manager| Ok(manager.quicklinks.delete(&id))).await
}
#[tauri::command]
pub(crate) async fn choose_quicklink_icon(
    window: WebviewWindow,
) -> Result<Option<QuicklinkIconCandidate>, serde_json::Value> {
    require_main(&window).map_err(failure)?;
    window
        .app_handle()
        .state::<SettingsStore>()
        .with_enabled_builtin(ManagedBuiltinFeature::Quicklinks, || ())
        .map_err(|_| failure("quicklinkLoadFailed"))?;
    let state = window
        .app_handle()
        .state::<Arc<ManagementState>>()
        .inner()
        .clone();
    let epoch = state.epoch().map_err(|e| failure(e.code()))?;
    let picked = crate::macos_plugin_management::choose_quicklink_icon_file(&window, epoch.clone())
        .await
        .map_err(failure)?;
    if !state.runtime_epoch_current(&epoch) {
        return Err(failure("staleRequest"));
    }
    let Some(picked) = picked else {
        return Ok(None);
    };
    work(window, move |manager| {
        if !state.runtime_epoch_current(&epoch) {
            return Err(Error::Stale);
        }
        let path = picked.path().map_err(|_| Error::Storage)?;
        let result = manager
            .quicklinks
            .create_icon_candidate_from_path(&path)
            .map(Some);
        drop(picked);
        Ok(result)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_limits_and_browser_url_boundary() {
        let input = crate::quicklinks::QuicklinkSaveInput {
            id: None,
            name: "搜索".into(),
            command: "fixture".into(),
            template: "https://example.test/?q={Query}".into(),
            icon_token: None,
        };
        assert!(validate_input(&input).is_ok());
        assert!(serde_json::from_value::<QuicklinkSaveInput>(serde_json::json!({"name":"link", "command":"link", "template":"https://example.test/?q={Query}", "url":"https://injected.test"})).is_err());
        let mut bad = input.clone();
        bad.id = Some("../unknown".into());
        assert!(validate_input(&bad).is_err());
        bad = input.clone();
        bad.name = "x".repeat(257);
        assert!(validate_input(&bad).is_err());
        for template in [
            "file:///tmp/{Query}",
            "https://user:pass@example.test/?q={Query}",
            "https://example.test/",
        ] {
            let mut bad = input.clone();
            bad.template = template.into();
            assert!(validate_input(&bad).is_err());
        }
        let url = query_url(&input.template, "中文 & # ?").unwrap();
        assert_eq!(
            url::Url::parse(&url)
                .unwrap()
                .query_pairs()
                .collect::<Vec<_>>(),
            vec![("q".into(), "中文 & # ?".into())]
        );
        assert!(query_url(&input.template, "\0").is_err());
        assert!(query_url(&input.template, &"a".repeat(4097)).is_err());
    }
}
