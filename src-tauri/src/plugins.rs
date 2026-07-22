use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock, RwLock,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{
    http::Response,
    webview::{NewWindowResponse, WebviewWindow},
    App, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::{
    model::ResultItem,
    result_registry::{ResultAction, ResultRegistry},
};

pub(crate) const PLUGIN_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'";
const PLUGIN_BRIDGE: &str = r#"
(() => {
  let handler = null;
  let pending = [];
  let activeRequest = null;
  let listening = false;
  const internals = () => window.__TAURI_INTERNALS__;
  const waitForInternals = () => new Promise((resolve) => {
    const tick = () => internals() ? resolve(internals()) : setTimeout(tick, 0);
    tick();
  });
  const deliver = (request) => handler ? run(request) : pending.push(request);
  const run = (request) => {
    activeRequest = request;
    try { handler(request.input); } finally { activeRequest = null; }
  };
  const ready = () => {
    if (handler && listening) document.title = 'uipilot-plugin-ready';
  };
  const api = Object.freeze({
    onQuery(next) {
      if (typeof next !== 'function') throw new TypeError('handler required');
      handler = next;
      for (const request of pending.splice(0)) run(request);
      ready();
    },
    publishResults(response) {
      if (!activeRequest) return Promise.reject(new Error('no active request'));
      return internals().invoke('publish_plugin_results', {
        response: {
          protocolVersion: 1,
          requestId: activeRequest.requestId,
          items: response.items,
        },
      });
    }
  });
  Object.defineProperty(window, 'uipilot', { value: api, configurable: false, writable: false });
  Object.freeze(window.uipilot);
  waitForInternals().then((tauri) => tauri.invoke('plugin:event|listen', {
    event: 'uipilot-plugin-query',
    target: { kind: 'Any' },
    handler: tauri.transformCallback((event) => deliver(event.payload)),
  })).then(() => {
    listening = true;
    ready();
  });
})();
"#;

#[derive(Debug)]
pub(crate) enum PluginSetupError {
    Io(io::Error),
    AlreadyLoaded,
}

impl fmt::Display for PluginSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "plugin setup I/O failed: {error}"),
            Self::AlreadyLoaded => formatter.write_str("plugin catalog is already loaded"),
        }
    }
}

impl std::error::Error for PluginSetupError {}

impl From<io::Error> for PluginSetupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) struct PluginManager {
    catalog: OnceLock<PluginCatalog>,
    ready: Arc<RuntimeReadiness>,
    disabled: Arc<RwLock<HashSet<String>>>,
    pending: RwLock<HashMap<String, PendingPluginQuery>>,
    timeouts: RwLock<HashMap<String, u8>>,
    next_request: AtomicU64,
}

impl PluginManager {
    pub(crate) fn new() -> Self {
        Self {
            catalog: OnceLock::new(),
            ready: Arc::new(RuntimeReadiness::default()),
            disabled: Arc::new(RwLock::new(HashSet::new())),
            pending: RwLock::new(HashMap::new()),
            timeouts: RwLock::new(HashMap::new()),
            next_request: AtomicU64::new(0),
        }
    }

    pub(crate) fn load(
        &self,
        app_data_dir: &Path,
        host_version: Version,
    ) -> Result<(), PluginSetupError> {
        let catalog = PluginCatalog::load(&app_data_dir.join("plugins"), host_version)?;
        self.catalog
            .set(catalog)
            .map_err(|_| PluginSetupError::AlreadyLoaded)
    }

    pub(crate) fn route(&self, query: &str) -> Option<PluginRoute> {
        self.catalog.get()?.route(query)
    }

    pub(crate) fn authorizes_clipboard(&self, plugin_id: &str) -> bool {
        let Some(catalog) = self.catalog.get() else {
            return false;
        };
        let Some(entry) = catalog.entries.iter().find(|entry| entry.id == plugin_id) else {
            return false;
        };
        self.disabled
            .read()
            .is_ok_and(|disabled| !disabled.contains(&entry.window_label))
            && catalog.authorizes_clipboard(plugin_id)
    }

    pub(crate) fn asset_response(&self, label: &str, request_path: &str) -> Response<Vec<u8>> {
        self.catalog.get().map_or_else(
            || response(403, Vec::new(), None),
            |catalog| catalog.asset_response(label, request_path),
        )
    }

