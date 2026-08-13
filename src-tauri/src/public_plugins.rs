mod activation;
mod manifest;
mod package;
mod runtime;
mod scheduler;
mod secrets;
mod state;
mod storage;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::Duration,
};

use tauri::{
    http::Response,
    webview::{NewWindowResponse, WebviewWindow},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

pub(crate) use activation::{
    PublicPluginInstallSource, PublicPluginManagementError, PublicPluginManager,
    PublicPluginMutation, PublicPluginPrepareSummary, PublicRuntimeCandidate,
};
#[cfg(test)]
pub(crate) use manifest::PublicActivationMode;
pub(crate) use manifest::{PublicManifestV1, PublicPermission, PublicPlatform};
pub(crate) use runtime::{
    parse_runtime_label, runtime_label, PluginApiRequest, PluginCommandCompletion,
    PluginRuntimeApi, PluginRuntimeError, PUBLIC_RUNTIME_BOOTSTRAP,
};
pub(crate) use scheduler::{
    PluginCompletionOutcome, PluginContextStatus, PluginRequestContext, PluginRequestScheduler,
};
pub(crate) use secrets::PluginSecretStore;
pub(crate) use state::{
    EffectivePluginConfig, PluginStateError, PluginStateStore, PublicPluginFault,
};
pub(crate) use storage::PluginStorageStore;
const PUBLIC_RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(crate) struct PublicPluginService {
    manager: OnceLock<Arc<PublicPluginManager>>,
}

impl PublicPluginService {
    pub(crate) fn initialize(
        &self,
        app_data_dir: &Path,
        reserved_names: impl IntoIterator<Item = String>,
    ) -> Result<Arc<PublicPluginManager>, PublicPluginManagementError> {
        let manager = Arc::new(PublicPluginManager::load(
            app_data_dir,
            PublicPluginHost::current(PublicPlatform::Windows),
            reserved_names,
        )?);
        self.manager
            .set(Arc::clone(&manager))
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        Ok(manager)
    }

    pub(crate) fn manager(&self) -> Result<&Arc<PublicPluginManager>, PublicPluginManagementError> {
        self.manager
            .get()
            .ok_or(PublicPluginManagementError::Unavailable)
    }

    pub(crate) fn asset_response(&self, label: &str, path: &str) -> Response<Vec<u8>> {
        let Some(asset) = self
            .manager()
            .ok()
            .and_then(|manager| manager.asset(label, path))
        else {
            return Response::builder().status(403).body(Vec::new()).unwrap();
        };
        Response::builder()
            .status(200)
            .header("content-type", asset.mime)
            .header("x-content-type-options", "nosniff")
            .body(asset.bytes)
            .unwrap()
    }

    pub(crate) fn create_runtime(
        &self,
        app: &AppHandle,
        candidate: &PublicRuntimeCandidate,
    ) -> Result<WebviewWindow, PublicPluginManagementError> {
        let url = tauri::Url::parse("uipilot-public-plugin://localhost/__uipilot_runtime.html")
            .map_err(|_| PublicPluginManagementError::Unavailable)?;
        let ready = Arc::new((Mutex::new(None), Condvar::new()));
        let title_ready = Arc::clone(&ready);
        let window = WebviewWindowBuilder::new(
            app,
            candidate.label.clone(),
            WebviewUrl::CustomProtocol(url),
        )
        .visible(false)
        .focusable(false)
        .skip_taskbar(true)
        .incognito(true)
        .initialization_script(PUBLIC_RUNTIME_BOOTSTRAP)
        .on_navigation(public_runtime_navigation_allowed)
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_document_title_changed(move |_, title| {
            let settled = match title.as_str() {
                "uipilot-public-plugin-ready" => Some(true),
                "uipilot-public-plugin-failed" => Some(false),
                _ => None,
            };
            if let Some(settled) = settled {
                if let Ok(mut state) = title_ready.0.lock() {
                    *state = Some(settled);
                    title_ready.1.notify_all();
                }
            }
        })
        .build()
        .map_err(|_| PublicPluginManagementError::RuntimeNotReady)?;
        let settled = ready
            .1
            .wait_timeout_while(
                ready
                    .0
                    .lock()
                    .map_err(|_| PublicPluginManagementError::Unavailable)?,
                PUBLIC_RUNTIME_READY_TIMEOUT,
                |state| state.is_none(),
            )
            .map_err(|_| PublicPluginManagementError::Unavailable)?
            .0;
        if *settled == Some(true) {
            Ok(window)
        } else {
            let _ = window.destroy();
            Err(PublicPluginManagementError::RuntimeNotReady)
        }
    }

    pub(crate) fn destroy_runtime(app: &AppHandle, label: Option<&str>) {
        if let Some(window) = label.and_then(|label| app.get_webview_window(label)) {
            let _ = window.destroy();
        }
    }
}

fn public_runtime_navigation_allowed(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "uipilot-public-plugin" | "http")
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host.eq_ignore_ascii_case("uipilot-public-plugin.localhost")
        })
        && url.port().is_none()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginDataScope {
    plugin_id: String,
}

