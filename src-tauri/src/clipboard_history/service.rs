use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
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
    ClipboardCapture, ClipboardHistoryError, ClipboardHistoryPasteError, ClipboardHistoryRecord,
    ClipboardHistorySnapshot, ClipboardHistoryStore,
};

const BUSY_RETRY_ATTEMPTS: usize = 3;
const BUSY_RETRY_DELAY: Duration = Duration::from_millis(50);
const MAX_TEMPORARY_COPY_SEQUENCES: usize = 64;

pub(crate) struct ClipboardHistoryService {
    root: PathBuf,
    reader: Arc<dyn ClipboardReader>,
    observer: Arc<dyn ClipboardObserver>,
    retry_delay: Duration,
    temporary_copy: Mutex<TemporaryClipboardCopy>,
    state: Mutex<ServiceState>,
}

/// Shares the observer admission gate with a native temporary-copy transaction.
/// Exact exclusions outlive the closure so delayed observer callbacks remain safe.
#[derive(Default)]
pub(crate) struct TemporaryClipboardCopy {
    excluded_sequences: VecDeque<u32>,
}

impl TemporaryClipboardCopy {
    pub(crate) fn exclude_sequence(&mut self, sequence: u32) {
        if sequence != 0 && !self.excluded_sequences.contains(&sequence) {
            self.excluded_sequences.push_back(sequence);
            if self.excluded_sequences.len() > MAX_TEMPORARY_COPY_SEQUENCES {
                // Callbacks query the current sequence, rather than carrying
                // historical values. Superseded exclusions can be bounded.
                self.excluded_sequences.pop_front();
            }
        }
    }
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
            temporary_copy: Mutex::new(TemporaryClipboardCopy::default()),
            state: Mutex::new(ServiceState::default()),
        }))
    }

    pub(crate) fn with_temporary_copy<T>(
        &self,
        action: impl FnOnce(&mut TemporaryClipboardCopy) -> T,
    ) -> Result<T, ClipboardHistoryError> {
        let mut gate = self
            .temporary_copy
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
        Ok(action(&mut gate))
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

    pub(crate) fn snapshot_query(
        &self,
        plugin_id: &str,
        query: Option<&str>,
    ) -> Result<ClipboardHistorySnapshot, ClipboardHistoryError> {
        self.store_for_plugin(plugin_id)?.snapshot_query(query)
    }

    #[allow(dead_code)]
    pub(crate) fn remove(&self, plugin_id: &str, id: &str) -> Result<bool, ClipboardHistoryError> {
        self.store_for_plugin(plugin_id)?.remove(id)
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&self, plugin_id: &str) -> Result<(), ClipboardHistoryError> {
        self.store_for_plugin(plugin_id)?.clear()
    }

    pub(crate) fn record_for_paste(
        &self,
        plugin_id: &str,
        id: &str,
    ) -> Result<ClipboardHistoryRecord, ClipboardHistoryPasteError> {
        self.store_for_plugin(plugin_id)
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
            .record_for_paste(id)
    }

    pub(crate) fn complete_paste(
        &self,
        plugin_id: &str,
        id: &str,
    ) -> Result<(), ClipboardHistoryPasteError> {
        self.store_for_plugin(plugin_id)
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
            .move_to_front(id)
            .map_err(|_| ClipboardHistoryPasteError::RecordUnavailable)?
            .then_some(())
            .ok_or(ClipboardHistoryPasteError::RecordNotFound)
    }

    pub(crate) fn begin_paste_restore_suppression(&self) -> Result<(), ClipboardHistoryError> {
        self.suppress_next_observer_change()
    }

    pub(crate) fn cancel_paste_restore_suppression(&self) -> Result<(), ClipboardHistoryError> {
        let mut state = self.lock()?;
        if state.suppressed_observer_changes > 0 {
            state.suppressed_observer_changes -= 1;
        }
        Ok(())
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
        // Keep this lock through store admission. Taking it only after reading
        // would allow a transaction to copy and restore before the read is stored.
        let gate = self
            .temporary_copy
            .lock()
            .map_err(|_| ClipboardHistoryError::Storage)?;
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
        let sequence = self.reader.sequence();
        if gate.excluded_sequences.contains(&sequence) {
            return Ok(());
        }
        let Some(capture) = self.read_capture_with_retry()? else {
            return Ok(());
        };
        // External copies do not use our gate. Never admit bytes read across an
        // external sequence change; a later callback can capture the new value.
        if self.reader.sequence() != sequence {
            return Ok(());
        }
        self.capture_for_authorized_plugins(capture)
    }

    fn capture_for_authorized_plugins(
        &self,
        capture: ClipboardCapture,
    ) -> Result<(), ClipboardHistoryError> {
        self.capture_for_authorized_plugins_with(capture, |store, capture| {
            store.capture(capture).map(|_| ())
        })
    }

    fn capture_for_authorized_plugins_with(
        &self,
        capture: ClipboardCapture,
        mut capture_into: impl FnMut(
            &ClipboardHistoryStore,
            ClipboardCapture,
        ) -> Result<(), ClipboardHistoryError>,
    ) -> Result<(), ClipboardHistoryError> {
        // Membership changes must wait for an admitted store write to finish.
        // Otherwise a callback can keep a removed owner's Arc and write history
        // after revocation (or uninstall) has already reported completion.
        let mut state = self.lock()?;
        let plugin_ids = state.authorized_plugins.iter().cloned().collect::<Vec<_>>();
        for plugin_id in plugin_ids {
            let store = self.store_for_plugin_in_state(&mut state, &plugin_id)?;
            capture_into(&store, capture.clone())?;
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
        self.store_for_plugin_in_state(&mut state, plugin_id)
    }

    fn store_for_plugin_in_state(
        &self,
        state: &mut ServiceState,
        plugin_id: &str,
    ) -> Result<Arc<ClipboardHistoryStore>, ClipboardHistoryError> {
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

#[cfg(test)]
mod temporary_copy_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct SequenceReader {
        sequence: AtomicU32,
        calls: AtomicUsize,
        on_read: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl ClipboardReader for SequenceReader {
        fn sequence(&self) -> u32 {
            self.sequence.load(Ordering::SeqCst)
        }

        fn read_capture(&self) -> Result<Option<ClipboardCapture>, ClipboardReadError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(on_read) = self.on_read.lock().unwrap().take() {
                on_read();
            }
            Ok(Some(ClipboardCapture::text(
                "selected",
                "2026-11-01T00:00:00Z",
            )))
        }
    }

    struct Fixture {
        root: PathBuf,
        service: Arc<ClipboardHistoryService>,
        reader: Arc<SequenceReader>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "uipilot-temporary-copy-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let reader = Arc::new(SequenceReader::default());
            reader.sequence.store(1, Ordering::SeqCst);
            let service = ClipboardHistoryService::load_with_dependencies(
                &root,
                reader.clone(),
                default_clipboard_observer(),
                Duration::ZERO,
            )
            .unwrap();
            service.sync_authorized_plugins(["test".into()]).unwrap();
            Self {
                root,
                service,
                reader,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.service.shutdown();
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn temporary_exclusions_are_bounded_by_insertion_order_across_sequence_wrap() {
        let mut copy = TemporaryClipboardCopy::default();
        for sequence in 2..=64 {
            copy.exclude_sequence(sequence);
        }
        copy.exclude_sequence(u32::MAX);
        copy.exclude_sequence(1);
        for _ in 0..100 {
            copy.exclude_sequence(1);
        }
        copy.exclude_sequence(0);
        assert_eq!(copy.excluded_sequences.len(), 64);
        assert!(!copy.excluded_sequences.contains(&2));
        assert!(copy.excluded_sequences.contains(&3));
        assert!(copy.excluded_sequences.contains(&u32::MAX));
        assert!(copy.excluded_sequences.contains(&1));
        assert!(!copy.excluded_sequences.contains(&0));
    }

    #[test]
    fn observer_drops_capture_if_sequence_changes_during_read() {
        let fixture = Fixture::new();
        let reader = fixture.reader.clone();
        *fixture.reader.on_read.lock().unwrap() = Some(Box::new(move || {
            reader.sequence.store(2, Ordering::SeqCst);
        }));
        fixture.service.capture_current().unwrap();
        assert!(fixture.service.snapshot("test").unwrap().entries.is_empty());
    }

    #[test]
    fn temporary_sequences_stay_excluded_after_transaction_without_changing_old_records() {
        let fixture = Fixture::new();
        fixture
            .service
            .capture_for_test(ClipboardCapture::text("selected", "old"))
            .unwrap();
        fixture
            .service
            .capture_for_test(ClipboardCapture::text("other", "old"))
            .unwrap();
        let before = fixture.service.snapshot("test").unwrap();
        let selected_id = before.entries[1].id();
        fixture
            .service
            .with_temporary_copy(|copy| {
                copy.exclude_sequence(2);
                copy.exclude_sequence(3);
            })
            .unwrap();
        for sequence in [2, 3, 2] {
            fixture.reader.sequence.store(sequence, Ordering::SeqCst);
            fixture.service.capture_current().unwrap();
        }
        assert_eq!(fixture.service.snapshot("test").unwrap(), before);
        assert_eq!(fixture.reader.calls.load(Ordering::SeqCst), 0);

        fixture.reader.sequence.store(4, Ordering::SeqCst);
        fixture.service.capture_current().unwrap();
        assert_eq!(fixture.reader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture.service.snapshot("test").unwrap().entries[0].id(),
            selected_id
        );
    }

    #[test]
    fn transaction_waits_for_observer_read_and_store_admission() {
        use std::sync::mpsc;
        let fixture = Fixture::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        *fixture.reader.on_read.lock().unwrap() = Some(Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
        let service = fixture.service.clone();
        let observer = thread::spawn(move || service.capture_current().unwrap());
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let service = fixture.service.clone();
        let transaction = thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            service
                .with_temporary_copy(|_| {
                    let stored = service.snapshot("test").unwrap().entries.len();
                    finished_tx.send(stored).unwrap();
                })
                .unwrap();
        });
        attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let returned_early = finished_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        release_tx.send(()).unwrap();
        observer.join().unwrap();
        transaction.join().unwrap();
        assert!(
            !returned_early,
            "transaction entered before observer read and admission completed"
        );
        assert_eq!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    }

    #[test]
    fn observer_waits_for_transaction_and_then_excludes_its_copy() {
        use std::sync::mpsc;
        let fixture = Fixture::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let service = fixture.service.clone();
        let reader = fixture.reader.clone();
        let transaction = thread::spawn(move || {
            service
                .with_temporary_copy(|copy| {
                    reader.sequence.store(2, Ordering::SeqCst);
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    copy.exclude_sequence(2);
                })
                .unwrap()
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (finished_tx, finished_rx) = mpsc::channel();
        let service = fixture.service.clone();
        let observer = thread::spawn(move || {
            service.capture_current().unwrap();
            finished_tx.send(()).unwrap();
        });
        let returned_early = finished_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        release_tx.send(()).unwrap();
        transaction.join().unwrap();
        observer.join().unwrap();
        assert!(
            !returned_early,
            "observer read a temporary copy before its exclusion was registered"
        );
        assert_eq!(fixture.reader.calls.load(Ordering::SeqCst), 0);
        assert!(fixture.service.snapshot("test").unwrap().entries.is_empty());
    }

    #[test]
    fn failed_transaction_releases_gate_and_does_not_suppress_normal_copies() {
        let fixture = Fixture::new();
        let outcome = fixture
            .service
            .with_temporary_copy(|copy| {
                copy.exclude_sequence(2);
                Err::<(), _>("copy failed")
            })
            .unwrap();
        assert_eq!(outcome, Err("copy failed"));
        fixture.reader.sequence.store(3, Ordering::SeqCst);
        fixture.service.capture_current().unwrap();
        assert_eq!(fixture.service.snapshot("test").unwrap().entries.len(), 1);
        assert!(fixture.service.with_temporary_copy(|_| ()).is_ok());
    }
}

#[cfg(test)]
mod permission_capture_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn permission_access_capture_finishes_before_revocation_returns_and_preserves_other_plugins() {
        let root = std::env::temp_dir().join(format!(
            "uipilot-permission-capture-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let service = ClipboardHistoryService::load(&root).unwrap();
        service
            .sync_authorized_plugins(["a".into(), "b".into()])
            .unwrap();
        service
            .capture_for_test(ClipboardCapture::text("before", "2026-10-30T00:00:00Z"))
            .unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let capture_service = Arc::clone(&service);
        let capture = thread::spawn(move || {
            let mut first = true;
            capture_service
                .capture_for_authorized_plugins_with(
                    ClipboardCapture::text("in-flight", "2026-10-30T00:00:01Z"),
                    |store, value| {
                        if first {
                            first = false;
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                        store.capture(value).map(|_| ())
                    },
                )
                .unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let revoke_service = Arc::clone(&service);
        let revoke = thread::spawn(move || {
            started_tx.send(()).unwrap();
            revoke_service
                .sync_authorized_plugins(["b".into()])
                .unwrap();
            finished_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let returned_early = finished_rx.recv_timeout(Duration::from_millis(150)).is_ok();
        release_tx.send(()).unwrap();
        capture.join().unwrap();
        revoke.join().unwrap();
        service
            .capture_for_test(ClipboardCapture::text("after", "2026-10-30T00:00:02Z"))
            .unwrap();
        let a = service.snapshot("a").unwrap();
        let b = service.snapshot("b").unwrap();
        let authorized = service.authorized_plugins_for_test();
        drop(service);
        fs::remove_dir_all(root).unwrap();
        assert!(
            !returned_early,
            "revocation returned while a capture could still write revoked history"
        );
        assert_eq!(
            a.entries.len(),
            2,
            "retain prior data but do not record after revoke"
        );
        assert_eq!(
            b.entries.len(),
            3,
            "the other authorized plugin keeps recording"
        );
        assert_eq!(authorized, vec!["b"]);
    }
}