    pub(crate) fn create_runtimes(
        &self,
        app: &App,
        app_data_dir: &Path,
    ) -> Result<(), PluginSetupError> {
        let Some(catalog) = self.catalog.get() else {
            return Ok(());
        };
        for entry in &catalog.entries {
            let Some(route) = self.route(&entry.feature.trigger) else {
                continue;
            };
            if route.plugin_id != entry.id
                || route.window_label != entry.window_label
                || !route.input.is_empty()
            {
                continue;
            }
            let _clipboard_allowed = self.authorizes_clipboard(&entry.id);
            let label = entry.window_label.clone();
            let runtime_name = entry
                .runtime
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| io::Error::other("invalid plugin runtime"))?;
            let url = tauri::Url::parse(&format!("uipilot-plugin://localhost/{runtime_name}"))
                .map_err(|error| io::Error::other(error.to_string()))?;
            let data_directory = app_data_dir
                .join("plugin-runtime-data")
                .join(&entry.window_label)
                .join(entry.version.to_path_segment())
                .join(&entry.feature.id);
            let manager = app.state::<std::sync::Arc<PluginManager>>().inner().clone();
            let ready_manager = manager.clone();
            let label_for_ready = label.clone();
            let disabled_manager = manager.clone();
            let label_for_failure = label.clone();
            let plugin_id = entry.id.clone();
            let failure_app = app.handle().clone();
            let window = WebviewWindowBuilder::new(app, label, WebviewUrl::CustomProtocol(url))
                .visible(false)
                .focusable(false)
                .skip_taskbar(true)
                .incognito(true)
                .data_directory(data_directory)
                .initialization_script(PLUGIN_BRIDGE)
                .on_navigation(plugin_navigation_allowed)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(|_, _| false)
                .on_document_title_changed(move |_, title| {
                    if title == "uipilot-plugin-ready" {
                        ready_manager.mark_ready(&label_for_ready);
                    }
                })
                .build()
                .map_err(|error| io::Error::other(error.to_string()))?;
            attach_process_failed_handler(&window, move || {
                disabled_manager.disable_runtime(&label_for_failure);
                let registry = failure_app.state::<ResultRegistry>();
                registry.invalidate_plugin(&plugin_id);
            })?;
        }
        Ok(())
    }

    pub(crate) fn mark_ready(&self, label: &str) {
        self.ready.mark(label);
    }

    pub(crate) fn disable_runtime(&self, label: &str) {
        if let Ok(mut disabled) = self.disabled.write() {
            disabled.insert(label.to_string());
        }
        self.ready.changed.notify_all();
        if let Ok(mut pending) = self.pending.write() {
            pending.retain(|_, query| {
                if query.window_label == label {
                    let _ = query.sender.send(Err(PluginQueryError::RuntimeDisabled));
                    false
                } else {
                    true
                }
            });
        }
    }

    pub(crate) async fn query(
        &self,
        app: &AppHandle,
        route: PluginRoute,
    ) -> Result<Vec<(ResultItem, ResultAction)>, PluginQueryError> {
        if self
            .disabled
            .read()
            .map_err(|_| PluginQueryError::RuntimeDisabled)?
            .contains(&route.window_label)
        {
            return Err(PluginQueryError::RuntimeDisabled);
        }
        let ready = Arc::clone(&self.ready);
        let disabled = Arc::clone(&self.disabled);
        let label = route.window_label.clone();
        let is_ready =
            tauri::async_runtime::spawn_blocking(move || wait_until_ready(ready, disabled, label))
                .await
                .map_err(|_| PluginQueryError::RuntimeDisabled)??;
        if !is_ready {
            return Ok(Vec::new());
        }
        let request_id = self.allocate_request_id();
        let (sender, receiver) = mpsc::channel();
        self.pending
            .write()
            .map_err(|_| PluginQueryError::RuntimeDisabled)?
            .insert(
                request_id.clone(),
                PendingPluginQuery {
                    plugin_id: route.plugin_id.clone(),
                    window_label: route.window_label.clone(),
                    sender,
                },
            );
        let request = PluginQueryRequest {
            protocol_version: 1,
            request_id: request_id.clone(),
            input: route.input,
        };
        let Some(window) = app.get_webview_window(&route.window_label) else {
            self.remove_pending(&request_id);
            return Err(PluginQueryError::RuntimeDisabled);
        };
        window
            .emit("uipilot-plugin-query", request)
            .map_err(|_| PluginQueryError::RuntimeDisabled)?;

        let label = route.window_label.clone();
        let received = tauri::async_runtime::spawn_blocking(move || {
            receiver.recv_timeout(Duration::from_millis(500))
        })
        .await
        .map_err(|_| PluginQueryError::RuntimeDisabled)?;
        match received {
            Ok(Ok(items)) => {
                self.reset_timeouts(&label);
                Ok(items)
            }
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.remove_pending(&request_id);
                self.record_timeout(&label);
                Err(PluginQueryError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(PluginQueryError::RuntimeDisabled),
        }
    }

    pub(crate) fn publish_response(
        &self,
        label: &str,
        response: serde_json::Value,
    ) -> Result<(), PluginQueryError> {
        let entry = self
            .catalog
            .get()
            .and_then(|catalog| {
                catalog
                    .entries
                    .iter()
                    .find(|entry| entry.window_label == label)
            })
            .ok_or(PluginQueryError::InvalidResponse)?;
        let serialized =
            serde_json::to_vec(&response).map_err(|_| PluginQueryError::InvalidResponse)?;
        if serialized.len() > 128 * 1024 {
            return self.reject_response(response);
        }
        let response: PluginQueryResponse = match serde_json::from_value(response.clone()) {
            Ok(response) => response,
            Err(_) => return self.reject_response(response),
        };
        if response.protocol_version != 1 || response.items.len() > 20 {
            return self.reject_request(&response.request_id);
        }
        let pending = self
            .pending
            .write()
            .map_err(|_| PluginQueryError::RuntimeDisabled)?
            .remove(&response.request_id)
            .ok_or(PluginQueryError::InvalidResponse)?;
        if pending.plugin_id != entry.id || pending.window_label != label {
            let _ = pending.sender.send(Err(PluginQueryError::InvalidResponse));
            return Err(PluginQueryError::InvalidResponse);
        }
        let mut items = Vec::with_capacity(response.items.len());
        for item in response.items {
            if item.title.is_empty()
                || item.title.chars().count() > 200
                || item
                    .subtitle
                    .as_ref()
                    .is_some_and(|value| value.chars().count() > 500)
            {
                let _ = pending.sender.send(Err(PluginQueryError::InvalidResponse));
                return Err(PluginQueryError::InvalidResponse);
            }
            let action = match item.action {
                PluginAction::CopyText { text } => {
                    if text.len() > 4096 || !self.authorizes_clipboard(&entry.id) {
                        let _ = pending.sender.send(Err(PluginQueryError::InvalidResponse));
                        return Err(PluginQueryError::InvalidResponse);
                    }
                    ResultAction::CopyText {
                        plugin_id: entry.id.clone(),
                        text,
                    }
                }
            };
            items.push((
                ResultItem {
                    result_id: String::new(),
                    title: item.title,
                    subtitle: item.subtitle,
                    icon: None,
                },
                action,
            ));
        }
        pending
            .sender
            .send(Ok(items))
            .map_err(|_| PluginQueryError::RuntimeDisabled)?;
        self.reset_timeouts(label);
        Ok(())
    }

    fn reject_response(&self, response: serde_json::Value) -> Result<(), PluginQueryError> {
        if let Some(request_id) = response.get("requestId").and_then(|value| value.as_str()) {
            self.reject_request(request_id)
        } else {
            Err(PluginQueryError::InvalidResponse)
        }
    }

    fn reject_request(&self, request_id: &str) -> Result<(), PluginQueryError> {
        if let Some(pending) = self.remove_pending(request_id) {
            let _ = pending.sender.send(Err(PluginQueryError::InvalidResponse));
        }
        Err(PluginQueryError::InvalidResponse)
    }

    fn remove_pending(&self, request_id: &str) -> Option<PendingPluginQuery> {
        self.pending.write().ok()?.remove(request_id)
    }

    fn reset_timeouts(&self, label: &str) {
        if let Ok(mut timeouts) = self.timeouts.write() {
            timeouts.remove(label);
        }
    }

    fn record_timeout(&self, label: &str) {
        if let Ok(mut timeouts) = self.timeouts.write() {
            let count = timeouts.entry(label.to_string()).or_default();
            *count = count.saturating_add(1);
            if *count >= 3 {
                self.disable_runtime(label);
            }
        }
    }

    fn allocate_request_id(&self) -> String {
        let previous = self.next_request.fetch_add(1, Ordering::Relaxed);
        format!("plugin-request-{:016x}", previous + 1)
    }
}

