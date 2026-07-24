use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock, RwLock,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{
    http::Response,
    webview::{NewWindowResponse, WebviewWindow},
    App, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use unicode_normalization::is_nfc;

use crate::{
    atomic_file::replace_current,
    model::{ResultItem, SearchResponse},
    result_registry::{QueryDomain, QueryToken, ResultAction, ResultRegistry},
};

pub(crate) const PLUGIN_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'";
pub(crate) const PLUGIN_RUNTIME_READY_TIMEOUT: Duration = Duration::from_millis(500);
const PLUGIN_DURABLE_DOCUMENT_MAX_BYTES: u64 = 64 * 1024;
const PLUGIN_CLEANUP_MAX_RECEIPTS: usize = 128;
const PLUGIN_CLEANUP_BATCH_RECEIPTS: usize = 8;
const PLUGIN_CLEANUP_BATCH_BYTES: u64 = 64 * 1024 * 1024;
const PLUGIN_CLEANUP_MAX_DIRECTORIES: usize = 512;
const PLUGIN_CLEANUP_MAX_FILES: usize = 1024;
const PLUGIN_README_MAX_BYTES: u64 = 16 * 1024;
const PLUGIN_PACKAGE_MAX_DIRECTORIES: usize = 64;
const PLUGIN_PACKAGE_MAX_FILES: usize = 256;
const PLUGIN_PACKAGE_MAX_DEPTH: usize = 8;
const PLUGIN_PACKAGE_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const PLUGIN_PACKAGE_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const PLUGIN_PACKAGE_MAX_PATH_BYTES: usize = 240;
const PLUGIN_PACKAGE_MAX_COMPONENT_BYTES: usize = 100;
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
    state: OnceLock<RwLock<PluginManagerState>>,
    config: OnceLock<PluginManagerConfig>,
    mutation: Mutex<()>,
    admission: RwLock<()>,
    disabled: Arc<RwLock<HashSet<String>>>,
    pending: RwLock<HashMap<String, PendingPluginQuery>>,
    timeouts: RwLock<HashMap<String, u8>>,
    next_request: AtomicU64,
    next_quarantine: AtomicU64,
}

#[derive(Clone)]
struct PluginManagerConfig {
    app_data_dir: PathBuf,
    plugin_root: PathBuf,
    quarantine_root: PathBuf,
    host_version: Version,
    development_root: Option<PathBuf>,
}

struct PluginManagerState {
    active: PluginCatalog,
    staged_assets: HashMap<RuntimeIdentity, PluginCatalogEntry>,
    ownership: HashMap<RuntimeIdentity, RuntimeOwnership>,
    latest_generations: HashMap<String, u64>,
    inventory_revision: u64,
}

