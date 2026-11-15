//! Only the two currently available managed Mac builtins are writable here.
use crate::{host_capabilities::require_main, settings::{ManagedBuiltinFeature, SettingsStore}};
use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewWindow};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Feature { WebSearch, Quicklinks }
impl Feature {
    fn managed(self) -> ManagedBuiltinFeature {
        match self { Self::WebSearch => ManagedBuiltinFeature::WebSearch, Self::Quicklinks => ManagedBuiltinFeature::Quicklinks }
    }
}
#[derive(Serialize)]
pub(crate) struct Row { feature: Feature, name: &'static str, command: &'static str, enabled: bool }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Update { feature: Feature, enabled: bool }
fn inventory(settings: &SettingsStore) -> Vec<Row> {
    let settings = settings.snapshot();
    [(Feature::WebSearch, "网页搜索", "/web-search"), (Feature::Quicklinks, "快速链接", "/quicklinks")]
        .into_iter().map(|(feature, name, command)| Row { feature, name, command, enabled: settings.builtin_feature_enabled(feature.managed()) }).collect()
}
fn update(settings: &SettingsStore, input: Update) -> Result<Vec<Row>, &'static str> {
    settings.set_builtin_feature_enabled(input.feature.managed(), input.enabled).map_err(|_| "builtinManagementUnavailable")?;
    Ok(inventory(settings))
}
#[tauri::command]
pub(crate) fn list_macos_builtin_features(window: WebviewWindow) -> Result<Vec<Row>, &'static str> {
    require_main(&window)?;
    Ok(inventory(window.state::<SettingsStore>().inner()))
}
#[tauri::command]
pub(crate) async fn set_macos_builtin_enabled(window: WebviewWindow, input: Update) -> Result<Vec<Row>, &'static str> {
    require_main(&window)?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || update(app.state::<SettingsStore>().inner(), input))
        .await.map_err(|_| "builtinManagementUnavailable")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{BuiltinFeature, WebSearchEngine};
    #[test]
    fn rejects_unavailable_features_and_extra_fields() {
        for value in [
            serde_json::json!({"feature":"screenshot","enabled":true}),
            serde_json::json!({"feature":"find","enabled":false}),
            serde_json::json!({"feature":"webSearch","enabled":"false"}),
            serde_json::json!({"feature":"quicklinks","enabled":true,"path":"/tmp/x"}),
        ] { assert!(serde_json::from_value::<Update>(value).is_err()); }
    }
    #[test]
    fn toggles_survive_reload_and_preserve_engine_favorites_and_other_features() {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!("uipilot-builtin-management-{}", std::process::id()));
        let settings = SettingsStore::load(&root).unwrap();
        settings.set_web_search_engine(WebSearchEngine::Baidu).unwrap();
        settings.set_builtin_feature_favorite(BuiltinFeature::Quicklinks, true).unwrap();
        update(&settings, Update { feature: Feature::Quicklinks, enabled: false }).unwrap();
        let loaded = SettingsStore::load(&root).unwrap();
        let rows = inventory(&loaded);
        assert!(rows[0].enabled); assert!(!rows[1].enabled);
        assert_eq!(loaded.snapshot().web_search_engine, WebSearchEngine::Baidu);
        assert!(loaded.snapshot().favorite_builtin_features.contains(&BuiltinFeature::Quicklinks));
        update(&loaded, Update { feature: Feature::Quicklinks, enabled: true }).unwrap();
        assert!(SettingsStore::load(&root).unwrap().snapshot().builtin_feature_enabled(ManagedBuiltinFeature::Quicklinks));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_settings_commit_keeps_the_previous_enabled_state() {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!("uipilot-builtin-failure-{}", std::process::id()));
        let settings = SettingsStore::load(&root).unwrap();
        update(&settings, Update { feature: Feature::WebSearch, enabled: false }).unwrap();
        std::fs::create_dir(root.join("settings.json.backup")).unwrap();
        assert!(update(&settings, Update { feature: Feature::WebSearch, enabled: true }).is_err());
        assert!(!inventory(&settings)[0].enabled);
        assert!(!inventory(&SettingsStore::load(&root).unwrap())[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }
}