#[derive(Default)]
struct RuntimeReadiness {
    labels: Mutex<HashSet<String>>,
    changed: Condvar,
}

impl RuntimeReadiness {
    fn mark(&self, label: &str) {
        if let Ok(mut labels) = self.labels.lock() {
            labels.insert(label.to_string());
            self.changed.notify_all();
        }
    }
}

fn wait_until_ready(
    ready: Arc<RuntimeReadiness>,
    disabled: Arc<RwLock<HashSet<String>>>,
    label: String,
) -> Result<bool, PluginQueryError> {
    let labels = ready
        .labels
        .lock()
        .map_err(|_| PluginQueryError::RuntimeDisabled)?;
    let (labels, _) = ready
        .changed
        .wait_timeout_while(labels, Duration::from_millis(500), |labels| {
            !labels.contains(&label)
                && disabled
                    .read()
                    .is_ok_and(|disabled| !disabled.contains(&label))
        })
        .map_err(|_| PluginQueryError::RuntimeDisabled)?;
    if disabled
        .read()
        .map_err(|_| PluginQueryError::RuntimeDisabled)?
        .contains(&label)
    {
        Err(PluginQueryError::RuntimeDisabled)
    } else {
        Ok(labels.contains(&label))
    }
}

struct PendingPluginQuery {
    plugin_id: String,
    window_label: String,
    sender: mpsc::Sender<Result<Vec<(ResultItem, ResultAction)>, PluginQueryError>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginQueryError {
    Timeout,
    RuntimeDisabled,
    InvalidResponse,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginQueryRequest {
    protocol_version: u32,
    request_id: String,
    input: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginQueryResponse {
    protocol_version: u32,
    request_id: String,
    items: Vec<PluginResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginResult {
    title: String,
    subtitle: Option<String>,
    action: PluginAction,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum PluginAction {
    CopyText { text: String },
}

pub(crate) struct PluginCatalog {
    entries: Vec<PluginCatalogEntry>,
}

#[derive(Clone)]
pub(crate) struct PluginCatalogEntry {
    pub(crate) id: String,
    pub(crate) version: Version,
    pub(crate) runtime: PathBuf,
    pub(crate) feature: PluginFeature,
    pub(crate) permissions: Vec<String>,
    pub(crate) root: PathBuf,
    pub(crate) window_label: String,
}

#[derive(Clone)]
pub(crate) struct PluginFeature {
    pub(crate) id: String,
    pub(crate) trigger: String,
}

#[derive(Clone)]
pub(crate) struct PluginRoute {
    pub(crate) plugin_id: String,
    pub(crate) window_label: String,
    pub(crate) input: String,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct Version([u32; 3]);

impl Version {
    pub(crate) fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self([major, minor, patch])
    }

    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let version = Self([
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ]);
        parts.next().is_none().then_some(version)
    }

    fn to_path_segment(self) -> String {
        format!("{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

impl PluginCatalog {
    pub(crate) fn load(root: &Path, host_version: Version) -> Result<Self, PluginSetupError> {
        let mut candidates = Vec::new();
        let children = match fs::read_dir(root) {
            Ok(children) => children,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    entries: Vec::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        if !ordinary_directory(root) {
            return Ok(Self {
                entries: Vec::new(),
            });
        }

        for child in children {
            let child = child?;
            if child.file_type()?.is_dir() && ordinary_directory(&child.path()) {
                if let Some(entry) = load_entry(&child.path(), host_version) {
                    candidates.push(entry);
                }
            }
        }

        let duplicate_ids = duplicates(candidates.iter().map(|entry| entry.id.as_str()));
        let duplicate_triggers = duplicates(
            candidates
                .iter()
                .map(|entry| entry.feature.trigger.as_str()),
        );
        candidates.retain(|entry| {
            !duplicate_ids.contains(entry.id.as_str())
                && !duplicate_triggers.contains(entry.feature.trigger.as_str())
        });
        Ok(Self {
            entries: candidates,
        })
    }

    pub(crate) fn route(&self, query: &str) -> Option<PluginRoute> {
        self.entries.iter().find_map(|entry| {
            if query == entry.feature.trigger {
                Some(route(entry, ""))
            } else {
                query
                    .strip_prefix(&entry.feature.trigger)
                    .and_then(|body| body.strip_prefix(' '))
                    .map(|input| route(entry, input))
            }
        })
    }

    pub(crate) fn authorizes_clipboard(&self, plugin_id: &str) -> bool {
        self.entries.iter().any(|entry| {
            entry.id == plugin_id
                && entry
                    .permissions
                    .iter()
                    .any(|permission| permission == "clipboard.writeText")
        })
    }

    pub(crate) fn asset_response(&self, label: &str, request_path: &str) -> Response<Vec<u8>> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.window_label == label)
        else {
            return response(403, Vec::new(), None);
        };
        let Some((relative, content_type)) = asset_path(request_path) else {
            return response(415, Vec::new(), None);
        };
        let path = entry.root.join(&relative);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return response(404, Vec::new(), None);
        };
        if !metadata.is_file() || !ordinary_file_below(&entry.root, &relative) {
            return response(403, Vec::new(), None);
        }
        let Ok(body) = fs::read(&path) else {
            return response(404, Vec::new(), None);
        };
        if !ordinary_file_below(&entry.root, &relative) {
            return response(403, Vec::new(), None);
        }
        response(200, body, Some(content_type))
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    manifest: u32,
    id: String,
    version: String,
    #[serde(rename = "minHostVersion")]
    min_host_version: String,
    runtime: String,
    feature: ManifestFeature,
    permissions: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFeature {
    id: String,
    trigger: String,
}

fn load_entry(root: &Path, host_version: Version) -> Option<PluginCatalogEntry> {
    let manifest_path = root.join("plugin.json");
    if !ordinary_file(&manifest_path) {
        return None;
    }
    let manifest = fs::read_to_string(&manifest_path).ok()?;
    let manifest: Manifest = serde_json::from_str(&manifest).ok()?;
    let version = Version::parse(&manifest.version)?;
    if manifest.manifest != 1
        || Version::parse(&manifest.min_host_version)? > host_version
        || !valid_id(&manifest.id)
        || !valid_id(&manifest.feature.id)
        || !valid_trigger(&manifest.feature.trigger)
        || manifest.runtime.contains(['/', '\\'])
        || Path::new(&manifest.runtime)
            .extension()
            .and_then(|value| value.to_str())
            != Some("html")
    {
        return None;
    }

    let runtime = root.join(&manifest.runtime);
    if !ordinary_file(&runtime) || has_bad_permissions(&manifest.permissions) {
        return None;
    }

    Some(PluginCatalogEntry {
        window_label: window_label(&manifest.id),
        id: manifest.id,
        version,
        runtime,
        feature: PluginFeature {
            id: manifest.feature.id,
            trigger: manifest.feature.trigger,
        },
        permissions: manifest.permissions,
        root: root.to_path_buf(),
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
}

fn valid_trigger(trigger: &str) -> bool {
    trigger.starts_with('/')
        && trigger.len() <= 64
        && trigger.len() > 1
        && trigger.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'/' || byte == b'-'
        })
}

fn has_bad_permissions(permissions: &[String]) -> bool {
    let mut seen = HashSet::new();
    permissions
        .iter()
        .any(|permission| permission != "clipboard.writeText" || !seen.insert(permission))
}

fn duplicates<'a>(values: impl Iterator<Item = &'a str>) -> HashSet<String> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(value, count)| (count > 1).then_some(value.to_string()))
        .collect()
}

fn window_label(id: &str) -> String {
    let mut label = String::from("plugin-");
    for byte in id.as_bytes() {
        label.push_str(&format!("{byte:02x}"));
    }
    label
}

fn route(entry: &PluginCatalogEntry, input: &str) -> PluginRoute {
    PluginRoute {
        plugin_id: entry.id.clone(),
        window_label: entry.window_label.clone(),
        input: input.to_string(),
    }
}

fn asset_path(request_path: &str) -> Option<(PathBuf, &'static str)> {
    let path = request_path.strip_prefix('/')?;
    if path.is_empty() || request_path.contains('%') || request_path.contains('\\') {
        return None;
    }
    let mut relative = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            return None;
        }
        relative.push(part);
    }
    let mime = match relative.extension()?.to_str()? {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        _ => return None,
    };
    Some((relative, mime))
}

fn ordinary_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !is_reparse_point(&metadata))
}