#[derive(Clone)]
struct RuntimeOwnership {
    slot: RuntimeSlot,
    attempt: Arc<RuntimeAttempt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeSlot {
    Active,
    Staged,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RuntimeIdentity {
    pub(crate) plugin_id: String,
    pub(crate) window_label: String,
    pub(crate) generation: u64,
}

#[derive(Default)]
struct RuntimeAttempt {
    state: Mutex<RuntimeAttemptState>,
    changed: Condvar,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RuntimeAttemptState {
    ready: bool,
    failed: bool,
}

impl RuntimeAttempt {
    fn mark_ready(&self) {
        if let Ok(mut state) = self.state.lock() {
            if !state.failed {
                state.ready = true;
            }
            self.changed.notify_all();
        }
    }

    fn mark_failed(&self) -> bool {
        if let Ok(mut state) = self.state.lock() {
            if state.failed {
                return false;
            }
            state.failed = true;
            self.changed.notify_all();
            true
        } else {
            false
        }
    }

    fn snapshot(&self) -> Option<RuntimeAttemptState> {
        self.state.lock().ok().map(|state| *state)
    }

    fn wait_until_settled(&self, timeout: Duration) -> Option<RuntimeAttemptState> {
        let state = self.state.lock().ok()?;
        let (state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |state| !state.ready && !state.failed)
            .ok()?;
        Some(*state)
    }
}

impl PluginManagerState {
    fn from_catalog(active: PluginCatalog) -> Self {
        let mut ownership = HashMap::new();
        let mut latest_generations = HashMap::new();
        for entry in &active.entries {
            let identity = entry.identity();
            ownership.insert(
                identity,
                RuntimeOwnership {
                    slot: RuntimeSlot::Active,
                    attempt: Arc::new(RuntimeAttempt::default()),
                },
            );
            latest_generations.insert(entry.id.clone(), entry.generation);
        }
        Self {
            active,
            staged_assets: HashMap::new(),
            ownership,
            latest_generations,
            inventory_revision: 0,
        }
    }
}

impl PluginManager {
    pub(crate) fn new() -> Self {
        Self {
            state: OnceLock::new(),
            config: OnceLock::new(),
            mutation: Mutex::new(()),
            admission: RwLock::new(()),
            disabled: Arc::new(RwLock::new(HashSet::new())),
            pending: RwLock::new(HashMap::new()),
            timeouts: RwLock::new(HashMap::new()),
            next_request: AtomicU64::new(0),
            next_quarantine: AtomicU64::new(0),
        }
    }

    pub(crate) fn load(
        &self,
        app_data_dir: &Path,
        host_version: Version,
    ) -> Result<(), PluginSetupError> {
        let plugin_root = app_data_dir.join("plugins");
        let quarantine_root = app_data_dir.join("plugin-quarantine");
        fs::create_dir_all(&quarantine_root)?;
        if !ordinary_directory(&quarantine_root) {
            return Err(PluginSetupError::Io(io::Error::other(
                "plugin quarantine unavailable",
            )));
        }
        cleanup_quarantine(&quarantine_root);
        let catalog = PluginCatalog::load(&plugin_root, host_version)?;
        self.config
            .set(PluginManagerConfig {
                app_data_dir: app_data_dir.to_path_buf(),
                plugin_root,
                quarantine_root,
                host_version,
                development_root: development_plugin_root(),
            })
            .map_err(|_| PluginSetupError::AlreadyLoaded)?;
        self.state
            .set(RwLock::new(PluginManagerState::from_catalog(catalog)))
            .map_err(|_| PluginSetupError::AlreadyLoaded)
    }

    pub(crate) fn route(&self, query: &str) -> Option<PluginRoute> {
        let _admission = self.admission.read().ok()?;
        self.state.get()?.read().ok()?.active.route(query)
    }

    pub(crate) fn list_views(&self) -> Result<Vec<PluginView>, PluginManagementError> {
        let _admission = self
            .admission
            .read()
            .map_err(|_| PluginManagementError::Unavailable)?;
        self.state
            .get()
            .and_then(|state| state.read().ok().map(|state| state.active.views()))
            .ok_or(PluginManagementError::Unavailable)
    }

    pub(crate) fn list_inventory(&self) -> Result<PluginInventorySnapshot, PluginManagementError> {
        let config = self
            .config
            .get()
            .cloned()
            .ok_or(PluginManagementError::Unavailable)?;
        for _ in 0..2 {
            let revision = self
                .state
                .get()
                .and_then(|state| state.read().ok().map(|state| state.inventory_revision))
                .ok_or(PluginManagementError::Unavailable)?;
            let snapshot = scan_inventory(
                &config.plugin_root,
                config.development_root.as_deref(),
                config.host_version,
                revision,
            )?;
            let current_revision = self
                .state
                .get()
                .and_then(|state| state.read().ok().map(|state| state.inventory_revision))
                .ok_or(PluginManagementError::Unavailable)?;
            if current_revision == revision {
                return Ok(snapshot);
            }
        }
        Err(PluginManagementError::Unavailable)
    }

    pub(crate) fn begin_routed_query(
        &self,
        query: &str,
        registry: &ResultRegistry,
        invocation_id: &str,
        query_sequence: u64,
    ) -> PluginQueryStart {
        let Ok(_admission) = self.admission.read() else {
            return PluginQueryStart::Rejected;
        };
        let Some(route) = self
            .state
            .get()
            .and_then(|state| state.read().ok()?.active.route(query))
        else {
            return PluginQueryStart::NoRoute;
        };
        let Some(token) = registry.begin_query(QueryDomain::Plugin, invocation_id, query_sequence)
        else {
            return PluginQueryStart::Rejected;
        };
        PluginQueryStart::Started { route, token }
    }

    pub(crate) fn publish_results(
        &self,
        registry: &ResultRegistry,
        token: QueryToken,
        route: &PluginRoute,
        entries: Vec<(ResultItem, ResultAction)>,
    ) -> Option<SearchResponse> {
        let _admission = self.admission.read().ok()?;
        let current = self
            .state
            .get()?
            .read()
            .ok()?
            .active
            .entries
            .iter()
            .any(|entry| route_matches(entry, route));
        if !current {
            return None;
        }
        registry.publish_if_latest(
            token,
            entries,
            || true,
            |request_id, items| SearchResponse {
                request_id,
                items: items
                    .into_iter()
                    .map(|(result_id, mut item)| {
                        item.result_id = result_id;
                        item
                    })
                    .collect(),
            },
        )
    }

    pub(crate) fn copy_text<F>(
        &self,
        plugin_id: &str,
        generation: u64,
        copy: F,
    ) -> Result<(), PluginCopyError>
    where
        F: FnOnce() -> Result<(), ()>,
    {
        let _admission = self
            .admission
            .read()
            .map_err(|_| PluginCopyError::PermissionDenied)?;
        let authorized = self
            .state
            .get()
            .and_then(|state| {
                let state = state.read().ok()?;
                state
                    .active
                    .entries
                    .iter()
                    .find(|entry| {
                        entry.id == plugin_id
                            && entry.generation == generation
                            && entry
                                .permissions
                                .iter()
                                .any(|permission| permission == "clipboard.writeText")
                    })
                    .map(|entry| entry.window_label.clone())
            })
            .is_some_and(|label| {
                self.disabled
                    .read()
                    .is_ok_and(|disabled| !disabled.contains(&label))
            });
        if !authorized {
            return Err(PluginCopyError::PermissionDenied);
        }
        copy().map_err(|_| PluginCopyError::SideEffectFailed)
    }

    #[cfg(test)]
    fn install_catalog_for_test(&self, catalog: PluginCatalog) {
        self.state
            .set(RwLock::new(PluginManagerState::from_catalog(catalog)))
            .unwrap_or_else(|_| panic!("test catalog already installed"));
    }

    #[cfg(test)]
    fn advance_generation_for_test(&self, registry: &ResultRegistry, plugin_id: &str) {
        let _admission = self.admission.write().expect("plugin admission poisoned");
        let mut state = self
            .state
            .get()
            .expect("test catalog missing")
            .write()
            .expect("plugin catalog poisoned");
        let (id, generation) = {
            let entry = state
                .active
                .entries
                .iter_mut()
                .find(|entry| entry.id == plugin_id)
                .expect("test plugin missing");
            entry.generation = entry
                .generation
                .checked_add(1)
                .expect("test generation overflow");
            (entry.id.clone(), entry.generation)
        };
        state.latest_generations.insert(id, generation);
        drop(state);
        registry
            .invalidate_domain(QueryDomain::Plugin)
            .expect("test plugin epoch exhausted");
    }

    #[cfg(test)]
    pub(crate) fn authorizes_clipboard(&self, plugin_id: &str) -> bool {
        let Ok(_admission) = self.admission.read() else {
            return false;
        };
        let Some(state) = self.state.get() else {
            return false;
        };
        let Ok(state) = state.read() else {
            return false;
        };
        let Some(entry) = state
            .active
            .entries
            .iter()
            .find(|entry| entry.id == plugin_id)
        else {
            return false;
        };
        self.disabled
            .read()
            .is_ok_and(|disabled| !disabled.contains(&entry.window_label))
            && state.active.authorizes_clipboard(plugin_id)
    }

    pub(crate) fn asset_response(&self, label: &str, request_path: &str) -> Response<Vec<u8>> {
        let entry = {
            let Ok(_admission) = self.admission.read() else {
                return response(403, Vec::new(), None);
            };
            self.state.get().and_then(|state| {
                let state = state.read().ok()?;
                state
                    .active
                    .entries
                    .iter()
                    .find(|entry| entry.window_label == label)
                    .or_else(|| {
                        state
                            .staged_assets
                            .values()
                            .find(|entry| entry.window_label == label)
                    })
                    .cloned()
            })
        };
        entry.map_or_else(
            || response(403, Vec::new(), None),
            |entry| asset_response(&entry, request_path),
        )
    }

    pub(crate) fn create_runtimes(
        self: &Arc<Self>,
        app: &App,
        _app_data_dir: &Path,
    ) -> Result<(), PluginSetupError> {
        let Some(state) = self.state.get() else {
            return Ok(());
        };
        let entries = state
            .read()
            .map_err(|_| io::Error::other("plugin catalog unavailable"))?
            .active
            .entries
            .clone();
        for entry in &entries {
            let Some(route) = self.route(&entry.feature.trigger) else {
                continue;
            };
            if route.plugin_id != entry.id
                || route.window_label != entry.window_label
                || !route.input.is_empty()
            {
                continue;
            }
            self.create_runtime_window(app.handle(), entry)?;
        }
        Ok(())
    }

    fn create_runtime_window(
        self: &Arc<Self>,
        app: &AppHandle,
        entry: &PluginCatalogEntry,
    ) -> Result<WebviewWindow, PluginSetupError> {
        let config = self
            .config
            .get()
            .ok_or_else(|| PluginSetupError::Io(io::Error::other("plugin manager unavailable")))?;
        let identity = entry.identity();
        let runtime_name = entry
            .runtime
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("invalid plugin runtime"))?;
        let url = tauri::Url::parse(&format!("uipilot-plugin://localhost/{runtime_name}"))
            .map_err(|error| io::Error::other(error.to_string()))?;
        let data_directory = runtime_data_directory(&config.app_data_dir, &identity);
        let ready_manager = Arc::clone(self);
        let identity_for_ready = identity.clone();
        let failed_manager = Arc::clone(self);
        let identity_for_failure = identity.clone();
        let failure_app = app.clone();
        let window = WebviewWindowBuilder::new(
            app,
            entry.window_label.clone(),
            WebviewUrl::CustomProtocol(url),
        )
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
                ready_manager.runtime_ready(&identity_for_ready);
            }
        })
        .build()
        .map_err(|error| io::Error::other(error.to_string()))?;
        attach_process_failed_handler(&window, move || {
            let registry = failure_app.state::<ResultRegistry>();
            failed_manager.runtime_failed(&identity_for_failure, &registry);
        })?;
        let destroyed_manager = Arc::clone(self);
        let identity_for_destroyed = identity;
        let destroyed_app = app.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let registry = destroyed_app.state::<ResultRegistry>();
                destroyed_manager.runtime_failed(&identity_for_destroyed, &registry);
            }
        });
        Ok(window)
    }

    pub(crate) fn reload_plugin(
        self: &Arc<Self>,
        app: &AppHandle,
        registry: &ResultRegistry,
        plugin_id: &str,
    ) -> Result<PluginView, PluginManagementError> {
        if !valid_id(plugin_id) {
            return Err(PluginManagementError::Unavailable);
        }
        let _mutation = self
            .mutation
            .lock()
            .map_err(|_| PluginManagementError::Unavailable)?;
        let config = self
            .config
            .get()
            .cloned()
            .ok_or(PluginManagementError::Unavailable)?;
        let old = {
            let _admission = self
                .admission
                .read()
                .map_err(|_| PluginManagementError::Unavailable)?;
            self.state
                .get()
                .and_then(|state| {
                    state
                        .read()
                        .ok()?
                        .active
                        .entries
                        .iter()
                        .find(|entry| entry.id == plugin_id)
                        .cloned()
                })
                .ok_or(PluginManagementError::Unavailable)?
        };
        let mut candidate = load_entry(&old.root, config.host_version)
            .filter(|entry| entry.id == plugin_id)
            .ok_or(PluginManagementError::Unavailable)?;
        let attempt = Arc::new(RuntimeAttempt::default());
        let identity = {
            let _admission = self
                .admission
                .write()
                .map_err(|_| PluginManagementError::Unavailable)?;
            let mut state = self
                .state
                .get()
                .ok_or(PluginManagementError::Unavailable)?
                .write()
                .map_err(|_| PluginManagementError::Unavailable)?;
            let current = state
                .active
                .entries
                .iter()
                .find(|entry| entry.id == plugin_id)
                .filter(|entry| entry.identity() == old.identity())
                .ok_or(PluginManagementError::Unavailable)?;
            if state.active.entries.iter().any(|entry| {
                entry.id != plugin_id
                    && (entry.id == candidate.id
                        || entry.feature.trigger == candidate.feature.trigger)
            }) {
                return Err(PluginManagementError::Unavailable);
            }
            let generation = state
                .latest_generations
                .get(plugin_id)
                .copied()
                .unwrap_or(current.generation)
                .checked_add(1)
                .ok_or(PluginManagementError::Unavailable)?;
            candidate.generation = generation;
            candidate.window_label = window_label(plugin_id, generation);
            let identity = candidate.identity();
            state
                .latest_generations
                .insert(plugin_id.to_string(), generation);
            state
                .staged_assets
                .insert(identity.clone(), candidate.clone());
            state.ownership.insert(
                identity.clone(),
                RuntimeOwnership {
                    slot: RuntimeSlot::Staged,
                    attempt: Arc::clone(&attempt),
                },
            );
            identity
        };

        let staged_window = match self.create_runtime_window(app, &candidate) {
            Ok(window) => window,
            Err(_) => {
                self.rollback_staged(app, &config, &identity);
                return Err(PluginManagementError::Unavailable);
            }
        };
        let settled = attempt.wait_until_settled(PLUGIN_RUNTIME_READY_TIMEOUT);
        if !settled.is_some_and(|state| state.ready && !state.failed) {
            self.rollback_staged(app, &config, &identity);
            return Err(PluginManagementError::Unavailable);
        }

        let old_identity = old.identity();
        let committed = {
            let _admission = self
                .admission
                .write()
                .map_err(|_| PluginManagementError::Unavailable)?;
            if app.get_webview_window(&identity.window_label).is_none() {
                false
            } else {
                let mut state = self
                    .state
                    .get()
                    .ok_or(PluginManagementError::Unavailable)?
                    .write()
                    .map_err(|_| PluginManagementError::Unavailable)?;
                let staged_asset_matches = state
                    .staged_assets
                    .get(&identity)
                    .is_some_and(|entry| entry.identity() == identity);
                let staged_owner_matches = state.ownership.get(&identity).is_some_and(|owner| {
                    owner.slot == RuntimeSlot::Staged
                        && Arc::ptr_eq(&owner.attempt, &attempt)
                        && owner
                            .attempt
                            .snapshot()
                            .is_some_and(|signal| signal.ready && !signal.failed)
                });
                let old_index =
                    state.active.entries.iter().position(|entry| {
                        entry.id == plugin_id && entry.identity() == old_identity
                    });
                if !staged_asset_matches || !staged_owner_matches {
                    false
                } else if let Some(old_index) = old_index {
                    state.active.entries[old_index] = candidate.clone();
                    state.staged_assets.remove(&identity);
                    state.ownership.remove(&old_identity);
                    state.ownership.insert(
                        identity.clone(),
                        RuntimeOwnership {
                            slot: RuntimeSlot::Active,
                            attempt: Arc::clone(&attempt),
                        },
                    );
                    drop(state);
                    if let Ok(mut pending) = self.pending.write() {
                        pending.retain(|_, query| {
                            if query.plugin_id == old_identity.plugin_id
                                && query.generation == old_identity.generation
                            {
                                let _ = query.sender.send(Err(PluginQueryError::RuntimeDisabled));
                                false
                            } else {
                                true
                            }
                        });
                    }
                    if let Ok(mut disabled) = self.disabled.write() {
                        disabled.remove(&identity.window_label);
                    }
                    let _ = registry.invalidate_domain(QueryDomain::Plugin);
                    true
                } else {
                    false
                }
            }
        };
        if !committed {
            drop(staged_window);
            self.rollback_staged(app, &config, &identity);
            return Err(PluginManagementError::Unavailable);
        }

        if let Some(window) = app.get_webview_window(&old_identity.window_label) {
            let _ = window.close();
        }
        let _ = fs::remove_dir_all(runtime_data_directory(&config.app_data_dir, &old_identity));
        drop(staged_window);
        Ok(plugin_view(&candidate))
    }

    pub(crate) fn delete_plugin(
        self: &Arc<Self>,
        app: &AppHandle,
        registry: &ResultRegistry,
        plugin_id: &str,
    ) -> Result<(), PluginManagementError> {
        if !valid_id(plugin_id) {
            return Err(PluginManagementError::Unavailable);
        }
        #[cfg(not(windows))]
        {
            let _ = (app, registry);
            return Err(PluginManagementError::Unavailable);
        }
        #[cfg(windows)]
        {
            let _mutation = self
                .mutation
                .lock()
                .map_err(|_| PluginManagementError::Unavailable)?;
            let config = self
                .config
                .get()
                .cloned()
                .ok_or(PluginManagementError::Unavailable)?;
            let active = {
                let _admission = self
                    .admission
                    .read()
                    .map_err(|_| PluginManagementError::Unavailable)?;
                self.state
                    .get()
                    .and_then(|state| {
                        state
                            .read()
                            .ok()?
                            .active
                            .entries
                            .iter()
                            .find(|entry| entry.id == plugin_id)
                            .cloned()
                    })
                    .ok_or(PluginManagementError::Unavailable)?
            };
            if active.root.parent() != Some(config.plugin_root.as_path()) {
                return Err(PluginManagementError::Unavailable);
            }
            let (package_handle, current_identity) = open_directory_handle(&active.root, true)
                .map_err(|_| PluginManagementError::Unavailable)?;
            if current_identity != active.package_identity {
                return Err(PluginManagementError::Unavailable);
            }
            let (_, quarantine_identity) = open_directory_handle(&config.quarantine_root, false)
                .map_err(|_| PluginManagementError::Unavailable)?;
            if quarantine_identity.volume != current_identity.volume {
                return Err(PluginManagementError::Unavailable);
            }
            let sequence = self.next_quarantine.fetch_add(1, Ordering::Relaxed);
            let quarantine_path = config.quarantine_root.join(format!(
                "removed-{}-{:016x}-{:016x}-{:08x}",
                plugin_id,
                active.generation,
                sequence,
                std::process::id()
            ));
            let identity = active.identity();
            {
                let _admission = self
                    .admission
                    .write()
                    .map_err(|_| PluginManagementError::Unavailable)?;
                let state = self
                    .state
                    .get()
                    .ok_or(PluginManagementError::Unavailable)?
                    .read()
                    .map_err(|_| PluginManagementError::Unavailable)?;
                let current_entry = state.active.entries.iter().any(|entry| {
                    entry.id == plugin_id
                        && entry.identity() == identity
                        && entry.package_identity == current_identity
                });
                if !current_entry {
                    return Err(PluginManagementError::Unavailable);
                }
                drop(state);
                move_directory_handle(&package_handle, &quarantine_path)
                    .map_err(|_| PluginManagementError::Unavailable)?;
                let mut state = self
                    .state
                    .get()
                    .expect("plugin state disappeared during delete")
                    .write()
                    .expect("plugin state poisoned during delete");
                let active_index = state
                    .active
                    .entries
                    .iter()
                    .position(|entry| entry.id == plugin_id && entry.identity() == identity)
                    .expect("active plugin changed while delete held admission");
                state.active.entries.remove(active_index);
                state.ownership.remove(&identity);
                state.latest_generations.remove(plugin_id);
                drop(state);
                if let Ok(mut pending) = self.pending.write() {
                    pending.retain(|_, query| {
                        if query.plugin_id == plugin_id {
                            let _ = query.sender.send(Err(PluginQueryError::RuntimeDisabled));
                            false
                        } else {
                            true
                        }
                    });
                }
                if let Ok(mut disabled) = self.disabled.write() {
                    disabled.remove(&identity.window_label);
                }
                let _ = registry.invalidate_domain(QueryDomain::Plugin);
            }
            drop(package_handle);
            if let Some(window) = app.get_webview_window(&identity.window_label) {
                let _ = window.close();
            }
            let _ = fs::remove_dir_all(runtime_data_directory(&config.app_data_dir, &identity));
            let _ = fs::remove_dir_all(quarantine_path);
            Ok(())
        }
    }

    fn rollback_staged(
        &self,
        app: &AppHandle,
        config: &PluginManagerConfig,
        identity: &RuntimeIdentity,
    ) {
        if let Ok(_admission) = self.admission.write() {
            if let Some(state) = self.state.get() {
                if let Ok(mut state) = state.write() {
                    state.staged_assets.remove(identity);
                    if state
                        .ownership
                        .get(identity)
                        .is_some_and(|owner| owner.slot == RuntimeSlot::Staged)
                    {
                        state.ownership.remove(identity);
                    }
                }
            }
        }
        if let Some(window) = app.get_webview_window(&identity.window_label) {
            let _ = window.close();
        }
        let _ = fs::remove_dir_all(runtime_data_directory(&config.app_data_dir, identity));
    }

    fn runtime_ready(&self, identity: &RuntimeIdentity) {
        let Ok(_admission) = self.admission.read() else {
            return;
        };
        let attempt = self.state.get().and_then(|state| {
            state
                .read()
                .ok()?
                .ownership
                .get(identity)
                .map(|ownership| Arc::clone(&ownership.attempt))
        });
        if let Some(attempt) = attempt {
            attempt.mark_ready();
        }
    }

    fn runtime_failed(&self, identity: &RuntimeIdentity, registry: &ResultRegistry) {
        let Ok(_admission) = self.admission.write() else {
            return;
        };
        let ownership = self
            .state
            .get()
            .and_then(|state| state.read().ok()?.ownership.get(identity).cloned());
        let Some(ownership) = ownership else {
            return;
        };
        if !ownership.attempt.mark_failed() || ownership.slot == RuntimeSlot::Staged {
            return;
        }
        self.disable_runtime(identity);
        let _ = registry.invalidate_domain(QueryDomain::Plugin);
    }

    fn disable_runtime(&self, identity: &RuntimeIdentity) {
        if let Ok(mut disabled) = self.disabled.write() {
            disabled.insert(identity.window_label.clone());
        }
        if let Some(attempt) = self.state.get().and_then(|state| {
            state
                .read()
                .ok()?
                .ownership
                .get(identity)
                .map(|ownership| Arc::clone(&ownership.attempt))
        }) {
            attempt.changed.notify_all();
        }
        if let Ok(mut pending) = self.pending.write() {
            pending.retain(|_, query| {
                if query.window_label == identity.window_label
                    && query.generation == identity.generation
                {
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
        let attempt = {
            let _admission = self
                .admission
                .read()
                .map_err(|_| PluginQueryError::RuntimeDisabled)?;
            self.state
                .get()
                .and_then(|state| {
                    let state = state.read().ok()?;
                    if !state
                        .active
                        .entries
                        .iter()
                        .any(|entry| route_matches(entry, &route))
                    {
                        return None;
                    }
                    state
                        .ownership
                        .get(&route.identity())
                        .filter(|ownership| ownership.slot == RuntimeSlot::Active)
                        .map(|ownership| Arc::clone(&ownership.attempt))
                })
                .ok_or(PluginQueryError::RuntimeDisabled)?
        };
        let disabled = Arc::clone(&self.disabled);
        let label = route.window_label.clone();
        let is_ready = tauri::async_runtime::spawn_blocking(move || {
            wait_until_ready(attempt, disabled, label)
        })
        .await
        .map_err(|_| PluginQueryError::RuntimeDisabled)??;
        if !is_ready {
            return Ok(Vec::new());
        }
        let request_id = self.allocate_request_id();
        let (sender, receiver) = mpsc::channel();
        {
            let _admission = self
                .admission
                .read()
                .map_err(|_| PluginQueryError::RuntimeDisabled)?;
            let current = self
                .state
                .get()
                .and_then(|state| {
                    state
                        .read()
                        .ok()?
                        .active
                        .entries
                        .iter()
                        .any(|entry| route_matches(entry, &route))
                        .then_some(())
                })
                .is_some();
            if !current {
                return Err(PluginQueryError::RuntimeDisabled);
            }
            self.pending
                .write()
                .map_err(|_| PluginQueryError::RuntimeDisabled)?
                .insert(
                    request_id.clone(),
                    PendingPluginQuery {
                        plugin_id: route.plugin_id.clone(),
                        window_label: route.window_label.clone(),
                        generation: route.generation,
                        sender,
                    },
                );
        }
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
        let _admission = self
            .admission
            .read()
            .map_err(|_| PluginQueryError::RuntimeDisabled)?;
        let entry = self
            .state
            .get()
            .and_then(|state| {
                state
                    .read()
                    .ok()?
                    .active
                    .entries
                    .iter()
                    .find(|entry| entry.window_label == label)
                    .cloned()
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
        if pending.plugin_id != entry.id
            || pending.window_label != label
            || pending.generation != entry.generation
        {
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
                    let disabled = self
                        .disabled
                        .read()
                        .map_err(|_| PluginQueryError::RuntimeDisabled)?
                        .contains(label);
                    if text.len() > 4096
                        || disabled
                        || !entry
                            .permissions
                            .iter()
                            .any(|permission| permission == "clipboard.writeText")
                    {
                        let _ = pending.sender.send(Err(PluginQueryError::InvalidResponse));
                        return Err(PluginQueryError::InvalidResponse);
                    }
                    ResultAction::CopyText {
                        plugin_id: entry.id.clone(),
                        generation: entry.generation,
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
        let should_disable = if let Ok(mut timeouts) = self.timeouts.write() {
            let count = timeouts.entry(label.to_string()).or_default();
            *count = count.saturating_add(1);
            *count >= 3
        } else {
            false
        };
        if should_disable {
            let identity = self.state.get().and_then(|state| {
                state
                    .read()
                    .ok()?
                    .active
                    .entries
                    .iter()
                    .find(|entry| entry.window_label == label)
                    .map(PluginCatalogEntry::identity)
            });
            if let Some(identity) = identity {
                self.disable_runtime(&identity);
            }
        }
    }

    fn allocate_request_id(&self) -> String {
        let previous = self.next_request.fetch_add(1, Ordering::Relaxed);
        format!("plugin-request-{:016x}", previous + 1)
    }
}

fn wait_until_ready(
    attempt: Arc<RuntimeAttempt>,
    disabled: Arc<RwLock<HashSet<String>>>,
    label: String,
) -> Result<bool, PluginQueryError> {
    let state = attempt
        .state
        .lock()
        .map_err(|_| PluginQueryError::RuntimeDisabled)?;
    let (state, _) = attempt
        .changed
        .wait_timeout_while(state, Duration::from_millis(500), |state| {
            !state.ready
                && !state.failed
                && disabled
                    .read()
                    .is_ok_and(|disabled| !disabled.contains(&label))
        })
        .map_err(|_| PluginQueryError::RuntimeDisabled)?;
    if state.failed
        || disabled
            .read()
            .map_err(|_| PluginQueryError::RuntimeDisabled)?
            .contains(&label)
    {
        Err(PluginQueryError::RuntimeDisabled)
    } else {
        Ok(state.ready)
    }
}

struct PendingPluginQuery {
    plugin_id: String,
    window_label: String,
    generation: u64,
    sender: mpsc::Sender<Result<Vec<(ResultItem, ResultAction)>, PluginQueryError>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PluginQueryError {
    Timeout,
    RuntimeDisabled,
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginManagementError {
    Unavailable,
}

pub(crate) enum PluginQueryStart {
    NoRoute,
    Rejected,
    Started {
        route: PluginRoute,
        token: QueryToken,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginCopyError {
    PermissionDenied,
    SideEffectFailed,
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
    pub(crate) description: Option<String>,
    pub(crate) generation: u64,
    package_identity: DirectoryIdentity,
    snapshot: Arc<GenerationAssetSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    volume: u64,
    file: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageIdentityV1 {
    algorithm: String,
    digest: String,
    volume_serial: u64,
    file_id: String,
}

#[derive(Clone)]
struct GenerationAssetSnapshot {
    package_identity: PackageIdentityV1,
    assets: HashMap<PathBuf, Vec<u8>>,
    total_bytes: u64,
}

#[derive(Debug)]
struct PackageScanError;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageRecordV1 {
    version: String,
    identity: PackageIdentityV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActivePluginStateV1 {
    schema: u32,
    plugin_id: String,
    active_version: Option<String>,
    packages: Vec<PackageRecordV1>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PluginTransactionOperation {
    Install,
    Update,
    DeleteWithFallback,
    DeleteLast,
    LegacyMigration,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PluginTransactionPhase {
    Prepared,
    PackagePlaced,
    StateCommitted,
    CleanupTransferred,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CleanupCondition {
    IfOldState,
    IfNewState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CleanupObjectRole {
    CandidatePackage,
    CandidateRuntimeData,
    PreviousRuntimeData,
    DeletedPackage,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CleanupOperation {
    RollbackStaging,
    DeleteVersion,
    DeleteLastVersion,
    RuntimeData,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TransactionRoot {
    PluginRoot,
    TransactionRoot,
    RuntimeDataRoot,
    QuarantineRoot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransactionObjectLocation {
    root: TransactionRoot,
    relative_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StableObjectIdentityV1 {
    volume_serial: u64,
    file_id: String,
    package_digest: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MovableObjectRole {
    CandidatePackage,
    LegacyPackage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MovableTransactionObjectV1 {
    role: MovableObjectRole,
    identity: StableObjectIdentityV1,
    allowed_locations: Vec<TransactionObjectLocation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FixedObjectRole {
    ActivationPackage,
    CandidateRuntimeData,
    PreviousRuntimeData,
    DeletedPackage,
    FallbackPackage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixedTransactionObjectV1 {
    role: FixedObjectRole,
    identity: StableObjectIdentityV1,
    location: TransactionObjectLocation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum InstallMode {
    NewVersion,
    ActivateExisting,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum TransactionObjectsV1 {
    Install {
        #[serde(rename = "commandOperation")]
        command_operation: PluginTransactionOperation,
        mode: InstallMode,
        #[serde(rename = "candidatePackage")]
        candidate_package: MovableTransactionObjectV1,
        #[serde(rename = "activationPackage")]
        activation_package: Option<FixedTransactionObjectV1>,
        #[serde(rename = "candidateRuntimeData")]
        candidate_runtime_data: FixedTransactionObjectV1,
        #[serde(rename = "previousRuntimeData")]
        previous_runtime_data: Option<FixedTransactionObjectV1>,
    },
    DeleteWithFallback {
        #[serde(rename = "deletedPackage")]
        deleted_package: FixedTransactionObjectV1,
        #[serde(rename = "fallbackPackage")]
        fallback_package: FixedTransactionObjectV1,
        #[serde(rename = "candidateRuntimeData")]
        candidate_runtime_data: FixedTransactionObjectV1,
        #[serde(rename = "previousRuntimeData")]
        previous_runtime_data: Option<FixedTransactionObjectV1>,
    },
    DeleteLast {
        #[serde(rename = "deletedPackage")]
        deleted_package: FixedTransactionObjectV1,
        #[serde(rename = "previousRuntimeData")]
        previous_runtime_data: Option<FixedTransactionObjectV1>,
    },
    LegacyMigration {
        #[serde(rename = "legacyPackage")]
        legacy_package: MovableTransactionObjectV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum CleanupMeasureV1 {
    Exact {
        bytes: u64,
    },
    Bounded {
        #[serde(rename = "maxBytes")]
        max_bytes: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupReceiptPlanV1 {
    receipt_id: String,
    condition: CleanupCondition,
    object_role: CleanupObjectRole,
    operation: CleanupOperation,
    planned_target: TransactionObjectLocation,
    measure: CleanupMeasureV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DurableStateKind {
    Absent,
    ActiveStateV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DurableStateReference {
    kind: DurableStateKind,
    sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginTransactionV1 {
    schema: u32,
    transaction_id: String,
    operation: PluginTransactionOperation,
    plugin_id: String,
    phase: PluginTransactionPhase,
    old_state: DurableStateReference,
    new_state: DurableStateReference,
    objects: TransactionObjectsV1,
    cleanup_plans: Vec<CleanupReceiptPlanV1>,
    cleanup_receipt_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CleanupReceiptPhase {
    Pending,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TransactionObjectIdentityRole {
    LegacySource,
    StagedPackage,
    InstalledPackage,
    DeletedPackage,
    RuntimeData,
    QuarantineTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransactionObjectIdentity {
    role: TransactionObjectIdentityRole,
    root: TransactionRoot,
    relative_path: String,
    volume_serial: u64,
    file_id: String,
    package_digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupReceiptV1 {
    schema: u32,
    receipt_id: String,
    origin_operation_id: String,
    plugin_id: String,
    operation: CleanupOperation,
    phase: CleanupReceiptPhase,
    source: TransactionObjectIdentity,
    planned_target: TransactionObjectLocation,
    target: Option<TransactionObjectIdentity>,
    measure: CleanupMeasureV1,
}

impl PluginCatalogEntry {
    fn identity(&self) -> RuntimeIdentity {
        RuntimeIdentity {
            plugin_id: self.id.clone(),
            window_label: self.window_label.clone(),
            generation: self.generation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginView {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) trigger: String,
    pub(crate) description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginInventorySnapshot {
    pub(crate) revision: String,
    pub(crate) items: Vec<PluginInventoryView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginInventoryView {
    pub(crate) key: String,
    pub(crate) id: Option<String>,
    pub(crate) display_name: String,
    pub(crate) installed: InstalledPluginView,
    pub(crate) development: DevelopmentPluginView,
    pub(crate) description: PluginDescriptionView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum InstalledPluginView {
    Absent,
    Valid {
        #[serde(rename = "activeVersion")]
        active_version: String,
        versions: Vec<String>,
        trigger: String,
    },
    Invalid {
        issue: &'static str,
        #[serde(rename = "activeVersion")]
        active_version: Option<String>,
        versions: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum DevelopmentPluginView {
    Absent,
    Valid { version: String, trigger: String },
    Invalid { reason: &'static str },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum PluginDescriptionView {
    Available {
        source: PluginDescriptionSource,
        markdown: String,
    },
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PluginDescriptionSource {
    Installed,
    Development,
}

struct InventoryRowBuilder {
    id: String,
    installed: InstalledPluginView,
    development: DevelopmentPluginView,
    installed_description: Option<String>,
    development_description: Option<String>,
    active_version: Option<Version>,
    development_version: Option<Version>,
}

#[derive(Clone)]
pub(crate) struct PluginFeature {
    pub(crate) trigger: String,
}

#[derive(Clone)]
pub(crate) struct PluginRoute {
    pub(crate) plugin_id: String,
    pub(crate) window_label: String,
    pub(crate) generation: u64,
    pub(crate) input: String,
}

impl PluginRoute {
    fn identity(&self) -> RuntimeIdentity {
        RuntimeIdentity {
            plugin_id: self.plugin_id.clone(),
            window_label: self.window_label.clone(),
            generation: self.generation,
        }
    }
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
        (parts.next().is_none() && version.to_path_segment() == text).then_some(version)
    }

    fn to_path_segment(self) -> String {
        format!("{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

fn parse_active_state(bytes: &[u8], expected_plugin_id: &str) -> Result<ActivePluginStateV1, ()> {
    let state: ActivePluginStateV1 = serde_json::from_slice(bytes).map_err(|_| ())?;
    if state.schema != 1
        || state.plugin_id != expected_plugin_id
        || !valid_id(&state.plugin_id)
        || state.packages.len() > 32
    {
        return Err(());
    }
    let mut previous = None;
    for package in &state.packages {
        let version = Version::parse(&package.version).ok_or(())?;
        if previous.is_some_and(|previous| previous >= version)
            || package.identity.algorithm != "sha256-tree-v1"
            || !valid_lower_hex(&package.identity.digest, 64)
            || !valid_lower_hex(&package.identity.file_id, 16)
        {
            return Err(());
        }
        previous = Some(version);
    }
    match (&state.active_version, state.packages.is_empty()) {
        (None, true) => {}
        (Some(active), false)
            if Version::parse(active).is_some()
                && state
                    .packages
                    .iter()
                    .any(|package| &package.version == active) => {}
        _ => return Err(()),
    }
    Ok(state)
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_plugin_transaction(bytes: &[u8]) -> Result<PluginTransactionV1, ()> {
    let transaction: PluginTransactionV1 = serde_json::from_slice(bytes).map_err(|_| ())?;
    validate_plugin_transaction(&transaction)?;
    Ok(transaction)
}

fn validate_plugin_transaction(transaction: &PluginTransactionV1) -> Result<(), ()> {
    if transaction.schema != 1
        || !valid_lower_hex(&transaction.transaction_id, 32)
        || !valid_id(&transaction.plugin_id)
        || transaction.cleanup_plans.len() > 8
        || !valid_state_reference(&transaction.old_state)
        || !valid_state_reference(&transaction.new_state)
    {
        return Err(());
    }
    let mut previous_id: Option<&str> = None;
    for plan in &transaction.cleanup_plans {
        if previous_id.is_some_and(|previous| previous >= plan.receipt_id.as_str())
            || !valid_cleanup_plan(plan)
        {
            return Err(());
        }
        previous_id = Some(&plan.receipt_id);
    }
    let plan_ids = transaction
        .cleanup_plans
        .iter()
        .map(|plan| plan.receipt_id.as_str())
        .collect::<Vec<_>>();
    match transaction.phase {
        PluginTransactionPhase::Prepared
        | PluginTransactionPhase::PackagePlaced
        | PluginTransactionPhase::StateCommitted
            if !transaction.cleanup_receipt_ids.is_empty() =>
        {
            return Err(())
        }
        PluginTransactionPhase::CleanupTransferred
            if transaction
                .cleanup_receipt_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                != plan_ids =>
        {
            return Err(())
        }
        _ => {}
    }
    validate_transaction_objects(transaction)?;
    validate_cleanup_coverage(transaction)
}

fn valid_state_reference(reference: &DurableStateReference) -> bool {
    match reference.kind {
        DurableStateKind::Absent => reference.sha256.is_none(),
        DurableStateKind::ActiveStateV1 => reference
            .sha256
            .as_deref()
            .is_some_and(|digest| valid_lower_hex(digest, 64)),
    }
}

fn valid_cleanup_plan(plan: &CleanupReceiptPlanV1) -> bool {
    if !valid_lower_hex(&plan.receipt_id, 32)
        || !valid_planned_target(&plan.planned_target, &plan.receipt_id)
    {
        return false;
    }
    matches!(
        (plan.object_role, &plan.measure),
        (
            CleanupObjectRole::CandidatePackage,
            CleanupMeasureV1::Exact { .. }
        ) | (
            CleanupObjectRole::DeletedPackage,
            CleanupMeasureV1::Exact { .. }
        ) | (
            CleanupObjectRole::CandidateRuntimeData,
            CleanupMeasureV1::Bounded { .. }
        ) | (
            CleanupObjectRole::PreviousRuntimeData,
            CleanupMeasureV1::Bounded { .. }
        )
    )
}

fn validate_transaction_objects(transaction: &PluginTransactionV1) -> Result<(), ()> {
    match (&transaction.operation, &transaction.objects) {
        (
            PluginTransactionOperation::Install | PluginTransactionOperation::Update,
            TransactionObjectsV1::Install {
                command_operation,
                mode,
                candidate_package,
                activation_package,
                candidate_runtime_data,
                previous_runtime_data,
            },
        ) => {
            if command_operation != &transaction.operation
                || candidate_package.role != MovableObjectRole::CandidatePackage
                || candidate_runtime_data.role != FixedObjectRole::CandidateRuntimeData
                || previous_runtime_data
                    .as_ref()
                    .is_some_and(|object| object.role != FixedObjectRole::PreviousRuntimeData)
                || !valid_stable_identity(&candidate_package.identity, true)
                || !valid_fixed_object(candidate_runtime_data, false)
                || previous_runtime_data
                    .as_ref()
                    .is_some_and(|object| !valid_fixed_object(object, false))
            {
                return Err(());
            }
            match mode {
                InstallMode::NewVersion if activation_package.is_none() => {}
                InstallMode::ActivateExisting
                    if activation_package.as_ref().is_some_and(|object| {
                        object.role == FixedObjectRole::ActivationPackage
                            && valid_fixed_object(object, true)
                    }) => {}
                _ => return Err(()),
            }
        }
        (
            PluginTransactionOperation::DeleteWithFallback,
            TransactionObjectsV1::DeleteWithFallback {
                deleted_package,
                fallback_package,
                candidate_runtime_data,
                previous_runtime_data,
            },
        ) => {
            if deleted_package.role != FixedObjectRole::DeletedPackage
                || fallback_package.role != FixedObjectRole::FallbackPackage
                || candidate_runtime_data.role != FixedObjectRole::CandidateRuntimeData
                || !valid_fixed_object(deleted_package, true)
                || !valid_fixed_object(fallback_package, true)
                || !valid_fixed_object(candidate_runtime_data, false)
                || previous_runtime_data.as_ref().is_some_and(|object| {
                    object.role != FixedObjectRole::PreviousRuntimeData
                        || !valid_fixed_object(object, false)
                })
            {
                return Err(());
            }
        }
        (
            PluginTransactionOperation::DeleteLast,
            TransactionObjectsV1::DeleteLast {
                deleted_package,
                previous_runtime_data,
            },
        ) => {
            if deleted_package.role != FixedObjectRole::DeletedPackage
                || !valid_fixed_object(deleted_package, true)
                || previous_runtime_data.as_ref().is_some_and(|object| {
                    object.role != FixedObjectRole::PreviousRuntimeData
                        || !valid_fixed_object(object, false)
                })
            {
                return Err(());
            }
        }
        (
            PluginTransactionOperation::LegacyMigration,
            TransactionObjectsV1::LegacyMigration { legacy_package },
        ) if legacy_package.role == MovableObjectRole::LegacyPackage
            && valid_stable_identity(&legacy_package.identity, true) => {}
        _ => return Err(()),
    }
    Ok(())
}

fn valid_fixed_object(object: &FixedTransactionObjectV1, package: bool) -> bool {
    valid_stable_identity(&object.identity, package) && valid_object_location(&object.location)
}

fn valid_stable_identity(identity: &StableObjectIdentityV1, package: bool) -> bool {
    valid_lower_hex(&identity.file_id, 16)
        && match (package, identity.package_digest.as_deref()) {
            (true, Some(digest)) => valid_lower_hex(digest, 64),
            (false, None) => true,
            _ => false,
        }
}

fn valid_object_location(location: &TransactionObjectLocation) -> bool {
    !location.relative_path.is_empty()
        && !location.relative_path.contains('\\')
        && !location.relative_path.starts_with('/')
        && location
            .relative_path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

fn valid_planned_target(location: &TransactionObjectLocation, receipt_id: &str) -> bool {
    location.root == TransactionRoot::QuarantineRoot && location.relative_path == receipt_id
}

fn validate_cleanup_coverage(transaction: &PluginTransactionV1) -> Result<(), ()> {
    let actual = transaction
        .cleanup_plans
        .iter()
        .map(|plan| (plan.condition, plan.object_role))
        .collect::<Vec<_>>();
    let mut expected = Vec::new();
    match &transaction.objects {
        TransactionObjectsV1::Install {
            mode,
            previous_runtime_data,
            ..
        } => {
            expected.extend([
                (
                    CleanupCondition::IfOldState,
                    CleanupObjectRole::CandidatePackage,
                ),
                (
                    CleanupCondition::IfOldState,
                    CleanupObjectRole::CandidateRuntimeData,
                ),
            ]);
            if *mode == InstallMode::ActivateExisting {
                expected.push((
                    CleanupCondition::IfNewState,
                    CleanupObjectRole::CandidatePackage,
                ));
            }
            if previous_runtime_data.is_some() {
                expected.push((
                    CleanupCondition::IfNewState,
                    CleanupObjectRole::PreviousRuntimeData,
                ));
            }
        }
        TransactionObjectsV1::DeleteWithFallback {
            previous_runtime_data,
            ..
        } => {
            expected.push((
                CleanupCondition::IfOldState,
                CleanupObjectRole::CandidateRuntimeData,
            ));
            expected.push((
                CleanupCondition::IfNewState,
                CleanupObjectRole::DeletedPackage,
            ));
            if previous_runtime_data.is_some() {
                expected.push((
                    CleanupCondition::IfNewState,
                    CleanupObjectRole::PreviousRuntimeData,
                ));
            }
        }
        TransactionObjectsV1::DeleteLast {
            previous_runtime_data,
            ..
        } => {
            expected.push((
                CleanupCondition::IfNewState,
                CleanupObjectRole::DeletedPackage,
            ));
            if previous_runtime_data.is_some() {
                expected.push((
                    CleanupCondition::IfNewState,
                    CleanupObjectRole::PreviousRuntimeData,
                ));
            }
        }
        TransactionObjectsV1::LegacyMigration { .. } => {}
    }
    let mut actual = actual;
    actual.sort_by_key(|value| (value.0 as u8, value.1 as u8));
    expected.sort_by_key(|value| (value.0 as u8, value.1 as u8));
    (actual == expected).then_some(()).ok_or(())
}

fn parse_cleanup_receipt(bytes: &[u8]) -> Result<CleanupReceiptV1, ()> {
    let receipt: CleanupReceiptV1 = serde_json::from_slice(bytes).map_err(|_| ())?;
    if receipt.schema != 1
        || !valid_lower_hex(&receipt.receipt_id, 32)
        || !valid_lower_hex(&receipt.origin_operation_id, 32)
        || !valid_id(&receipt.plugin_id)
        || !valid_planned_target(&receipt.planned_target, &receipt.receipt_id)
        || !valid_transaction_identity(&receipt.source)
    {
        return Err(());
    }
    match (&receipt.phase, &receipt.target) {
        (CleanupReceiptPhase::Pending, None) => {}
        (CleanupReceiptPhase::Quarantined, Some(target))
            if target.root == TransactionRoot::QuarantineRoot
                && target.relative_path == receipt.planned_target.relative_path
                && target.role == TransactionObjectIdentityRole::QuarantineTarget
                && valid_transaction_identity(target) => {}
        _ => return Err(()),
    }
    let package = receipt.source.package_digest.is_some();
    if !matches!(
        (package, &receipt.measure),
        (true, CleanupMeasureV1::Exact { .. }) | (false, CleanupMeasureV1::Bounded { .. })
    ) {
        return Err(());
    }
    Ok(receipt)
}

fn valid_transaction_identity(identity: &TransactionObjectIdentity) -> bool {
    valid_object_location(&TransactionObjectLocation {
        root: identity.root,
        relative_path: identity.relative_path.clone(),
    }) && valid_lower_hex(&identity.file_id, 16)
        && identity
            .package_digest
            .as_deref()
            .is_none_or(|digest| valid_lower_hex(digest, 64))
}

fn receipt_worker_eligible(
    receipt: &CleanupReceiptV1,
    active_transaction: Option<&PluginTransactionV1>,
) -> bool {
    !active_transaction
        .is_some_and(|transaction| receipt.origin_operation_id == transaction.transaction_id)
}

fn run_cleanup_worker(app_data_dir: &Path) -> Result<(), PluginManagementError> {
    let transaction_root = app_data_dir.join("plugin-transactions");
    let active_transaction = read_active_transaction(&transaction_root)?;
    let receipts_root = transaction_root.join("receipts");
    let mut receipts = read_cleanup_receipt_paths(&receipts_root)?;
    receipts.sort();

    let mut processed = 0usize;
    let mut processed_bytes = 0u64;
    for receipt_path in receipts {
        if processed == PLUGIN_CLEANUP_BATCH_RECEIPTS {
            break;
        }
        let receipt = read_cleanup_receipt(&receipt_path)?;
        if !receipt_worker_eligible(&receipt, active_transaction.as_ref()) {
            continue;
        }
        let source_path = cleanup_location_path(app_data_dir, &receipt.source_location())?;
        let target_path = cleanup_location_path(app_data_dir, &receipt.planned_target)?;
        let source_exists = ordinary_directory(&source_path);
        let target_exists = ordinary_directory(&target_path);
        let object_path = match (receipt.phase, source_exists, target_exists) {
            (CleanupReceiptPhase::Pending, true, false) => &source_path,
            (CleanupReceiptPhase::Pending, false, true)
            | (CleanupReceiptPhase::Quarantined, false, true) => &target_path,
            _ => return Err(PluginManagementError::Unavailable),
        };
        let actual_bytes = validate_cleanup_object(object_path, &receipt)?;
        if actual_bytes > PLUGIN_CLEANUP_BATCH_BYTES {
            return Err(PluginManagementError::Unavailable);
        }
        let next_bytes = processed_bytes
            .checked_add(actual_bytes)
            .ok_or(PluginManagementError::Unavailable)?;
        if next_bytes > PLUGIN_CLEANUP_BATCH_BYTES {
            break;
        }

        let mut receipt = receipt;
        if receipt.phase == CleanupReceiptPhase::Pending {
            if source_exists {
                move_cleanup_directory(&source_path, &target_path, &receipt.source)?;
            }
            let target_identity =
                directory_identity(&target_path).ok_or(PluginManagementError::Unavailable)?;
            if target_identity.volume != receipt.source.volume_serial
                || format!("{:016x}", target_identity.file) != receipt.source.file_id
            {
                return Err(PluginManagementError::Unavailable);
            }
            receipt.phase = CleanupReceiptPhase::Quarantined;
            receipt.target = Some(TransactionObjectIdentity {
                role: TransactionObjectIdentityRole::QuarantineTarget,
                root: TransactionRoot::QuarantineRoot,
                relative_path: receipt.planned_target.relative_path.clone(),
                volume_serial: target_identity.volume,
                file_id: format!("{:016x}", target_identity.file),
                package_digest: receipt.source.package_digest.clone(),
            });
            let bytes =
                serde_json::to_vec(&receipt).map_err(|_| PluginManagementError::Unavailable)?;
            if bytes.len() as u64 > PLUGIN_DURABLE_DOCUMENT_MAX_BYTES {
                return Err(PluginManagementError::Unavailable);
            }
            replace_current(&receipt_path, &bytes)
                .map_err(|_| PluginManagementError::Unavailable)?;
        }

        validate_cleanup_object(&target_path, &receipt)?;
        fs::remove_dir_all(&target_path).map_err(|_| PluginManagementError::Unavailable)?;
        fs::remove_file(&receipt_path).map_err(|_| PluginManagementError::Unavailable)?;
        processed += 1;
        processed_bytes = next_bytes;
    }
    Ok(())
}

impl CleanupReceiptV1 {
    fn source_location(&self) -> TransactionObjectLocation {
        TransactionObjectLocation {
            root: self.source.root,
            relative_path: self.source.relative_path.clone(),
        }
    }
}

fn read_active_transaction(
    transaction_root: &Path,
) -> Result<Option<PluginTransactionV1>, PluginManagementError> {
    let path = transaction_root.join("active").join("current.json");
    let Some(bytes) = read_bounded_document(&path)? else {
        return Ok(None);
    };
    parse_plugin_transaction(&bytes)
        .map(Some)
        .map_err(|_| PluginManagementError::Unavailable)
}

fn read_cleanup_receipt_paths(root: &Path) -> Result<Vec<PathBuf>, PluginManagementError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(PluginManagementError::Unavailable),
    };
    if !ordinary_directory(root) {
        return Err(PluginManagementError::Unavailable);
    }
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| PluginManagementError::Unavailable)?;
        let metadata = entry
            .metadata()
            .map_err(|_| PluginManagementError::Unavailable)?;
        let path = entry.path();
        let valid_name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| valid_lower_hex(name, 32));
        if !metadata.is_file()
            || is_reparse_point(&metadata)
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || !valid_name
        {
            return Err(PluginManagementError::Unavailable);
        }
        paths.push(path);
        if paths.len() > PLUGIN_CLEANUP_MAX_RECEIPTS {
            return Err(PluginManagementError::Unavailable);
        }
    }
    Ok(paths)
}

fn read_cleanup_receipt(path: &Path) -> Result<CleanupReceiptV1, PluginManagementError> {
    let bytes = read_bounded_document(path)?.ok_or(PluginManagementError::Unavailable)?;
    let receipt = parse_cleanup_receipt(&bytes).map_err(|_| PluginManagementError::Unavailable)?;
    let expected_name = format!("{}.json", receipt.receipt_id);
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err(PluginManagementError::Unavailable);
    }
    Ok(receipt)
}

fn read_bounded_document(path: &Path) -> Result<Option<Vec<u8>>, PluginManagementError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PluginManagementError::Unavailable),
    };
    if !metadata.is_file()
        || is_reparse_point(&metadata)
        || metadata.len() > PLUGIN_DURABLE_DOCUMENT_MAX_BYTES
    {
        return Err(PluginManagementError::Unavailable);
    }
    let bytes = fs::read(path).map_err(|_| PluginManagementError::Unavailable)?;
    if bytes.len() as u64 > PLUGIN_DURABLE_DOCUMENT_MAX_BYTES {
        return Err(PluginManagementError::Unavailable);
    }
    Ok(Some(bytes))
}

fn cleanup_location_path(
    app_data_dir: &Path,
    location: &TransactionObjectLocation,
) -> Result<PathBuf, PluginManagementError> {
    if !valid_object_location(location) {
        return Err(PluginManagementError::Unavailable);
    }
    let root = match location.root {
        TransactionRoot::PluginRoot => app_data_dir.join("plugins"),
        TransactionRoot::TransactionRoot => app_data_dir.join("plugin-transactions"),
        TransactionRoot::RuntimeDataRoot => app_data_dir.join("plugin-runtime-data"),
        TransactionRoot::QuarantineRoot => app_data_dir.join("plugin-quarantine"),
    };
    Ok(location
        .relative_path
        .split('/')
        .fold(root, |path, component| path.join(component)))
}

fn validate_cleanup_object(
    path: &Path,
    receipt: &CleanupReceiptV1,
) -> Result<u64, PluginManagementError> {
    let identity = directory_identity(path).ok_or(PluginManagementError::Unavailable)?;
    if identity.volume != receipt.source.volume_serial
        || format!("{:016x}", identity.file) != receipt.source.file_id
    {
        return Err(PluginManagementError::Unavailable);
    }
    match receipt.measure {
        CleanupMeasureV1::Exact { bytes } => {
            let snapshot =
                scan_package_snapshot(path).map_err(|_| PluginManagementError::Unavailable)?;
            if snapshot.total_bytes != bytes
                || receipt.source.package_digest.as_deref()
                    != Some(snapshot.package_identity.digest.as_str())
            {
                return Err(PluginManagementError::Unavailable);
            }
            Ok(snapshot.total_bytes)
        }
        CleanupMeasureV1::Bounded { max_bytes } => {
            let actual = measure_cleanup_directory(path)?;
            if actual > max_bytes || actual > PLUGIN_CLEANUP_BATCH_BYTES {
                return Err(PluginManagementError::Unavailable);
            }
            Ok(actual)
        }
    }
}

fn measure_cleanup_directory(root: &Path) -> Result<u64, PluginManagementError> {
    fn visit(
        directory: &Path,
        directories: &mut usize,
        files: &mut usize,
        bytes: &mut u64,
    ) -> Result<(), PluginManagementError> {
        for entry in fs::read_dir(directory).map_err(|_| PluginManagementError::Unavailable)? {
            let entry = entry.map_err(|_| PluginManagementError::Unavailable)?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| PluginManagementError::Unavailable)?;
            if is_reparse_point(&metadata) {
                return Err(PluginManagementError::Unavailable);
            }
            if metadata.is_dir() {
                *directories = directories
                    .checked_add(1)
                    .ok_or(PluginManagementError::Unavailable)?;
                if *directories > PLUGIN_CLEANUP_MAX_DIRECTORIES {
                    return Err(PluginManagementError::Unavailable);
                }
                visit(&entry.path(), directories, files, bytes)?;
            } else if metadata.is_file() {
                *files = files
                    .checked_add(1)
                    .ok_or(PluginManagementError::Unavailable)?;
                *bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or(PluginManagementError::Unavailable)?;
                if *files > PLUGIN_CLEANUP_MAX_FILES || *bytes > PLUGIN_CLEANUP_BATCH_BYTES {
                    return Err(PluginManagementError::Unavailable);
                }
            } else {
                return Err(PluginManagementError::Unavailable);
            }
        }
        Ok(())
    }

    if !ordinary_directory(root) {
        return Err(PluginManagementError::Unavailable);
    }
    let mut directories = 0;
    let mut files = 0;
    let mut bytes = 0;
    visit(root, &mut directories, &mut files, &mut bytes)?;
    Ok(bytes)
}

fn move_cleanup_directory(
    source: &Path,
    target: &Path,
    expected: &TransactionObjectIdentity,
) -> Result<(), PluginManagementError> {
    if target.exists() {
        return Err(PluginManagementError::Unavailable);
    }
    #[cfg(windows)]
    {
        let (handle, identity) =
            open_directory_handle(source, true).map_err(|_| PluginManagementError::Unavailable)?;
        if identity.volume != expected.volume_serial
            || format!("{:016x}", identity.file) != expected.file_id
        {
            return Err(PluginManagementError::Unavailable);
        }
        move_directory_handle(&handle, target).map_err(|_| PluginManagementError::Unavailable)
    }
    #[cfg(not(windows))]
    {
        let identity = directory_identity(source).ok_or(PluginManagementError::Unavailable)?;
        if identity.volume != expected.volume_serial
            || format!("{:016x}", identity.file) != expected.file_id
        {
            return Err(PluginManagementError::Unavailable);
        }
        fs::rename(source, target).map_err(|_| PluginManagementError::Unavailable)
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
                let path = child.path();
                let candidate = if ordinary_file(&path.join("active.json")) {
                    load_active_entry(&path, host_version)
                } else {
                    load_entry(&path, host_version)
                };
                if let Some(entry) = candidate {
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

    pub(crate) fn views(&self) -> Vec<PluginView> {
        let mut views = self.entries.iter().map(plugin_view).collect::<Vec<_>>();
        views.sort_by(|left, right| left.id.cmp(&right.id));
        views
    }

    #[cfg(test)]
    pub(crate) fn authorizes_clipboard(&self, plugin_id: &str) -> bool {
        self.entries.iter().any(|entry| {
            entry.id == plugin_id
                && entry
                    .permissions
                    .iter()
                    .any(|permission| permission == "clipboard.writeText")
        })
    }

    #[cfg(test)]
    pub(crate) fn asset_response(&self, label: &str, request_path: &str) -> Response<Vec<u8>> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.window_label == label)
        else {
            return response(403, Vec::new(), None);
        };
        asset_response(entry, request_path)
    }
}

fn plugin_view(entry: &PluginCatalogEntry) -> PluginView {
    PluginView {
        id: entry.id.clone(),
        version: entry.version.to_path_segment(),
        trigger: entry.feature.trigger.clone(),
        description: entry.description.clone(),
    }
}

fn scan_inventory(
    installed_root: &Path,
    development_root: Option<&Path>,
    host_version: Version,
    revision: u64,
) -> Result<PluginInventorySnapshot, PluginManagementError> {
    let mut rows = BTreeMap::<String, InventoryRowBuilder>::new();
    let mut invalid_items = Vec::new();
    scan_installed_rows(installed_root, host_version, &mut rows)?;
    if let Some(development_root) = development_root {
        scan_development_rows(
            development_root,
            host_version,
            &mut rows,
            &mut invalid_items,
        )?;
    }
    let mut items = rows
        .into_values()
        .map(|row| {
            let description = if row.development_description.is_some()
                && (matches!(row.installed, InstalledPluginView::Absent)
                    || row
                        .active_version
                        .zip(row.development_version)
                        .is_some_and(|(active, development)| development > active))
            {
                PluginDescriptionView::Available {
                    source: PluginDescriptionSource::Development,
                    markdown: row.development_description.expect("description checked"),
                }
            } else if let Some(markdown) = row.installed_description {
                PluginDescriptionView::Available {
                    source: PluginDescriptionSource::Installed,
                    markdown,
                }
            } else {
                PluginDescriptionView::Unavailable
            };
            PluginInventoryView {
                key: plugin_inventory_key(&row.id),
                display_name: row.id.clone(),
                id: Some(row.id),
                installed: row.installed,
                development: row.development,
                description,
            }
        })
        .collect::<Vec<_>>();
    items.extend(invalid_items);
    items.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(PluginInventorySnapshot {
        revision: revision.to_string(),
        items,
    })
}

fn scan_installed_rows(
    root: &Path,
    host_version: Version,
    rows: &mut BTreeMap<String, InventoryRowBuilder>,
) -> Result<(), PluginManagementError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) if ordinary_directory(root) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        _ => return Err(PluginManagementError::Unavailable),
    };
    let mut direct_entries = 0usize;
    let mut containers = 0usize;
    for entry in entries {
        direct_entries += 1;
        if direct_entries > 128 {
            return Err(PluginManagementError::Unavailable);
        }
        let entry = entry.map_err(|_| PluginManagementError::Unavailable)?;
        if !entry
            .file_type()
            .map_err(|_| PluginManagementError::Unavailable)?
            .is_dir()
            || !ordinary_directory(&entry.path())
        {
            continue;
        }
        containers += 1;
        if containers > 64 {
            return Err(PluginManagementError::Unavailable);
        }
        let Some(plugin_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !valid_id(&plugin_id) {
            continue;
        }
        let container = entry.path();
        let state_path = container.join("active.json");
        let state_bytes = match fs::read(&state_path) {
            Ok(bytes) if bytes.len() <= 64 * 1024 && ordinary_file(&state_path) => bytes,
            _ => {
                rows.insert(
                    plugin_id.clone(),
                    invalid_installed_row(plugin_id, "stateMissing"),
                );
                continue;
            }
        };
        let state = match parse_active_state(&state_bytes, &plugin_id) {
            Ok(state) => state,
            Err(()) => {
                rows.insert(
                    plugin_id.clone(),
                    invalid_installed_row(plugin_id, "stateInvariantViolation"),
                );
                continue;
            }
        };
        if state.packages.is_empty() {
            continue;
        }
        let active_version = state
            .active_version
            .as_deref()
            .and_then(Version::parse)
            .ok_or(PluginManagementError::Unavailable)?;
        let mut active_entry = None;
        let mut valid = true;
        for record in &state.packages {
            let package_root = container.join(&record.version);
            let Ok(snapshot) = scan_package_snapshot(&package_root) else {
                valid = false;
                break;
            };
            if snapshot.package_identity != record.identity {
                valid = false;
                break;
            }
            let Some(manifest) = manifest_from_snapshot(&snapshot, host_version) else {
                valid = false;
                break;
            };
            if manifest.id != plugin_id || manifest.version != record.version {
                valid = false;
                break;
            }
            if state.active_version.as_deref() == Some(record.version.as_str()) {
                active_entry = Some((manifest, snapshot));
            }
        }
        let Some((manifest, snapshot)) = active_entry.filter(|_| valid) else {
            rows.insert(
                plugin_id.clone(),
                invalid_installed_row(plugin_id, "packageInvalid"),
            );
            continue;
        };
        rows.insert(
            plugin_id.clone(),
            InventoryRowBuilder {
                id: plugin_id,
                installed: InstalledPluginView::Valid {
                    active_version: state.active_version.expect("validated active version"),
                    versions: state
                        .packages
                        .iter()
                        .map(|record| record.version.clone())
                        .collect(),
                    trigger: manifest.feature.trigger,
                },
                development: DevelopmentPluginView::Absent,
                installed_description: read_description(&snapshot),
                development_description: None,
                active_version: Some(active_version),
                development_version: None,
            },
        );
    }
    Ok(())
}

fn scan_development_rows(
    root: &Path,
    host_version: Version,
    rows: &mut BTreeMap<String, InventoryRowBuilder>,
    invalid_items: &mut Vec<PluginInventoryView>,
) -> Result<(), PluginManagementError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) if ordinary_directory(root) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        _ => return Err(PluginManagementError::Unavailable),
    };
    let mut direct_entries = 0usize;
    let mut candidates = 0usize;
    for entry in entries {
        direct_entries += 1;
        if direct_entries > 128 {
            return Err(PluginManagementError::Unavailable);
        }
        let entry = entry.map_err(|_| PluginManagementError::Unavailable)?;
        if !entry
            .file_type()
            .map_err(|_| PluginManagementError::Unavailable)?
            .is_dir()
            || !ordinary_directory(&entry.path())
        {
            continue;
        }
        candidates += 1;
        if candidates > 64 {
            return Err(PluginManagementError::Unavailable);
        }
        let Some(candidate) = load_entry(&entry.path(), host_version) else {
            invalid_items.push(invalid_development_view(&entry, "invalidManifest"));
            continue;
        };
        if entry.file_name().to_str() != Some(candidate.id.as_str()) {
            invalid_items.push(invalid_development_view(&entry, "invalidId"));
            continue;
        }
        let version = candidate.version;
        let row = rows
            .entry(candidate.id.clone())
            .or_insert_with(|| InventoryRowBuilder {
                id: candidate.id.clone(),
                installed: InstalledPluginView::Absent,
                development: DevelopmentPluginView::Absent,
                installed_description: None,
                development_description: None,
                active_version: None,
                development_version: None,
            });
        row.development = DevelopmentPluginView::Valid {
            version: version.to_path_segment(),
            trigger: candidate.feature.trigger,
        };
        row.development_description = candidate.description;
        row.development_version = Some(version);
    }
    Ok(())
}

fn invalid_development_view(entry: &fs::DirEntry, reason: &'static str) -> PluginInventoryView {
    let identity =
        directory_identity(&entry.path()).unwrap_or(DirectoryIdentity { volume: 0, file: 0 });
    let mut hasher = Sha256::new();
    hasher.update(b"UIPILOT-DEVELOPMENT-INVALID");
    hasher.update([0]);
    hasher.update(entry.file_name().to_string_lossy().as_bytes());
    hasher.update(identity.volume.to_le_bytes());
    hasher.update(identity.file.to_le_bytes());
    let digest = lower_hex(&hasher.finalize());
    PluginInventoryView {
        key: format!("development-invalid:{digest}"),
        id: None,
        display_name: format!("无效开发包 {}", &digest[..12]),
        installed: InstalledPluginView::Absent,
        development: DevelopmentPluginView::Invalid { reason },
        description: PluginDescriptionView::Unavailable,
    }
}

fn invalid_installed_row(plugin_id: String, issue: &'static str) -> InventoryRowBuilder {
    InventoryRowBuilder {
        id: plugin_id,
        installed: InstalledPluginView::Invalid {
            issue,
            active_version: None,
            versions: Vec::new(),
        },
        development: DevelopmentPluginView::Absent,
        installed_description: None,
        development_description: None,
        active_version: None,
        development_version: None,
    }
}

fn plugin_inventory_key(plugin_id: &str) -> String {
    format!("plugin:{}", lower_hex(plugin_id.as_bytes()))
}

fn manifest_from_snapshot(
    snapshot: &GenerationAssetSnapshot,
    host_version: Version,
) -> Option<Manifest> {
    let manifest: Manifest =
        serde_json::from_slice(snapshot.assets.get(Path::new("plugin.json"))?).ok()?;
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
        || !snapshot.assets.contains_key(Path::new(&manifest.runtime))
        || has_bad_permissions(&manifest.permissions)
        || version.to_path_segment() != manifest.version
    {
        return None;
    }
    Some(manifest)
}

fn load_active_entry(container: &Path, host_version: Version) -> Option<PluginCatalogEntry> {
    let plugin_id = container.file_name()?.to_str()?;
    if !valid_id(plugin_id) {
        return None;
    }
    let state_path = container.join("active.json");
    let state_bytes = fs::read(&state_path).ok()?;
    if state_bytes.len() > 64 * 1024 || !ordinary_file(&state_path) {
        return None;
    }
    let state = parse_active_state(&state_bytes, plugin_id).ok()?;
    let active_version = state.active_version?;
    let record = state
        .packages
        .iter()
        .find(|record| record.version == active_version)?;
    let package_root = container.join(&active_version);
    let snapshot = scan_package_snapshot(&package_root).ok()?;
    if snapshot.package_identity != record.identity {
        return None;
    }
    let manifest = manifest_from_snapshot(&snapshot, host_version)?;
    if manifest.id != plugin_id || manifest.version != active_version {
        return None;
    }
    catalog_entry_from_snapshot(package_root, manifest, snapshot)
}

fn catalog_entry_from_snapshot(
    root: PathBuf,
    manifest: Manifest,
    snapshot: GenerationAssetSnapshot,
) -> Option<PluginCatalogEntry> {
    let version = Version::parse(&manifest.version)?;
    let runtime = root.join(&manifest.runtime);
    let description = read_description(&snapshot);
    Some(PluginCatalogEntry {
        window_label: window_label(&manifest.id, 1),
        id: manifest.id,
        version,
        runtime,
        feature: PluginFeature {
            trigger: manifest.feature.trigger,
        },
        permissions: manifest.permissions,
        package_identity: directory_identity(&root)?,
        root,
        description,
        generation: 1,
        snapshot: Arc::new(snapshot),
    })
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
    let snapshot = scan_package_snapshot(root).ok()?;
    if !snapshot.assets.contains_key(Path::new(&manifest.runtime)) {
        return None;
    }
    let description = read_description(&snapshot);

    Some(PluginCatalogEntry {
        window_label: window_label(&manifest.id, 1),
        id: manifest.id,
        version,
        runtime,
        feature: PluginFeature {
            trigger: manifest.feature.trigger,
        },
        permissions: manifest.permissions,
        root: root.to_path_buf(),
        description,
        generation: 1,
        package_identity: directory_identity(root)?,
        snapshot: Arc::new(snapshot),
    })
}

enum PackageHashEntry {
    Directory(String),
    File(String, u64, [u8; 32]),
}

fn scan_package_snapshot(root: &Path) -> Result<GenerationAssetSnapshot, PackageScanError> {
    if !ordinary_directory(root) {
        return Err(PackageScanError);
    }
    let mut hash_entries = Vec::new();
    let mut assets = HashMap::new();
    let mut casefolded = HashSet::new();
    let mut directories = 0usize;
    let mut files = 0usize;
    let mut total_bytes = 0u64;
    scan_package_directory(
        root,
        root,
        &mut hash_entries,
        &mut assets,
        &mut casefolded,
        &mut directories,
        &mut files,
        &mut total_bytes,
    )?;
    hash_entries.sort_by(|left, right| package_hash_path(left).cmp(package_hash_path(right)));
    let mut tree = Sha256::new();
    tree.update(b"UIPILOT-PACKAGE");
    tree.update([0]);
    tree.update(b"SHA256-TREE-V1");
    tree.update([0]);
    tree.update(
        u32::try_from(hash_entries.len())
            .map_err(|_| PackageScanError)?
            .to_le_bytes(),
    );
    for entry in hash_entries {
        match entry {
            PackageHashEntry::Directory(path) => {
                tree.update([1]);
                tree.update(
                    u32::try_from(path.len())
                        .map_err(|_| PackageScanError)?
                        .to_le_bytes(),
                );
                tree.update(path.as_bytes());
            }
            PackageHashEntry::File(path, length, digest) => {
                tree.update([2]);
                tree.update(
                    u32::try_from(path.len())
                        .map_err(|_| PackageScanError)?
                        .to_le_bytes(),
                );
                tree.update(path.as_bytes());
                tree.update(length.to_le_bytes());
                tree.update(digest);
            }
        }
    }
    let identity = directory_identity(root).ok_or(PackageScanError)?;
    Ok(GenerationAssetSnapshot {
        package_identity: PackageIdentityV1 {
            algorithm: "sha256-tree-v1".into(),
            digest: lower_hex(&tree.finalize()),
            volume_serial: identity.volume,
            file_id: format!("{:016x}", identity.file),
        },
        assets,
        total_bytes,
    })
}

#[allow(clippy::too_many_arguments)]
fn scan_package_directory(
    root: &Path,
    directory: &Path,
    hash_entries: &mut Vec<PackageHashEntry>,
    assets: &mut HashMap<PathBuf, Vec<u8>>,
    casefolded: &mut HashSet<String>,
    directories: &mut usize,
    files: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), PackageScanError> {
    let entries = fs::read_dir(directory).map_err(|_| PackageScanError)?;
    for entry in entries {
        let entry = entry.map_err(|_| PackageScanError)?;
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|_| PackageScanError)?;
        let canonical = canonical_package_path(relative)?;
        if !casefolded.insert(canonical.to_lowercase()) {
            return Err(PackageScanError);
        }
        let metadata = fs::symlink_metadata(&path).map_err(|_| PackageScanError)?;
        if is_reparse_point(&metadata) {
            if canonical == "README.md" {
                continue;
            }
            return Err(PackageScanError);
        }
        if metadata.is_dir() {
            *directories = directories.checked_add(1).ok_or(PackageScanError)?;
            if *directories > PLUGIN_PACKAGE_MAX_DIRECTORIES {
                return Err(PackageScanError);
            }
            hash_entries.push(PackageHashEntry::Directory(canonical));
            scan_package_directory(
                root,
                &path,
                hash_entries,
                assets,
                casefolded,
                directories,
                files,
                total_bytes,
            )?;
            continue;
        }
        if !metadata.is_file() || !allowed_package_file(&canonical) {
            return Err(PackageScanError);
        }
        *files = files.checked_add(1).ok_or(PackageScanError)?;
        if *files > PLUGIN_PACKAGE_MAX_FILES || metadata.len() > PLUGIN_PACKAGE_MAX_FILE_BYTES {
            return Err(PackageScanError);
        }
        let bytes = fs::read(&path).map_err(|_| PackageScanError)?;
        if bytes.len() as u64 != metadata.len() || !ordinary_file(&path) {
            return Err(PackageScanError);
        }
        *total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .filter(|total| *total <= PLUGIN_PACKAGE_MAX_TOTAL_BYTES)
            .ok_or(PackageScanError)?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        hash_entries.push(PackageHashEntry::File(
            canonical,
            bytes.len() as u64,
            digest,
        ));
        assets.insert(relative.to_path_buf(), bytes);
    }
    Ok(())
}

fn package_hash_path(entry: &PackageHashEntry) -> &str {
    match entry {
        PackageHashEntry::Directory(path) | PackageHashEntry::File(path, ..) => path,
    }
}

fn canonical_package_path(path: &Path) -> Result<String, PackageScanError> {
    if path.components().count() > PLUGIN_PACKAGE_MAX_DEPTH {
        return Err(PackageScanError);
    }
    let mut components = Vec::new();
    for component in path.iter() {
        let component = component.to_str().ok_or(PackageScanError)?;
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.len() > PLUGIN_PACKAGE_MAX_COMPONENT_BYTES
            || component.ends_with(['.', ' '])
            || component.contains(':')
            || !is_nfc(component)
        {
            return Err(PackageScanError);
        }
        components.push(component);
    }
    let canonical = components.join("/");
    if canonical.is_empty() || canonical.len() > PLUGIN_PACKAGE_MAX_PATH_BYTES {
        return Err(PackageScanError);
    }
    Ok(canonical)
}

fn allowed_package_file(path: &str) -> bool {
    if matches!(path, "plugin.json" | "README.md") {
        return true;
    }
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some(
            "html"
                | "js"
                | "mjs"
                | "css"
                | "json"
                | "md"
                | "txt"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "ico"
                | "svg"
                | "woff"
                | "woff2"
                | "ttf"
                | "otf"
        )
    )
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn read_description(snapshot: &GenerationAssetSnapshot) -> Option<String> {
    let bytes = snapshot.assets.get(Path::new("README.md"))?;
    if bytes.len() as u64 > PLUGIN_README_MAX_BYTES {
        return None;
    }
    String::from_utf8(bytes.clone()).ok()
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

fn window_label(id: &str, generation: u64) -> String {
    let mut label = String::from("plugin-");
    for byte in id.as_bytes() {
        label.push_str(&format!("{byte:02x}"));
    }
    label.push_str(&format!("-g{generation:016x}"));
    label
}

fn runtime_data_directory(app_data_dir: &Path, identity: &RuntimeIdentity) -> PathBuf {
    app_data_dir
        .join("plugin-runtime-data")
        .join(&identity.window_label)
}

fn asset_response(entry: &PluginCatalogEntry, request_path: &str) -> Response<Vec<u8>> {
    let Some((relative, content_type)) = asset_path(request_path) else {
        return response(415, Vec::new(), None);
    };
    debug_assert!(entry.snapshot.total_bytes <= PLUGIN_PACKAGE_MAX_TOTAL_BYTES);
    let Some(body) = entry.snapshot.assets.get(&relative) else {
        return response(404, Vec::new(), None);
    };
    response(200, body.clone(), Some(content_type))
}

fn route(entry: &PluginCatalogEntry, input: &str) -> PluginRoute {
    PluginRoute {
        plugin_id: entry.id.clone(),
        window_label: entry.window_label.clone(),
        generation: entry.generation,
        input: input.to_string(),
    }
}

fn route_matches(entry: &PluginCatalogEntry, route: &PluginRoute) -> bool {
    entry.id == route.plugin_id
        && entry.window_label == route.window_label
        && entry.generation == route.generation
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

#[cfg(debug_assertions)]
fn development_plugin_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("examples").join("plugins"))
}

#[cfg(not(debug_assertions))]
fn development_plugin_root() -> Option<PathBuf> {
    None
}

fn cleanup_quarantine(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if ordinary_directory(&path) {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(windows)]
struct DirectoryHandle(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for DirectoryHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn open_directory_handle(
    path: &Path,
    delete: bool,
) -> io::Result<(DirectoryHandle, DirectoryIdentity)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    let desired = FILE_READ_ATTRIBUTES.0 | if delete { DELETE.0 } else { 0 };
    let share = if delete {
        FILE_SHARE_READ | FILE_SHARE_WRITE
    } else {
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
    };
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            desired,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(io::Error::other)?;
    let handle = DirectoryHandle(handle);
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle.0, &mut information) }.map_err(io::Error::other)?;
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(io::Error::other("plugin directory unavailable"));
    }
    Ok((
        handle,
        DirectoryIdentity {
            volume: u64::from(information.dwVolumeSerialNumber),
            file: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        },
    ))
}

#[cfg(windows)]
fn directory_identity(path: &Path) -> Option<DirectoryIdentity> {
    open_directory_handle(path, false)
        .ok()
        .map(|(_, identity)| identity)
}

#[cfg(windows)]
fn move_directory_handle(handle: &DirectoryHandle, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO, FILE_RENAME_INFO_0,
        },
    };

    let name = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| io::Error::other("plugin delete unavailable"))?;
    let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes as usize)
        .ok_or_else(|| io::Error::other("plugin delete unavailable"))?;
    let mut buffer = vec![0u64; size.div_ceil(std::mem::size_of::<u64>())];
    let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*information).Anonymous = FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        };
        (*information).RootDirectory = HANDLE::default();
        (*information).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*information).FileName).cast::<u16>(),
            name.len(),
        );
        SetFileInformationByHandle(
            handle.0,
            FileRenameInfo,
            information.cast(),
            u32::try_from(size).map_err(|_| io::Error::other("plugin delete unavailable"))?,
        )
    }
    .map_err(io::Error::other)
}

#[cfg(not(windows))]
fn directory_identity(path: &Path) -> Option<DirectoryIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).ok()?;
    (metadata.is_dir() && !metadata.file_type().is_symlink()).then_some(DirectoryIdentity {
        volume: metadata.dev(),
        file: metadata.ino(),
    })
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

    use super::{parse_active_state, scan_package_snapshot, PluginCatalog, Version};

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

    mod package_state {
        use std::fs;

        use super::{
            load, parse_active_state, scan_package_snapshot, valid_manifest, TestRoot, Version,
        };

        #[test]
        fn canonical_version_rejects_leading_zeroes() {
            assert_eq!(
                Version::parse("1.2.3").map(Version::to_path_segment),
                Some("1.2.3".into())
            );
            assert!(Version::parse("01.2.3").is_none());
            assert!(Version::parse("1.02.3").is_none());
            assert!(Version::parse("1.2.03").is_none());
        }

        #[test]
        fn catalog_assets_are_an_immutable_snapshot() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let package = root.path.join("plugin");
            fs::write(package.join("runtime.js"), "original").unwrap();
            let catalog = load(&root);
            let label = catalog.entries[0].window_label.clone();

            assert_eq!(
                catalog.asset_response(&label, "/runtime.js").body(),
                b"original"
            );
            fs::write(package.join("runtime.js"), "changed").unwrap();
            assert_eq!(
                catalog.asset_response(&label, "/runtime.js").body(),
                b"original"
            );
        }

        #[test]
        fn active_state_enforces_empty_and_non_empty_invariants() {
            let empty = br#"{
                "schema":1,
                "pluginId":"internal.math",
                "activeVersion":null,
                "packages":[]
            }"#;
            let parsed = parse_active_state(empty, "internal.math").unwrap();
            assert!(parsed.active_version.is_none());
            assert!(parsed.packages.is_empty());

            for invalid in [
                br#"{"schema":1,"pluginId":"other","activeVersion":null,"packages":[]}"#.as_slice(),
                br#"{"schema":1,"pluginId":"internal.math","activeVersion":"1.0.0","packages":[]}"#.as_slice(),
                br#"{"schema":1,"pluginId":"internal.math","activeVersion":null,"packages":[{"version":"1.0.0","identity":{"algorithm":"sha256-tree-v1","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","volumeSerial":1,"fileId":"0000000000000001"}}]}"#.as_slice(),
                br#"{"schema":2,"pluginId":"internal.math","activeVersion":null,"packages":[]}"#.as_slice(),
            ] {
                assert!(parse_active_state(invalid, "internal.math").is_err());
            }
        }

        #[test]
        fn package_digest_changes_with_file_content() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let package = root.path.join("plugin");
            let original = scan_package_snapshot(&package).unwrap();
            fs::write(package.join("index.html"), "changed").unwrap();
            let changed = scan_package_snapshot(&package).unwrap();

            assert_ne!(
                original.package_identity.digest,
                changed.package_identity.digest
            );
            assert_eq!(original.package_identity.digest.len(), 64);
            assert!(original
                .package_identity
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }

        #[test]
        fn nested_assets_are_part_of_the_immutable_snapshot() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let package = root.path.join("plugin");
            fs::create_dir(package.join("assets")).unwrap();
            fs::write(package.join("assets").join("nested.js"), "original").unwrap();
            let catalog = load(&root);
            let label = catalog.entries[0].window_label.clone();

            fs::write(package.join("assets").join("nested.js"), "changed").unwrap();
            assert_eq!(
                catalog.asset_response(&label, "/assets/nested.js").body(),
                b"original"
            );
        }

        #[test]
        fn package_scan_rejects_more_than_256_files() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let package = root.path.join("plugin");
            for index in 0..255 {
                fs::write(package.join(format!("asset-{index}.js")), "").unwrap();
            }
            assert!(scan_package_snapshot(&package).is_err());
        }
    }

    mod inventory {
        use std::{fs, path::Path};

        use super::{valid_manifest, TestRoot};
        use crate::plugins::{
            scan_inventory, scan_package_snapshot, ActivePluginStateV1, PackageRecordV1,
            PluginCatalog, Version,
        };

        fn write_version(
            container: &Path,
            plugin_id: &str,
            version: &str,
            description: &str,
        ) -> PackageRecordV1 {
            let package = container.join(version);
            fs::create_dir(&package).unwrap();
            fs::write(
                package.join("plugin.json"),
                valid_manifest(plugin_id, "/math").replace(
                    r#""version":"1.0.0""#,
                    format!(r#""version":"{version}""#).as_str(),
                ),
            )
            .unwrap();
            fs::write(package.join("index.html"), "").unwrap();
            fs::write(package.join("README.md"), description).unwrap();
            PackageRecordV1 {
                version: version.into(),
                identity: scan_package_snapshot(&package).unwrap().package_identity,
            }
        }

        fn write_installed(
            installed_root: &Path,
            plugin_id: &str,
            active_version: &str,
            versions: &[(&str, &str)],
        ) {
            let container = installed_root.join(plugin_id);
            fs::create_dir(&container).unwrap();
            let packages = versions
                .iter()
                .map(|(version, description)| {
                    write_version(&container, plugin_id, version, description)
                })
                .collect();
            let state = ActivePluginStateV1 {
                schema: 1,
                plugin_id: plugin_id.into(),
                active_version: Some(active_version.into()),
                packages,
            };
            fs::write(
                container.join("active.json"),
                serde_json::to_vec(&state).unwrap(),
            )
            .unwrap();
        }

        #[test]
        fn development_only_inventory_uses_revisioned_union_dto() {
            let installed = TestRoot::new();
            let development = TestRoot::new();
            development.write_plugin("internal.math", valid_manifest("internal.math", "/math"));
            fs::write(
                development.path.join("internal.math").join("README.md"),
                "# Development math",
            )
            .unwrap();

            let snapshot = scan_inventory(
                &installed.path,
                Some(&development.path),
                Version::new(0, 2, 0),
                7,
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(snapshot).unwrap(),
                serde_json::json!({
                    "revision":"7",
                    "items":[{
                        "key":"plugin:696e7465726e616c2e6d617468",
                        "id":"internal.math",
                        "displayName":"internal.math",
                        "installed":{"state":"absent"},
                        "development":{"state":"valid","version":"1.0.0","trigger":"/math"},
                        "description":{"state":"available","source":"development","markdown":"# Development math"}
                    }]
                })
            );
        }

        #[test]
        fn installed_inventory_exposes_sorted_versions_and_active_description() {
            let installed = TestRoot::new();
            write_installed(
                &installed.path,
                "internal.math",
                "0.2.0",
                &[("0.1.0", "old"), ("0.2.0", "# Installed math")],
            );

            let snapshot =
                scan_inventory(&installed.path, None, Version::new(0, 2, 0), 11).unwrap();
            let value = serde_json::to_value(snapshot).unwrap();
            assert_eq!(value["revision"], "11");
            assert_eq!(value["items"][0]["installed"]["state"], "valid");
            assert_eq!(value["items"][0]["installed"]["activeVersion"], "0.2.0");
            assert_eq!(
                value["items"][0]["installed"]["versions"],
                serde_json::json!(["0.1.0", "0.2.0"])
            );
            assert_eq!(value["items"][0]["development"]["state"], "absent");
            assert_eq!(value["items"][0]["description"]["source"], "installed");
            assert_eq!(
                value["items"][0]["description"]["markdown"],
                "# Installed math"
            );
        }

        #[test]
        fn runtime_catalog_loads_only_the_active_registered_version() {
            let installed = TestRoot::new();
            write_installed(
                &installed.path,
                "internal.math",
                "0.2.0",
                &[("0.1.0", "old"), ("0.2.0", "active")],
            );

            let catalog = PluginCatalog::load(&installed.path, Version::new(0, 2, 0)).unwrap();
            assert_eq!(catalog.views().len(), 1);
            assert_eq!(catalog.views()[0].id, "internal.math");
            assert_eq!(catalog.views()[0].version, "0.2.0");
            assert!(catalog.route("/math 1+1").is_some());
        }

        #[test]
        fn manager_lists_inventory_with_its_current_revision() {
            let app_data = TestRoot::new();
            let installed_root = app_data.path.join("plugins");
            fs::create_dir(&installed_root).unwrap();
            write_installed(
                &installed_root,
                "other.plugin",
                "1.0.0",
                &[("1.0.0", "installed")],
            );
            let manager = crate::plugins::PluginManager::new();
            manager.load(&app_data.path, Version::new(0, 2, 0)).unwrap();

            let snapshot = manager.list_inventory().unwrap();
            assert_eq!(snapshot.revision, "0");
            let installed = snapshot
                .items
                .iter()
                .find(|item| item.id.as_deref() == Some("other.plugin"))
                .unwrap();
            assert!(matches!(
                installed.installed,
                crate::plugins::InstalledPluginView::Valid { .. }
            ));
        }

        #[test]
        fn newer_development_version_supplies_update_description() {
            let installed = TestRoot::new();
            let development = TestRoot::new();
            write_installed(
                &installed.path,
                "internal.math",
                "1.0.0",
                &[("1.0.0", "installed")],
            );
            development.write_plugin(
                "internal.math",
                valid_manifest("internal.math", "/math")
                    .replace(r#""version":"1.0.0""#, r#""version":"2.0.0""#),
            );
            fs::write(
                development.path.join("internal.math").join("README.md"),
                "development update",
            )
            .unwrap();

            let snapshot = scan_inventory(
                &installed.path,
                Some(&development.path),
                Version::new(0, 2, 0),
                12,
            )
            .unwrap();
            let value = serde_json::to_value(snapshot).unwrap();
            assert_eq!(value["items"][0]["installed"]["activeVersion"], "1.0.0");
            assert_eq!(value["items"][0]["development"]["version"], "2.0.0");
            assert_eq!(value["items"][0]["description"]["source"], "development");
            assert_eq!(
                value["items"][0]["description"]["markdown"],
                "development update"
            );
        }

        #[test]
        fn invalid_development_package_has_stable_path_free_identity() {
            let installed = TestRoot::new();
            let development = TestRoot::new();
            let invalid = development.path.join("private-source-name");
            fs::create_dir(&invalid).unwrap();
            fs::write(invalid.join("plugin.json"), "{}").unwrap();
            fs::write(invalid.join("index.html"), "").unwrap();

            let snapshot = scan_inventory(
                &installed.path,
                Some(&development.path),
                Version::new(0, 2, 0),
                13,
            )
            .unwrap();
            let value = serde_json::to_value(snapshot).unwrap();
            assert_eq!(value["items"].as_array().unwrap().len(), 1);
            let item = &value["items"][0];
            assert!(item["id"].is_null());
            let key = item["key"].as_str().unwrap();
            assert!(key.starts_with("development-invalid:"));
            assert_eq!(key.len(), "development-invalid:".len() + 64);
            let display_name = item["displayName"].as_str().unwrap();
            assert!(display_name.starts_with("无效开发包 "));
            assert!(!display_name.contains("private-source-name"));
            assert_eq!(item["installed"]["state"], "absent");
            assert_eq!(item["development"]["state"], "invalid");
            assert_eq!(item["development"]["reason"], "invalidManifest");
            assert_eq!(item["description"]["state"], "unavailable");
        }
    }

    mod recovery {
        use std::fs;

        use super::TestRoot;
        use crate::plugins::{
            directory_identity, parse_cleanup_receipt, parse_plugin_transaction,
            receipt_worker_eligible, run_cleanup_worker, scan_package_snapshot,
        };

        fn transaction(phase: &str, receipt_ids: serde_json::Value) -> serde_json::Value {
            let receipt_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
            serde_json::json!({
                "schema":1,
                "transactionId":"11111111111111111111111111111111",
                "operation":"delete-last",
                "pluginId":"internal.math",
                "phase":phase,
                "oldState":{"kind":"active-state-v1","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
                "newState":{"kind":"active-state-v1","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"},
                "objects":{
                    "kind":"delete-last",
                    "deletedPackage":{
                        "role":"deleted-package",
                        "identity":{"volumeSerial":1,"fileId":"0000000000000001","packageDigest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"},
                        "location":{"root":"plugin-root","relativePath":"internal.math/1.0.0"}
                    },
                    "previousRuntimeData":null
                },
                "cleanupPlans":[{
                    "receiptId":receipt_id,
                    "condition":"if-new-state",
                    "objectRole":"deleted-package",
                    "operation":"delete-last-version",
                    "plannedTarget":{"root":"quarantine-root","relativePath":receipt_id},
                    "measure":{"kind":"exact","bytes":1}
                }],
                "cleanupReceiptIds":receipt_ids
            })
        }

        fn receipt(planned_root: &str, planned_path: &str) -> serde_json::Value {
            serde_json::json!({
                "schema":1,
                "receiptId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "originOperationId":"11111111111111111111111111111111",
                "pluginId":"internal.math",
                "operation":"delete-last-version",
                "phase":"pending",
                "source":{
                    "role":"deleted-package",
                    "root":"plugin-root",
                    "relativePath":"internal.math/1.0.0",
                    "volumeSerial":1,
                    "fileId":"0000000000000001",
                    "packageDigest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                },
                "plannedTarget":{"root":planned_root,"relativePath":planned_path},
                "target":null,
                "measure":{"kind":"exact","bytes":1}
            })
        }

        #[test]
        fn strict_transaction_phase_and_cleanup_ids_are_validated() {
            assert!(parse_plugin_transaction(
                &serde_json::to_vec(&transaction("prepared", serde_json::json!([]))).unwrap()
            )
            .is_ok());
            assert!(parse_plugin_transaction(
                &serde_json::to_vec(&transaction(
                    "prepared",
                    serde_json::json!(["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"])
                ))
                .unwrap()
            )
            .is_err());
            assert!(parse_plugin_transaction(
                &serde_json::to_vec(&transaction("cleanup-transferred", serde_json::json!([])))
                    .unwrap()
            )
            .is_err());
        }

        #[test]
        fn receipt_requires_persisted_quarantine_target_for_its_full_id() {
            assert!(parse_cleanup_receipt(
                &serde_json::to_vec(&receipt(
                    "quarantine-root",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                ))
                .unwrap()
            )
            .is_ok());
            assert!(parse_cleanup_receipt(
                &serde_json::to_vec(&receipt("plugin-root", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))
                    .unwrap()
            )
            .is_err());
            assert!(parse_cleanup_receipt(
                &serde_json::to_vec(&receipt("quarantine-root", "aaaaaaaaaaaa")).unwrap()
            )
            .is_err());
        }

        #[test]
        fn active_journal_lease_blocks_generic_receipt_worker() {
            let transaction = parse_plugin_transaction(
                &serde_json::to_vec(&transaction("prepared", serde_json::json!([]))).unwrap(),
            )
            .unwrap();
            let receipt = parse_cleanup_receipt(
                &serde_json::to_vec(&receipt(
                    "quarantine-root",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ))
                .unwrap(),
            )
            .unwrap();

            assert!(!receipt_worker_eligible(&receipt, Some(&transaction)));
            assert!(receipt_worker_eligible(&receipt, None));
        }

        #[test]
        fn durable_journal_lease_blocks_worker_until_journal_is_removed() {
            let app_data = TestRoot::new();
            let source = app_data
                .path
                .join("plugins")
                .join("internal.math")
                .join("1.0.0");
            fs::create_dir_all(&source).unwrap();
            fs::write(source.join("plugin.json"), "{}").unwrap();
            let snapshot = scan_package_snapshot(&source).unwrap();
            let identity = directory_identity(&source).unwrap();
            let active_root = app_data.path.join("plugin-transactions").join("active");
            let receipts_root = app_data.path.join("plugin-transactions").join("receipts");
            fs::create_dir_all(&active_root).unwrap();
            fs::create_dir_all(&receipts_root).unwrap();
            fs::create_dir_all(app_data.path.join("plugin-quarantine")).unwrap();
            let journal_path = active_root.join("current.json");
            fs::write(
                &journal_path,
                serde_json::to_vec(&transaction("prepared", serde_json::json!([]))).unwrap(),
            )
            .unwrap();
            let mut cleanup_receipt =
                receipt("quarantine-root", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
            cleanup_receipt["source"]["volumeSerial"] = identity.volume.into();
            cleanup_receipt["source"]["fileId"] = format!("{:016x}", identity.file).into();
            cleanup_receipt["source"]["packageDigest"] =
                snapshot.package_identity.digest.clone().into();
            cleanup_receipt["measure"]["bytes"] = snapshot.total_bytes.into();
            let receipt_path = receipts_root.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json");
            fs::write(&receipt_path, serde_json::to_vec(&cleanup_receipt).unwrap()).unwrap();

            run_cleanup_worker(&app_data.path).unwrap();
            assert!(source.exists());
            assert!(receipt_path.exists());

            fs::remove_file(journal_path).unwrap();
            run_cleanup_worker(&app_data.path).unwrap();
            assert!(!source.exists());
            assert!(!receipt_path.exists());
            assert!(!app_data
                .path
                .join("plugin-quarantine")
                .join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .exists());
        }
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
        assert_eq!(route.window_label, "plugin-706c7567696e-g0000000000000001");
        assert_eq!(route.generation, 1);
        assert_eq!(route.input, "body");
        assert_eq!(loaded.route("/go").unwrap().input, "");
        assert!(loaded.route("/go\tbody").is_none());
        assert!(loaded.route("/good body").is_none());
        assert!(loaded.route("ordinary query").is_none());
        assert!(loaded.authorizes_clipboard("plugin"));
    }

    mod description {
        use std::fs;

        use super::{load, valid_manifest, TestRoot};

        #[test]
        fn reads_only_valid_root_readme_with_a_fixed_size_limit() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let package = root.path.join("plugin");

            fs::write(package.join("README.md"), "# Plugin\n\nWorks.").unwrap();
            assert_eq!(
                load(&root).views()[0].description.as_deref(),
                Some("# Plugin\n\nWorks.")
            );

            fs::write(package.join("README.md"), vec![b'x'; 16 * 1024]).unwrap();
            assert_eq!(
                load(&root).views()[0].description.as_deref(),
                Some("x".repeat(16 * 1024).as_str())
            );

            fs::write(package.join("README.md"), vec![b'x'; 16 * 1024 + 1]).unwrap();
            assert_eq!(load(&root).views()[0].description, None);
            fs::write(package.join("README.md"), [0xff, 0xfe]).unwrap();
            assert_eq!(load(&root).views()[0].description, None);
            fs::remove_file(package.join("README.md")).unwrap();
            assert_eq!(load(&root).views()[0].description, None);
            fs::create_dir(package.join("README.md")).unwrap();
            assert_eq!(load(&root).views()[0].description, None);
        }

        #[test]
        fn rejects_reparse_readme_without_disabling_the_plugin() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let package = root.path.join("plugin");
            let target = root.path.join("outside.md");
            fs::write(&target, "private").unwrap();

            #[cfg(windows)]
            if std::os::windows::fs::symlink_file(&target, package.join("README.md")).is_err() {
                return;
            }

            let catalog = load(&root);
            assert_eq!(catalog.views()[0].description, None);
            assert!(catalog.route("/plugin").is_some());
        }

        #[test]
        fn inventory_is_sorted_and_serializes_only_the_approved_fields() {
            let root = TestRoot::new();
            root.write_plugin("zeta", valid_manifest("zeta", "/zeta"));
            root.write_plugin("alpha", valid_manifest("alpha", "/alpha"));
            fs::write(root.path.join("zeta").join("README.md"), "Zeta docs").unwrap();

            let views = load(&root).views();
            assert_eq!(
                views
                    .iter()
                    .map(|view| view.id.as_str())
                    .collect::<Vec<_>>(),
                ["alpha", "zeta"]
            );
            assert_eq!(
                serde_json::to_value(&views).unwrap(),
                serde_json::json!([
                    {"id":"alpha","version":"1.0.0","trigger":"/alpha","description":null},
                    {"id":"zeta","version":"1.0.0","trigger":"/zeta","description":"Zeta docs"}
                ])
            );
        }
    }

    mod generation {
        use std::{
            sync::{mpsc, Arc},
            time::Duration,
        };

        use super::{load, valid_manifest, TestRoot};
        use crate::{
            model::ResultItem,
            plugins::{PluginCopyError, PluginManager, PluginQueryStart},
            result_registry::ResultRegistry,
        };

        fn manager(root: &TestRoot) -> Arc<PluginManager> {
            let manager = Arc::new(PluginManager::new());
            manager.install_catalog_for_test(load(root));
            manager
        }

        #[test]
        fn old_route_and_token_cannot_publish_after_generation_commit() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            let registry = ResultRegistry::default();
            registry.on_show("invocation".into());
            let PluginQueryStart::Started { route, token } =
                manager.begin_routed_query("/plugin 1+1", &registry, "invocation", 1)
            else {
                panic!("plugin route must start");
            };

            manager.advance_generation_for_test(&registry, "plugin");

            assert!(manager
                .publish_results(
                    &registry,
                    token,
                    &route,
                    vec![(
                        ResultItem {
                            result_id: String::new(),
                            title: "late".into(),
                            subtitle: None,
                            icon: None,
                        },
                        crate::result_registry::ResultAction::CopyText {
                            plugin_id: "plugin".into(),
                            generation: 1,
                            text: "late".into(),
                        },
                    )],
                )
                .is_none());
        }

        #[test]
        fn clipboard_side_effect_holds_admission_and_old_action_fails_after_commit() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            let registry = ResultRegistry::default();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let copy_manager = Arc::clone(&manager);
            let copy = std::thread::spawn(move || {
                copy_manager.copy_text("plugin", 1, || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                })
            });
            entered_rx.recv().unwrap();

            let (committed_tx, committed_rx) = mpsc::channel();
            let commit_manager = Arc::clone(&manager);
            let commit_registry = registry.clone();
            let commit = std::thread::spawn(move || {
                commit_manager.advance_generation_for_test(&commit_registry, "plugin");
                committed_tx.send(()).unwrap();
            });
            assert_eq!(
                committed_rx.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            );
            release_tx.send(()).unwrap();
            assert_eq!(copy.join().unwrap(), Ok(()));
            commit.join().unwrap();

            let writes = std::cell::Cell::new(0);
            assert_eq!(
                manager.copy_text("plugin", 1, || {
                    writes.set(writes.get() + 1);
                    Ok(())
                }),
                Err(PluginCopyError::PermissionDenied)
            );
            assert_eq!(writes.get(), 0);
        }

        #[test]
        fn already_resolved_copy_action_is_rejected_after_generation_commit() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            let registry = ResultRegistry::default();
            registry.on_show("invocation".into());
            let PluginQueryStart::Started { route, token } =
                manager.begin_routed_query("/plugin 1+1", &registry, "invocation", 1)
            else {
                panic!("plugin route must start");
            };
            let (request_id, result_id) = manager
                .publish_results(
                    &registry,
                    token,
                    &route,
                    vec![(
                        ResultItem {
                            result_id: String::new(),
                            title: "2".into(),
                            subtitle: None,
                            icon: None,
                        },
                        crate::result_registry::ResultAction::CopyText {
                            plugin_id: "plugin".into(),
                            generation: 1,
                            text: "2".into(),
                        },
                    )],
                )
                .map(|response| {
                    (
                        response.request_id,
                        response.items.into_iter().next().unwrap().result_id,
                    )
                })
                .unwrap();
            let action = registry.resolve(&request_id, &result_id).unwrap();

            manager.advance_generation_for_test(&registry, "plugin");

            let crate::result_registry::ResultAction::CopyText {
                plugin_id,
                generation,
                ..
            } = action
            else {
                panic!("plugin result must resolve to CopyText");
            };
            let writes = std::cell::Cell::new(0);
            assert_eq!(
                manager.copy_text(&plugin_id, generation, || {
                    writes.set(writes.get() + 1);
                    Ok(())
                }),
                Err(PluginCopyError::PermissionDenied)
            );
            assert_eq!(writes.get(), 0);
        }
    }

    mod ownership {
        use std::sync::Arc;

        use super::{load, valid_manifest, TestRoot};
        use crate::{
            plugins::{
                window_label, PluginManager, RuntimeAttempt, RuntimeOwnership, RuntimeSlot,
                PLUGIN_RUNTIME_READY_TIMEOUT,
            },
            result_registry::{QueryDomain, ResultRegistry},
        };

        fn manager(root: &TestRoot) -> Arc<PluginManager> {
            let manager = Arc::new(PluginManager::new());
            manager.install_catalog_for_test(load(root));
            manager
        }

        #[test]
        fn staged_assets_are_served_without_becoming_query_routes() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            let mut staged = manager.state.get().unwrap().read().unwrap().active.entries[0].clone();
            staged.generation = 2;
            staged.window_label = window_label("plugin", 2);
            let identity = staged.identity();
            let attempt = Arc::new(RuntimeAttempt::default());
            {
                let mut state = manager.state.get().unwrap().write().unwrap();
                state.staged_assets.insert(identity.clone(), staged);
                state.ownership.insert(
                    identity.clone(),
                    RuntimeOwnership {
                        slot: RuntimeSlot::Staged,
                        attempt,
                    },
                );
            }

            assert_eq!(manager.route("/plugin").unwrap().generation, 1);
            assert_eq!(
                manager
                    .asset_response(&identity.window_label, "/index.html")
                    .status(),
                200
            );
        }

        #[test]
        fn callbacks_resolve_current_slot_and_ignore_removed_identity() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            let old = manager.route("/plugin").unwrap().identity();
            let mut promoted_entry =
                manager.state.get().unwrap().read().unwrap().active.entries[0].clone();
            promoted_entry.generation = 2;
            promoted_entry.window_label = window_label("plugin", 2);
            let promoted = promoted_entry.identity();
            let attempt = Arc::new(RuntimeAttempt::default());
            {
                let mut state = manager.state.get().unwrap().write().unwrap();
                state
                    .staged_assets
                    .insert(promoted.clone(), promoted_entry.clone());
                state.ownership.insert(
                    promoted.clone(),
                    RuntimeOwnership {
                        slot: RuntimeSlot::Staged,
                        attempt: Arc::clone(&attempt),
                    },
                );
            }
            manager.runtime_ready(&promoted);
            assert!(attempt.snapshot().unwrap().ready);
            {
                let _admission = manager.admission.write().unwrap();
                let mut state = manager.state.get().unwrap().write().unwrap();
                state.active.entries[0] = promoted_entry;
                state.staged_assets.remove(&promoted);
                state.ownership.remove(&old);
                state.ownership.insert(
                    promoted.clone(),
                    RuntimeOwnership {
                        slot: RuntimeSlot::Active,
                        attempt,
                    },
                );
            }

            let registry = ResultRegistry::default();
            registry.on_show("invocation".into());
            let old_token = registry
                .begin_query(QueryDomain::Plugin, "invocation", 1)
                .unwrap();
            manager.runtime_failed(&old, &registry);
            assert!(!manager.disabled.read().unwrap().contains(&old.window_label));
            manager.runtime_failed(&promoted, &registry);
            assert!(manager
                .disabled
                .read()
                .unwrap()
                .contains(&promoted.window_label));
            assert!(registry
                .publish_if_latest(old_token, Vec::<((), _)>::new(), || true, |_, _| ())
                .is_none());
        }

        #[test]
        fn generation_labels_are_unique_and_overflow_is_rejected() {
            assert_ne!(window_label("plugin", 1), window_label("plugin", 2));
            assert_eq!(u64::MAX.checked_add(1), None);
        }

        #[test]
        fn readiness_timeout_is_fixed_and_does_not_wedge_the_mutation_lock() {
            assert_eq!(
                PLUGIN_RUNTIME_READY_TIMEOUT,
                std::time::Duration::from_millis(500)
            );
            let attempt = RuntimeAttempt::default();
            assert_eq!(
                attempt.wait_until_settled(std::time::Duration::from_millis(1)),
                Some(Default::default())
            );
            let manager = PluginManager::new();
            assert!(manager.mutation.try_lock().is_ok());
        }
    }

    #[cfg(windows)]
    mod delete {
        use std::fs;

        use super::{load, valid_manifest, TestRoot};
        use crate::plugins::{move_directory_handle, open_directory_handle};

        #[test]
        fn no_follow_handle_move_removes_original_path_and_preserves_identity() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let original = root.path.join("plugin");
            let quarantine = root.path.join("quarantine");
            fs::create_dir(&quarantine).unwrap();
            let destination = quarantine.join("removed");
            let expected = load(&root).entries[0].package_identity;
            let (handle, current) = open_directory_handle(&original, true).unwrap();
            assert_eq!(current, expected);

            move_directory_handle(&handle, &destination).unwrap();
            drop(handle);

            assert!(!original.exists());
            assert_eq!(
                open_directory_handle(&destination, false).unwrap().1,
                expected
            );
        }

        #[test]
        fn replacement_and_reparse_directories_fail_identity_validation() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let original = root.path.join("plugin");
            let parked = root.path.join("parked");
            let expected = load(&root).entries[0].package_identity;
            fs::rename(&original, &parked).unwrap();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            assert_ne!(open_directory_handle(&original, true).unwrap().1, expected);

            fs::remove_dir_all(&original).unwrap();
            if std::os::windows::fs::symlink_dir(&parked, &original).is_ok() {
                assert!(open_directory_handle(&original, true).is_err());
            }
        }

        #[test]
        fn move_failure_leaves_original_path_and_contents_untouched() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let original = root.path.join("plugin");
            let (handle, _) = open_directory_handle(&original, true).unwrap();
            let missing_parent = root.path.join("missing").join("removed");

            assert!(move_directory_handle(&handle, &missing_parent).is_err());
            drop(handle);

            assert!(original.join("plugin.json").is_file());
            assert!(original.join("index.html").is_file());
        }

        #[test]
        fn destination_collision_is_non_overwriting() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let original = root.path.join("plugin");
            let destination = root.path.join("occupied");
            fs::create_dir(&destination).unwrap();
            fs::write(destination.join("owner.txt"), "foreign").unwrap();
            let (handle, _) = open_directory_handle(&original, true).unwrap();

            assert!(move_directory_handle(&handle, &destination).is_err());
            drop(handle);

            assert!(original.exists());
            assert_eq!(
                fs::read_to_string(destination.join("owner.txt")).unwrap(),
                "foreign"
            );
        }
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

            let html = catalog.asset_response("plugin-6f6e65-g0000000000000001", "/index.html");
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

            let script = catalog.asset_response("plugin-6f6e65-g0000000000000001", "/main.js");
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
            let label = "plugin-6f6e65-g0000000000000001";

            assert_eq!(
                catalog
                    .asset_response("plugin-74776f-g0000000000000001", "/index.html")
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
                    .asset_response("plugin-6f6e65-g0000000000000001", "/../two/index.html",)
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
                            .asset_response("plugin-6f6e65-g0000000000000001", "/linked.html",)
                            .status(),
                        404
                    );
                }

                let nested = root.path.join("one").join("nested");
                if std::os::windows::fs::symlink_dir(root.path.join("two"), nested).is_ok() {
                    assert_eq!(
                        catalog
                            .asset_response(
                                "plugin-6f6e65-g0000000000000001",
                                "/nested/index.html",
                            )
                            .status(),
                        404
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
                        .asset_response("plugin-6f6e65-g0000000000000001", "/junction/index.html",)
                        .status(),
                    404
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
                let response =
                    catalog.asset_response("plugin-6f6e65-g0000000000000001", "/index.html");
                assert_eq!(response.status(), 200);
                assert!(response.body().is_empty());
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
            let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
            assert!(production.contains("wait_until_ready(attempt, disabled, label)"));
            assert!(production.contains("tauri::async_runtime::spawn_blocking"));
            assert!(!production.contains(&["thread", "::sleep"].concat()));
        }

        #[test]
        fn runtime_failure_invalidates_the_plugin_domain() {
            let source = std::fs::read_to_string(file!()).unwrap();
            let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
            assert!(production.contains("registry.invalidate_domain(QueryDomain::Plugin)"));
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
            wait_until_ready, PendingPluginQuery, PluginManager, PluginQueryError, RuntimeAttempt,
        };
        use super::{load, valid_manifest, TestRoot};

        const LABEL: &str = "plugin-706c7567696e-g0000000000000001";

        fn manager(root: &TestRoot) -> PluginManager {
            let manager = PluginManager::new();
            manager.install_catalog_for_test(load(root));
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
                    window_label: LABEL.into(),
                    generation: 1,
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
            manager.record_timeout(LABEL);
            let receiver = wait_for(&manager, "request");
            manager
                .publish_response(
                    LABEL,
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
                    LABEL,
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
                    LABEL,
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
                    LABEL,
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
                    manager.publish_response(LABEL, response),
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
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            for _ in 0..3 {
                manager.record_timeout(LABEL);
            }
            assert!(manager.disabled.read().unwrap().contains(LABEL));
        }

        #[test]
        fn disabled_runtime_loses_clipboard_authorization() {
            let root = TestRoot::new();
            root.write_plugin("plugin", valid_manifest("plugin", "/plugin"));
            let manager = manager(&root);
            assert!(manager.authorizes_clipboard("plugin"));

            let identity = manager.route("/plugin").unwrap().identity();
            manager.disable_runtime(&identity);

            assert!(!manager.authorizes_clipboard("plugin"));
        }

        #[test]
        fn readiness_wait_completes_after_runtime_marks_ready() {
            let ready = Arc::new(RuntimeAttempt::default());
            let disabled = Arc::new(RwLock::new(std::collections::HashSet::new()));
            let marker = Arc::clone(&ready);
            let worker = std::thread::spawn(move || marker.mark_ready());

            assert_eq!(
                wait_until_ready(ready, disabled, "plugin-label".into()),
                Ok(true)
            );
            worker.join().unwrap();
        }
    }
}
