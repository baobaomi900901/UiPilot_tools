use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    thread,
    time::Duration,
};

use super::{
    observer::{
        default_clipboard_observer, default_clipboard_reader, ClipboardObserver,
        ClipboardObserverHandle, ClipboardReadError, ClipboardReader,
    },
    ClipboardCapture, ClipboardHistoryError, ClipboardHistorySnapshot, ClipboardHistoryStore,
};

const BUSY_RETRY_ATTEMPTS: usize = 3;
const BUSY_RETRY_DELAY: Duration = Duration::from_millis(50);

pub(crate) struct ClipboardHistoryService {
    root: PathBuf,
    reader: Arc<dyn ClipboardReader>,
    observer: Arc<dyn ClipboardObserver>,
    retry_delay: Duration,
    state: Mutex<ServiceState>,
}

#[derive(Default)]
struct ServiceState {
    authorized_plugins: BTreeSet<String>,
    stores: BTreeMap<String, Arc<ClipboardHistoryStore>>,
    observer_handle: Option<Box<dyn ClipboardObserverHandle>>,
    suppressed_observer_changes: usize,
}

impl ClipboardHistoryService {
    pub(crate) fn load(root: &Path) -> Result<Arc<Self>, ClipboardHistoryError> {
        Self::load_with_dependencies(
            root,
            default_clipboard_reader(),
            default_clipboard_observer(),
            BUSY_RETRY_DELAY,
        )
    }

    pub(crate) fn load_with_dependencies(
        root: &Path,
        reader: Arc<dyn ClipboardReader>,
        observer: Arc<dyn ClipboardObserver>,
        retry_delay: Duration,
    ) -> Result<Arc<Self>, ClipboardHistoryError> {
        fs::create_dir_all(root).map_err(|_| ClipboardHistoryError::Storage)?;
        Ok(Arc::new(Self {
            root: root.to_path_buf(),
            reader,
            observer,
            retry_delay,
            state: Mutex::new(ServiceState::default()),
        }))
    }

    pub(crate) fn sync_authorized_plugins(
        self: &Arc<Self>,
        plugin_ids: impl IntoIterator<Item = String>,
    ) -> Result<(), ClipboardHistoryError> {
        let next = plugin_ids.into_iter().collect::<BTreeSet<_>>();
        let previous_handle = {
            let mut state = self.lock()?;
            state.authorized_plugins = next;
            let authorized_plugins = state.authorized_plugins.clone();
            state
                .stores
                .retain(|plugin_id, _| authorized_plugins.contains(plugin_id));
            if state.authorized_plugins.is_empty() {
                state.observer_handle.take()
            } else {
                None
            }
        };
        if let Some(handle) = previous_handle {
            handle.stop();
        }
        self.ensure_observer_state()
    }