fn ordinary_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !is_reparse_point(&metadata))
}

fn ordinary_file_below(root: &Path, relative: &Path) -> bool {
    if !ordinary_directory(root) {
        return false;
    }
    let mut path = root.to_path_buf();
    let mut components = relative.iter().peekable();
    while let Some(component) = components.next() {
        path.push(component);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if is_reparse_point(&metadata)
            || (components.peek().is_some() && !metadata.is_dir())
            || (components.peek().is_none() && !metadata.is_file())
        {
            return false;
        }
    }
    true
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn plugin_navigation_allowed(url: &tauri::Url) -> bool {
    url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("uipilot-plugin", Some("localhost")) | ("http", Some("uipilot-plugin.localhost"))
        )
}

fn response(status: u16, body: Vec<u8>, content_type: Option<&str>) -> Response<Vec<u8>> {
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder
            .header("content-type", content_type)
            .header("content-security-policy", PLUGIN_CSP);
    }
    builder.body(body).unwrap()
}

#[cfg(windows)]
fn attach_process_failed_handler<F>(
    window: &WebviewWindow,
    callback: F,
) -> Result<(), PluginSetupError>
where
    F: Fn() + Send + 'static,
{
    use webview2_com::ProcessFailedEventHandler;

    WebviewWindow::with_webview(window, move |webview| unsafe {
        if let Ok(core) = webview.controller().CoreWebView2() {
            let handler = ProcessFailedEventHandler::create(Box::new(move |_, _| {
                callback();
                Ok(())
            }));
            let mut token = 0;
            let _ = core.add_ProcessFailed(&handler, &mut token);
        }
    })
    .map_err(|error| PluginSetupError::Io(io::Error::other(error.to_string())))
}