impl PluginDataScope {
    pub(crate) fn new(plugin_id: &str) -> Result<Self, PublicPackageError> {
        manifest::valid_plugin_id(plugin_id)
            .then(|| Self {
                plugin_id: plugin_id.into(),
            })
            .ok_or(PublicPackageError::InvalidPackage)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidPluginScope;

fn authorize_plugin_scope(
    scope: &PluginDataScope,
    plugin_id: &str,
) -> Result<(), InvalidPluginScope> {
    (scope.plugin_id == plugin_id)
        .then_some(())
        .ok_or(InvalidPluginScope)
}

fn valid_json_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => true,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(f64::is_finite),
        serde_json::Value::Array(values) => values.iter().all(valid_json_value),
        serde_json::Value::Object(values) => values.iter().all(|(key, value)| {
            !matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                && valid_json_value(value)
        }),
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PublicPackageSource {
    Archive(PathBuf),
    DevelopmentDirectory(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PublicPluginHost {
    pub(crate) platform: PublicPlatform,
    pub(crate) version: [u32; 3],
    pub(crate) api_version: u32,
}

impl PublicPluginHost {
    pub(crate) const fn current(platform: PublicPlatform) -> Self {
        Self {
            platform,
            version: [0, 2, 0],
            api_version: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublicPackageError {
    InvalidPackage,
    IncompatiblePlatform,
    IncompatibleApi,
    UnsupportedPermission,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicResource {
    pub(crate) mime: &'static str,
    pub(crate) length: u64,
    pub(crate) sha256: String,
}

#[derive(Debug)]
pub(crate) struct PreparedPublicPlugin {
    transaction_root: Option<PathBuf>,
    pub(crate) package_root: PathBuf,
    pub(crate) manifest: PublicManifestV1,
    pub(crate) digest: String,
    pub(crate) resources: BTreeMap<String, PublicResource>,
}

impl PreparedPublicPlugin {
    fn new(
        transaction_root: PathBuf,
        package_root: PathBuf,
        manifest: PublicManifestV1,
        digest: String,
        resources: BTreeMap<String, PublicResource>,
    ) -> Self {
        Self {
            transaction_root: Some(transaction_root),
            package_root,
            manifest,
            digest,
            resources,
        }
    }

    pub(crate) fn transaction_root(&self) -> &Path {
        self.transaction_root
            .as_deref()
            .expect("prepared transaction root missing")
    }

    pub(crate) fn revalidate(&self) -> Result<(), PublicPackageError> {
        package::revalidate_snapshot(&self.package_root, &self.digest, &self.resources)
    }

    pub(crate) fn persist(mut self, destination: &Path) -> Result<bool, PublicPackageError> {
        self.revalidate()?;
        if destination.exists() {
            package::revalidate_snapshot(destination, &self.digest, &self.resources)?;
            return Ok(false);
        }
        let parent = destination
            .parent()
            .ok_or(PublicPackageError::InvalidPackage)?;
        std::fs::create_dir_all(parent).map_err(|_| PublicPackageError::InvalidPackage)?;
        let transaction_root = self
            .transaction_root
            .take()
            .expect("prepared transaction root missing");
        if std::fs::rename(&self.package_root, destination).is_err() {
            package::remove_transaction(transaction_root);
            return Err(PublicPackageError::InvalidPackage);
        }
        package::remove_transaction(transaction_root);
        Ok(true)
    }
}

impl Drop for PreparedPublicPlugin {
    fn drop(&mut self) {
        if let Some(path) = self.transaction_root.take() {
            package::remove_transaction(path);
        }
    }
}

pub(crate) fn stage_public_package(
    source: PublicPackageSource,
    staging_root: &Path,
    host: &PublicPluginHost,
) -> Result<PreparedPublicPlugin, PublicPackageError> {
    package::stage(source, staging_root, host)
}