    pub(crate) fn shutdown(&self) {
        let handle = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.observer_handle.take());
        if let Some(handle) = handle {
            handle.stop();
        }
    }

    pub(crate) fn snapshot(
        &self,
        plugin_id: &str,
    ) -> Result<ClipboardHistorySnapshot, ClipboardHistoryError> {
        let store = self.store_for_plugin(plugin_id)?;
        store.snapshot()
    }

    #[allow(dead_code)]
    pub(crate) fn remove(&self, plugin_id: &str, id: &str) -> Result<bool, ClipboardHistoryError> {
        self.store_for_plugin(plugin_id)?.remove(id)
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&self, plugin_id: &str) -> Result<(), ClipboardHistoryError> {
        self.store_for_plugin(plugin_id)?.clear()
    }

    #[allow(dead_code)]
    pub(crate) fn move_to_front(
        &self,
        plugin_id: &str,
        id: &str,
    ) -> Result<bool, ClipboardHistoryError> {
        self.store_for_plugin(plugin_id)?.move_to_front(id)
    }

    #[allow(dead_code)]
    pub(crate) fn suppress_next_observer_change(&self) -> Result<(), ClipboardHistoryError> {
        self.lock()?.suppressed_observer_changes += 1;
        Ok(())
    }

    pub(crate) fn uninstall(
        &self,
        plugin_id: &str,
        retain_data: bool,
    ) -> Result<(), ClipboardHistoryError> {
        let handle = {
            let mut state = self.lock()?;
            state.authorized_plugins.remove(plugin_id);
            state.stores.remove(plugin_id);
            if state.authorized_plugins.is_empty() {
                state.observer_handle.take()
            } else {
                None
            }
        };
        if let Some(handle) = handle {
            handle.stop();
        }
        if !retain_data {
            let path = self.plugin_root(plugin_id);
            if path.exists() {
                fs::remove_dir_all(path).map_err(|_| ClipboardHistoryError::Storage)?;
            }
        }
        Ok(())
    }

    fn ensure_observer_state(self: &Arc<Self>) -> Result<(), ClipboardHistoryError> {
        let should_start = {
            let state = self.lock()?;
            !state.authorized_plugins.is_empty() && state.observer_handle.is_none()
        };
        if !should_start {
            return Ok(());
        }
        let weak = Arc::downgrade(self);
        let handle = self.observer.start(Arc::new(move || {
            if let Some(service) = Weak::upgrade(&weak) {
                let _ = service.capture_current();
            }
        }))?;
        let mut stop_new_handle = None;
        {
            let mut state = self.lock()?;
            if state.authorized_plugins.is_empty() || state.observer_handle.is_some() {
                stop_new_handle = Some(handle);
            } else {
                state.observer_handle = Some(handle);
            }
        }
        if let Some(handle) = stop_new_handle {
            handle.stop();
        }
        Ok(())
    }

    fn capture_current(&self) -> Result<(), ClipboardHistoryError> {
        if self.consume_suppressed_observer_change()? {
            return Ok(());
        }
        let plugin_ids = {
            let state = self.lock()?;
            state.authorized_plugins.iter().cloned().collect::<Vec<_>>()
        };
        if plugin_ids.is_empty() {
            return Ok(());
        }
        let Some(capture) = self.read_capture_with_retry()? else {
            return Ok(());
        };
        self.capture_for_authorized_plugins(capture)
    }

    fn capture_for_authorized_plugins(
        &self,
        capture: ClipboardCapture,
    ) -> Result<(), ClipboardHistoryError> {
        let plugin_ids = {
            let state = self.lock()?;
            state.authorized_plugins.iter().cloned().collect::<Vec<_>>()
        };
        for plugin_id in plugin_ids {
            let store = self.store_for_plugin(&plugin_id)?;
            let _ = store.capture(capture.clone())?;
        }
        Ok(())
    }

    fn read_capture_with_retry(&self) -> Result<Option<ClipboardCapture>, ClipboardHistoryError> {
        for attempt in 1..=BUSY_RETRY_ATTEMPTS {
            match self.reader.read_capture() {
                Ok(capture) => return Ok(capture),
                Err(ClipboardReadError::Busy) if attempt < BUSY_RETRY_ATTEMPTS => {
                    if !self.retry_delay.is_zero() {
                        thread::sleep(self.retry_delay);
                    }
                }
                Err(ClipboardReadError::Busy | ClipboardReadError::Unavailable) => {
                    return Ok(None);
                }
            }
        }
        Ok(None)
    }

    fn consume_suppressed_observer_change(&self) -> Result<bool, ClipboardHistoryError> {
        let mut state = self.lock()?;
        if state.suppressed_observer_changes == 0 {
            return Ok(false);
        }
        state.suppressed_observer_changes -= 1;
        Ok(true)
    }

    fn store_for_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<Arc<ClipboardHistoryStore>, ClipboardHistoryError> {
        let mut state = self.lock()?;
        if let Some(store) = state.stores.get(plugin_id) {
            return Ok(Arc::clone(store));
        }
        let store = Arc::new(ClipboardHistoryStore::load(&self.plugin_root(plugin_id))?);
        state.stores.insert(plugin_id.into(), Arc::clone(&store));
        Ok(store)
    }

    fn plugin_root(&self, plugin_id: &str) -> PathBuf {
        self.root.join(plugin_id)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ServiceState>, ClipboardHistoryError> {
        self.state
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)
    }

    #[cfg(test)]
    pub(crate) fn capture_current_for_test(self: &Arc<Self>) -> Result<(), ClipboardHistoryError> {
        self.capture_current()
    }

    #[cfg(test)]
    pub(crate) fn capture_for_test(
        &self,
        capture: ClipboardCapture,
    ) -> Result<(), ClipboardHistoryError> {
        self.capture_for_authorized_plugins(capture)
    }

    #[cfg(test)]
    pub(crate) fn authorized_plugins_for_test(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .authorized_plugins
            .iter()
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn store_path_for_test(&self, plugin_id: &str) -> PathBuf {
        self.plugin_root(plugin_id)
    }
}

impl Drop for ClipboardHistoryService {
    fn drop(&mut self) {
        self.shutdown();
    }
}