#[cfg(not(windows))]
fn attach_process_failed_handler<F>(
    _window: &WebviewWindow,
    _callback: F,
) -> Result<(), PluginSetupError>
where
    F: Fn() + Send + 'static,
{
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{PluginCatalog, Version};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestRoot {
        path: PathBuf,
    }

    impl TestRoot {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-plugin-catalog-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn write_plugin(&self, id: &str, manifest: String) {
            assert!(!id.contains(std::path::MAIN_SEPARATOR));
            let root = self.path.join(id);
            fs::create_dir(&root).unwrap();
            fs::write(root.join("plugin.json"), manifest).unwrap();
            fs::write(root.join("index.html"), "").unwrap();
        }

        fn remove_plugin(&self, id: &str) {
            assert!(!id.contains(std::path::MAIN_SEPARATOR));
            fs::remove_dir_all(self.path.join(id)).unwrap();
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            if self.path.exists() {
                fs::remove_dir_all(&self.path).unwrap();
            }
        }
    }

    fn package_id() -> String {
        ["internal", "math"].join(".")
    }

    fn trigger() -> String {
        ["/", "math"].concat()
    }

    fn valid_manifest(plugin_id: &str, trigger: &str) -> String {
        format!(
            r#"{{
                "manifest":1,
                "id":"{plugin_id}",
                "version":"1.0.0",
                "minHostVersion":"0.2.0",
                "runtime":"index.html",
                "feature":{{"id":"calculate","trigger":"{trigger}"}},
                "permissions":["clipboard.writeText"]
            }}"#
        )
    }

    fn load(root: &TestRoot) -> PluginCatalog {
        PluginCatalog::load(&root.path, Version::new(0, 2, 0)).unwrap()
    }

    #[test]
    fn package_presence_registers_trigger_and_removal_on_reload_removes_it() {
        let root = TestRoot::new();
        let id = package_id();
        let slash_trigger = trigger();
        root.write_plugin(&id, valid_manifest(&id, &slash_trigger));
        let loaded = load(&root);
        assert_eq!(
            loaded.route(&format!("{slash_trigger} 1+1")).unwrap().input,
            "1+1"
        );

        root.remove_plugin(&id);
        let reloaded = load(&root);
        assert!(reloaded.route(&format!("{slash_trigger} 1+1")).is_none());
    }

    #[test]
    fn exact_keys_versions_and_bounds_are_required() {
        let cases = [
            valid_manifest("one", "/one")
                .replace(r#""permissions""#, r#""extra":true,"permissions""#),
            valid_manifest("manifest-missing", "/manifest-missing").replace(r#""manifest":1,"#, ""),
            valid_manifest("manifest-wrong", "/manifest-wrong")
                .replace(r#""manifest":1"#, r#""manifest":2"#),
            valid_manifest("two", "/two").replace(r#""1.0.0""#, r#""1.0""#),
            valid_manifest("three", "/three").replace(r#""0.2.0""#, r#""0.2""#),
            valid_manifest("four", "/four")
                .replace(r#""minHostVersion":"0.2.0""#, r#""minHostVersion":"0.3.0""#),
            valid_manifest("", "/empty"),
            valid_manifest("bad/id", "/bad"),
            valid_manifest("feature", "/feature").replace(r#""id":"calculate""#, r#""id":""#),
            valid_manifest("missing-trigger", ""),
            valid_manifest("long-trigger", &format!("/{}", "x".repeat(65))),
        ];
        for (index, manifest) in cases.into_iter().enumerate() {
            let root = TestRoot::new();
            let id = format!("plugin-{index}");
            root.write_plugin(&id, manifest);
            assert!(load(&root).route("/anything").is_none(), "case {index}");
        }
    }

    #[test]
    fn unknown_and_duplicate_permissions_disable_package() {
        for manifest in [
            valid_manifest("unknown", "/unknown").replace(
                r#""clipboard.writeText""#,
                r#""clipboard.writeText","network.fetch""#,
            ),
            valid_manifest("duplicate", "/duplicate").replace(
                r#""clipboard.writeText""#,
                r#""clipboard.writeText","clipboard.writeText""#,
            ),
        ] {
            let root = TestRoot::new();
            root.write_plugin("plugin", manifest);
            assert!(load(&root).route("/unknown body").is_none());
            assert!(!load(&root).authorizes_clipboard("plugin"));
        }
    }

    #[test]
    fn runtime_entry_must_be_html() {
        let root = TestRoot::new();
        root.write_plugin(
            "plugin",
            valid_manifest("plugin", "/plugin").replace("index.html", "index.js"),
        );
        fs::write(root.path.join("plugin").join("index.js"), "").unwrap();

        assert!(load(&root).route("/plugin").is_none());
    }

    #[test]
    fn duplicate_ids_or_triggers_disable_every_participant() {
        let root = TestRoot::new();
        root.write_plugin("one", valid_manifest("same", "/one"));
        root.write_plugin("two", valid_manifest("same", "/two"));
        let loaded = load(&root);
        assert!(loaded.route("/one body").is_none());
        assert!(loaded.route("/two body").is_none());

        let root = TestRoot::new();
        root.write_plugin("one", valid_manifest("one", "/same"));
        root.write_plugin("two", valid_manifest("two", "/same"));
        assert!(load(&root).route("/same body").is_none());
    }

    #[test]
    fn scans_direct_child_directories_with_ordinary_files_only() {
        let root = TestRoot::new();
        root.write_plugin("valid", valid_manifest("valid", "/valid"));
        fs::write(
            root.path.join("loose.json"),
            valid_manifest("loose", "/loose"),
        )
        .unwrap();
        let nested = root.path.join("parent").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("plugin.json"),
            valid_manifest("nested", "/nested"),
        )
        .unwrap();
        fs::write(nested.join("index.html"), "").unwrap();
        fs::remove_file(root.path.join("valid").join("index.html")).unwrap();
        fs::create_dir(root.path.join("valid").join("index.html")).unwrap();

        let loaded = load(&root);
        assert!(loaded.route("/valid body").is_none());
        assert!(loaded.route("/nested body").is_none());
        assert!(loaded.route("/loose body").is_none());
    }

    #[test]
    fn reparsed_paths_and_symlinks_are_rejected() {
        let root = TestRoot::new();
        root.write_plugin("valid", valid_manifest("valid", "/valid"));

        #[cfg(windows)]
        {
            let link = root.path.join("linked");
            if std::os::windows::fs::symlink_dir(root.path.join("valid"), link).is_err() {
                return;
            }
        }

        let loaded = load(&root);
        assert!(loaded.route("/linked body").is_none());
        assert!(loaded.route("/valid body").is_some());
    }

    #[test]
    fn route_semantics_are_trigger_then_ascii_space_body_only() {
        let root = TestRoot::new();
        root.write_plugin("plugin", valid_manifest("plugin", "/go"));
        let loaded = load(&root);

        let route = loaded.route("/go body").unwrap();
        assert_eq!(route.plugin_id, "plugin");
        assert_eq!(route.window_label, "plugin-706c7567696e");
        assert_eq!(route.input, "body");
        assert_eq!(loaded.route("/go").unwrap().input, "");
        assert!(loaded.route("/go\tbody").is_none());
        assert!(loaded.route("/good body").is_none());
        assert!(loaded.route("ordinary query").is_none());
        assert!(loaded.authorizes_clipboard("plugin"));
    }

    mod asset {
        use std::fs;

        use super::{load, valid_manifest, TestRoot};

        fn header(response: &tauri::http::Response<Vec<u8>>, name: &str) -> String {
            response
                .headers()
                .get(name)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        }

        #[test]
        fn serves_label_bound_html_and_js_with_csp_and_bridge() {
            let root = TestRoot::new();
            root.write_plugin("one", valid_manifest("one", "/one"));
            root.write_plugin("two", valid_manifest("two", "/two"));
            fs::write(root.path.join("one").join("index.html"), "<h1>one</h1>").unwrap();
            fs::write(root.path.join("two").join("index.html"), "<h1>two</h1>").unwrap();
            fs::write(root.path.join("one").join("main.js"), "window.answer=1;").unwrap();
            let catalog = load(&root);

            let html = catalog.asset_response("plugin-6f6e65", "/index.html");
            assert_eq!(html.status(), 200);
            assert_eq!(
                header(&html, "content-security-policy"),
                super::super::PLUGIN_CSP
            );
            assert_eq!(header(&html, "content-type"), "text/html; charset=utf-8");
            let body = String::from_utf8(html.into_body()).unwrap();
            assert_eq!(body, "<h1>one</h1>");
            assert!(!body.contains("window.uipilot"));
            assert!(!body.contains("publishResults"));
            assert!(!body.contains("<h1>two</h1>"));

            let script = catalog.asset_response("plugin-6f6e65", "/main.js");
            assert_eq!(script.status(), 200);
            assert_eq!(
                header(&script, "content-type"),
                "text/javascript; charset=utf-8"
            );
            assert_eq!(script.into_body(), b"window.answer=1;");
        }

        #[test]
        fn rejects_unknown_labels_and_bad_paths_with_fixed_statuses() {
            let root = TestRoot::new();
            root.write_plugin("one", valid_manifest("one", "/one"));
            fs::write(root.path.join("one").join("style.css"), "").unwrap();
            let catalog = load(&root);
            let label = "plugin-6f6e65";

            assert_eq!(
                catalog
                    .asset_response("plugin-74776f", "/index.html")
                    .status(),
                403
            );
            for path in [
                "",
                "/",
                "/.",
                "/./index.html",
                "/../index.html",
                "/nested/../index.html",
                "C:/index.html",
                "/index.html%00",
                "/index.html:ads",
                "/style.css",
            ] {
                assert_eq!(
                    catalog.asset_response(label, path).status(),
                    415,
                    "bad path accepted: {path}"
                );
            }
            assert_eq!(catalog.asset_response(label, "/missing.html").status(), 404);
        }

        #[test]
        fn rejects_another_plugin_root_and_reparse_assets() {
            let root = TestRoot::new();
            root.write_plugin("one", valid_manifest("one", "/one"));
            root.write_plugin("two", valid_manifest("two", "/two"));
            let catalog = load(&root);

            assert_eq!(
                catalog
                    .asset_response("plugin-6f6e65", "/../two/index.html")
                    .status(),
                415
            );

            #[cfg(windows)]
            {
                let link = root.path.join("one").join("linked.html");
                if std::os::windows::fs::symlink_file(
                    root.path.join("two").join("index.html"),
                    link,
                )
                .is_ok()
                {
                    assert_eq!(
                        catalog
                            .asset_response("plugin-6f6e65", "/linked.html")
                            .status(),
                        403
                    );
                }

                let nested = root.path.join("one").join("nested");
                if std::os::windows::fs::symlink_dir(root.path.join("two"), nested).is_ok() {
                    assert_eq!(
                        catalog
                            .asset_response("plugin-6f6e65", "/nested/index.html")
                            .status(),
                        403
                    );
                }

                let junction = root.path.join("one").join("junction");
                let output = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(&junction)
                    .arg(root.path.join("two"))
                    .output()
                    .unwrap();
                assert!(output.status.success(), "junction creation failed");
                assert_eq!(
                    catalog
                        .asset_response("plugin-6f6e65", "/junction/index.html")
                        .status(),
                    403
                );
                fs::remove_dir(junction).unwrap();

                let plugin_root = root.path.join("one");
                let parked_root = root.path.join("one-parked");
                fs::rename(&plugin_root, &parked_root).unwrap();
                let output = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(&plugin_root)
                    .arg(root.path.join("two"))
                    .output()
                    .unwrap();
                assert!(output.status.success(), "junction replacement failed");
                assert_eq!(
                    catalog
                        .asset_response("plugin-6f6e65", "/index.html")
                        .status(),
                    403
                );
                fs::remove_dir(&plugin_root).unwrap();
                fs::rename(parked_root, plugin_root).unwrap();
            }
        }

        #[test]
        fn bridge_is_loaded_by_tauri_not_csp_blocked_html() {
            let source = std::fs::read_to_string(file!()).unwrap();
            let bridge = super::super::PLUGIN_BRIDGE;
            assert!(source.contains(".initialization_script(PLUGIN_BRIDGE)"));
            assert!(
                bridge
                    .find("Object.defineProperty(window, 'uipilot'")
                    .unwrap()
                    < bridge.find("plugin:event|listen").unwrap()
            );
            assert!(bridge.contains("handler(request.input)"));
            assert!(bridge.contains("const internals = () => window.__TAURI_INTERNALS__"));
            assert!(bridge.contains("waitForInternals().then((tauri) =>"));
            assert!(bridge.contains("tauri.invoke('plugin:event|listen'"));
            assert!(bridge.contains("tauri.transformCallback"));
            assert!(bridge.contains("requestId: activeRequest.requestId"));
            assert!(bridge.contains("finally { activeRequest = null; }"));
            assert!(bridge.contains("protocolVersion: 1"));
            assert!(bridge.contains("uipilot-plugin-ready"));
        }

        #[test]
        fn query_waits_for_runtime_ready_off_the_async_executor() {
            let source = std::fs::read_to_string(file!()).unwrap();
            let production = source.split("#[cfg(test)]").next().unwrap();
            assert!(production.contains("wait_until_ready(ready, disabled, label)"));
            assert!(production.contains("tauri::async_runtime::spawn_blocking"));
            assert!(!production.contains(&["thread", "::sleep"].concat()));
        }

        #[test]
        fn runtime_failure_invalidates_only_its_plugin_results() {
            let source = std::fs::read_to_string(file!()).unwrap();
            let production = source.split("#[cfg(test)]").next().unwrap();
            assert!(production.contains("registry.invalidate_plugin(&plugin_id)"));
        }

        #[test]
        fn allows_only_plugin_navigation_origins() {
            for url in [
                "uipilot-plugin://localhost/runtime.html",
                "http://uipilot-plugin.localhost/runtime.html",
            ] {
                assert!(super::super::plugin_navigation_allowed(
                    &tauri::Url::parse(url).unwrap()
                ));
            }
            for url in [
                "http://uipilot-plugin.localhost.evil/runtime.html",
                "http://uipilot-plugin.localhost:1420/runtime.html",
                "https://example.com/runtime.html",
            ] {
                assert!(!super::super::plugin_navigation_allowed(
                    &tauri::Url::parse(url).unwrap()
                ));
            }
        }
    }

    mod query {
        use std::sync::{mpsc, Arc, RwLock};

        use serde_json::json;

        use super::super::{
            wait_until_ready, PendingPluginQuery, PluginManager, PluginQueryError, RuntimeReadiness,
        };
        use super::{load, valid_manifest, TestRoot};

        fn manager(root: &TestRoot) -> PluginManager {
            let manager = PluginManager::new();
            assert!(manager.catalog.set(load(root)).is_ok());
            manager
        }

        fn wait_for(
            manager: &PluginManager,
            request_id: &str,
        ) -> mpsc::Receiver<
            Result<
                Vec<(
                    crate::model::ResultItem,
                    crate::result_registry::ResultAction,
                )>,
                PluginQueryError,
            >,
        > {
            let (sender, receiver) = mpsc::channel();
            manager.pending.write().unwrap().insert(
                request_id.into(),
                PendingPluginQuery {
                    plugin_id: "plugin".into(),
                    window_label: "plugin-706c7567696e".into(),
                    sender,
                },
            );
            receiver
        }

        #[test]
        fn valid_zero_and_twenty_items_publish_and_reset_timeouts() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/go"));
            let manager = manager(&root);
            manager.record_timeout("plugin-706c7567696e");
            let receiver = wait_for(&manager, "request");
            manager
                .publish_response(
                    "plugin-706c7567696e",
                    json!({"protocolVersion":1,"requestId":"request","items":[]}),
                )
                .unwrap();
            assert_eq!(receiver.recv().unwrap().unwrap().len(), 0);
            assert!(manager.timeouts.read().unwrap().is_empty());

            let receiver = wait_for(&manager, "request-2");
            let items = (0..20)
                .map(|index| {
                    json!({"title":format!("Item {index}"),"subtitle":null,"action":{"type":"copyText","text":"copy"}})
                })
                .collect::<Vec<_>>();
            manager
                .publish_response(
                    "plugin-706c7567696e",
                    json!({"protocolVersion":1,"requestId":"request-2","items":items}),
                )
                .unwrap();
            assert_eq!(receiver.recv().unwrap().unwrap().len(), 20);
        }

        #[test]
        fn invalid_responses_notify_waiter_and_duplicate_is_rejected() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/go"));
            let manager = manager(&root);
            let receiver = wait_for(&manager, "request");
            assert_eq!(
                manager.publish_response(
                    "plugin-706c7567696e",
                    json!({"protocolVersion":2,"requestId":"request","items":[]}),
                ),
                Err(PluginQueryError::InvalidResponse)
            );
            assert!(matches!(
                receiver.recv().unwrap(),
                Err(PluginQueryError::InvalidResponse)
            ));
            assert_eq!(
                manager.publish_response(
                    "plugin-706c7567696e",
                    json!({"protocolVersion":1,"requestId":"request","items":[]}),
                ),
                Err(PluginQueryError::InvalidResponse)
            );
        }

        #[test]
        fn count_size_unknown_key_and_text_limits_fail_as_one_response() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/go"));
            let manager = manager(&root);
            for (request_id, response) in [
                (
                    "too-many",
                    json!({"protocolVersion":1,"requestId":"too-many","items":(0..21).map(|_| json!({"title":"x","subtitle":null,"action":{"type":"copyText","text":"x"}})).collect::<Vec<_>>()}),
                ),
                (
                    "unknown",
                    json!({"protocolVersion":1,"requestId":"unknown","items":[],"extra":true}),
                ),
                (
                    "text",
                    json!({"protocolVersion":1,"requestId":"text","items":[{"title":"x","subtitle":null,"action":{"type":"copyText","text":"x".repeat(4097)}}]}),
                ),
            ] {
                let receiver = wait_for(&manager, request_id);
                assert_eq!(
                    manager.publish_response("plugin-706c7567696e", response),
                    Err(PluginQueryError::InvalidResponse)
                );
                assert!(matches!(
                    receiver.recv().unwrap(),
                    Err(PluginQueryError::InvalidResponse)
                ));
            }
        }

        #[test]
        fn three_timeouts_disable_runtime_until_restart() {
            let manager = PluginManager::new();
            for _ in 0..3 {
                manager.record_timeout("plugin-label");
            }
            assert!(manager.disabled.read().unwrap().contains("plugin-label"));
        }

        #[test]
        fn disabled_runtime_loses_clipboard_authorization() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            assert!(manager.authorizes_clipboard("plugin"));

            manager.disable_runtime("plugin-706c7567696e");

            assert!(!manager.authorizes_clipboard("plugin"));
        }

        #[test]
        fn readiness_wait_completes_after_runtime_marks_ready() {
            let ready = Arc::new(RuntimeReadiness::default());
            let disabled = Arc::new(RwLock::new(std::collections::HashSet::new()));
            let marker = Arc::clone(&ready);
            let worker = std::thread::spawn(move || marker.mark("plugin-label"));

            assert_eq!(
                wait_until_ready(ready, disabled, "plugin-label".into()),
                Ok(true)
            );
            worker.join().unwrap();
        }
    }
}
