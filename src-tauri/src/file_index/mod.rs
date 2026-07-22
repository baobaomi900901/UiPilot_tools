use std::{
    collections::{HashMap, HashSet},
    fs, io,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
};

#[cfg(not(test))]
use std::{sync::mpsc, thread};

use icu_casemap::CaseMapper;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
    UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
};

use crate::{
    lifecycle::{FileIndexPhase, LifecycleCoordinator},
    result_registry::{QueryDomain, ResultRegistry},
};

mod store;
mod windows_backend;

use store::{ordinal_sort_identity, Store, StoreError, StoreQueryResult};
#[cfg(not(test))]
use windows_backend::{
    filter_replay_events, fixed_volumes, materialize_events, scan_volume, system_exclusions,
    ScanSummary, Watcher,
};
use windows_backend::{BackendError, EventBuffer, ExcludedPrefix, FixedVolume};

pub(crate) const FOLD_ALGORITHM_ID: &str = "uipilot-unicode-15.1-full-fold-nfc-v1";

pub(crate) fn fold_name(value: &str) -> String {
    let first_nfc: String = value.nfc().collect();
    let folded = CaseMapper::new().fold_string(&first_nfc);
    folded.nfc().collect()
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc, Arc, Barrier, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    use super::{
        authenticate_app_data_root, begin_lazy_init_locked, fold_name, open_store,
        validate_index_path_shape, AdmissionError, FileCategory, FileIndex, FileIndexError,
        FileIndexStatus, FileSort, IndexState, IndexedKind, LazyInitDecision, LifecycleMode,
        OpenIndexedPath, QuerySpec, StoreError, VolumeIdentity, VolumeRuntime, FOLD_ALGORITHM_ID,
    };
    use icu_casemap::CaseMapper;
    use rusqlite::Connection;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    use super::store::{Store, TestEntry};
    use crate::result_registry::{QueryDomain, RegistryError, ResultAction, ResultRegistry};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            Self(
                std::env::temp_dir()
                    .join(format!("uipilot-file-index-{}-{id}", std::process::id())),
            )
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn query() -> QuerySpec {
        QuerySpec {
            folded_query: "find".into(),
            category: FileCategory::All,
            sort: FileSort::ModifiedDesc,
        }
    }

    #[test]
    fn production_callbacks_use_one_managed_index() {
        let lib = include_str!("../lib.rs").replace("\r\n", "\n");
        let production = lib
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("test module marker is missing");
        assert_eq!(
            production
                .matches("let file_index = Arc::new(file_index::FileIndex::new(")
                .count(),
            1
        );
        assert_eq!(
            production
                .matches(".manage(Arc::clone(&file_index))")
                .count(),
            1
        );
        assert_eq!(
            production
                .matches("app.state::<Arc<file_index::FileIndex>>()")
                .count(),
            1
        );
        assert_eq!(
            include_str!("../lifecycle.rs")
                .replace("\r\n", "\n")
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
                .matches("Arc::clone(app.state::<Arc<FileIndex>>().inner())")
                .count(),
            2
        );
        assert_eq!(
            production
                .matches("let run_file_index = Arc::clone(&file_index);")
                .count(),
            1
        );

        let run_exit = production
            .split("tauri::RunEvent::Exit => {")
            .nth(1)
            .and_then(|tail| tail.split("_ => {}").next())
            .expect("run exit branch is missing");
        assert!(run_exit.contains("run_file_index.enter_terminal();"));
        assert!(!production.contains("FileIndex::default()"));
        assert!(!production.contains("file_index::FileIndex::default()"));
    }

    #[test]
    fn dependency_contract_unicode_15_1_and_full_fold() {
        assert_eq!(FOLD_ALGORITHM_ID, "uipilot-unicode-15.1-full-fold-nfc-v1");
        assert_eq!(unicode_normalization::UNICODE_VERSION, (15, 1, 0));
        for (input, expected) in [
            ("UiPilot", "uipilot"),
            ("Straße", "strasse"),
            ("CAFE\u{301}", "café"),
            ("Σ", "σ"),
            ("σ", "σ"),
            ("ς", "σ"),
            ("İ", "i\u{307}"),
            ("Ｕｉ", "ｕｉ"),
        ] {
            assert_eq!(fold_name(input), expected);
        }
        assert_ne!(fold_name("Ｕｉ"), "ui");
        let mapper = CaseMapper::new();
        assert_eq!(mapper.simple_fold('\u{1fd3}'), '\u{0390}');
        assert_eq!(mapper.simple_fold('\u{1fe3}'), '\u{03b0}');
        assert_eq!(mapper.simple_fold('\u{fb05}'), '\u{fb06}');
    }

    #[test]
    fn dependency_contract_bundled_sqlite_identity_and_fts5() {
        let connection = Connection::open_in_memory().unwrap();
        let identity = connection
            .query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap();
        assert_eq!(identity.0, "3.53.2");
        assert_eq!(
            identity.1,
            "2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        connection.execute_batch("CREATE VIRTUAL TABLE names USING fts5(value, tokenize='trigram case_sensitive 1');").unwrap();
    }

    #[test]
    fn base_publication_state_linearizes_query_and_nls_reindex() {
        let dir = TestDir::new();
        fs::create_dir_all(dir.path()).unwrap();
        let database = dir.path().join("revision.sqlite3");
        let mut state = IndexState {
            store: Some(Store::open(&database, "identity-a").unwrap()),
            ..IndexState::default()
        };
        let publication_generation = AtomicU64::new(0);
        assert_eq!(state.index_revision_high_water, 0);
        assert_eq!(
            state
                .advance_revision_locked(&publication_generation)
                .unwrap(),
            1
        );
        assert_eq!(state.index_revision_high_water, 1);
        assert_eq!(state.store.as_ref().unwrap().index_revision_for_test(), 1);

        state.index_revision_high_water = u64::MAX;
        assert_eq!(
            state.advance_revision_locked(&publication_generation),
            Err(AdmissionError::CounterExhausted)
        );
        assert!(!state.fatal_unavailable);
        assert_eq!(
            Store::open(&database, "identity-a")
                .unwrap()
                .index_revision_for_test(),
            1
        );
    }

    #[test]
    fn revision_persistence_failure_does_not_advance_memory_and_latches_unavailable() {
        let store = Store::open_in_memory_for_test("identity-a").unwrap();
        store.remove_metadata_for_test();
        let mut state = IndexState {
            store: Some(store),
            ..IndexState::default()
        };
        let publication_generation = AtomicU64::new(0);

        assert_eq!(
            state.advance_revision_locked(&publication_generation),
            Err(AdmissionError::Unavailable)
        );
        assert_eq!(state.index_revision_high_water, 0);
        assert!(state.fatal_unavailable);
        assert!(!state.admission_open);
    }

    #[test]
    fn base_runtime_epoch_authorizes_shared_publication() {
        let index = Arc::new(FileIndex::default());
        assert!(index.authorizes_publication(0, 0));

        index.publication_runtime_epoch.store(1, Ordering::Release);
        assert!(!index.authorizes_publication(0, 0));
        assert!(index.authorizes_publication(1, 0));
    }

    #[test]
    fn lazy_init_has_one_opening_owner_and_concurrent_building_snapshot() {
        let mut state = IndexState::default();
        let publication_generation = AtomicU64::new(0);
        assert_eq!(
            begin_lazy_init_locked(&mut state, 0, &publication_generation),
            Ok(LazyInitDecision::Start { owner: 1 })
        );
        assert_eq!(state.mode, LifecycleMode::Opening { owner: 1 });
        assert!(!state.admission_open);

        let before = (
            state.mode,
            state.lazy_owner_high_water,
            state.admission_open,
            state.runtime_epoch,
        );
        assert_eq!(
            begin_lazy_init_locked(&mut state, 0, &publication_generation),
            Ok(LazyInitDecision::ObserveBuilding)
        );
        assert_eq!(
            (
                state.mode,
                state.lazy_owner_high_water,
                state.admission_open,
                state.runtime_epoch,
            ),
            before
        );

        assert_eq!(
            begin_lazy_init_locked(&mut state, 1, &publication_generation),
            Err(AdmissionError::EpochMismatch)
        );
        state.mode = LifecycleMode::Active;
        assert_eq!(
            begin_lazy_init_locked(&mut state, 0, &publication_generation),
            Err(AdmissionError::WrongMode)
        );

        state = IndexState::default();
        state.lazy_owner_high_water = u64::MAX;
        assert_eq!(
            begin_lazy_init_locked(&mut state, 0, &publication_generation),
            Err(AdmissionError::OwnerExhausted)
        );
        assert!(!state.fatal_unavailable);
        let index = Arc::new(FileIndex::default());
        index.state.lock().unwrap().lazy_owner_high_water = u64::MAX;
        assert!(index.begin_search(0).is_err());
        assert!(index.state.lock().unwrap().fatal_unavailable);
    }

    #[test]
    fn app_data_root_is_canonical_and_rejects_wrong_kind_or_reparse_shape() {
        let dir = TestDir::new();
        let database = authenticate_app_data_root(dir.path()).unwrap();
        let canonical_root = fs::canonicalize(dir.path()).unwrap();
        assert_eq!(database.parent(), Some(canonical_root.as_path()));
        assert_eq!(database.file_name().unwrap(), "file-index.sqlite3");

        let file_root = dir.path().join("not-a-directory");
        fs::write(&file_root, b"x").unwrap();
        assert!(authenticate_app_data_root(&file_root).is_err());

        assert!(
            validate_index_path_shape(true, false, FILE_ATTRIBUTE_REPARSE_POINT.0, true,).is_err()
        );
        assert!(
            validate_index_path_shape(false, true, FILE_ATTRIBUTE_REPARSE_POINT.0, false,).is_err()
        );

        let trusted = dir.path().join("trusted");
        fs::create_dir_all(&trusted).unwrap();
        let index = Arc::new(FileIndex::default());
        index
            .search_with(
                Path::new(r"C:\untrusted-alias"),
                query(),
                0,
                |_| Ok(trusted.join("file-index.sqlite3")),
                |_| {
                    Ok((
                        Store::open_in_memory_for_test("identity-a").unwrap(),
                        0,
                        None,
                    ))
                },
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(
            index
                .state
                .lock()
                .unwrap()
                .authenticated_app_data_root
                .as_deref(),
            Some(trusted.as_path())
        );
        index
            .search_with(
                Path::new(r"C:\replaced-alias"),
                query(),
                0,
                |_| panic!("active query must not reauthenticate an untrusted alias"),
                |_| panic!("active query must not reopen the store"),
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(
            index
                .state
                .lock()
                .unwrap()
                .authenticated_app_data_root
                .as_deref(),
            Some(trusted.as_path())
        );
    }

    #[test]
    fn store_failure_latches_and_second_query_performs_zero_store_work() {
        let index = Arc::new(FileIndex::default());
        let dir = TestDir::new();
        fs::create_dir_all(dir.path()).unwrap();
        let database = dir.path().join("file-index.sqlite3");
        Connection::open(&database)
            .unwrap()
            .execute_batch("CREATE TABLE foreign_data(value TEXT);")
            .unwrap();
        let auth_calls = Cell::new(0);
        let open_calls = Cell::new(0);
        let query_calls = Cell::new(0);

        let first = index.search_with(
            dir.path(),
            query(),
            0,
            |root| {
                auth_calls.set(auth_calls.get() + 1);
                Ok(root.join("file-index.sqlite3"))
            },
            |path| {
                open_calls.set(open_calls.get() + 1);
                open_store(path)
            },
            |_, _| {
                query_calls.set(query_calls.get() + 1);
                unreachable!()
            },
        );
        assert!(first.is_err());

        let second = index
            .search_with(
                dir.path(),
                query(),
                0,
                |root| {
                    auth_calls.set(auth_calls.get() + 1);
                    Ok(root.join("file-index.sqlite3"))
                },
                |path| {
                    open_calls.set(open_calls.get() + 1);
                    open_store(path)
                },
                |_, _| {
                    query_calls.set(query_calls.get() + 1);
                    unreachable!()
                },
            )
            .unwrap();
        assert_eq!(second.status, FileIndexStatus::Unavailable);
        assert_eq!(auth_calls.get(), 1);
        assert_eq!(open_calls.get(), 1);
        assert_eq!(query_calls.get(), 0);
    }

    #[test]
    fn query_failure_latches_and_observer_performs_zero_authentication() {
        let index = Arc::new(FileIndex::default());
        let dir = TestDir::new();
        let open_calls = Cell::new(0);
        let query_calls = Cell::new(0);
        let first_stop = Arc::new(AtomicBool::new(false));
        let second_stop = Arc::new(AtomicBool::new(false));
        let integrity_stop = Arc::new(AtomicBool::new(false));
        let integrity_effects = Arc::new(AtomicU64::new(0));
        let integrity_thread_stop = Arc::clone(&integrity_stop);
        let integrity_thread_effects = Arc::clone(&integrity_effects);
        let integrity_join = thread::spawn(move || {
            while !integrity_thread_stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
            let _ = &integrity_thread_effects;
        });
        *index.integrity_worker.lock().unwrap() = Some(super::IntegrityWorkerRecord {
            runtime_epoch: 0,
            stop: Arc::clone(&integrity_stop),
            join: Some(integrity_join),
        });
        let first_identity = volume();
        let mut second_identity = volume();
        second_identity.volume_serial = 43;
        {
            let mut workers = index.workers.lock().unwrap();
            for (identity, owner, mount, stop) in [
                (first_identity.clone(), 1, r"C:\", Arc::clone(&first_stop)),
                (second_identity.clone(), 2, r"D:\", Arc::clone(&second_stop)),
            ] {
                workers.by_volume.insert(
                    identity,
                    super::WorkerRecord {
                        owner,
                        runtime_epoch: index.runtime_epoch(),
                        mount_point: PathBuf::from(mount),
                        stop,
                        generation: Arc::new(AtomicU64::new(1)),
                        join: None,
                        failed: false,
                    },
                );
            }
        }
        {
            let mut coordinator = index.coordinator.state.lock().unwrap();
            coordinator.pending_root = Some(PathBuf::from(r"C:\app-data"));
            coordinator.active_root = coordinator.pending_root.clone();
            coordinator
                .volumes
                .insert(first_identity, super::VolumeRuntime::default());
        }
        assert!(index
            .search_with(
                dir.path(),
                query(),
                0,
                |root| Ok(root.join("file-index.sqlite3")),
                |_| {
                    open_calls.set(open_calls.get() + 1);
                    Ok((
                        Store::open_in_memory_for_test("identity-a").unwrap(),
                        0,
                        None,
                    ))
                },
                |_, _| {
                    query_calls.set(query_calls.get() + 1);
                    Err(StoreError::InvalidData)
                },
            )
            .is_err());
        assert!(first_stop.load(Ordering::Acquire));
        assert!(second_stop.load(Ordering::Acquire));
        assert!(integrity_stop.load(Ordering::Acquire));
        assert!(index.coordinator.stop.load(Ordering::Acquire));
        while index
            .integrity_worker
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|worker| worker.join.as_ref())
            .is_some_and(|join| !join.is_finished())
        {
            thread::yield_now();
        }
        index.reap_finished_integrity();
        assert!(index.integrity_worker.lock().unwrap().is_none());
        assert_eq!(integrity_effects.load(Ordering::Acquire), 0);
        {
            let coordinator = index.coordinator.state.lock().unwrap();
            assert!(coordinator.pending_root.is_none());
            assert!(coordinator.active_root.is_none());
            assert!(coordinator.volumes.is_empty());
        }
        let second = index
            .search_with(
                dir.path(),
                query(),
                0,
                |_| panic!("fatal query must not authenticate again"),
                |_| panic!("fatal query must not reopen"),
                |_, _| panic!("fatal query must not touch the store"),
            )
            .unwrap();
        assert_eq!(second.status, FileIndexStatus::Unavailable);
        assert_eq!(open_calls.get(), 1);
        assert_eq!(query_calls.get(), 1);
        assert!(!index.schedule_calibration());

        let observer = Arc::new(FileIndex::default());
        {
            let mut state = observer.state.lock().unwrap();
            state.mode = LifecycleMode::Opening { owner: 1 };
            state.lazy_owner_high_water = 1;
        }
        let auth_calls = Cell::new(0);
        let building = observer
            .search_with(
                dir.path(),
                query(),
                0,
                |_| {
                    auth_calls.set(auth_calls.get() + 1);
                    Err(FileIndexError::Unavailable)
                },
                |_| panic!("observer must not open"),
                |_, _| panic!("observer must not query"),
            )
            .unwrap();
        assert_eq!(building.status, FileIndexStatus::Building);
        assert_eq!(auth_calls.get(), 0);
    }

    #[test]
    fn fatal_latch_invalidates_captured_building_before_publication() {
        let index = Arc::new(FileIndex::default());
        let dir = TestDir::new();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let owner_index = Arc::clone(&index);
        let owner_root = dir.path().to_path_buf();
        let owner_entered = Arc::clone(&entered);
        let owner_release = Arc::clone(&release);
        let owner = thread::spawn(move || {
            owner_index.search_with(
                &owner_root,
                query(),
                0,
                move |root| {
                    owner_entered.wait();
                    owner_release.wait();
                    Ok(root.join("file-index.sqlite3"))
                },
                |_| Err(FileIndexError::Unavailable),
                |_, _| panic!("failed open must not query"),
            )
        });

        entered.wait();
        let building = index
            .search_with(
                dir.path(),
                query(),
                0,
                |_| panic!("observer must not authenticate"),
                |_| panic!("observer must not open"),
                |_, _| panic!("observer must not query"),
            )
            .unwrap();
        assert_eq!(building.status, FileIndexStatus::Building);

        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let token = registry
            .begin_query(QueryDomain::File, "invocation", 1)
            .unwrap();
        release.wait();
        assert!(owner.join().unwrap().is_err());

        let stale = registry.publish_if_latest(
            token,
            Vec::<((), ResultAction)>::new(),
            || {
                index
                    .authorizes_publication(building.runtime_epoch, building.publication_generation)
            },
            |request_id, _| request_id,
        );
        assert_eq!(stale, None);

        let unavailable = index
            .search_with(
                dir.path(),
                query(),
                0,
                |_| panic!("fatal query must not authenticate"),
                |_| panic!("fatal query must not reopen"),
                |_, _| panic!("fatal query must not touch the store"),
            )
            .unwrap();
        assert_eq!(unavailable.status, FileIndexStatus::Unavailable);
        let published = registry
            .publish_if_latest(
                token,
                Vec::<((), ResultAction)>::new(),
                || {
                    index.authorizes_publication(
                        unavailable.runtime_epoch,
                        unavailable.publication_generation,
                    )
                },
                |request_id, items| (request_id, items),
            )
            .unwrap();
        assert_eq!(published.0, "req-0000000000000001");
        assert!(published.1.is_empty());
        assert_eq!(
            registry.resolve(&published.0, "item-0000000000000002"),
            Err(RegistryError::UnknownResult)
        );
    }

    fn candidate_entry(name: &str, generation: u64) -> TestEntry {
        TestEntry {
            relative_path: name.into(),
            display_path: format!(r"C:\{name}"),
            name: name.into(),
            folded_name: fold_name(name),
            kind: super::IndexedKind::File,
            category: "all".into(),
            size_bytes: Some(4),
            modified_utc_ms: 1_725_120_000_000,
            generation,
        }
    }

    fn volume() -> super::VolumeIdentity {
        super::VolumeIdentity {
            volume_guid_path: r"\\?\VOLUME{TASK4}\".into(),
            volume_serial: 42,
            filesystem_name: "NTFS".into(),
        }
    }

    fn file_action() -> ResultAction {
        ResultAction::OpenIndexedPath(OpenIndexedPath::for_test(
            0,
            1,
            volume(),
            "file.txt",
            IndexedKind::File,
        ))
    }

    #[test]
    fn coordinator_consumes_existing_active_lazy_open() {
        let index = Arc::new(FileIndex::default());
        let sentinel = Store::open_in_memory_for_test("identity-a").unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(sentinel);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }

        assert!(index.schedule_calibration());
        assert!(!index.schedule_calibration());
        let snapshot = index.coordinator_snapshot_for_test();
        assert_eq!(snapshot.pending_signals, 1);
        assert_eq!(snapshot.wakes, 1);
        assert_eq!(snapshot.thread_starts, 1);
        assert_eq!(index.state.lock().unwrap().mode, LifecycleMode::Active);
    }

    #[test]
    fn coordinator_does_not_claim_calibration_after_phase_store() {
        let (index, lifecycle) = active_task7_index();
        {
            let mut coordinator = index.coordinator.state.lock().unwrap();
            coordinator.pending_root = Some(PathBuf::from(r"C:\app-data"));
            coordinator.pending_runtime_epoch = Some(0);
            coordinator.thread_started = true;
        }

        let claimed = index.claim_calibration_run_with(|| {
            lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);
        });

        assert!(claimed.is_none());
        assert_eq!(index.db_work_count_for_test(), 0);
        let coordinator = index.coordinator.state.lock().unwrap();
        assert!(!coordinator.running);
        assert_eq!(
            coordinator.pending_root,
            Some(PathBuf::from(r"C:\app-data"))
        );
    }

    #[test]
    fn coordinator_rejects_pending_from_an_old_runtime_epoch() {
        let (index, _) = active_task7_index();
        {
            let mut coordinator = index.coordinator.state.lock().unwrap();
            coordinator.pending_root = Some(PathBuf::from(r"C:\app-data"));
            coordinator.pending_runtime_epoch = Some(1);
        }

        assert!(index.claim_calibration_run_with(|| {}).is_none());
        assert_eq!(index.db_work_count_for_test(), 0);
        assert!(!index.coordinator.state.lock().unwrap().running);
    }

    #[test]
    fn integrity_schedule_after_phase_store_creates_no_coordinator_owner() {
        let (index, lifecycle) = active_task7_index();
        index.state.lock().unwrap().integrity_pending = true;
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);

        assert!(!index.schedule_integrity());
        assert!(index.coordinator.join.lock().unwrap().is_none());
        assert!(!index.coordinator.state.lock().unwrap().thread_started);
        assert_eq!(index.db_work_count_for_test(), 0);
    }

    #[test]
    fn integrity_worker_runs_once_and_records_timestamp() {
        let directory = TestDir::new();
        fs::create_dir_all(directory.path()).unwrap();
        let database = authenticate_app_data_root(directory.path()).unwrap();
        let root = database.parent().unwrap().to_path_buf();
        let store = Store::open(&database, "identity-a").unwrap();
        let (index, _) = active_task7_index();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.authenticated_app_data_root = Some(root);
            state.integrity_pending = true;
        }

        assert!(index.drive_integrity());
        assert!(!index.drive_integrity());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let finished = index
                .integrity_worker
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|worker| worker.join.as_ref())
                .is_some_and(thread::JoinHandle::is_finished);
            if finished {
                break;
            }
            assert!(Instant::now() < deadline, "integrity worker did not finish");
            thread::yield_now();
        }
        index.reap_finished_integrity();

        assert!(index.integrity_worker.lock().unwrap().is_none());
        assert_eq!(index.db_work_count_for_test(), 0);
        let state = index.state.lock().unwrap();
        assert!(state.integrity_started);
        assert!(!state.integrity_pending);
        drop(state);
        let timestamp = Connection::open(&database)
            .unwrap()
            .query_row(
                "SELECT last_integrity_check_utc FROM metadata WHERE singleton=1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .unwrap();
        assert!(timestamp.is_some());
        drop(index);
    }

    #[test]
    fn integrity_timestamp_rejects_phase_or_authenticated_path_changes() {
        let directory = TestDir::new();
        fs::create_dir_all(directory.path()).unwrap();
        let database = authenticate_app_data_root(directory.path()).unwrap();
        drop(Store::open(&database, "identity-a").unwrap());
        let root = database.parent().unwrap().to_path_buf();
        let (index, lifecycle) = active_task7_index();
        let stop = Arc::new(AtomicBool::new(false));
        *index.integrity_worker.lock().unwrap() = Some(super::IntegrityWorkerRecord {
            runtime_epoch: 0,
            stop: Arc::clone(&stop),
            join: None,
        });

        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);
        assert!(matches!(
            index.record_integrity_timestamp(&root, &database, 0, &stop),
            Err(StoreError::InvalidData)
        ));
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Running, 1);
        let other_root = directory.path().join("replacement");
        fs::create_dir_all(&other_root).unwrap();
        assert!(matches!(
            index.record_integrity_timestamp(&other_root, &database, 0, &stop),
            Err(StoreError::InvalidData)
        ));

        let timestamp = Connection::open(&database)
            .unwrap()
            .query_row(
                "SELECT last_integrity_check_utc FROM metadata WHERE singleton=1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .unwrap();
        assert_eq!(timestamp, None);
        index.integrity_worker.lock().unwrap().take();
        drop(index);
    }

    #[test]
    fn inventory_change_during_running_calibration_remains_pending() {
        let index = Arc::new(FileIndex::default());
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        {
            let mut coordinator = index.coordinator.state.lock().unwrap();
            coordinator.thread_started = true;
            coordinator.running = true;
        }

        assert!(index.schedule_calibration());
        let coordinator = index.coordinator.state.lock().unwrap();
        assert_eq!(
            coordinator.pending_root,
            Some(PathBuf::from(r"C:\app-data"))
        );
        assert!(!coordinator.calibrated);
        drop(coordinator);
        FileIndex::finish_calibration_run(&index.coordinator, true);
        let coordinator = index.coordinator.state.lock().unwrap();
        assert_eq!(
            coordinator.pending_root,
            Some(PathBuf::from(r"C:\app-data"))
        );
        assert!(!coordinator.running);
        assert!(!coordinator.calibrated);
    }

    #[test]
    fn coordinator_inventory_reconcile_quarantines_before_worker_start() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-old.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_volumes = vec![identity.clone()];
            state.authenticated_mounts = vec![(identity.clone(), r"C:\".into())];
        }
        let remounted = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        let before_record = index
            .state
            .lock()
            .unwrap()
            .store
            .as_ref()
            .unwrap()
            .index_revision_for_test();
        {
            let mut state = index.state.lock().unwrap();
            assert!(index
                .record_inventory_locked(&mut state, std::slice::from_ref(&remounted))
                .unwrap());
        }
        let concurrent = Arc::clone(&index);
        let concurrent_identity = identity.clone();
        thread::spawn(move || {
            let mut state = concurrent.state.lock().unwrap();
            assert!(!state.authenticated_volumes.contains(&concurrent_identity));
            let identities = state.authenticated_volumes.clone();
            let snapshot = state
                .store
                .as_mut()
                .unwrap()
                .query_for_test(&query(), &identities)
                .unwrap();
            assert_eq!(snapshot.index_revision, before_record);
            assert_eq!(snapshot.total, 0);
        })
        .join()
        .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            assert!(index.reconcile_inventory_locked(&mut state).unwrap());
        }
        let state = index.state.lock().unwrap();
        assert!(state.quarantined_volumes.contains(&identity));
        assert!(state.authenticated_volumes.is_empty());
        assert_eq!(
            state
                .store
                .as_ref()
                .unwrap()
                .mount_point_for_test(&identity),
            r"D:\"
        );
        assert!(state.index_revision_high_water > 0);
        drop(state);

        let stale_observation = index.state.lock().unwrap().inventory_observation;
        index.refresh_query_volumes_with(|| Ok(Vec::new())).unwrap();
        assert_eq!(
            index
                .reconcile_calibration_inventory(
                    stale_observation,
                    std::slice::from_ref(&remounted),
                )
                .unwrap(),
            None
        );
        assert!(index.state.lock().unwrap().authenticated_mounts.is_empty());
    }

    #[test]
    fn repeated_inventory_does_not_authenticate_quarantined_volume() {
        let index = Arc::new(FileIndex::default());
        let ready = volume();
        let mut pending_identity = volume();
        pending_identity.volume_serial = 43;
        let ready_volume = super::FixedVolume {
            identity: ready.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let pending_volume = super::FixedVolume {
            identity: pending_identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        let inventory = vec![ready_volume, pending_volume];
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&ready, [candidate_entry("find-ready.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_volumes = vec![ready.clone()];
            state.authenticated_mounts = vec![(ready.clone(), r"C:\".into())];
            index
                .record_inventory_locked(&mut state, &inventory)
                .unwrap();
            index.reconcile_inventory_locked(&mut state).unwrap();
            assert_eq!(state.authenticated_volumes, vec![ready.clone()]);
            assert!(state.quarantined_volumes.contains(&pending_identity));
        }
        let before = {
            let mut state = index.state.lock().unwrap();
            let identities = state.authenticated_volumes.clone();
            let result = state
                .store
                .as_mut()
                .unwrap()
                .query_for_test(&query(), &identities)
                .unwrap();
            (result.index_revision, result.status, result.total)
        };

        {
            let mut state = index.state.lock().unwrap();
            index
                .record_inventory_locked(&mut state, &inventory)
                .unwrap();
            index.reconcile_inventory_locked(&mut state).unwrap();
            assert_eq!(state.authenticated_volumes, vec![ready.clone()]);
            assert!(state.quarantined_volumes.contains(&pending_identity));
            let identities = state.authenticated_volumes.clone();
            let result = state
                .store
                .as_mut()
                .unwrap()
                .query_for_test(&query(), &identities)
                .unwrap();
            assert_eq!((result.index_revision, result.status, result.total), before);
        }
    }

    #[test]
    fn brand_new_candidate_opens_provisionally_but_remount_stays_quarantined() {
        fn install_worker(index: &Arc<FileIndex>, volume: &super::FixedVolume) -> u64 {
            let owner = match index.prepare_worker(volume).unwrap() {
                super::WorkerPreparation::Start { owner } => owner,
                super::WorkerPreparation::Existing => panic!("worker must be new"),
            };
            index.install_worker(
                volume,
                super::WorkerRecord {
                    owner,
                    runtime_epoch: index.runtime_epoch(),
                    mount_point: volume.mount_point.clone(),
                    stop: Arc::new(AtomicBool::new(false)),
                    generation: Arc::new(AtomicU64::new(0)),
                    join: None,
                    failed: false,
                },
            );
            owner
        }

        let index = Arc::new(FileIndex::default());
        let ready = volume();
        let mut new_identity = volume();
        new_identity.volume_serial = 43;
        let new_volume = super::FixedVolume {
            identity: new_identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&ready, [candidate_entry("find-ready.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_volumes = vec![ready.clone()];
            state.authenticated_mounts = vec![
                (ready.clone(), r"C:\".into()),
                (new_identity.clone(), r"D:\".into()),
            ];
            state.quarantined_volumes.insert(new_identity.clone());
        }
        let owner = install_worker(&index, &new_volume);
        let (generation, has_committed) = index
            .begin_worker_candidate(&new_volume, owner, index.runtime_epoch())
            .unwrap();
        assert!(!has_committed);
        {
            let mut state = index.state.lock().unwrap();
            assert_eq!(state.index_revision_high_water, 1);
            assert!(state.authenticated_volumes.contains(&new_identity));
            assert!(!state.quarantined_volumes.contains(&new_identity));
            let identities = state.authenticated_volumes.clone();
            let revision = state
                .store
                .as_mut()
                .unwrap()
                .append_candidate_for_test_with_identities(
                    &new_identity,
                    generation,
                    [candidate_entry("find-new.txt", generation)],
                    &identities,
                )
                .unwrap();
            state.index_revision_high_water = revision;
            let visible = state
                .store
                .as_mut()
                .unwrap()
                .query_for_test(&query(), &identities)
                .unwrap();
            assert_eq!(visible.status, FileIndexStatus::Building);
            assert_eq!(visible.total, 2);
            assert_eq!(visible.index_revision, 2);
        }
        index.mark_fixed_volume_dirty_for_test(&new_volume).unwrap();
        {
            let mut state = index.state.lock().unwrap();
            let identities = state.authenticated_volumes.clone();
            let visible = state
                .store
                .as_mut()
                .unwrap()
                .query_for_test(&query(), &identities)
                .unwrap();
            assert_eq!(visible.status, FileIndexStatus::Building);
            assert_eq!(visible.total, 1);
            assert_eq!(visible.index_revision, 3);
        }

        let first = Arc::new(FileIndex::default());
        let first_owner = {
            let mut state = first.state.lock().unwrap();
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_mounts = vec![(new_identity.clone(), r"D:\".into())];
            state.quarantined_volumes.insert(new_identity.clone());
            drop(state);
            install_worker(&first, &new_volume)
        };
        first
            .begin_worker_candidate(&new_volume, first_owner, first.runtime_epoch())
            .unwrap();
        assert_eq!(first.state.lock().unwrap().index_revision_high_water, 0);

        let remount = Arc::new(FileIndex::default());
        let mut remount_store = Store::open_in_memory_for_test("identity-a").unwrap();
        remount_store
            .seed_committed_for_test(&new_identity, [candidate_entry("find-old-mount.txt", 1)])
            .unwrap();
        {
            let mut state = remount.state.lock().unwrap();
            state.store = Some(remount_store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_mounts = vec![(new_identity.clone(), r"D:\".into())];
            state.quarantined_volumes.insert(new_identity.clone());
        }
        let remount_owner = install_worker(&remount, &new_volume);
        assert!(
            remount
                .begin_worker_candidate(&new_volume, remount_owner, remount.runtime_epoch())
                .unwrap()
                .1
        );
        let state = remount.state.lock().unwrap();
        assert!(!state.authenticated_volumes.contains(&new_identity));
        assert!(state.quarantined_volumes.contains(&new_identity));
    }

    #[test]
    fn successful_remount_commit_restores_visibility_in_same_gate() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let remounted = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-old.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_mounts = vec![(identity.clone(), r"D:\".into())];
            state.quarantined_volumes.insert(identity.clone());
        }
        let owner = match index.prepare_worker(&remounted).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("worker must start"),
        };
        let generation_owner = Arc::new(AtomicU64::new(0));
        index.install_worker(
            &remounted,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: remounted.mount_point.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                generation: Arc::clone(&generation_owner),
                join: None,
                failed: false,
            },
        );
        let (generation, has_committed) = index
            .begin_worker_candidate(&remounted, owner, index.runtime_epoch())
            .unwrap();
        assert!(has_committed);
        generation_owner.store(generation, Ordering::Release);
        let mut current = candidate_entry("find-current.txt", generation);
        current.display_path = r"D:\find-current.txt".into();

        index
            .commit_worker_candidate(
                &remounted,
                owner,
                generation,
                vec![super::IndexEntry::from(current)],
                &[],
                |_| Ok(()),
            )
            .unwrap();

        let mut state = index.state.lock().unwrap();
        assert_eq!(state.authenticated_volumes, vec![identity.clone()]);
        assert!(!state.quarantined_volumes.contains(&identity));
        assert_eq!(state.index_revision_high_water, 1);
        let identities = state.authenticated_volumes.clone();
        let visible = state
            .store
            .as_mut()
            .unwrap()
            .query_for_test(&query(), &identities)
            .unwrap();
        assert_eq!(visible.index_revision, 1);
        assert_eq!(visible.status, FileIndexStatus::Ready);
        assert_eq!(visible.total, 1);
        assert_eq!(visible.entries[0].name, "find-current.txt");
        assert_eq!(visible.entries[0].display_path, r"D:\find-current.txt");
    }

    #[test]
    fn arm_failure_before_candidate_is_volume_local_and_retryable() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        let owner = match index.prepare_worker(&fixed).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("first worker must start"),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
        });
        index.install_worker(
            &fixed,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: fixed.mount_point.clone(),
                stop,
                generation: Arc::new(AtomicU64::new(0)),
                join: Some(join),
                failed: false,
            },
        );

        index.handle_worker_failure_for_test(&fixed, owner);
        let mut state = index.state.lock().unwrap();
        assert!(!state.fatal_unavailable);
        assert!(state.admission_open);
        let result = state
            .store
            .as_mut()
            .unwrap()
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(result.status, FileIndexStatus::Building);
        assert_eq!(result.total, 0);
        drop(state);
        assert!(index.schedule_calibration());
    }

    #[test]
    fn coordinator_preflight_failure_is_process_fatal_and_not_rescheduled() {
        let index = Arc::new(FileIndex::default());
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        assert!(index.schedule_calibration());
        assert!(index
            .calibration_inputs_with(
                || Err(FileIndexError::Unavailable),
                |_| panic!("exclusions must not run after volume inventory failure"),
            )
            .is_err());
        let state = index.state.lock().unwrap();
        assert!(state.fatal_unavailable);
        assert_eq!(state.availability, super::Availability::Unavailable);
        assert!(!state.admission_open);
        assert!(state.store.is_none());
        drop(state);
        assert!(!index.schedule_calibration());
    }

    #[test]
    fn volume_workers_are_unique_replaced_and_owner_checked() {
        fn install(
            index: &Arc<FileIndex>,
            volume: &super::FixedVolume,
            order: &Arc<Mutex<Vec<String>>>,
        ) -> (bool, u64) {
            let owner = match index.prepare_worker(volume).unwrap() {
                super::WorkerPreparation::Existing => {
                    let owner = index
                        .workers
                        .lock()
                        .unwrap()
                        .by_volume
                        .get(&volume.identity)
                        .unwrap()
                        .owner;
                    return (false, owner);
                }
                super::WorkerPreparation::Start { owner } => owner,
            };
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = Arc::clone(&stop);
            let thread_order = Arc::clone(order);
            let join = thread::spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                thread_order.lock().unwrap().push(format!("stop-{owner}"));
            });
            index.install_worker(
                volume,
                super::WorkerRecord {
                    owner,
                    runtime_epoch: index.runtime_epoch(),
                    mount_point: volume.mount_point.clone(),
                    stop,
                    generation: Arc::new(AtomicU64::new(7)),
                    join: Some(join),
                    failed: false,
                },
            );
            order.lock().unwrap().push(format!("start-{owner}"));
            (true, owner)
        }

        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let order = Arc::new(Mutex::new(Vec::new()));
        let c = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let d = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_mounts = vec![(identity.clone(), r"C:\".into())];
        }
        let (started, old_owner) = install(&index, &c, &order);
        assert!(started);
        assert_eq!(install(&index, &c, &order), (false, old_owner));
        index.state.lock().unwrap().authenticated_mounts = vec![(identity.clone(), r"D:\".into())];
        let (started, new_owner) = install(&index, &d, &order);
        assert!(started);
        assert_ne!(old_owner, new_owner);
        assert_eq!(
            *order.lock().unwrap(),
            [
                format!("start-{old_owner}"),
                format!("stop-{old_owner}"),
                format!("start-{new_owner}"),
            ]
        );
        assert!(!index.worker_is_current(&c, old_owner, Some(7)));
        assert!(index.worker_is_current(&d, new_owner, Some(7)));

        {
            let mut state = index.state.lock().unwrap();
            state.authenticated_mounts = vec![(identity.clone(), r"D:\".into())];
            state.quarantined_volumes.insert(identity.clone());
        }
        assert!(index
            .complete_volume_calibration(&c, old_owner, index.runtime_epoch())
            .is_err());
        assert!(index
            .state
            .lock()
            .unwrap()
            .quarantined_volumes
            .contains(&identity));
        index
            .complete_volume_calibration(&d, new_owner, index.runtime_epoch())
            .unwrap();
        assert!(!index
            .state
            .lock()
            .unwrap()
            .quarantined_volumes
            .contains(&identity));
        assert!(index.mark_worker_stopped(&identity, new_owner));
        {
            let workers = index.workers.lock().unwrap();
            let failed = workers.by_volume.get(&identity).unwrap();
            assert!(failed.failed);
            assert!(failed.join.is_some());
        }
        let (started, replacement_owner) = install(&index, &d, &order);
        assert!(started);
        assert_ne!(replacement_owner, new_owner);
        assert!(index.worker_is_current(&d, replacement_owner, Some(7)));
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-kept.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
        }
        index.stop_detached_workers_for_test(&[]).unwrap();
        assert!(index.workers.lock().unwrap().by_volume.is_empty());
        assert!(index
            .begin_worker_candidate(&d, replacement_owner, index.runtime_epoch())
            .is_err());
        let state = index.state.lock().unwrap();
        let store = state.store.as_ref().unwrap();
        assert_eq!(store.generation_state_for_test(&identity).1, None);
        assert!(store.candidate_rows_for_test(&identity).is_empty());
        drop(state);
        let coordinator = Arc::clone(&index.coordinator);
        let exited = Arc::new(AtomicBool::new(false));
        let thread_exited = Arc::clone(&exited);
        let join = thread::spawn(move || {
            let mut state = coordinator.state.lock().unwrap();
            while !coordinator.stop.load(Ordering::Acquire) {
                state = coordinator.signal.wait(state).unwrap();
            }
            thread_exited.store(true, Ordering::Release);
        });
        *index.coordinator.join.lock().unwrap() = Some(join);
        drop(index);
        assert!(exited.load(Ordering::Acquire));
    }

    #[test]
    fn detached_worker_clears_candidate_and_preserves_committed_rows() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        let generation = store.begin_candidate_for_test(&identity, r"C:\").unwrap();
        store
            .append_candidate_for_test(
                &identity,
                generation,
                [candidate_entry("find-uncommitted.txt", generation)],
            )
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
        }

        let owner = match index.prepare_worker(&fixed).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("first worker must start"),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
        });
        index.install_worker(
            &fixed,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: fixed.mount_point.clone(),
                stop,
                generation: Arc::new(AtomicU64::new(generation)),
                join: Some(join),
                failed: false,
            },
        );

        index.stop_detached_workers_for_test(&[]).unwrap();
        let mut state = index.state.lock().unwrap();
        let store = state.store.as_mut().unwrap();
        assert_eq!(store.generation_state_for_test(&identity).1, None);
        assert!(store.candidate_rows_for_test(&identity).is_empty());
        let visible = store
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-stable.txt"));
        assert!(!visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-uncommitted.txt"));
    }

    #[test]
    fn stale_remount_worker_failure_cleans_candidate_on_current_mount() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let stale = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        let generation = store.begin_candidate_for_test(&identity, r"D:\").unwrap();
        store
            .append_candidate_for_test(
                &identity,
                generation,
                [candidate_entry("find-stale.txt", generation)],
            )
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_volumes = vec![identity.clone()];
            state.authenticated_mounts = vec![(identity.clone(), r"D:\".into())];
        }
        let owner = match index.prepare_worker(&stale).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("worker must start"),
        };
        let stop = Arc::new(AtomicBool::new(false));
        index.install_worker(
            &stale,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: stale.mount_point.clone(),
                stop: Arc::clone(&stop),
                generation: Arc::new(AtomicU64::new(generation)),
                join: None,
                failed: false,
            },
        );
        let current = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let before = {
            let mut state = index.state.lock().unwrap();
            index
                .record_inventory_locked(&mut state, std::slice::from_ref(&current))
                .unwrap();
            assert!(stop.load(Ordering::Acquire));
            index.reconcile_inventory_locked(&mut state).unwrap();
            state.store.as_ref().unwrap().index_revision_for_test()
        };

        index.mark_fixed_volume_dirty_for_test(&stale).unwrap();
        assert!(index
            .complete_volume_calibration(&stale, owner, index.runtime_epoch())
            .is_err());

        let mut state = index.state.lock().unwrap();
        assert!(state.quarantined_volumes.contains(&identity));
        let store = state.store.as_mut().unwrap();
        assert_eq!(store.mount_point_for_test(&identity), r"C:\");
        assert_eq!(store.generation_state_for_test(&identity).1, None);
        assert!(store.candidate_rows_for_test(&identity).is_empty());
        assert_eq!(store.index_revision_for_test(), before);
        let visible = store
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-stable.txt"));
        assert!(!visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-stale.txt"));
    }

    #[test]
    fn finished_or_panicked_worker_is_joined_and_never_reused() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        let owner = match index.prepare_worker(&fixed).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("first worker must start"),
        };
        let join = thread::spawn(|| panic!("worker panic fixture"));
        while !join.is_finished() {
            thread::yield_now();
        }
        index.install_worker(
            &fixed,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: fixed.mount_point.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                generation: Arc::new(AtomicU64::new(1)),
                join: Some(join),
                failed: false,
            },
        );

        index.coordinator.state.lock().unwrap().calibrated = true;
        assert!(index.schedule_calibration());
        assert!(matches!(
            index.prepare_worker(&fixed),
            Ok(super::WorkerPreparation::Start { owner: replacement }) if replacement != owner
        ));
        assert!(!index.state.lock().unwrap().fatal_unavailable);
    }

    #[test]
    fn startup_disconnect_after_candidate_clears_partial_state_and_allows_retry() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        let generation = store.begin_candidate_for_test(&identity, r"C:\").unwrap();
        store
            .append_candidate_for_test(
                &identity,
                generation,
                [candidate_entry("find-uncommitted.txt", generation)],
            )
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        let owner = match index.prepare_worker(&fixed).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("first worker must start"),
        };
        let join = thread::spawn(|| panic!("worker panic after candidate fixture"));
        while !join.is_finished() {
            thread::yield_now();
        }
        index.install_worker(
            &fixed,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: fixed.mount_point.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                generation: Arc::new(AtomicU64::new(generation)),
                join: Some(join),
                failed: false,
            },
        );
        let (completed_sender, completed_receiver) = mpsc::sync_channel::<bool>(1);
        drop(completed_sender);

        assert!(index
            .finish_worker_start(&fixed, owner, completed_receiver)
            .is_err());

        let mut state = index.state.lock().unwrap();
        assert!(!state.fatal_unavailable);
        assert!(state.admission_open);
        let store = state.store.as_mut().unwrap();
        assert_eq!(store.generation_state_for_test(&identity).1, None);
        assert!(store.candidate_rows_for_test(&identity).is_empty());
        let visible = store
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-stable.txt"));
        assert!(!visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-uncommitted.txt"));
        drop(state);
        assert!(index.workers.lock().unwrap().by_volume.is_empty());
        assert!(index.schedule_calibration());
    }

    #[test]
    fn query_reauthentication_excludes_detached_or_reused_volumes() {
        let index = Arc::new(FileIndex::default());
        let attached = volume();
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&attached, [candidate_entry("find-old.txt", 1)])
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_volumes = vec![attached.clone()];
            state.authenticated_mounts = vec![(attached.clone(), r"C:\".into())];
        }

        index.refresh_query_volumes_with(|| Ok(Vec::new())).unwrap();
        let detached = index
            .search_with(
                Path::new(r"C:\untrusted"),
                query(),
                0,
                |_| panic!("active query must not reauthenticate app-data"),
                |_| panic!("active query must not reopen"),
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(detached.total, 0);

        index
            .refresh_query_volumes_with(|| {
                Ok(vec![super::FixedVolume {
                    identity: attached.clone(),
                    mount_point: PathBuf::from(r"C:\"),
                }])
            })
            .unwrap();
        let same_mount_reconnected = index
            .search_with(
                Path::new(r"C:\untrusted"),
                query(),
                0,
                |_| panic!("active query must not reauthenticate app-data"),
                |_| panic!("active query must not reopen"),
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(same_mount_reconnected.total, 0);
        index.refresh_query_volumes_with(|| Ok(Vec::new())).unwrap();
        assert_eq!(
            index
                .search_with(
                    Path::new(r"C:\untrusted"),
                    query(),
                    0,
                    |_| panic!("active query must not reauthenticate app-data"),
                    |_| panic!("active query must not reopen"),
                    |store, spec| store.query(spec, &[]),
                )
                .unwrap()
                .total,
            0
        );

        index
            .refresh_query_volumes_with(|| {
                Ok(vec![super::FixedVolume {
                    identity: attached.clone(),
                    mount_point: PathBuf::from(r"D:\"),
                }])
            })
            .unwrap();
        let remounted = index
            .search_with(
                Path::new(r"C:\untrusted"),
                query(),
                0,
                |_| panic!("active query must not reauthenticate app-data"),
                |_| panic!("active query must not reopen"),
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(remounted.total, 0);
        {
            let mut state = index.state.lock().unwrap();
            let store = state.store.as_mut().unwrap();
            assert_eq!(store.mount_point_for_test(&attached), r"D:\");
            let generation = store.begin_candidate_for_test(&attached, r"D:\").unwrap();
            let mut entry = candidate_entry("find-new-mount.txt", generation);
            entry.display_path = r"D:\find-new-mount.txt".into();
            store
                .append_candidate_for_test(&attached, generation, [entry])
                .unwrap();
            store
                .commit_candidate_for_test(&attached, generation, &[])
                .unwrap();
            state.quarantined_volumes.remove(&attached);
        }
        index
            .refresh_query_volumes_with(|| {
                Ok(vec![super::FixedVolume {
                    identity: attached.clone(),
                    mount_point: PathBuf::from(r"D:\"),
                }])
            })
            .unwrap();
        let calibrated = index
            .search_with(
                Path::new(r"C:\untrusted"),
                query(),
                0,
                |_| panic!("active query must not reauthenticate app-data"),
                |_| panic!("active query must not reopen"),
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(calibrated.total, 1);
        assert_eq!(calibrated.items[0].full_path, r"D:\find-new-mount.txt");

        let reused = super::VolumeIdentity {
            volume_guid_path: r"\\?\VOLUME{REUSED}\".into(),
            volume_serial: 70,
            filesystem_name: "NTFS".into(),
        };
        index
            .refresh_query_volumes_with(|| {
                Ok(vec![super::FixedVolume {
                    identity: reused,
                    mount_point: PathBuf::from(r"C:\"),
                }])
            })
            .unwrap();
        let replaced = index
            .search_with(
                Path::new(r"C:\untrusted"),
                query(),
                0,
                |_| panic!("active query must not reauthenticate app-data"),
                |_| panic!("active query must not reopen"),
                |store, spec| store.query(spec, &[]),
            )
            .unwrap();
        assert_eq!(replaced.total, 0);
    }

    #[test]
    fn first_candidate_is_queryable_and_commits_atomically() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let volume = volume();
        let generation = store.begin_candidate_for_test(&volume, r"C:\").unwrap();
        assert_eq!(generation, 1);
        let revision = store
            .append_candidate_for_test(
                &volume,
                generation,
                [candidate_entry("find-first.txt", generation)],
            )
            .unwrap();
        let provisional = store
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(provisional.index_revision, revision);
        assert_eq!(provisional.status, FileIndexStatus::Building);
        assert_eq!(provisional.entries.len(), 1);

        let committed = store
            .commit_candidate_for_test(&volume, generation, &[])
            .unwrap();
        let ready = store
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(ready.index_revision, committed);
        assert_eq!(ready.status, FileIndexStatus::Ready);
        assert_eq!(ready.entries.len(), 1);
        assert!(store.candidate_rows_for_test(&volume).is_empty());

        let mut rollback = Store::open_in_memory_for_test("identity-a").unwrap();
        let generation = rollback.begin_candidate_for_test(&volume, r"C:\").unwrap();
        let before = rollback.index_revision_for_test();
        rollback.fail_revision_updates_for_test();
        assert!(rollback
            .append_candidate_for_test(
                &volume,
                generation,
                [candidate_entry("find-rollback.txt", generation)],
            )
            .is_err());
        assert_eq!(rollback.index_revision_for_test(), before);
        assert!(rollback.candidate_rows_for_test(&volume).is_empty());

        let mut partial = Store::open_in_memory_for_test("identity-a").unwrap();
        partial
            .seed_committed_for_test(
                &volume,
                [
                    candidate_entry(r"denied\find-secret.txt", 1),
                    candidate_entry(r"replace\find-old.txt", 1),
                ],
            )
            .unwrap();
        let generation = partial.begin_candidate_for_test(&volume, r"D:\").unwrap();
        partial
            .append_candidate_for_test(
                &volume,
                generation,
                [candidate_entry(r"replace\find-new.txt", generation)],
            )
            .unwrap();
        partial
            .commit_candidate_for_test(&volume, generation, &["denied"])
            .unwrap();
        let visible = partial
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(visible.status, FileIndexStatus::Partial);
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.name == r"denied\find-secret.txt"));
        assert!(visible
            .entries
            .iter()
            .find(|entry| entry.name == r"denied\find-secret.txt")
            .unwrap()
            .display_path
            .starts_with(r"D:\"));
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.name == r"replace\find-new.txt"));
        assert!(!visible
            .entries
            .iter()
            .any(|entry| entry.name == r"replace\find-old.txt"));

        let mut atomic = Store::open_in_memory_for_test("identity-a").unwrap();
        atomic
            .seed_committed_for_test(&volume, [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        let generation = atomic.begin_candidate_for_test(&volume, r"C:\").unwrap();
        atomic.fail_revision_updates_for_test();
        assert!(atomic
            .commit_candidate_with_replay_for_test(
                &volume,
                generation,
                [candidate_entry("find-scanned.txt", generation)],
                [candidate_entry("find-replay.txt", generation)],
                &[],
            )
            .is_err());
        let after_failure = atomic
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert!(after_failure
            .entries
            .iter()
            .any(|entry| entry.name == "find-stable.txt"));
        assert!(!after_failure
            .entries
            .iter()
            .any(|entry| entry.name == "find-replay.txt"));
        assert!(atomic.candidate_rows_for_test(&volume).is_empty());
    }

    #[test]
    fn revision_transitions_are_monotonic_and_checked() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let identity = volume();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-ready.txt", 1)])
            .unwrap();
        let first = store
            .mark_volume_dirty(&identity, r"C:\", std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(first, 1);
        let unchanged = store
            .mark_volume_dirty(&identity, r"C:\", std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(unchanged, first, "unchanged dirty state advanced revision");

        let publication_generation = AtomicU64::new(7);
        let mut state = IndexState {
            store: Some(store),
            mode: LifecycleMode::Active,
            admission_open: true,
            index_revision_high_water: u64::MAX,
            ..IndexState::default()
        };
        assert_eq!(
            state.advance_revision_locked(&publication_generation),
            Err(AdmissionError::CounterExhausted)
        );
        assert_eq!(state.index_revision_high_water, u64::MAX);
        assert_eq!(publication_generation.load(Ordering::Acquire), 7);
        assert!(!state.fatal_unavailable);
    }

    #[test]
    fn store_writer_revision_exhaustion_is_fatal_and_cancels_retry() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let mut other_identity = volume();
        other_identity.volume_serial = 43;
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let current_stop = Arc::new(AtomicBool::new(false));
        let other_stop = Arc::new(AtomicBool::new(false));
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-ready.txt", 1)])
            .unwrap();
        store.set_index_revision_for_test(u64::MAX);
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.index_revision_high_water = u64::MAX;
            state.authenticated_volumes = vec![identity.clone()];
            state.authenticated_mounts = vec![(identity.clone(), r"C:\".into())];
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        {
            let mut coordinator = index.coordinator.state.lock().unwrap();
            coordinator.pending_root = Some(PathBuf::from(r"C:\app-data"));
            coordinator.active_root = coordinator.pending_root.clone();
            coordinator.volumes.insert(
                identity.clone(),
                super::VolumeRuntime {
                    calibration: super::Calibration::Pending {
                        deadline: std::time::Instant::now(),
                        runtime_epoch: 0,
                    },
                    consecutive_failures: 1,
                },
            );
        }
        {
            let mut workers = index.workers.lock().unwrap();
            workers.by_volume.insert(
                identity.clone(),
                super::WorkerRecord {
                    owner: 1,
                    runtime_epoch: index.runtime_epoch(),
                    mount_point: PathBuf::from(r"C:\"),
                    stop: Arc::clone(&current_stop),
                    generation: Arc::new(AtomicU64::new(1)),
                    join: None,
                    failed: false,
                },
            );
            workers.by_volume.insert(
                other_identity.clone(),
                super::WorkerRecord {
                    owner: 2,
                    runtime_epoch: index.runtime_epoch(),
                    mount_point: PathBuf::from(r"D:\"),
                    stop: Arc::clone(&other_stop),
                    generation: Arc::new(AtomicU64::new(1)),
                    join: None,
                    failed: false,
                },
            );
        }
        let old_generation = index.publication_generation.load(Ordering::Acquire);

        assert!(matches!(
            index.mark_fixed_volume_dirty_for_test(&fixed),
            Err(FileIndexError::Unavailable)
        ));

        let state = index.state.lock().unwrap();
        assert!(state.fatal_unavailable);
        assert!(!state.admission_open);
        assert!(state.store.is_none());
        assert!(state.authenticated_volumes.is_empty());
        assert!(state.authenticated_mounts.is_empty());
        assert_eq!(state.index_revision_high_water, u64::MAX);
        drop(state);
        assert!(!index.authorizes_publication(0, old_generation));
        let coordinator = index.coordinator.state.lock().unwrap();
        assert!(coordinator.pending_root.is_none());
        assert!(coordinator.volumes.is_empty());
        drop(coordinator);
        assert!(current_stop.load(Ordering::Acquire));
        assert!(other_stop.load(Ordering::Acquire));
        assert!(!index.finish_volume_attempt(&identity, false, Instant::now(), 0, 0));
        assert!(!index.schedule_calibration());
        assert_eq!(index.workers.lock().unwrap().by_volume.len(), 2);
    }

    #[test]
    fn exhaustion_invalidates_file_domain_and_posts_close_exactly_once_without_locks() {
        let (index, _) = active_task7_index();
        index.install_main_window_hwnd(42).unwrap();
        index.registry.on_show("fatal-close".into());
        let application = index
            .registry
            .begin_query(QueryDomain::Application, "fatal-close", 1)
            .unwrap();
        let application = index
            .registry
            .publish_if_latest(
                application,
                vec![((), file_action())],
                || true,
                |request, items| (request, items[0].0.clone()),
            )
            .unwrap();
        {
            let mut state = index.state.lock().unwrap();
            assert!(index.latch_exhaustion_locked(&mut state));
        }
        let posts = Cell::new(0);
        index.consume_fatal_effects_with(|hwnd| {
            assert_eq!(hwnd, 42);
            assert!(index.state.try_lock().is_ok());
            posts.set(posts.get() + 1);
            true
        });
        index.consume_fatal_effects_with(|_| {
            posts.set(posts.get() + 1);
            true
        });
        assert_eq!(posts.get(), 1);
        assert_eq!(
            index.registry.resolve(&application.0, &application.1),
            Ok(file_action())
        );

        let cleared = Arc::new(FileIndex::default());
        cleared.install_main_window_hwnd(84).unwrap();
        cleared.clear_main_window_hwnd(84);
        {
            let mut state = cleared.state.lock().unwrap();
            assert!(cleared.latch_exhaustion_locked(&mut state));
        }
        let cleared_posts = Cell::new(0);
        cleared.consume_fatal_effects_with(|_| {
            cleared_posts.set(cleared_posts.get() + 1);
            true
        });
        assert_eq!(cleared_posts.get(), 0);
        let cleared_state = cleared.state.lock().unwrap();
        assert!(cleared_state.hide_requested);
        assert!(!cleared_state.hide_issued);
        drop(cleared_state);

        let retry = Arc::new(FileIndex::default());
        {
            let mut state = retry.state.lock().unwrap();
            assert!(retry.latch_exhaustion_locked(&mut state));
        }
        let retry_posts = Cell::new(0);
        retry.consume_fatal_effects_with(|_| {
            retry_posts.set(retry_posts.get() + 1);
            true
        });
        assert_eq!(retry_posts.get(), 0);
        assert!(retry.state.lock().unwrap().hide_requested);
        retry.install_main_window_hwnd(126).unwrap();
        retry.consume_fatal_effects_with(|_| {
            retry_posts.set(retry_posts.get() + 1);
            false
        });
        {
            let state = retry.state.lock().unwrap();
            assert!(state.hide_requested);
            assert!(!state.hide_issued);
        }
        retry.consume_fatal_effects_with(|_| {
            retry_posts.set(retry_posts.get() + 1);
            true
        });
        retry.consume_fatal_effects_with(|_| {
            retry_posts.set(retry_posts.get() + 1);
            true
        });
        assert_eq!(retry_posts.get(), 2);
        let retry_state = retry.state.lock().unwrap();
        assert!(!retry_state.hide_requested);
        assert!(retry_state.hide_issued);
    }

    #[test]
    fn fatal_before_schedule_linearization_cannot_restore_pending_work() {
        let index = Arc::new(FileIndex::default());
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        let reached = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_index = Arc::clone(&index);
        let worker_reached = Arc::clone(&reached);
        let worker_release = Arc::clone(&release);
        let scheduled = thread::spawn(move || {
            worker_index.mark_calibration_pending_with(PathBuf::from(r"C:\app-data"), || {
                worker_reached.wait();
                worker_release.wait();
            })
        });
        reached.wait();
        {
            let mut state = index.state.lock().unwrap();
            index.latch_process_fatal(&mut state);
        }
        release.wait();

        assert_eq!(scheduled.join().unwrap(), (false, false));
        let coordinator = index.coordinator.state.lock().unwrap();
        assert!(coordinator.pending_root.is_none());
        assert!(coordinator.volumes.is_empty());
        assert_eq!(coordinator.wakes, 0);
    }

    #[test]
    fn store_writer_revision_max_noop_does_not_latch() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .mark_volume_dirty(&identity, r"C:\", std::slice::from_ref(&identity))
            .unwrap();
        store.set_index_revision_for_test(u64::MAX);
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.index_revision_high_water = u64::MAX;
            state.authenticated_volumes = vec![identity.clone()];
            state.authenticated_mounts = vec![(identity, r"C:\".into())];
        }

        index.mark_fixed_volume_dirty_for_test(&fixed).unwrap();

        let state = index.state.lock().unwrap();
        assert!(!state.fatal_unavailable);
        assert!(state.admission_open);
        assert_eq!(state.index_revision_high_water, u64::MAX);
        assert_eq!(
            state.store.as_ref().unwrap().index_revision_for_test(),
            u64::MAX
        );
    }

    #[test]
    fn ordinary_volume_transaction_failure_keeps_single_backoff_owner() {
        let index = Arc::new(FileIndex::default());
        let identity = volume();
        let fixed = super::FixedVolume {
            identity: identity.clone(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-ready.txt", 1)])
            .unwrap();
        store.fail_revision_updates_for_test();
        {
            let mut state = index.state.lock().unwrap();
            state.store = Some(store);
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.authenticated_volumes = vec![identity.clone()];
            state.authenticated_mounts = vec![(identity.clone(), r"C:\".into())];
        }
        let owner = match index.prepare_worker(&fixed).unwrap() {
            super::WorkerPreparation::Start { owner } => owner,
            super::WorkerPreparation::Existing => panic!("worker must start"),
        };
        index.install_worker(
            &fixed,
            super::WorkerRecord {
                owner,
                runtime_epoch: index.runtime_epoch(),
                mount_point: fixed.mount_point.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                generation: Arc::new(AtomicU64::new(1)),
                join: None,
                failed: false,
            },
        );

        index.handle_worker_failure_for_test(&fixed, owner);

        let state = index.state.lock().unwrap();
        assert!(!state.fatal_unavailable);
        assert!(state.admission_open);
        assert!(state.store.is_some());
        drop(state);
        let coordinator = index.coordinator.state.lock().unwrap();
        let runtime = coordinator.volumes.get(&identity).unwrap();
        assert_eq!(runtime.consecutive_failures, 1);
        assert!(matches!(
            runtime.calibration,
            super::Calibration::Pending {
                runtime_epoch: 0,
                ..
            }
        ));
    }

    #[test]
    fn status_only_candidate_retry_does_not_advance_revision() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let identity = volume();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        let dirty_revision = store
            .mark_volume_dirty(&identity, r"C:\", std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(dirty_revision, 1);

        store
            .begin_candidate_for_test_with_identities(
                &identity,
                r"C:\",
                std::slice::from_ref(&identity),
            )
            .unwrap();
        assert_eq!(store.index_revision_for_test(), dirty_revision);
    }

    #[test]
    fn multi_volume_status_only_dirty_does_not_advance_revision() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let ready = volume();
        let mut building = volume();
        building.volume_serial = 43;
        store
            .seed_committed_for_test(&ready, [candidate_entry("find-ready.txt", 1)])
            .unwrap();

        let before = store.index_revision_for_test();
        let identities = [ready.clone(), building.clone()];
        assert_eq!(
            store
                .mark_volume_dirty(&building, r"D:\", &identities)
                .unwrap(),
            before
        );
        assert_eq!(
            store
                .mark_volume_dirty(&ready, r"C:\", &identities)
                .unwrap(),
            before
        );
    }

    #[test]
    fn calibration_commit_revision_tracks_rows_or_aggregate_status_only() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let ready = volume();
        let mut building = volume();
        building.volume_serial = 43;
        let stable = candidate_entry("find-stable.txt", 1);
        store
            .seed_committed_for_test(&ready, [stable.clone()])
            .unwrap();
        let identities = [ready.clone(), building.clone()];
        store
            .mark_volume_dirty(&building, r"D:\", &identities)
            .unwrap();

        let before_begin = store.index_revision_for_test();
        let generation = store
            .begin_candidate_for_test_with_identities(&ready, r"C:\", &identities)
            .unwrap();
        assert_eq!(store.index_revision_for_test(), before_begin);
        store
            .append_candidate_for_test_with_identities(&ready, generation, [stable], &identities)
            .unwrap();
        let before_equal_commit = store.index_revision_for_test();
        assert_eq!(
            store
                .commit_candidate_for_test_with_identities(&ready, generation, &[], &identities,)
                .unwrap(),
            before_equal_commit
        );

        let before_begin = store.index_revision_for_test();
        let generation = store
            .begin_candidate_for_test_with_identities(&ready, r"C:\", &identities)
            .unwrap();
        assert_eq!(store.index_revision_for_test(), before_begin);
        store
            .append_candidate_for_test_with_identities(
                &ready,
                generation,
                [candidate_entry("find-changed.txt", generation)],
                &identities,
            )
            .unwrap();
        let before_changed_commit = store.index_revision_for_test();
        assert_eq!(
            store
                .commit_candidate_for_test_with_identities(&ready, generation, &[], &identities,)
                .unwrap(),
            before_changed_commit + 1
        );
    }

    #[test]
    fn inventory_reconcile_revision_tracks_authenticated_wire_changes() {
        let mut hidden = Store::open_in_memory_for_test("identity-a").unwrap();
        let empty = volume();
        hidden
            .mark_volume_dirty(&empty, r"C:\", std::slice::from_ref(&empty))
            .unwrap();
        let before_hidden_transition = hidden.index_revision_for_test();
        let (_, hidden_revision, inventory_changed) = hidden
            .reconcile_current_mounts(
                &[(empty.clone(), r"D:\".into())],
                std::slice::from_ref(&empty),
                std::slice::from_ref(&empty),
                &std::collections::HashSet::new(),
            )
            .unwrap();
        assert!(inventory_changed);
        assert_eq!(hidden_revision, before_hidden_transition);

        let mut visible = Store::open_in_memory_for_test("identity-a").unwrap();
        visible
            .seed_committed_for_test(&empty, [candidate_entry("find-visible.txt", 1)])
            .unwrap();
        let before_detach = visible.index_revision_for_test();
        let (_, detach_revision, inventory_changed) = visible
            .reconcile_current_mounts(
                &[],
                std::slice::from_ref(&empty),
                std::slice::from_ref(&empty),
                &std::collections::HashSet::new(),
            )
            .unwrap();
        assert!(inventory_changed);
        assert_eq!(detach_revision, before_detach + 1);
    }

    #[test]
    fn revision_status_count_and_items_share_one_linearization() {
        let dir = TestDir::new();
        fs::create_dir_all(dir.path()).unwrap();
        let database = dir.path().join("linearization.sqlite3");
        let identity = volume();
        let mut reader = Store::open(&database, "identity-a").unwrap();
        reader
            .seed_committed_for_test(&identity, [candidate_entry("find-one.txt", 1)])
            .unwrap();

        let writer_database = database.clone();
        let writer_identity = identity.clone();
        let (start_tx, start_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            start_rx.recv().unwrap();
            let mut writer = Store::open(&writer_database, "identity-a").unwrap();
            let transaction = writer.connection_for_test().transaction().unwrap();
            transaction
                .execute(
                    "UPDATE metadata SET index_revision='1' WHERE singleton=1",
                    [],
                )
                .unwrap();
            transaction
                .execute(
                    "UPDATE volumes SET scan_state='dirty' WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                    rusqlite::params![
                        writer_identity.volume_guid_path,
                        writer_identity.volume_serial,
                        writer_identity.filesystem_name,
                    ],
                )
                .unwrap();
            let second = candidate_entry("find-two.txt", 1);
            transaction
                .execute(
                    "INSERT INTO entries(volume_guid_path,volume_serial,filesystem_name,relative_path,display_path,name,folded_name,kind,category,size_bytes,modified_utc_ms,generation) VALUES(?1,?2,?3,?4,?5,?6,?7,'file',?8,?9,?10,'1')",
                    rusqlite::params![
                        writer_identity.volume_guid_path,
                        writer_identity.volume_serial,
                        writer_identity.filesystem_name,
                        second.relative_path,
                        second.display_path,
                        second.name,
                        second.folded_name,
                        second.category,
                        second.size_bytes.map(|value| value.to_string()),
                        second.modified_utc_ms,
                    ],
                )
                .unwrap();
            transaction.commit().unwrap();
            done_tx.send(()).unwrap();
        });

        let before = reader
            .query_with_hook_for_test(&query(), std::slice::from_ref(&identity), || {
                start_tx.send(()).unwrap();
                done_rx.recv().unwrap();
            })
            .unwrap();
        writer.join().unwrap();
        assert_eq!(
            (
                before.index_revision,
                before.status,
                before.total,
                before.entries.len()
            ),
            (0, FileIndexStatus::Ready, 1, 1)
        );

        let after = reader
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(
            (
                after.index_revision,
                after.status,
                after.total,
                after.entries.len()
            ),
            (1, FileIndexStatus::Building, 2, 2)
        );
    }

    #[test]
    fn multi_volume_status_uses_the_frozen_priority() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let ready = volume();
        let mut partial = volume();
        partial.volume_serial = 43;
        let mut building = volume();
        building.volume_serial = 44;
        store
            .seed_committed_for_test(&ready, [candidate_entry("find-ready.txt", 1)])
            .unwrap();
        store
            .seed_committed_for_test(&partial, [candidate_entry("find-partial.txt", 1)])
            .unwrap();
        let generation = store.begin_candidate_for_test(&partial, r"D:\").unwrap();
        store
            .commit_candidate_for_test(&partial, generation, &[""])
            .unwrap();
        let identities = [ready.clone(), partial.clone(), building.clone()];
        store
            .mark_volume_dirty(&building, r"E:\", &identities)
            .unwrap();

        assert_eq!(
            store
                .query_for_test(&query(), &[ready.clone(), partial.clone(), building])
                .unwrap()
                .status,
            FileIndexStatus::Building
        );
        assert_eq!(
            store
                .query_for_test(&query(), &[ready.clone(), partial])
                .unwrap()
                .status,
            FileIndexStatus::Partial
        );
        assert_eq!(
            store.query_for_test(&query(), &[ready]).unwrap().status,
            FileIndexStatus::Ready
        );
    }

    #[test]
    fn calibration_backoff_is_bounded_single_owner_and_recovers() {
        let origin = Instant::now();
        let mut runtime = VolumeRuntime::default();
        assert!(runtime.request(origin, 0));

        for second in [0, 1, 3, 7, 15, 31, 63] {
            let now = origin + Duration::from_secs(second);
            assert!(runtime.start_if_due(now, 0));
            assert!(!runtime.start_if_due(now, 0));
            if second == 63 {
                runtime.finish_success(0);
            } else {
                runtime.finish_failure(now, 0).unwrap();
            }
        }

        assert_eq!(runtime.consecutive_failures, 0);
        assert!(runtime.request(origin + Duration::from_secs(63), 0));
        assert!(runtime.start_if_due(origin + Duration::from_secs(63), 0));
        runtime
            .finish_failure(origin + Duration::from_secs(63), 0)
            .unwrap();
        runtime.cancel_pending();
        assert!(runtime.request(origin + Duration::from_secs(63), 0));
        assert!(!runtime.start_if_due(origin + Duration::from_secs(63), 0));
        assert!(runtime.start_if_due(origin + Duration::from_secs(64), 0));
    }

    #[test]
    fn one_physical_calibration_failure_is_counted_once() {
        let origin = Instant::now();
        let mut completed_worker_failure = VolumeRuntime::default();
        completed_worker_failure.request(origin, 0);
        assert!(completed_worker_failure.start_if_due(origin, 0));
        let failures_before = completed_worker_failure.consecutive_failures;
        completed_worker_failure.finish_failure(origin, 0).unwrap();
        completed_worker_failure
            .finish_start_attempt(false, origin, 0, failures_before)
            .unwrap();
        assert_eq!(completed_worker_failure.consecutive_failures, 1);
        assert_eq!(
            completed_worker_failure.calibration,
            super::Calibration::Pending {
                deadline: origin + Duration::from_secs(1),
                runtime_epoch: 0,
            }
        );

        let mut startup_failure = VolumeRuntime::default();
        startup_failure.request(origin, 0);
        assert!(startup_failure.start_if_due(origin, 0));
        startup_failure
            .finish_start_attempt(false, origin, 0, 0)
            .unwrap();
        assert_eq!(startup_failure.consecutive_failures, 1);

        let mut inventory_pending = VolumeRuntime::default();
        inventory_pending.request(origin, 0);
        assert!(inventory_pending.start_if_due(origin, 0));
        inventory_pending.request(origin + Duration::from_millis(1), 0);
        inventory_pending
            .finish_start_attempt(false, origin, 0, 0)
            .unwrap();
        assert_eq!(inventory_pending.consecutive_failures, 1);
    }

    #[test]
    fn live_and_candidate_replay_apply_identical_event_semantics() {
        fn prepared_store() -> (Store, u64) {
            let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
            store
                .seed_committed_for_test(&volume(), [candidate_entry(r"tree\find-old.txt", 1)])
                .unwrap();
            let generation = store.begin_candidate_for_test(&volume(), r"C:\").unwrap();
            store
                .append_candidate_for_test(
                    &volume(),
                    generation,
                    [candidate_entry(r"tree\find-old.txt", generation)],
                )
                .unwrap();
            (store, generation)
        }

        let replacement = candidate_entry(r"tree\find-new.txt", 2);
        let (mut live, live_generation) = prepared_store();
        let before_live = live.index_revision_for_test();
        let changed_revision = live
            .apply_live_streaming(
                &volume(),
                live_generation,
                std::slice::from_ref(&volume()),
                |apply| {
                    apply(super::IndexChangeBatch {
                        deleted_prefixes: vec![r"tree".into()],
                        entries: Vec::new(),
                    })?;
                    apply(super::IndexChangeBatch {
                        deleted_prefixes: Vec::new(),
                        entries: vec![super::IndexEntry::from(replacement.clone())],
                    })
                },
            )
            .unwrap();
        assert_eq!(changed_revision, before_live + 1);
        let unchanged_revision = live
            .apply_live_changes(
                &volume(),
                live_generation,
                std::iter::empty::<&str>(),
                [super::IndexEntry::from(replacement.clone())],
            )
            .unwrap();
        assert_eq!(unchanged_revision, changed_revision);
        live.commit_candidate(&volume(), live_generation, Vec::new(), &[], Vec::new(), &[])
            .unwrap();

        let (mut replay, replay_generation) = prepared_store();
        replay
            .commit_candidate(
                &volume(),
                replay_generation,
                Vec::new(),
                &[r"tree".into()],
                vec![super::IndexEntry::from(replacement)],
                &[],
            )
            .unwrap();

        let live_names = live
            .query_for_test(&query(), &[volume()])
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        let replay_names = replay
            .query_for_test(&query(), &[volume()])
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        assert_eq!(live_names, [r"tree\find-new.txt"]);
        assert_eq!(replay_names, live_names);

        let mut hidden = Store::open_in_memory_for_test("identity-a").unwrap();
        hidden
            .seed_committed_for_test(&volume(), [candidate_entry("find-visible.txt", 1)])
            .unwrap();
        let generation = hidden.begin_candidate_for_test(&volume(), r"C:\").unwrap();
        let before_hidden_append = hidden.index_revision_for_test();
        hidden
            .append_candidate_for_test(
                &volume(),
                generation,
                [candidate_entry("find-hidden.txt", generation)],
            )
            .unwrap();
        assert_eq!(hidden.index_revision_for_test(), before_hidden_append);

        let before_candidate_only = Store::open_in_memory_for_test("identity-a").unwrap();
        let mut candidate_only = before_candidate_only;
        let generation = candidate_only
            .begin_candidate_for_test(&volume(), r"C:\")
            .unwrap();
        let before_visible_append = candidate_only.index_revision_for_test();
        candidate_only
            .append_candidate_for_test(
                &volume(),
                generation,
                [candidate_entry("find-first-visible.txt", generation)],
            )
            .unwrap();
        assert_eq!(
            candidate_only.index_revision_for_test(),
            before_visible_append + 1
        );
        assert_eq!(
            candidate_only
                .append_candidate_for_test(
                    &volume(),
                    generation,
                    [candidate_entry("find-first-visible.txt", generation)],
                )
                .unwrap(),
            before_visible_append + 1
        );

        let mut quarantined_candidate = Store::open_in_memory_for_test("identity-a").unwrap();
        let quarantined = volume();
        let generation = quarantined_candidate
            .begin_candidate_for_test(&quarantined, r"C:\")
            .unwrap();
        let before_quarantined_append = quarantined_candidate.index_revision_for_test();
        assert_eq!(
            quarantined_candidate
                .append_candidate_for_test_with_identities(
                    &quarantined,
                    generation,
                    [candidate_entry("find-quarantined.txt", generation)],
                    &[],
                )
                .unwrap(),
            before_quarantined_append
        );
        assert_eq!(
            quarantined_candidate.candidate_rows_for_test(&quarantined),
            ["find-quarantined.txt"]
        );

        let mut candidate_internal = Store::open_in_memory_for_test("identity-a").unwrap();
        let same = candidate_entry("find-same.txt", 1);
        candidate_internal
            .seed_committed_for_test(&volume(), [same.clone()])
            .unwrap();
        let generation = candidate_internal
            .begin_candidate_for_test(&volume(), r"C:\")
            .unwrap();
        let before_internal_only = candidate_internal.index_revision_for_test();
        let revision = candidate_internal
            .apply_live_changes(
                &volume(),
                generation,
                std::iter::empty::<&str>(),
                [super::IndexEntry::from(same)],
            )
            .unwrap();
        assert_eq!(revision, before_internal_only);

        let dir = TestDir::new();
        fs::create_dir_all(dir.path()).unwrap();
        let database = dir.path().join("live-stream.sqlite3");
        let mut writer = Store::open(&database, "identity-a").unwrap();
        writer
            .seed_committed_for_test(&volume(), [candidate_entry("find-old.txt", 1)])
            .unwrap();
        let reader_database = database.clone();
        let (read_tx, read_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut reader = Store::open(&reader_database, "identity-a").unwrap();
            ready_tx.send(()).unwrap();
            read_rx.recv().unwrap();
            let snapshot = reader.query_for_test(&query(), &[volume()]).unwrap();
            done_tx.send(snapshot).unwrap();
        });
        ready_rx.recv().unwrap();
        let revision = writer
            .apply_live_streaming(&volume(), 1, std::slice::from_ref(&volume()), |apply| {
                apply(super::IndexChangeBatch {
                    deleted_prefixes: vec!["find-old.txt".into()],
                    entries: Vec::new(),
                })?;
                read_tx.send(()).unwrap();
                let during = done_rx.recv().unwrap();
                assert_eq!(during.index_revision, 0);
                assert_eq!(during.total, 1);
                assert_eq!(during.entries[0].name, "find-old.txt");
                apply(super::IndexChangeBatch {
                    deleted_prefixes: Vec::new(),
                    entries: vec![super::IndexEntry::from(candidate_entry("find-new.txt", 1))],
                })
            })
            .unwrap();
        reader.join().unwrap();
        assert_eq!(revision, 1);
        let committed = writer.query_for_test(&query(), &[volume()]).unwrap();
        assert_eq!(committed.index_revision, 1);
        assert_eq!(committed.total, 1);
        assert_eq!(committed.entries[0].name, "find-new.txt");

        assert!(writer
            .apply_live_streaming(&volume(), 1, std::slice::from_ref(&volume()), |apply| {
                apply(super::IndexChangeBatch {
                    deleted_prefixes: vec!["find-new.txt".into()],
                    entries: Vec::new(),
                })?;
                Err(StoreError::InvalidData)
            })
            .is_err());
        let rolled_back = writer.query_for_test(&query(), &[volume()]).unwrap();
        assert_eq!(rolled_back.index_revision, 1);
        assert_eq!(rolled_back.total, 1);
        assert_eq!(rolled_back.entries[0].name, "find-new.txt");
        assert_eq!(
            writer
                .apply_live_changes(
                    &volume(),
                    1,
                    std::iter::empty::<&str>(),
                    [super::IndexEntry::from(candidate_entry("find-new.txt", 1))],
                )
                .unwrap(),
            1
        );

        let mut unchanged_tree = Store::open_in_memory_for_test("identity-a").unwrap();
        let mut tree_entries = Vec::with_capacity(514);
        let mut directory = candidate_entry("find-tree", 1);
        directory.kind = super::IndexedKind::Directory;
        directory.size_bytes = None;
        tree_entries.push(directory);
        tree_entries.extend(
            (0..513).map(|index| candidate_entry(&format!(r"find-tree\find-{index}.txt"), 1)),
        );
        unchanged_tree
            .seed_committed_for_test(&volume(), tree_entries.clone())
            .unwrap();
        let exact_plan = unchanged_tree.exact_live_visibility_plan_for_test(
            &volume(),
            1,
            r"find-tree\find-0.txt",
        );
        assert!(exact_plan
            .iter()
            .any(|detail| detail.contains("SEARCH e USING INDEX sqlite_autoindex_entries_1")));
        assert!(exact_plan.iter().all(|detail| !detail.contains("SCAN e")));
        let before_tree = unchanged_tree
            .query_for_test(&query(), &[volume()])
            .unwrap();
        let reinsert_same_tree = |store: &mut Store| {
            store.apply_live_streaming(&volume(), 1, std::slice::from_ref(&volume()), |apply| {
                apply(super::IndexChangeBatch {
                    deleted_prefixes: vec!["find-tree".into()],
                    entries: Vec::new(),
                })?;
                for batch in tree_entries.chunks(512) {
                    apply(super::IndexChangeBatch {
                        deleted_prefixes: Vec::new(),
                        entries: batch.iter().cloned().map(super::IndexEntry::from).collect(),
                    })?;
                }
                Ok(())
            })
        };
        let unchanged_revision = reinsert_same_tree(&mut unchanged_tree).unwrap();
        let after_tree = unchanged_tree
            .query_for_test(&query(), &[volume()])
            .unwrap();
        assert_eq!(unchanged_revision, before_tree.index_revision);
        assert_eq!(after_tree.index_revision, before_tree.index_revision);
        assert_eq!(after_tree.total, before_tree.total);
        assert_eq!(after_tree.entries, before_tree.entries);
        assert_eq!(
            reinsert_same_tree(&mut unchanged_tree).unwrap(),
            before_tree.index_revision
        );
        let committed_revision = unchanged_tree.index_revision_for_test();
        let committed_noop = unchanged_tree
            .apply_committed_streaming(&volume(), 1, std::slice::from_ref(&volume()), |apply| {
                apply(super::IndexChangeBatch {
                    deleted_prefixes: vec!["find-tree".into()],
                    entries: Vec::new(),
                })?;
                for batch in tree_entries.chunks(512) {
                    apply(super::IndexChangeBatch {
                        deleted_prefixes: Vec::new(),
                        entries: batch.iter().cloned().map(super::IndexEntry::from).collect(),
                    })?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(committed_noop, committed_revision);
        assert_eq!(
            unchanged_tree
                .apply_committed_changes_during_scan(
                    &volume(),
                    1,
                    std::iter::empty::<&str>(),
                    [super::IndexEntry::from(candidate_entry(
                        "find-added.txt",
                        1
                    ))],
                )
                .unwrap(),
            committed_revision + 1
        );

        let mut cancelled = Store::open_in_memory_for_test("identity-a").unwrap();
        cancelled
            .seed_committed_for_test(&volume(), [candidate_entry("find-stable.txt", 1)])
            .unwrap();
        let generation = cancelled
            .begin_candidate_for_test(&volume(), r"C:\")
            .unwrap();
        let before_cancel = cancelled.index_revision_for_test();
        assert!(cancelled
            .commit_candidate_streaming(
                &volume(),
                generation,
                Vec::new(),
                &[],
                (
                    std::slice::from_ref(&volume()),
                    std::slice::from_ref(&volume()),
                ),
                |apply| {
                    apply(super::IndexChangeBatch {
                        deleted_prefixes: Vec::new(),
                        entries: (0..512)
                            .map(|index| {
                                super::IndexEntry::from(candidate_entry(
                                    &format!("find-cancel-{index}.txt"),
                                    generation,
                                ))
                            })
                            .collect(),
                    })?;
                    Err(StoreError::InvalidData)
                },
            )
            .is_err());
        assert_eq!(cancelled.index_revision_for_test(), before_cancel);
        let after_cancel = cancelled.query_for_test(&query(), &[volume()]).unwrap();
        assert_eq!(after_cancel.total, 1);
        assert_eq!(after_cancel.entries[0].name, "find-stable.txt");
    }

    #[test]
    fn prefix_mutations_are_binary_and_preserve_case_siblings() {
        fn case_entries() -> [TestEntry; 2] {
            let mut upper = candidate_entry(r"Case\find-upper.txt", 1);
            upper.display_path = r"C:\Case\find-upper.txt".into();
            upper.name = "find-upper.txt".into();
            upper.folded_name = fold_name(&upper.name);
            let mut lower = candidate_entry(r"case\find-lower.txt", 1);
            lower.display_path = r"C:\case\find-lower.txt".into();
            lower.name = "find-lower.txt".into();
            lower.folded_name = fold_name(&lower.name);
            [upper, lower]
        }

        let identity = volume();
        let mut deleted = Store::open_in_memory_for_test("identity-a").unwrap();
        deleted
            .seed_committed_for_test(&identity, case_entries())
            .unwrap();
        let revision = deleted
            .apply_live_changes(
                &identity,
                1,
                ["Case"],
                std::iter::empty::<super::IndexEntry>(),
            )
            .unwrap();
        let visible = deleted
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(revision, 1);
        assert_eq!(visible.total, 1);
        assert_eq!(visible.entries[0].name, "find-lower.txt");

        let mut unchanged = Store::open_in_memory_for_test("identity-a").unwrap();
        unchanged
            .seed_committed_for_test(&identity, case_entries())
            .unwrap();
        let before = unchanged.index_revision_for_test();
        let revision = unchanged
            .apply_live_changes(
                &identity,
                1,
                ["Case"],
                [super::IndexEntry::from(case_entries()[0].clone())],
            )
            .unwrap();
        assert_eq!(revision, before);

        let mut denied = Store::open_in_memory_for_test("identity-a").unwrap();
        denied
            .seed_committed_for_test(&identity, case_entries())
            .unwrap();
        let generation = denied.begin_candidate_for_test(&identity, r"D:\").unwrap();
        denied
            .commit_candidate_for_test(&identity, generation, &["Case"])
            .unwrap();
        let visible = denied
            .query_for_test(&query(), std::slice::from_ref(&identity))
            .unwrap();
        assert_eq!(visible.total, 1);
        assert_eq!(visible.entries[0].name, "find-upper.txt");
        assert_eq!(visible.entries[0].display_path, r"D:\Case\find-upper.txt");
    }

    #[test]
    fn scan_updates_existing_committed_rows_before_candidate_commit() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let volume = volume();
        store
            .seed_committed_for_test(&volume, [candidate_entry("find-old.txt", 1)])
            .unwrap();
        let before_feedback = store.index_revision_for_test();
        let mut feedback_queue = super::windows_backend::EventBuffer::new();
        let feedback = super::stage_replay_events(
            &volume,
            &[super::windows_backend::StructuredEvent::new(
                windows::Win32::Storage::FileSystem::FILE_ACTION_MODIFIED,
                r"Users\me\AppData\UiPilot\file-index.sqlite3-wal",
            )],
            &[super::windows_backend::ExcludedPrefix::new(
                volume.clone(),
                r"Users\me\AppData\UiPilot",
            )],
            &mut feedback_queue,
        )
        .unwrap();
        assert!(feedback.is_empty());
        assert!(feedback_queue.events().is_empty());
        assert_eq!(feedback_queue.last_sequence(), Some(0));
        assert_eq!(store.index_revision_for_test(), before_feedback);
        let generation = store.begin_candidate_for_test(&volume, r"C:\").unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let second_attempt = Arc::new(Barrier::new(2));
        let second_sent = Arc::new(AtomicBool::new(false));
        let producer_attempt = Arc::clone(&second_attempt);
        let producer_sent = Arc::clone(&second_sent);
        let producer = thread::spawn(move || {
            sender
                .send(vec![candidate_entry("find-scan-one.txt", generation)])
                .unwrap();
            producer_attempt.wait();
            sender
                .send(vec![candidate_entry("find-scan-two.txt", generation)])
                .unwrap();
            producer_sent.store(true, Ordering::Release);
        });
        second_attempt.wait();
        thread::yield_now();
        assert!(!second_sent.load(Ordering::Acquire));
        store
            .apply_committed_entry_for_test(
                &volume,
                generation,
                candidate_entry("find-live.txt", generation),
            )
            .unwrap();

        let during_scan = store
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(during_scan.status, FileIndexStatus::Building);
        assert!(during_scan
            .entries
            .iter()
            .any(|entry| entry.name == "find-live.txt"));
        assert!(store.candidate_rows_for_test(&volume).is_empty());

        store
            .append_candidate_for_test(&volume, generation, receiver.recv().unwrap())
            .unwrap();
        let second = receiver.recv().unwrap();
        producer.join().unwrap();
        assert!(second_sent.load(Ordering::Acquire));
        store
            .commit_candidate_with_replay_for_test(
                &volume,
                generation,
                second,
                [candidate_entry("find-live.txt", generation)],
                &[],
            )
            .unwrap();
        let committed = store
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(
            committed
                .entries
                .iter()
                .filter(|entry| entry.name == "find-live.txt")
                .count(),
            1
        );
        assert!(store.candidate_rows_for_test(&volume).is_empty());
    }

    #[test]
    fn failed_candidate_is_cleared_without_a_third_generation() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let volume = volume();
        store
            .seed_committed_for_test(&volume, [candidate_entry("find-kept.txt", 1)])
            .unwrap();
        let generation = store.begin_candidate_for_test(&volume, r"C:\").unwrap();
        store
            .append_candidate_for_test(
                &volume,
                generation,
                [candidate_entry("find-discarded.txt", generation)],
            )
            .unwrap();
        store.fail_candidate_for_test(&volume).unwrap();

        assert!(store.candidate_rows_for_test(&volume).is_empty());
        assert_eq!(
            store.generation_state_for_test(&volume),
            (Some(1), None, 3, "dirty".into())
        );
        let visible = store
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-kept.txt"));
        assert!(!visible
            .entries
            .iter()
            .any(|entry| entry.name == "find-discarded.txt"));
    }

    #[test]
    fn candidate_crash_recovery_preserves_only_committed_rows() {
        let dir = TestDir::new();
        fs::create_dir_all(dir.path()).unwrap();
        let database = dir.path().join("candidate.sqlite3");
        let volume = volume();
        {
            let mut store = Store::open(&database, "identity-a").unwrap();
            store
                .seed_committed_for_test(&volume, [candidate_entry("find-kept.txt", 1)])
                .unwrap();
            let generation = store.begin_candidate_for_test(&volume, r"C:\").unwrap();
            store
                .append_candidate_for_test(
                    &volume,
                    generation,
                    [candidate_entry("find-crashed.txt", generation)],
                )
                .unwrap();
        }

        let mut reopened = Store::open(&database, "identity-a").unwrap();
        let before_hidden_recovery = reopened.index_revision_for_test();
        assert_eq!(
            reopened.recover_candidates_for_test().unwrap(),
            Some(before_hidden_recovery)
        );
        assert!(reopened.candidate_rows_for_test(&volume).is_empty());
        assert_eq!(reopened.generation_state_for_test(&volume).1, None);
        let visible = reopened
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(visible.entries.len(), 1);
        assert_eq!(visible.entries[0].name, "find-kept.txt");

        let mut visible_candidate = Store::open_in_memory_for_test("identity-a").unwrap();
        let generation = visible_candidate
            .begin_candidate_for_test(&volume, r"C:\")
            .unwrap();
        visible_candidate
            .append_candidate_for_test(
                &volume,
                generation,
                [candidate_entry("find-provisional.txt", generation)],
            )
            .unwrap();
        let before_visible_recovery = visible_candidate.index_revision_for_test();
        assert_eq!(
            visible_candidate.recover_candidates_for_test().unwrap(),
            Some(before_visible_recovery + 1)
        );
        assert!(visible_candidate
            .candidate_rows_for_test(&volume)
            .is_empty());

        let mut empty_candidate = Store::open_in_memory_for_test("identity-a").unwrap();
        empty_candidate
            .begin_candidate_for_test(&volume, r"C:\")
            .unwrap();
        let before_empty_recovery = empty_candidate.index_revision_for_test();
        assert_eq!(
            empty_candidate.recover_candidates_for_test().unwrap(),
            Some(before_empty_recovery)
        );
        assert!(empty_candidate.candidate_rows_for_test(&volume).is_empty());
    }

    fn active_task7_index() -> (Arc<FileIndex>, Arc<crate::lifecycle::LifecycleCoordinator>) {
        let lifecycle = Arc::new(crate::lifecycle::LifecycleCoordinator::default());
        let index = Arc::new(FileIndex::new(
            Arc::clone(&lifecycle),
            ResultRegistry::default(),
        ));
        {
            let mut state = index.state.lock().unwrap();
            state.mode = LifecycleMode::Active;
            state.admission_open = true;
            state.store = Some(Store::open_in_memory_for_test("identity-a").unwrap());
        }
        (index, lifecycle)
    }

    #[test]
    fn admission_rejects_every_kind_after_phase_store() {
        let (index, lifecycle) = active_task7_index();
        let reservation = index.reserve_db_work_for_test(0).unwrap();
        assert_eq!(index.db_work_count_for_test(), 1);
        drop(reservation);
        assert_eq!(index.db_work_count_for_test(), 0);

        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);
        assert!(matches!(
            index.reserve_db_work_for_test(0),
            Err(AdmissionError::Lifecycle)
        ));
        assert_eq!(index.db_work_count_for_test(), 0);

        let lazy = Arc::new(FileIndex::new(
            Arc::new(crate::lifecycle::LifecycleCoordinator::default()),
            ResultRegistry::default(),
        ));
        assert_eq!(
            lazy.begin_lazy_for_test(0),
            Ok(LazyInitDecision::Start { owner: 1 })
        );
        assert_eq!(
            lazy.begin_lazy_for_test(0),
            Ok(LazyInitDecision::ObserveBuilding)
        );
        assert_eq!(lazy.db_work_count_for_test(), 0);
    }

    #[test]
    fn late_worker_placeholder_is_cancelled_before_path_or_database_work() {
        let (index, lifecycle) = active_task7_index();
        let fixed = super::FixedVolume {
            identity: volume(),
            mount_point: PathBuf::from(r"C:\"),
        };
        {
            let mut state = index.state.lock().unwrap();
            state.authenticated_volumes = vec![fixed.identity.clone()];
            state.authenticated_mounts = vec![(fixed.identity.clone(), r"C:\".to_owned())];
        }
        let start = match index.reserve_and_prepare_worker(&fixed).unwrap() {
            super::WorkerStartDecision::Start(start) => start,
            super::WorkerStartDecision::Existing => panic!("first worker must reserve an owner"),
        };
        assert_eq!(index.db_work_count_for_test(), 1);
        assert_eq!(start.runtime_epoch, 0);
        assert_eq!(start.generation.load(Ordering::Acquire), 0);
        assert!(!start.stop.load(Ordering::Acquire));

        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);
        assert!(index.start_cleaning_until(
            1,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        ));
        assert!(start.stop.load(Ordering::Acquire));

        let touched = Arc::new(AtomicU64::new(0));
        let worker_index = Arc::clone(&index);
        let worker_volume = fixed.clone();
        let worker_touched = Arc::clone(&touched);
        let worker_stop = Arc::clone(&start.stop);
        let runtime_epoch = start.runtime_epoch;
        let owner = start.owner;
        let reservation = start.reservation;
        let join = std::thread::spawn(move || {
            if !worker_stop.load(Ordering::Acquire)
                && worker_index.worker_start_authorized(
                    &worker_volume,
                    owner,
                    runtime_epoch,
                    &reservation,
                )
            {
                worker_touched.fetch_add(1, Ordering::AcqRel);
            }
        });
        match index.attach_worker_join(&fixed, owner, runtime_epoch, join) {
            Ok(()) => {
                if let Some(worker) = index.remove_worker_if_owner(&fixed.identity, owner) {
                    super::stop_and_join_worker(worker);
                }
            }
            Err(join) => join.join().unwrap(),
        }
        assert_eq!(touched.load(Ordering::Acquire), 0);
        assert_eq!(index.db_work_count_for_test(), 0);
    }

    #[test]
    fn recovery_reporters_never_join_themselves() {
        let (index, _) = active_task7_index();
        let fixed = super::FixedVolume {
            identity: volume(),
            mount_point: PathBuf::from(r"C:\"),
        };
        let reservation = index.reserve_db_work_for_test(0).unwrap();
        let start = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_index = Arc::clone(&index);
        let worker_start = Arc::clone(&start);
        let worker_release = Arc::clone(&release);
        let (reported_tx, reported_rx) = mpsc::sync_channel(1);
        let join = thread::spawn(move || {
            worker_start.wait();
            reported_tx
                .send(worker_index.request_recovery(&reservation))
                .unwrap();
            worker_release.wait();
            drop(reservation);
        });
        let stop = Arc::new(AtomicBool::new(false));
        index.install_worker(
            &fixed,
            super::WorkerRecord {
                owner: 1,
                runtime_epoch: 0,
                mount_point: fixed.mount_point.clone(),
                stop: Arc::clone(&stop),
                generation: Arc::new(AtomicU64::new(1)),
                join: Some(join),
                failed: false,
            },
        );
        start.wait();
        assert!(reported_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        assert!(stop.load(Ordering::Acquire));
        release.wait();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !index.workers.lock().unwrap().by_volume.is_empty() {
            assert!(
                Instant::now() < deadline,
                "coordinator did not reap reporter"
            );
            index.coordinator.signal.notify_all();
            thread::yield_now();
        }
        assert_eq!(index.db_work_count_for_test(), 0);
    }

    #[test]
    fn recovery_transition_releases_gate_before_domain_invalidation() {
        let (index, _) = active_task7_index();
        let registry = index.registry.clone();
        registry.on_show("recovery-domain".into());
        let application = registry
            .begin_query(QueryDomain::Application, "recovery-domain", 1)
            .unwrap();
        let application = registry
            .publish_if_latest(
                application,
                vec![((), file_action())],
                || true,
                |request, items| (request, items[0].0.clone()),
            )
            .unwrap();
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        let observed_unlocked = Cell::new(false);
        assert!(index.transition_recovery(&reporter, || {
            observed_unlocked.set(index.state.try_lock().is_ok());
            registry.invalidate_domain(QueryDomain::File)
        }));
        assert_eq!(
            registry.resolve(&application.0, &application.1),
            Ok(file_action())
        );
        assert!(observed_unlocked.get());
    }

    #[test]
    fn recovery_quiesces_before_destructive_operations() {
        let (index, _) = active_task7_index();
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        let destructive = Cell::new(0);
        let reopen = Cell::new(0);
        assert!(!index.drive_recovery_with(
            std::time::Instant::now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(Vec::new()),
            |_| {
                destructive.set(destructive.get() + 1);
                Ok(())
            },
            |_, _| {
                reopen.set(reopen.get() + 1);
                Store::open_in_memory_for_test("identity-a")
                    .map_err(|_| FileIndexError::Unavailable)
            },
        ));
        assert_eq!(destructive.get(), 0);
        drop(reporter);
        assert!(index.drive_recovery_with(
            std::time::Instant::now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(Vec::new()),
            |_| {
                destructive.set(destructive.get() + 1);
                Ok(())
            },
            |_, _| {
                reopen.set(reopen.get() + 1);
                Store::open_in_memory_for_test("identity-a")
                    .map_err(|_| FileIndexError::Unavailable)
            },
        ));
        assert_eq!(destructive.get(), 3);
        assert_eq!(reopen.get(), 1);
    }

    #[test]
    fn recovery_reopens_once_with_monotonic_revision() {
        let (index, _) = active_task7_index();
        let before = index.revision_for_test();
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        drop(reporter);
        let reopen = Cell::new(0);
        assert!(index.drive_recovery_with(
            std::time::Instant::now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(Vec::new()),
            |_| Ok(()),
            |_, _| {
                reopen.set(reopen.get() + 1);
                Store::open_in_memory_for_test("identity-a")
                    .map_err(|_| FileIndexError::Unavailable)
            },
        ));
        assert_eq!(reopen.get(), 1);
        assert!(index.revision_for_test() > before);
        assert!(index.admission_open_for_test());
    }

    #[test]
    fn recovery_first_use_corruption_is_consumed_by_owned_coordinator() {
        let lifecycle = Arc::new(crate::lifecycle::LifecycleCoordinator::default());
        let index = Arc::new(FileIndex::new(lifecycle, ResultRegistry::default()));
        let dir = TestDir::new();
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(
            dir.path().join("file-index.sqlite3"),
            b"not a sqlite database",
        )
        .unwrap();

        assert!(index
            .search_with(
                dir.path(),
                query(),
                0,
                authenticate_app_data_root,
                open_store,
                |_, _| panic!("corrupt first open must not query"),
            )
            .is_err());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            {
                let state = index.state.lock().unwrap();
                if state.recovery_owner.is_none() && state.admission_open && state.store.is_some() {
                    assert_eq!(
                        state.authenticated_app_data_root.as_deref(),
                        Some(fs::canonicalize(dir.path()).unwrap().as_path())
                    );
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "owned coordinator did not consume recovery"
            );
            thread::sleep(Duration::from_millis(5));
        }
        drop(index);
    }

    #[test]
    fn recovery_waits_for_integrity_join_after_db_reservation_drops() {
        let (index, _) = active_task7_index();
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        let integrity_reservation = index.reserve_db_work_for_test(0).unwrap();
        let release = Arc::new(Barrier::new(2));
        let worker_release = Arc::clone(&release);
        let stop = Arc::new(AtomicBool::new(false));
        let join = thread::spawn(move || {
            drop(integrity_reservation);
            worker_release.wait();
        });
        *index.integrity_worker.lock().unwrap() = Some(super::IntegrityWorkerRecord {
            runtime_epoch: 0,
            stop,
            join: Some(join),
        });
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        drop(reporter);
        let deletes = Cell::new(0);
        assert!(!index.drive_recovery_with(
            Instant::now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(Vec::new()),
            |_| {
                deletes.set(deletes.get() + 1);
                Ok(())
            },
            |_, _| Store::open_in_memory_for_test("identity-a")
                .map_err(|_| FileIndexError::Unavailable),
        ));
        assert_eq!(deletes.get(), 0);
        release.wait();
        while index
            .integrity_worker
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|worker| worker.join.as_ref())
            .is_some_and(|join| !join.is_finished())
        {
            thread::yield_now();
        }
        assert!(index.drive_recovery_with(
            Instant::now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(Vec::new()),
            |_| {
                deletes.set(deletes.get() + 1);
                Ok(())
            },
            |_, _| Store::open_in_memory_for_test("identity-a")
                .map_err(|_| FileIndexError::Unavailable),
        ));
        assert_eq!(deletes.get(), 3);
    }

    #[test]
    fn recovery_final_publish_uses_current_inventory_and_clears_old_ownership() {
        let (index, _) = active_task7_index();
        let old = volume();
        let mut current_identity = volume();
        current_identity.volume_serial = old.volume_serial + 1;
        let current = super::FixedVolume {
            identity: current_identity.clone(),
            mount_point: PathBuf::from(r"D:\"),
        };
        {
            let mut state = index.state.lock().unwrap();
            state.inventory_previous_authenticated = Some(vec![old.clone()]);
            state.pending_inventory_transitions.insert(old.clone());
            state.authenticated_volumes = vec![old.clone()];
            state.authenticated_mounts = vec![(old.clone(), r"C:\".to_owned())];
            state.authenticated_app_data_root = Some(PathBuf::from(r"C:\app-data"));
        }
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        drop(reporter);
        assert!(index.drive_recovery_with(
            Instant::now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(vec![current.clone()]),
            |_| Ok(()),
            |_, _| Store::open_in_memory_for_test("identity-a")
                .map_err(|_| FileIndexError::Unavailable),
        ));
        let state = index.state.lock().unwrap();
        assert!(state.inventory_previous_authenticated.is_none());
        assert!(state.pending_inventory_transitions.is_empty());
        assert!(state.authenticated_volumes.is_empty());
        assert_eq!(
            state.authenticated_mounts,
            vec![(current_identity.clone(), r"D:\".to_owned())]
        );
        assert_eq!(
            state.quarantined_volumes,
            std::collections::HashSet::from([current_identity.clone()])
        );
        drop(state);
        let coordinator = index.coordinator.state.lock().unwrap();
        assert_eq!(
            coordinator.volumes.keys().cloned().collect::<Vec<_>>(),
            vec![current_identity]
        );
        assert_eq!(coordinator.active_root, Some(PathBuf::from(r"C:\app-data")));
    }

    #[test]
    fn recovery_deadline_at_final_publish_latches_without_recursive_coordinator_lock() {
        let (index, _) = active_task7_index();
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        drop(reporter);
        let deadline = index.state.lock().unwrap().recovery_deadline.unwrap();
        let inventory_seen = Cell::new(false);
        let after_inventory = Cell::new(0usize);
        let now = || {
            if !inventory_seen.get() {
                deadline - Duration::from_nanos(1)
            } else {
                let call = after_inventory.get() + 1;
                after_inventory.set(call);
                if call < 3 {
                    deadline - Duration::from_nanos(1)
                } else {
                    deadline
                }
            }
        };
        assert!(!index.drive_recovery_with(
            now,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || {
                inventory_seen.set(true);
                Ok(Vec::new())
            },
            |_| Ok(()),
            |_, _| Store::open_in_memory_for_test("identity-a")
                .map_err(|_| FileIndexError::Unavailable),
        ));
        let state = index.state.lock().unwrap();
        assert!(state.fatal_unavailable);
        assert!(state.recovery_owner.is_none());
        assert!(state.store.is_none());
        drop(state);
        assert!(index.coordinator.state.try_lock().is_ok());
    }

    #[test]
    fn recovery_timeout_wins_over_outstanding_db_work_without_closing_store() {
        let (index, _) = active_task7_index();
        let reporter = index.reserve_db_work_for_test(0).unwrap();
        let blocker = index.reserve_db_work_for_test(0).unwrap();
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        drop(reporter);
        let deadline = index.state.lock().unwrap().recovery_deadline.unwrap();
        let deletes = Cell::new(0);
        let opens = Cell::new(0);
        assert!(!index.drive_recovery_with(
            || deadline,
            || Ok(PathBuf::from("file-index.sqlite3")),
            || Ok(Vec::new()),
            |_| {
                deletes.set(deletes.get() + 1);
                Ok(())
            },
            |_, _| {
                opens.set(opens.get() + 1);
                Store::open_in_memory_for_test("identity-a")
                    .map_err(|_| FileIndexError::Unavailable)
            },
        ));
        let state = index.state.lock().unwrap();
        assert!(state.fatal_unavailable);
        assert!(!state.admission_open);
        assert!(state.store.is_some());
        assert!(state.recovery_owner.is_none());
        drop(state);
        assert_eq!(deletes.get(), 0);
        assert_eq!(opens.get(), 0);
        drop(blocker);
        assert_eq!(index.db_work_count_for_test(), 0);
        assert!(!index.drive_recovery());
    }

    #[test]
    fn recovery_create_schema_and_seed_revalidate_cleaning_at_every_stage() {
        for cancelled_stage in 1..=7usize {
            let (index, lifecycle) = active_task7_index();
            let reporter = index.reserve_db_work_for_test(0).unwrap();
            assert!(index.transition_recovery(&reporter, || {
                index.registry.invalidate_domain(QueryDomain::File)
            }));
            drop(reporter);
            let directory = TestDir::new();
            fs::create_dir_all(directory.path()).unwrap();
            let database = directory.path().join("file-index.sqlite3");
            let authorizations = Cell::new(0usize);
            let later_stage = Cell::new(false);
            assert!(!index.drive_recovery_with(
                Instant::now,
                || Ok(database.clone()),
                || Ok(Vec::new()),
                |_| Ok(()),
                |path, authorize| {
                    Store::open_authorized(path, "identity-a", || {
                        let call = authorizations.get() + 1;
                        authorizations.set(call);
                        if call == cancelled_stage {
                            lifecycle.set_file_index_mirror_for_test(
                                crate::lifecycle::FileIndexPhase::Cleaning,
                                1,
                            );
                        } else if call > cancelled_stage {
                            later_stage.set(true);
                        }
                        authorize()
                    })
                    .map_err(super::map_store_error)
                },
            ));
            assert_eq!(authorizations.get(), cancelled_stage);
            assert!(!later_stage.get());
            assert!(!index.state.lock().unwrap().fatal_unavailable);
            drop(index);
        }
    }

    #[test]
    fn cleaning_pause_return_running_and_terminal_are_linearized() {
        let (index, lifecycle) = active_task7_index();
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);
        assert!(index.start_cleaning_for_test(1));
        assert!(!index.admission_open_for_test());
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Running, 1);
        assert!(index.return_running_for_test(1));
        assert!(index.complete_pause_for_test());
        assert_eq!(index.mode_for_test(), LifecycleMode::Uninitialized);
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Terminal, 1);
        index.terminal_for_test(1);
        assert_eq!(index.mode_for_test(), LifecycleMode::Terminal);
        assert!(!index.return_running_for_test(1));
    }

    #[test]
    fn cleaning_attempt_handover_cannot_wedge() {
        let (index, lifecycle) = active_task7_index();
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 1);
        assert!(index.start_cleaning_for_test(1));
        let epoch = index.runtime_epoch();
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 2);
        assert!(index.start_cleaning_for_test(2));
        assert_eq!(index.runtime_epoch(), epoch);
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Running, 2);
        assert!(index.return_running_for_test(2));
        assert!(index.complete_pause_for_test());
        assert_eq!(index.mode_for_test(), LifecycleMode::Uninitialized);
    }

    #[test]
    fn cleaning_without_a_started_file_session_is_vacuously_clean() {
        let lifecycle = Arc::new(crate::lifecycle::LifecycleCoordinator::default());
        let index = Arc::new(FileIndex::new(
            Arc::clone(&lifecycle),
            ResultRegistry::default(),
        ));
        lifecycle.set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 11);
        let deadline = Instant::now() + Duration::from_secs(5);
        assert!(index.start_cleaning_until(11, deadline));
        let waits = Cell::new(0);
        assert!(index.mark_clean_close_with(11, Instant::now, || { waits.set(waits.get() + 1) }));
        assert_eq!(waits.get(), 0);

        let blocked_lifecycle = Arc::new(crate::lifecycle::LifecycleCoordinator::default());
        let blocked = Arc::new(FileIndex::new(
            Arc::clone(&blocked_lifecycle),
            ResultRegistry::default(),
        ));
        blocked.state.lock().unwrap().session_started = true;
        blocked_lifecycle
            .set_file_index_mirror_for_test(crate::lifecycle::FileIndexPhase::Cleaning, 12);
        assert!(blocked.start_cleaning_until(12, deadline));
        assert!(!blocked.mark_clean_close_with(12, Instant::now, || {
            panic!("unreachable clean-close state must reject without waiting")
        }));
    }

    #[test]
    fn file_execution_binds_epoch_row_volume_path_and_kind() {
        let (index, _) = active_task7_index();
        let volume = VolumeIdentity::for_test(r"\\?\Volume{EXECUTION}\", 41, "ntfs");
        let action = OpenIndexedPath::for_test(
            index.runtime_epoch(),
            19,
            volume.clone(),
            r"docs\report.pdf",
            IndexedKind::File,
        );

        let reservation = index.reserve_execution_for_test(&action).unwrap();
        assert_eq!(reservation.runtime_epoch(), action.runtime_epoch());
        assert_eq!(index.execution_count_for_test(), 1);
        assert!(index.execution_action_matches_for_test(
            &action,
            19,
            &volume,
            r"docs\report.pdf",
            IndexedKind::File,
        ));
        assert!(!index.execution_action_matches_for_test(
            &action,
            19,
            &volume,
            r"docs\other.pdf",
            IndexedKind::File,
        ));
        drop(reservation);
        assert_eq!(index.execution_count_for_test(), 0);
    }

    #[test]
    fn execution_and_recovery_have_two_valid_linearizations() {
        let (index, _) = active_task7_index();
        let action = OpenIndexedPath::for_test(
            index.runtime_epoch(),
            1,
            volume(),
            "report.pdf",
            IndexedKind::File,
        );
        let execution = index.reserve_execution_for_test(&action).unwrap();
        assert!(!index.recovery_quiescent_for_test());
        drop(execution);
        assert!(index.recovery_quiescent_for_test());

        let reporter = index
            .reserve_db_work_for_test(index.runtime_epoch())
            .unwrap();
        assert!(index.transition_recovery(&reporter, || {
            index.registry.invalidate_domain(QueryDomain::File)
        }));
        assert!(index.reserve_execution_for_test(&action).is_err());
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct VolumeIdentity {
    volume_guid_path: String,
    volume_serial: u32,
    filesystem_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IndexedKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OpenIndexedPath {
    runtime_epoch: u64,
    row_id: i64,
    volume_identity: VolumeIdentity,
    relative_path: String,
    kind: IndexedKind,
}

impl OpenIndexedPath {
    #[cfg(test)]
    pub(crate) fn for_test(
        runtime_epoch: u64,
        row_id: i64,
        volume_identity: VolumeIdentity,
        relative_path: impl Into<String>,
        kind: IndexedKind,
    ) -> Self {
        Self {
            runtime_epoch,
            row_id,
            volume_identity,
            relative_path: relative_path.into(),
            kind,
        }
    }

    pub(crate) fn runtime_epoch(&self) -> u64 {
        self.runtime_epoch
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionOutcome {
    FileRevealRequested,
    FolderOpenRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionError {
    SearchUnavailable,
    Stale,
    NotFound,
    OpenFailed,
}

impl VolumeIdentity {
    #[cfg(test)]
    pub(crate) fn for_test(
        volume_guid_path: impl Into<String>,
        volume_serial: u32,
        filesystem_name: impl Into<String>,
    ) -> Self {
        Self {
            volume_guid_path: volume_guid_path.into(),
            volume_serial,
            filesystem_name: filesystem_name.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileCategory {
    All,
    Folder,
    Excel,
    Word,
    Ppt,
    Pdf,
    Image,
    Video,
    Audio,
    Archive,
}

impl FileCategory {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "all" => Self::All,
            "folder" => Self::Folder,
            "excel" => Self::Excel,
            "word" => Self::Word,
            "ppt" => Self::Ppt,
            "pdf" => Self::Pdf,
            "image" => Self::Image,
            "video" => Self::Video,
            "audio" => Self::Audio,
            "archive" => Self::Archive,
            _ => return None,
        })
    }

    fn store_value(self) -> Option<&'static str> {
        Some(match self {
            Self::All => return None,
            Self::Folder => "folder",
            Self::Excel => "excel",
            Self::Word => "word",
            Self::Ppt => "ppt",
            Self::Pdf => "pdf",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Archive => "archive",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileSort {
    ModifiedDesc,
    ModifiedAsc,
}

impl FileSort {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "modifiedDesc" => Some(Self::ModifiedDesc),
            "modifiedAsc" => Some(Self::ModifiedAsc),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuerySpec {
    pub(crate) folded_query: String,
    pub(crate) category: FileCategory,
    pub(crate) sort: FileSort,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndexEntry {
    relative_path: String,
    display_path: String,
    name: String,
    folded_name: String,
    kind: IndexedKind,
    category: String,
    size_bytes: Option<u64>,
    modified_utc_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileIndexStatus {
    Building,
    Ready,
    Partial,
    Rebuilding,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileResultKind {
    File,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileResultDraft {
    pub(crate) action: OpenIndexedPath,
    pub(crate) name: String,
    pub(crate) kind: FileResultKind,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_utc: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileResultItem {
    pub(crate) result_id: String,
    pub(crate) name: String,
    pub(crate) kind: FileResultKind,
    pub(crate) size_bytes: Option<String>,
    pub(crate) modified_utc: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileSearchResponse {
    pub(crate) request_id: String,
    pub(crate) index_revision: String,
    pub(crate) total: String,
    pub(crate) status: FileIndexStatus,
    pub(crate) items: Vec<FileResultItem>,
}

pub(crate) struct FileSearchBatch {
    pub(crate) runtime_epoch: u64,
    pub(crate) publication_generation: u64,
    pub(crate) index_revision: u64,
    pub(crate) total: u64,
    pub(crate) status: FileIndexStatus,
    pub(crate) items: Vec<FileResultDraft>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Availability {
    Normal,
    Rebuilding,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleMode {
    Uninitialized,
    Opening {
        owner: u64,
    },
    Active,
    Pausing {
        attempt_epoch: u64,
        resume_requested: bool,
    },
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LazyInitDecision {
    Start { owner: u64 },
    ObserveBuilding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionError {
    Unavailable,
    EpochMismatch,
    OwnerExhausted,
    WrongMode,
    Lifecycle,
    CounterExhausted,
}

#[derive(Clone, Copy)]
enum AdmissionKind {
    LazyInit,
    DbWork,
    Execution,
}

struct IndexState {
    mode: LifecycleMode,
    lazy_owner_high_water: u64,
    availability: Availability,
    admission_open: bool,
    fatal_unavailable: bool,
    runtime_epoch: u64,
    index_revision_high_water: u64,
    db_work: usize,
    execution_work: usize,
    recovery_owner: Option<u64>,
    recovery_deadline: Option<std::time::Instant>,
    pause_deadline: Option<std::time::Instant>,
    inventory_observation: u64,
    authenticated_volumes: Vec<VolumeIdentity>,
    authenticated_mounts: Vec<(VolumeIdentity, String)>,
    inventory_previous_authenticated: Option<Vec<VolumeIdentity>>,
    pending_inventory_transitions: HashSet<VolumeIdentity>,
    quarantined_volumes: HashSet<VolumeIdentity>,
    authenticated_app_data_root: Option<PathBuf>,
    store: Option<Store>,
    retained_store: Option<Store>,
    session_started: bool,
    prior_integrity: Option<store::PriorIntegrityMetadata>,
    integrity_started: bool,
    integrity_pending: bool,
    clean_close_permit_issued: bool,
    hide_requested: bool,
    hide_dispatching: bool,
    hide_issued: bool,
}

impl Default for IndexState {
    fn default() -> Self {
        Self {
            mode: LifecycleMode::Uninitialized,
            lazy_owner_high_water: 0,
            availability: Availability::Normal,
            admission_open: false,
            fatal_unavailable: false,
            runtime_epoch: 0,
            index_revision_high_water: 0,
            db_work: 0,
            execution_work: 0,
            recovery_owner: None,
            recovery_deadline: None,
            pause_deadline: None,
            inventory_observation: 0,
            authenticated_volumes: Vec::new(),
            authenticated_mounts: Vec::new(),
            inventory_previous_authenticated: None,
            pending_inventory_transitions: HashSet::new(),
            quarantined_volumes: HashSet::new(),
            authenticated_app_data_root: None,
            store: None,
            retained_store: None,
            session_started: false,
            prior_integrity: None,
            integrity_started: false,
            integrity_pending: false,
            clean_close_permit_issued: false,
            hide_requested: false,
            hide_dispatching: false,
            hide_issued: false,
        }
    }
}

impl IndexState {
    fn advance_revision_locked(
        &mut self,
        publication_generation: &AtomicU64,
    ) -> Result<u64, AdmissionError> {
        let Some(next) = self.index_revision_high_water.checked_add(1) else {
            return Err(AdmissionError::CounterExhausted);
        };
        let persisted = self
            .store
            .as_mut()
            .ok_or(AdmissionError::Unavailable)
            .and_then(|store| {
                store
                    .persist_index_revision(next)
                    .map_err(|_| AdmissionError::Unavailable)
            });
        if persisted.is_err() {
            self.latch_unavailable(publication_generation);
            return Err(AdmissionError::Unavailable);
        }
        self.index_revision_high_water = next;
        Ok(next)
    }

    fn latch_unavailable(&mut self, publication_generation: &AtomicU64) -> bool {
        let newly_fatal = !self.fatal_unavailable;
        if newly_fatal {
            let _ = publication_generation.fetch_update(
                Ordering::AcqRel,
                Ordering::Acquire,
                |generation| generation.checked_add(1),
            );
        }
        self.fatal_unavailable = true;
        self.availability = Availability::Unavailable;
        self.admission_open = false;
        self.authenticated_volumes.clear();
        self.authenticated_mounts.clear();
        self.inventory_previous_authenticated = None;
        self.pending_inventory_transitions.clear();
        self.quarantined_volumes.clear();
        self.authenticated_app_data_root = None;
        self.store = None;
        self.retained_store = None;
        newly_fatal
    }
}

fn begin_lazy_init_locked(
    state: &mut IndexState,
    expected_runtime_epoch: u64,
    _publication_generation: &AtomicU64,
) -> Result<LazyInitDecision, AdmissionError> {
    if state.fatal_unavailable || state.availability == Availability::Unavailable {
        return Err(AdmissionError::Unavailable);
    }
    if state.runtime_epoch != expected_runtime_epoch {
        return Err(AdmissionError::EpochMismatch);
    }
    match state.mode {
        LifecycleMode::Uninitialized => {
            let Some(owner) = state.lazy_owner_high_water.checked_add(1) else {
                return Err(AdmissionError::OwnerExhausted);
            };
            state.lazy_owner_high_water = owner;
            state.mode = LifecycleMode::Opening { owner };
            state.admission_open = false;
            Ok(LazyInitDecision::Start { owner })
        }
        LifecycleMode::Opening { .. } => Ok(LazyInitDecision::ObserveBuilding),
        LifecycleMode::Active | LifecycleMode::Pausing { .. } | LifecycleMode::Terminal => {
            Err(AdmissionError::WrongMode)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileIndexError {
    Unavailable,
    RecoveryRequired,
}

fn validate_index_path_shape(
    is_directory: bool,
    is_file: bool,
    attributes: u32,
    expected_directory: bool,
) -> Result<(), FileIndexError> {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || (expected_directory && !is_directory)
        || (!expected_directory && !is_file)
    {
        return Err(FileIndexError::Unavailable);
    }
    Ok(())
}

fn validate_index_path(
    metadata: &fs::Metadata,
    expected_directory: bool,
) -> Result<(), FileIndexError> {
    validate_index_path_shape(
        metadata.is_dir(),
        metadata.is_file(),
        metadata.file_attributes(),
        expected_directory,
    )
}

fn authenticate_app_data_root(app_data_dir: &Path) -> Result<PathBuf, FileIndexError> {
    fs::create_dir_all(app_data_dir).map_err(|_| FileIndexError::Unavailable)?;
    let root_metadata =
        fs::symlink_metadata(app_data_dir).map_err(|_| FileIndexError::Unavailable)?;
    validate_index_path(&root_metadata, true)?;
    let root = fs::canonicalize(app_data_dir).map_err(|_| FileIndexError::Unavailable)?;
    let canonical_metadata =
        fs::symlink_metadata(&root).map_err(|_| FileIndexError::Unavailable)?;
    validate_index_path(&canonical_metadata, true)?;

    let database = root.join("file-index.sqlite3");
    if database.parent() != Some(root.as_path()) {
        return Err(FileIndexError::Unavailable);
    }
    match fs::symlink_metadata(&database) {
        Ok(metadata) => {
            validate_index_path(&metadata, false)?;
            let database = fs::canonicalize(database).map_err(|_| FileIndexError::Unavailable)?;
            if database.parent() != Some(root.as_path()) {
                return Err(FileIndexError::Unavailable);
            }
            Ok(database)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(database),
        Err(_) => Err(FileIndexError::Unavailable),
    }
}

fn open_store(database: &Path) -> Result<(Store, u64, Option<u64>), FileIndexError> {
    let identity = ordinal_sort_identity().map_err(|_| FileIndexError::Unavailable)?;
    let mut store = Store::open(database, &identity).map_err(map_store_error)?;
    let identity_change = store
        .ensure_sort_identity(&identity)
        .map_err(map_store_error)?;
    let mut revision = match identity_change {
        Some((_, revision)) => revision,
        None => store.index_revision().map_err(map_store_error)?,
    };
    let recovered = store.recover_candidates().map_err(map_store_error)?;
    if let Some(recovered) = recovered {
        revision = recovered;
    }
    Ok((
        store,
        revision,
        recovered
            .is_none()
            .then(|| identity_change.map(|(previous, _)| previous))
            .flatten(),
    ))
}

pub(crate) struct FileIndex {
    state: Arc<Mutex<IndexState>>,
    lifecycle: Arc<LifecycleCoordinator>,
    registry: ResultRegistry,
    main_window_hwnd: AtomicIsize,
    coordinator: Arc<CoordinatorControl>,
    workers: Mutex<WorkerRegistry>,
    integrity_worker: Mutex<Option<IntegrityWorkerRecord>>,
    publication_runtime_epoch: AtomicU64,
    publication_generation: AtomicU64,
}

fn map_store_error(error: StoreError) -> FileIndexError {
    match error {
        StoreError::Corrupt => FileIndexError::RecoveryRequired,
        StoreError::Sqlite
        | StoreError::InvalidData
        | StoreError::Platform
        | StoreError::RevisionExhausted => FileIndexError::Unavailable,
    }
}

struct DbWorkReservation {
    state: Arc<Mutex<IndexState>>,
    coordinator: Arc<CoordinatorControl>,
    runtime_epoch: u64,
    released: bool,
}

pub(crate) struct FileExecutionReservation {
    state: Arc<Mutex<IndexState>>,
    coordinator: Arc<CoordinatorControl>,
    runtime_epoch: u64,
    released: bool,
}

impl FileExecutionReservation {
    #[cfg(test)]
    fn runtime_epoch(&self) -> u64 {
        self.runtime_epoch
    }
}

enum SearchAdmission {
    Work {
        owner: Option<u64>,
        reservation: DbWorkReservation,
    },
    Immediate(FileSearchBatch),
}

struct CleanCloseMarkerPermit {
    attempt_epoch: u64,
    state: Weak<Mutex<IndexState>>,
    lifecycle: Arc<LifecycleCoordinator>,
}

impl CleanCloseMarkerPermit {
    fn is_authorized(&self) -> bool {
        if self.lifecycle.file_index_phase() != FileIndexPhase::Cleaning
            || self.lifecycle.file_index_attempt_epoch() != self.attempt_epoch
        {
            return false;
        }
        self.state.upgrade().is_some_and(|state| {
            let state = state.lock().expect("file index lock poisoned");
            state.mode
                == (LifecycleMode::Pausing {
                    attempt_epoch: self.attempt_epoch,
                    resume_requested: false,
                })
                && state.clean_close_permit_issued
        })
    }
}

enum CleanCloseReadiness {
    Permit(Store, CleanCloseMarkerPermit),
    Vacuous,
    Wait,
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryBoundary {
    Authorized,
    Waiting,
    Cancelled,
    TimedOut,
}

impl Drop for DbWorkReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        state.db_work = state
            .db_work
            .checked_sub(1)
            .expect("file index DB-work reservation underflow");
        self.released = true;
        drop(state);
        self.coordinator.signal.notify_all();
    }
}

impl Drop for FileExecutionReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        state.execution_work = state
            .execution_work
            .checked_sub(1)
            .expect("file index execution reservation underflow");
        self.released = true;
        drop(state);
        self.coordinator.signal.notify_all();
    }
}

struct IndexChangeBatch {
    deleted_prefixes: Vec<String>,
    entries: Vec<IndexEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Calibration {
    Idle,
    Pending {
        deadline: std::time::Instant,
        runtime_epoch: u64,
    },
    Running {
        runtime_epoch: u64,
    },
}

struct VolumeRuntime {
    calibration: Calibration,
    consecutive_failures: u32,
}

impl Default for VolumeRuntime {
    fn default() -> Self {
        Self {
            calibration: Calibration::Idle,
            consecutive_failures: 0,
        }
    }
}

impl VolumeRuntime {
    fn request(&mut self, now: std::time::Instant, runtime_epoch: u64) -> bool {
        match self.calibration {
            Calibration::Idle => {
                self.calibration = Calibration::Pending {
                    deadline: now + calibration_backoff(self.consecutive_failures),
                    runtime_epoch,
                };
                true
            }
            Calibration::Pending {
                runtime_epoch: pending_epoch,
                ..
            } if pending_epoch == runtime_epoch => false,
            Calibration::Running {
                runtime_epoch: running_epoch,
            } if running_epoch == runtime_epoch => {
                self.calibration = Calibration::Pending {
                    deadline: now,
                    runtime_epoch,
                };
                true
            }
            Calibration::Pending { .. } | Calibration::Running { .. } => {
                self.calibration = Calibration::Pending {
                    deadline: now,
                    runtime_epoch,
                };
                true
            }
        }
    }

    fn start_if_due(&mut self, now: std::time::Instant, runtime_epoch: u64) -> bool {
        match self.calibration {
            Calibration::Pending {
                deadline,
                runtime_epoch: pending_epoch,
            } if pending_epoch == runtime_epoch && deadline <= now => {
                self.calibration = Calibration::Running { runtime_epoch };
                true
            }
            Calibration::Idle | Calibration::Pending { .. } | Calibration::Running { .. } => false,
        }
    }

    fn finish_success(&mut self, runtime_epoch: u64) {
        if self.calibration == (Calibration::Running { runtime_epoch }) {
            self.calibration = Calibration::Idle;
            self.consecutive_failures = 0;
        }
    }

    fn cancel_pending(&mut self) {
        self.calibration = Calibration::Idle;
    }

    fn finish_failure(
        &mut self,
        now: std::time::Instant,
        runtime_epoch: u64,
    ) -> Result<(), FileIndexError> {
        if !matches!(
            self.calibration,
            Calibration::Running { runtime_epoch: epoch }
                | Calibration::Pending { runtime_epoch: epoch, .. }
                if epoch == runtime_epoch
        ) {
            return Err(FileIndexError::Unavailable);
        }
        self.consecutive_failures = self
            .consecutive_failures
            .checked_add(1)
            .ok_or(FileIndexError::Unavailable)?;
        self.calibration = Calibration::Pending {
            deadline: now + calibration_backoff(self.consecutive_failures),
            runtime_epoch,
        };
        Ok(())
    }

    fn finish_start_attempt(
        &mut self,
        succeeded: bool,
        now: std::time::Instant,
        runtime_epoch: u64,
        failures_before: u32,
    ) -> Result<(), FileIndexError> {
        if succeeded {
            self.finish_success(runtime_epoch);
            return Ok(());
        }
        if self.consecutive_failures == failures_before {
            self.finish_failure(now, runtime_epoch)?;
        }
        Ok(())
    }
}

fn calibration_backoff(consecutive_failures: u32) -> std::time::Duration {
    if consecutive_failures == 0 {
        return std::time::Duration::ZERO;
    }
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    std::time::Duration::from_secs(1u64.checked_shl(exponent).unwrap_or(64).min(60))
}

#[derive(Default)]
struct CoordinatorState {
    thread_started: bool,
    running: bool,
    calibrated: bool,
    pending_root: Option<PathBuf>,
    pending_runtime_epoch: Option<u64>,
    active_root: Option<PathBuf>,
    wakes: u64,
    volumes: HashMap<VolumeIdentity, VolumeRuntime>,
}

#[derive(Default)]
struct CoordinatorControl {
    state: Mutex<CoordinatorState>,
    signal: Condvar,
    stop: AtomicBool,
    join: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Default)]
struct WorkerRegistry {
    next_owner: u64,
    by_volume: HashMap<VolumeIdentity, WorkerRecord>,
}

struct WorkerRecord {
    owner: u64,
    runtime_epoch: u64,
    mount_point: PathBuf,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    join: Option<std::thread::JoinHandle<()>>,
    failed: bool,
}

#[cfg(test)]
enum WorkerPreparation {
    Existing,
    Start { owner: u64 },
}

struct WorkerStart {
    owner: u64,
    runtime_epoch: u64,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    reservation: DbWorkReservation,
}

enum WorkerStartDecision {
    Existing,
    Start(WorkerStart),
}

#[cfg(not(test))]
enum ScanMessage {
    Batch(Vec<IndexEntry>),
    Finished(Result<ScanSummary, BackendError>),
}

#[cfg(not(test))]
struct ScanReplayContext<'a> {
    volume: &'a FixedVolume,
    exclusions: &'a [ExcludedPrefix],
    generation: u64,
    has_committed: bool,
    owner: u64,
    runtime_epoch: u64,
}

#[cfg(not(test))]
struct ScannerGuard {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(test))]
impl ScannerGuard {
    fn finish(mut self) -> Result<(), FileIndexError> {
        self.stop.store(true, Ordering::Release);
        let join = self.join.take().expect("scanner join disappeared");
        join.join().map_err(|_| FileIndexError::Unavailable)
    }
}

#[cfg(not(test))]
impl Drop for ScannerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(not(test))]
fn send_scan_message(
    sender: &mpsc::SyncSender<ScanMessage>,
    mut message: ScanMessage,
    stop: &AtomicBool,
) -> Result<(), BackendError> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(BackendError::Stopped);
        }
        match sender.try_send(message) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned)) => {
                message = returned;
                thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return Err(BackendError::Stopped),
        }
    }
}

impl Default for FileIndex {
    fn default() -> Self {
        Self::new(
            Arc::new(LifecycleCoordinator::default()),
            ResultRegistry::default(),
        )
    }
}

struct IntegrityWorkerRecord {
    runtime_epoch: u64,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl FileIndex {
    pub(crate) fn new(lifecycle: Arc<LifecycleCoordinator>, registry: ResultRegistry) -> Self {
        Self {
            state: Arc::new(Mutex::new(IndexState::default())),
            lifecycle,
            registry,
            main_window_hwnd: AtomicIsize::new(0),
            coordinator: Arc::new(CoordinatorControl::default()),
            workers: Mutex::new(WorkerRegistry::default()),
            integrity_worker: Mutex::new(None),
            publication_runtime_epoch: AtomicU64::new(0),
            publication_generation: AtomicU64::new(0),
        }
    }

    pub(crate) fn install_main_window_hwnd(&self, hwnd: isize) -> Result<(), FileIndexError> {
        if hwnd == 0 {
            return Err(FileIndexError::Unavailable);
        }
        self.main_window_hwnd
            .compare_exchange(0, hwnd, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| FileIndexError::Unavailable)
    }

    pub(crate) fn clear_main_window_hwnd(&self, hwnd: isize) {
        let _ =
            self.main_window_hwnd
                .compare_exchange(hwnd, 0, Ordering::AcqRel, Ordering::Acquire);
    }

    fn post_close_after_fatal(hwnd: isize) -> bool {
        unsafe {
            PostMessageW(
                Some(HWND(hwnd as *mut std::ffi::c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
            .is_ok()
        }
    }

    pub(crate) fn fail_closed_exhaustion(&self) {
        let newly_fatal = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            self.latch_exhaustion_locked(&mut state)
        };
        if newly_fatal {
            self.consume_fatal_effects();
        }
    }

    fn latch_exhaustion_locked(&self, state: &mut IndexState) -> bool {
        let newly_fatal = self.latch_process_fatal(state);
        if newly_fatal {
            state.hide_requested = true;
        }
        newly_fatal
    }

    fn consume_fatal_effects(&self) {
        self.consume_fatal_effects_with(Self::post_close_after_fatal);
    }

    fn consume_fatal_effects_with<P>(&self, post: P)
    where
        P: FnOnce(isize) -> bool,
    {
        {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if !state.hide_requested || state.hide_dispatching || state.hide_issued {
                return;
            }
            state.hide_dispatching = true;
        }
        let _ = self.registry.invalidate_domain(QueryDomain::File);
        let hwnd = self.main_window_hwnd.load(Ordering::Acquire);
        let posted = hwnd != 0 && post(hwnd);
        {
            let mut state = self.state.lock().expect("file index lock poisoned");
            state.hide_dispatching = false;
            if posted && !state.hide_issued {
                state.hide_issued = true;
                state.hide_requested = false;
            }
        }
    }

    pub(crate) fn runtime_epoch(&self) -> u64 {
        self.publication_runtime_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn execute_indexed_path(
        self: &Arc<Self>,
        action: OpenIndexedPath,
    ) -> Result<FileExecutionOutcome, FileExecutionError> {
        let reservation = self
            .reserve_execution(action.runtime_epoch())
            .map_err(|_| FileExecutionError::SearchUnavailable)?;
        let (volume, row_match) = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            let mount = state
                .authenticated_mounts
                .iter()
                .find(|(identity, _)| identity == &action.volume_identity)
                .map(|(_, mount)| mount.clone())
                .ok_or(FileExecutionError::Stale)?;
            let result = state
                .store
                .as_mut()
                .ok_or(FileExecutionError::SearchUnavailable)?
                .execution_row_matches(&action);
            (
                FixedVolume {
                    identity: action.volume_identity.clone(),
                    mount_point: PathBuf::from(mount),
                },
                result,
            )
        };
        let row_match = match row_match {
            Ok(row_match) => row_match,
            Err(StoreError::Corrupt) => {
                let _ = self.request_recovery_from_execution(&reservation);
                return Err(FileExecutionError::SearchUnavailable);
            }
            Err(
                StoreError::Sqlite
                | StoreError::InvalidData
                | StoreError::Platform
                | StoreError::RevisionExhausted,
            ) => return Err(FileExecutionError::SearchUnavailable),
        };
        if !row_match {
            return Err(FileExecutionError::Stale);
        }
        windows_backend::reauthenticate_volume(&volume).map_err(|error| match error {
            BackendError::Missing => FileExecutionError::NotFound,
            BackendError::InvalidData => FileExecutionError::Stale,
            BackendError::Platform
            | BackendError::Denied
            | BackendError::Overflow
            | BackendError::Stopped => FileExecutionError::OpenFailed,
        })?;
        windows_backend::execute_indexed_path(&volume, &action).map_err(|error| match error {
            BackendError::Missing => FileExecutionError::NotFound,
            BackendError::InvalidData => FileExecutionError::Stale,
            BackendError::Platform
            | BackendError::Denied
            | BackendError::Overflow
            | BackendError::Stopped => FileExecutionError::OpenFailed,
        })
    }

    fn admit_locked(
        &self,
        state: &IndexState,
        kind: AdmissionKind,
        expected_runtime_epoch: u64,
    ) -> Result<(), AdmissionError> {
        if self.lifecycle.file_index_phase() != FileIndexPhase::Running {
            return Err(AdmissionError::Lifecycle);
        }
        if state.fatal_unavailable || state.availability == Availability::Unavailable {
            return Err(AdmissionError::Unavailable);
        }
        if state.runtime_epoch != expected_runtime_epoch {
            return Err(AdmissionError::EpochMismatch);
        }
        match kind {
            AdmissionKind::LazyInit
                if matches!(
                    state.mode,
                    LifecycleMode::Uninitialized | LifecycleMode::Opening { .. }
                ) =>
            {
                Ok(())
            }
            AdmissionKind::DbWork
                if state.mode == LifecycleMode::Active && state.admission_open =>
            {
                Ok(())
            }
            AdmissionKind::Execution
                if state.mode == LifecycleMode::Active && state.admission_open =>
            {
                Ok(())
            }
            AdmissionKind::LazyInit | AdmissionKind::DbWork | AdmissionKind::Execution => {
                Err(AdmissionError::WrongMode)
            }
        }
    }

    fn reserve_execution(
        &self,
        expected_runtime_epoch: u64,
    ) -> Result<FileExecutionReservation, AdmissionError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        self.admit_locked(&state, AdmissionKind::Execution, expected_runtime_epoch)?;
        let Some(next) = state.execution_work.checked_add(1) else {
            let newly_fatal = self.latch_exhaustion_locked(&mut state);
            drop(state);
            if newly_fatal {
                self.consume_fatal_effects();
            }
            return Err(AdmissionError::CounterExhausted);
        };
        state.execution_work = next;
        Ok(FileExecutionReservation {
            state: Arc::clone(&self.state),
            coordinator: Arc::clone(&self.coordinator),
            runtime_epoch: expected_runtime_epoch,
            released: false,
        })
    }

    fn reserve_db_work(
        &self,
        expected_runtime_epoch: u64,
    ) -> Result<DbWorkReservation, AdmissionError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        self.admit_locked(&state, AdmissionKind::DbWork, expected_runtime_epoch)?;
        let result = self.reserve_db_work_locked(&mut state, expected_runtime_epoch);
        let consume_fatal = state.hide_requested;
        drop(state);
        if consume_fatal {
            self.consume_fatal_effects();
        }
        result
    }

    fn reserve_db_work_locked(
        &self,
        state: &mut IndexState,
        expected_runtime_epoch: u64,
    ) -> Result<DbWorkReservation, AdmissionError> {
        let Some(next) = state.db_work.checked_add(1) else {
            self.latch_exhaustion_locked(state);
            return Err(AdmissionError::CounterExhausted);
        };
        state.db_work = next;
        Ok(DbWorkReservation {
            state: Arc::clone(&self.state),
            coordinator: Arc::clone(&self.coordinator),
            runtime_epoch: expected_runtime_epoch,
            released: false,
        })
    }

    fn begin_search(&self, expected_runtime_epoch: u64) -> Result<SearchAdmission, FileIndexError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if self.lifecycle.file_index_phase() != FileIndexPhase::Running {
            return Ok(SearchAdmission::Immediate(empty_batch(
                expected_runtime_epoch,
                self.publication_generation.load(Ordering::Acquire),
                state.index_revision_high_water,
                FileIndexStatus::Unavailable,
            )));
        }
        if state.runtime_epoch != expected_runtime_epoch {
            return Err(FileIndexError::Unavailable);
        }
        if state.fatal_unavailable {
            return Ok(SearchAdmission::Immediate(empty_batch(
                expected_runtime_epoch,
                self.publication_generation.load(Ordering::Acquire),
                state.index_revision_high_water,
                FileIndexStatus::Unavailable,
            )));
        }
        if state.availability == Availability::Rebuilding {
            return Ok(SearchAdmission::Immediate(empty_batch(
                expected_runtime_epoch,
                self.publication_generation.load(Ordering::Acquire),
                state.index_revision_high_water,
                FileIndexStatus::Rebuilding,
            )));
        }
        let owner = match state.mode {
            LifecycleMode::Active => {
                self.admit_locked(&state, AdmissionKind::DbWork, expected_runtime_epoch)
                    .map_err(|_| FileIndexError::Unavailable)?;
                None
            }
            LifecycleMode::Uninitialized | LifecycleMode::Opening { .. } => {
                self.admit_locked(&state, AdmissionKind::LazyInit, expected_runtime_epoch)
                    .map_err(|_| FileIndexError::Unavailable)?;
                match begin_lazy_init_locked(
                    &mut state,
                    expected_runtime_epoch,
                    &self.publication_generation,
                ) {
                    Err(AdmissionError::OwnerExhausted) => {
                        let newly_fatal = self.latch_exhaustion_locked(&mut state);
                        drop(state);
                        if newly_fatal {
                            self.consume_fatal_effects();
                        }
                        return Err(FileIndexError::Unavailable);
                    }
                    Err(_) => return Err(FileIndexError::Unavailable),
                    Ok(LazyInitDecision::Start { owner }) => Some(owner),
                    Ok(LazyInitDecision::ObserveBuilding) => {
                        return Ok(SearchAdmission::Immediate(empty_batch(
                            expected_runtime_epoch,
                            self.publication_generation.load(Ordering::Acquire),
                            state.index_revision_high_water,
                            FileIndexStatus::Building,
                        )));
                    }
                }
            }
            LifecycleMode::Pausing { .. } | LifecycleMode::Terminal => {
                return Ok(SearchAdmission::Immediate(empty_batch(
                    expected_runtime_epoch,
                    self.publication_generation.load(Ordering::Acquire),
                    state.index_revision_high_water,
                    FileIndexStatus::Unavailable,
                )));
            }
        };
        let reservation = self
            .reserve_db_work_locked(&mut state, expected_runtime_epoch)
            .map_err(|_| FileIndexError::Unavailable);
        let consume_fatal = state.hide_requested;
        drop(state);
        if consume_fatal {
            self.consume_fatal_effects();
        }
        Ok(SearchAdmission::Work {
            owner,
            reservation: reservation?,
        })
    }

    #[cfg(test)]
    fn reserve_db_work_for_test(
        &self,
        expected_runtime_epoch: u64,
    ) -> Result<DbWorkReservation, AdmissionError> {
        self.reserve_db_work(expected_runtime_epoch)
    }

    #[cfg(test)]
    fn reserve_execution_for_test(
        &self,
        action: &OpenIndexedPath,
    ) -> Result<FileExecutionReservation, AdmissionError> {
        self.reserve_execution(action.runtime_epoch)
    }

    #[cfg(test)]
    fn execution_count_for_test(&self) -> usize {
        self.state
            .lock()
            .expect("file index lock poisoned")
            .execution_work
    }

    #[cfg(test)]
    fn recovery_quiescent_for_test(&self) -> bool {
        let state = self.state.lock().expect("file index lock poisoned");
        state.db_work == 0 && state.execution_work == 0
    }

    #[cfg(test)]
    fn execution_action_matches_for_test(
        &self,
        action: &OpenIndexedPath,
        row_id: i64,
        identity: &VolumeIdentity,
        relative_path: &str,
        kind: IndexedKind,
    ) -> bool {
        action.row_id == row_id
            && &action.volume_identity == identity
            && action.relative_path == relative_path
            && action.kind == kind
    }

    #[cfg(test)]
    fn db_work_count_for_test(&self) -> usize {
        self.state.lock().expect("file index lock poisoned").db_work
    }

    #[cfg(test)]
    fn begin_lazy_for_test(
        &self,
        expected_runtime_epoch: u64,
    ) -> Result<LazyInitDecision, AdmissionError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        self.admit_locked(&state, AdmissionKind::LazyInit, expected_runtime_epoch)?;
        begin_lazy_init_locked(
            &mut state,
            expected_runtime_epoch,
            &self.publication_generation,
        )
    }

    fn transition_recovery<I>(self: &Arc<Self>, reporter: &DbWorkReservation, invalidate: I) -> bool
    where
        I: FnOnce() -> Result<(), crate::result_registry::DomainEpochExhausted>,
    {
        self.transition_recovery_with(
            &reporter.state,
            reporter.runtime_epoch,
            AdmissionKind::DbWork,
            invalidate,
        )
    }

    fn transition_recovery_with<I>(
        self: &Arc<Self>,
        reporter_state: &Arc<Mutex<IndexState>>,
        reporter_runtime_epoch: u64,
        reporter_kind: AdmissionKind,
        invalidate: I,
    ) -> bool
    where
        I: FnOnce() -> Result<(), crate::result_registry::DomainEpochExhausted>,
    {
        if !Arc::ptr_eq(&self.state, reporter_state) {
            return false;
        }
        let recovery_owner = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if self
                .admit_locked(&state, reporter_kind, reporter_runtime_epoch)
                .is_err()
                || state.recovery_owner.is_some()
            {
                return false;
            }
            let Some(runtime_epoch) = state.runtime_epoch.checked_add(1) else {
                let newly_fatal = self.latch_exhaustion_locked(&mut state);
                drop(state);
                if newly_fatal {
                    self.consume_fatal_effects();
                }
                return false;
            };
            let Some(revision) = state.index_revision_high_water.checked_add(1) else {
                let newly_fatal = self.latch_exhaustion_locked(&mut state);
                drop(state);
                if newly_fatal {
                    self.consume_fatal_effects();
                }
                return false;
            };
            state.index_revision_high_water = revision;
            state.recovery_owner = Some(runtime_epoch);
            state.recovery_deadline = Some(
                std::time::Instant::now()
                    .checked_add(std::time::Duration::from_secs(5))
                    .expect("five second recovery deadline overflowed"),
            );
            state.runtime_epoch = runtime_epoch;
            state.availability = Availability::Rebuilding;
            state.admission_open = false;
            self.publication_runtime_epoch
                .store(runtime_epoch, Ordering::Release);
            runtime_epoch
        };

        {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            for worker in workers.by_volume.values_mut() {
                worker.stop.store(true, Ordering::Release);
            }
        }
        if let Some(worker) = self
            .integrity_worker
            .lock()
            .expect("file index integrity join lock poisoned")
            .as_ref()
        {
            worker.stop.store(true, Ordering::Release);
        }
        {
            let mut coordinator = self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned");
            coordinator.pending_root = None;
            coordinator.pending_runtime_epoch = None;
            coordinator.calibrated = false;
            for runtime in coordinator.volumes.values_mut() {
                runtime.cancel_pending();
            }
        }
        if invalidate().is_err() {
            let still_owner = self
                .state
                .lock()
                .expect("file index lock poisoned")
                .recovery_owner
                == Some(recovery_owner);
            if still_owner {
                self.fail_closed_exhaustion();
            }
            return false;
        }
        self.coordinator.signal.notify_all();
        true
    }

    fn request_recovery(self: &Arc<Self>, reporter: &DbWorkReservation) -> bool {
        let won = self.transition_recovery(reporter, || {
            self.registry.invalidate_domain(QueryDomain::File)
        });
        if won {
            self.ensure_coordinator_thread();
            self.coordinator.signal.notify_all();
        }
        won
    }

    fn request_recovery_from_execution(
        self: &Arc<Self>,
        reporter: &FileExecutionReservation,
    ) -> bool {
        let won = self.transition_recovery_with(
            &reporter.state,
            reporter.runtime_epoch,
            AdmissionKind::Execution,
            || self.registry.invalidate_domain(QueryDomain::File),
        );
        if won {
            self.ensure_coordinator_thread();
            self.coordinator.signal.notify_all();
        }
        won
    }

    fn classify_recovery_boundary(
        &self,
        state: &IndexState,
        owner: u64,
        now: std::time::Instant,
    ) -> RecoveryBoundary {
        if state.recovery_owner != Some(owner)
            || state.runtime_epoch != owner
            || state.mode != LifecycleMode::Active
            || state.fatal_unavailable
            || self.lifecycle.file_index_phase() != FileIndexPhase::Running
        {
            return RecoveryBoundary::Cancelled;
        }
        if state
            .recovery_deadline
            .is_none_or(|deadline| now >= deadline)
        {
            return RecoveryBoundary::TimedOut;
        }
        if state.db_work != 0 || state.execution_work != 0 {
            return RecoveryBoundary::Waiting;
        }
        RecoveryBoundary::Authorized
    }

    fn recovery_boundary_locked(
        &self,
        state: &mut IndexState,
        owner: u64,
        now: std::time::Instant,
    ) -> RecoveryBoundary {
        let boundary = self.classify_recovery_boundary(state, owner, now);
        if boundary == RecoveryBoundary::TimedOut {
            state.recovery_owner = None;
            state.recovery_deadline = None;
            self.latch_recovery_timeout(state);
        }
        boundary
    }

    fn recovery_boundary(&self, owner: u64, now: std::time::Instant) -> RecoveryBoundary {
        let mut state = self.state.lock().expect("file index lock poisoned");
        self.recovery_boundary_locked(&mut state, owner, now)
    }

    fn fail_recovery_if_authorized(&self, owner: u64, now: std::time::Instant) {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if self.recovery_boundary_locked(&mut state, owner, now) == RecoveryBoundary::Authorized {
            state.recovery_owner = None;
            state.recovery_deadline = None;
            self.latch_process_fatal(&mut state);
        }
    }

    fn drive_recovery_with<N, R, V, D, O>(
        &self,
        mut now: N,
        mut reauthenticate: R,
        mut inventory: V,
        mut delete: D,
        mut open: O,
    ) -> bool
    where
        N: FnMut() -> std::time::Instant,
        R: FnMut() -> Result<PathBuf, FileIndexError>,
        V: FnMut() -> Result<Vec<FixedVolume>, FileIndexError>,
        D: FnMut(&Path) -> Result<(), FileIndexError>,
        O: FnMut(&Path, &mut dyn FnMut() -> bool) -> Result<Store, FileIndexError>,
    {
        let owner = {
            let state = self.state.lock().expect("file index lock poisoned");
            let Some(owner) = state.recovery_owner else {
                return false;
            };
            owner
        };
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        self.reap_finished_workers();
        self.reap_finished_integrity();
        if !self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .is_empty()
            || self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned")
                .is_some()
            || self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized
        {
            return false;
        }

        let database = match reauthenticate() {
            Ok(database)
                if self.recovery_boundary(owner, now()) == RecoveryBoundary::Authorized =>
            {
                database
            }
            _ => return false,
        };
        let retained = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if self.recovery_boundary_locked(&mut state, owner, now())
                != RecoveryBoundary::Authorized
            {
                return false;
            }
            state.store.take()
        };
        drop(retained);
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }

        for path in [
            database.clone(),
            PathBuf::from(format!("{}-wal", database.display())),
            PathBuf::from(format!("{}-shm", database.display())),
        ] {
            if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized
                || reauthenticate().ok().as_deref() != Some(database.as_path())
            {
                return false;
            }
            if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
                return false;
            }
            if delete(&path).is_err() {
                self.fail_recovery_if_authorized(owner, now());
                return false;
            }
            if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
                return false;
            }
        }
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let mut authorize_open =
            || self.recovery_boundary(owner, now()) == RecoveryBoundary::Authorized;
        let mut store = match open(&database, &mut authorize_open) {
            Ok(store) => store,
            Err(_) => {
                self.fail_recovery_if_authorized(owner, now());
                return false;
            }
        };
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let high_water = self
            .state
            .lock()
            .expect("file index lock poisoned")
            .index_revision_high_water;
        let mut authorize_high_water =
            || self.recovery_boundary(owner, now()) == RecoveryBoundary::Authorized;
        if store
            .persist_index_revision_authorized(high_water, &mut authorize_high_water)
            .is_err()
        {
            self.fail_recovery_if_authorized(owner, now());
            return false;
        }
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let Some(building_revision) = high_water.checked_add(1) else {
            self.fail_closed_exhaustion();
            return false;
        };
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let mut authorize_building =
            || self.recovery_boundary(owner, now()) == RecoveryBoundary::Authorized;
        if store
            .persist_index_revision_authorized(building_revision, &mut authorize_building)
            .is_err()
        {
            self.fail_recovery_if_authorized(owner, now());
            return false;
        }
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let volumes = match inventory() {
            Ok(volumes) => volumes,
            Err(_) => {
                self.fail_recovery_if_authorized(owner, now());
                return false;
            }
        };
        if self.recovery_boundary(owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let mounts = match volumes
            .iter()
            .map(|volume| {
                volume
                    .mount_point
                    .to_str()
                    .map(|mount| (volume.identity.clone(), mount.to_owned()))
                    .ok_or(FileIndexError::Unavailable)
            })
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(mounts) => mounts,
            Err(_) => {
                self.fail_recovery_if_authorized(owner, now());
                return false;
            }
        };
        let mut state = self.state.lock().expect("file index lock poisoned");
        if self.recovery_boundary_locked(&mut state, owner, now()) != RecoveryBoundary::Authorized {
            return false;
        }
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        let final_now = now();
        let final_boundary = self.classify_recovery_boundary(&state, owner, final_now);
        if final_boundary != RecoveryBoundary::Authorized {
            drop(coordinator);
            if final_boundary == RecoveryBoundary::TimedOut {
                state.recovery_owner = None;
                state.recovery_deadline = None;
                self.latch_recovery_timeout(&mut state);
            }
            drop(state);
            return false;
        }
        state.prior_integrity = Some(store.prior_integrity_metadata());
        state.session_started = true;
        state.store = Some(store);
        state.index_revision_high_water = building_revision;
        state.recovery_owner = None;
        state.recovery_deadline = None;
        state.availability = Availability::Normal;
        state.admission_open = true;
        state.authenticated_volumes.clear();
        state.authenticated_mounts = mounts;
        state.inventory_previous_authenticated = None;
        state.pending_inventory_transitions.clear();
        state.quarantined_volumes = volumes
            .iter()
            .map(|volume| volume.identity.clone())
            .collect();
        let runtime_epoch = state.runtime_epoch;
        let pending_at = now();
        coordinator.volumes.clear();
        for volume in &volumes {
            let runtime = coordinator
                .volumes
                .entry(volume.identity.clone())
                .or_default();
            runtime.calibration = Calibration::Pending {
                deadline: pending_at,
                runtime_epoch,
            };
            runtime.consecutive_failures = 0;
        }
        coordinator.active_root = state.authenticated_app_data_root.clone();
        coordinator.pending_root = coordinator.active_root.clone();
        coordinator.pending_runtime_epoch =
            coordinator.pending_root.as_ref().map(|_| runtime_epoch);
        coordinator.calibrated = volumes.is_empty();
        drop(coordinator);
        drop(state);
        self.coordinator.signal.notify_all();
        true
    }

    fn drive_recovery(&self) -> bool {
        let root = self
            .state
            .lock()
            .expect("file index lock poisoned")
            .authenticated_app_data_root
            .clone();
        let Some(root) = root else {
            let owner = self
                .state
                .lock()
                .expect("file index lock poisoned")
                .recovery_owner;
            if let Some(owner) = owner {
                let _ = self.recovery_boundary(owner, std::time::Instant::now());
            }
            return false;
        };
        self.drive_recovery_with(
            std::time::Instant::now,
            || authenticate_app_data_root(&root),
            || {
                #[cfg(not(test))]
                {
                    fixed_volumes().map_err(|_| FileIndexError::Unavailable)
                }
                #[cfg(test)]
                {
                    Ok(Vec::new())
                }
            },
            |path| match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    validate_index_path(&metadata, false)?;
                    fs::remove_file(path).map_err(|_| FileIndexError::Unavailable)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(FileIndexError::Unavailable),
            },
            |path, authorize| {
                Store::open_authorized(
                    path,
                    &ordinal_sort_identity().map_err(|_| FileIndexError::Unavailable)?,
                    authorize,
                )
                .map_err(map_store_error)
            },
        )
    }

    #[cfg(test)]
    fn revision_for_test(&self) -> u64 {
        self.state
            .lock()
            .expect("file index lock poisoned")
            .index_revision_high_water
    }

    #[cfg(test)]
    fn admission_open_for_test(&self) -> bool {
        self.state
            .lock()
            .expect("file index lock poisoned")
            .admission_open
    }

    pub(crate) fn start_cleaning_until(
        self: &Arc<Self>,
        attempt_epoch: u64,
        deadline: std::time::Instant,
    ) -> bool {
        let increment_runtime = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if self.lifecycle.file_index_phase() != FileIndexPhase::Cleaning
                || self.lifecycle.file_index_attempt_epoch() != attempt_epoch
            {
                return false;
            }
            match state.mode {
                LifecycleMode::Terminal => return false,
                LifecycleMode::Pausing {
                    attempt_epoch: current,
                    ..
                } if attempt_epoch <= current => return attempt_epoch == current,
                LifecycleMode::Pausing { .. } => {
                    state.mode = LifecycleMode::Pausing {
                        attempt_epoch,
                        resume_requested: false,
                    };
                    state.pause_deadline = Some(deadline);
                    false
                }
                LifecycleMode::Uninitialized
                | LifecycleMode::Opening { .. }
                | LifecycleMode::Active => {
                    let Some(runtime_epoch) = state.runtime_epoch.checked_add(1) else {
                        let newly_fatal = self.latch_exhaustion_locked(&mut state);
                        drop(state);
                        if newly_fatal {
                            self.consume_fatal_effects();
                        }
                        return false;
                    };
                    state.runtime_epoch = runtime_epoch;
                    state.mode = LifecycleMode::Pausing {
                        attempt_epoch,
                        resume_requested: false,
                    };
                    state.admission_open = false;
                    state.recovery_owner = None;
                    state.recovery_deadline = None;
                    state.pause_deadline = Some(deadline);
                    state.retained_store = state.store.take();
                    state.clean_close_permit_issued = false;
                    self.publication_runtime_epoch
                        .store(runtime_epoch, Ordering::Release);
                    true
                }
            }
        };
        if increment_runtime {
            if let Some(worker) = self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned")
                .as_ref()
            {
                worker.stop.store(true, Ordering::Release);
            }
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            for worker in workers.by_volume.values_mut() {
                worker.stop.store(true, Ordering::Release);
            }
            drop(workers);
            let mut coordinator = self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned");
            coordinator.pending_root = None;
            coordinator.pending_runtime_epoch = None;
            for runtime in coordinator.volumes.values_mut() {
                runtime.cancel_pending();
            }
        }
        if self.registry.invalidate_domain(QueryDomain::File).is_err() {
            self.fail_closed_exhaustion();
            return false;
        }
        #[cfg(not(test))]
        self.ensure_coordinator_thread();
        self.coordinator.signal.notify_all();
        true
    }

    pub(crate) fn return_running(self: &Arc<Self>, attempt_epoch: u64) -> bool {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if self.lifecycle.file_index_phase() != FileIndexPhase::Running
            || self.lifecycle.file_index_attempt_epoch() != attempt_epoch
        {
            return false;
        }
        let LifecycleMode::Pausing {
            attempt_epoch: current,
            ..
        } = state.mode
        else {
            return false;
        };
        if attempt_epoch < current {
            return false;
        }
        state.mode = LifecycleMode::Pausing {
            attempt_epoch,
            resume_requested: true,
        };
        drop(state);
        self.coordinator.signal.notify_all();
        true
    }

    fn complete_pause_if_ready(&self) -> bool {
        self.reap_finished_integrity();
        let (attempt_epoch, workers) = {
            let state = self.state.lock().expect("file index lock poisoned");
            let LifecycleMode::Pausing {
                attempt_epoch,
                resume_requested: true,
            } = state.mode
            else {
                return false;
            };
            if state.db_work != 0
                || state.execution_work != 0
                || self.lifecycle.file_index_phase() != FileIndexPhase::Running
                || self.lifecycle.file_index_attempt_epoch() != attempt_epoch
            {
                return false;
            }
            if self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned")
                .is_some()
            {
                return false;
            }
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            if workers
                .by_volume
                .values()
                .any(|worker| worker.join.as_ref().is_some_and(|join| !join.is_finished()))
            {
                return false;
            }
            if self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned")
                .running
            {
                return false;
            }
            let workers = workers
                .by_volume
                .drain()
                .map(|(_, worker)| worker)
                .collect::<Vec<_>>();
            (attempt_epoch, workers)
        };
        for worker in workers {
            stop_and_join_worker(worker);
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.db_work != 0
            || state.execution_work != 0
            || state.mode
                != (LifecycleMode::Pausing {
                    attempt_epoch,
                    resume_requested: true,
                })
            || self.lifecycle.file_index_phase() != FileIndexPhase::Running
            || self.lifecycle.file_index_attempt_epoch() != attempt_epoch
        {
            return false;
        }
        state.retained_store = None;
        if !state.fatal_unavailable {
            state.availability = Availability::Normal;
        }
        state.mode = if state.fatal_unavailable {
            LifecycleMode::Active
        } else {
            LifecycleMode::Uninitialized
        };
        state.admission_open = false;
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        coordinator.pending_root = None;
        coordinator.pending_runtime_epoch = None;
        coordinator.active_root = None;
        coordinator.volumes.clear();
        true
    }

    fn clean_close_readiness(
        &self,
        attempt_epoch: u64,
        now: std::time::Instant,
    ) -> CleanCloseReadiness {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.mode
            != (LifecycleMode::Pausing {
                attempt_epoch,
                resume_requested: false,
            })
            || self.lifecycle.file_index_phase() != FileIndexPhase::Cleaning
            || self.lifecycle.file_index_attempt_epoch() != attempt_epoch
            || state.db_work != 0
            || state.execution_work != 0
            || state.store.is_some()
            || state.clean_close_permit_issued
            || state.pause_deadline.is_none_or(|deadline| now >= deadline)
        {
            return CleanCloseReadiness::Reject;
        }
        if !self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .is_empty()
            || self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned")
                .is_some()
            || self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned")
                .running
        {
            return CleanCloseReadiness::Wait;
        }
        if let Some(store) = state.retained_store.take() {
            state.clean_close_permit_issued = true;
            CleanCloseReadiness::Permit(
                store,
                CleanCloseMarkerPermit {
                    attempt_epoch,
                    state: Arc::downgrade(&self.state),
                    lifecycle: Arc::clone(&self.lifecycle),
                },
            )
        } else if state.session_started {
            CleanCloseReadiness::Reject
        } else {
            CleanCloseReadiness::Vacuous
        }
    }

    #[cfg(test)]
    fn take_clean_close_marker(
        &self,
        attempt_epoch: u64,
    ) -> Option<(Store, CleanCloseMarkerPermit)> {
        match self.clean_close_readiness(attempt_epoch, std::time::Instant::now()) {
            CleanCloseReadiness::Permit(store, permit) => Some((store, permit)),
            CleanCloseReadiness::Vacuous
            | CleanCloseReadiness::Wait
            | CleanCloseReadiness::Reject => None,
        }
    }

    pub(crate) fn mark_clean_close(&self, attempt_epoch: u64) -> bool {
        self.mark_clean_close_with(attempt_epoch, std::time::Instant::now, || {
            std::thread::sleep(std::time::Duration::from_millis(10))
        })
    }

    fn mark_clean_close_with<N, W>(&self, attempt_epoch: u64, mut now: N, mut wait: W) -> bool
    where
        N: FnMut() -> std::time::Instant,
        W: FnMut(),
    {
        let deadline = {
            let state = self.state.lock().expect("file index lock poisoned");
            if !matches!(
                state.mode,
                LifecycleMode::Pausing {
                    attempt_epoch: current,
                    ..
                } if current == attempt_epoch
            ) || self.lifecycle.file_index_phase() != FileIndexPhase::Cleaning
                || self.lifecycle.file_index_attempt_epoch() != attempt_epoch
            {
                return false;
            }
            state.pause_deadline.unwrap_or_else(&mut now)
        };
        loop {
            self.reap_finished_workers();
            let current = now();
            match self.clean_close_readiness(attempt_epoch, current) {
                CleanCloseReadiness::Permit(store, permit) => {
                    return store.write_clean_close(permit).is_ok();
                }
                CleanCloseReadiness::Vacuous => return true,
                CleanCloseReadiness::Reject => return false,
                CleanCloseReadiness::Wait if current < deadline => wait(),
                CleanCloseReadiness::Wait => return false,
            }
        }
    }

    fn reap_finished_workers(&self) {
        let workers = {
            let mut registry = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            let finished = registry
                .by_volume
                .iter()
                .filter(|(_, worker)| {
                    worker
                        .join
                        .as_ref()
                        .is_none_or(std::thread::JoinHandle::is_finished)
                })
                .map(|(identity, _)| identity.clone())
                .collect::<Vec<_>>();
            finished
                .into_iter()
                .filter_map(|identity| registry.by_volume.remove(&identity))
                .collect::<Vec<_>>()
        };
        for worker in workers {
            stop_and_join_worker(worker);
        }
    }

    pub(crate) fn enter_terminal(&self) {
        let mut should_invalidate = false;
        {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if self.lifecycle.file_index_phase() != FileIndexPhase::Terminal {
                return;
            }
            if state.mode != LifecycleMode::Terminal {
                if !matches!(state.mode, LifecycleMode::Pausing { .. }) {
                    if let Some(runtime_epoch) = state.runtime_epoch.checked_add(1) {
                        state.runtime_epoch = runtime_epoch;
                        self.publication_runtime_epoch
                            .store(runtime_epoch, Ordering::Release);
                    } else {
                        state.latch_unavailable(&self.publication_generation);
                    }
                }
                state.mode = LifecycleMode::Terminal;
                state.admission_open = false;
                state.recovery_owner = None;
                state.recovery_deadline = None;
                state.store = None;
                state.retained_store = None;
                should_invalidate = true;
            }
        }
        if should_invalidate {
            self.coordinator.stop.store(true, Ordering::Release);
            if let Some(worker) = self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned")
                .as_ref()
            {
                worker.stop.store(true, Ordering::Release);
            }
            {
                let mut workers = self
                    .workers
                    .lock()
                    .expect("file index worker lock poisoned");
                for worker in workers.by_volume.values_mut() {
                    worker.stop.store(true, Ordering::Release);
                }
            }
            {
                let mut coordinator = self
                    .coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned");
                coordinator.pending_root = None;
                coordinator.pending_runtime_epoch = None;
                coordinator.active_root = None;
                coordinator.volumes.clear();
                coordinator.calibrated = true;
            }
            let _ = self.registry.invalidate_domain(QueryDomain::File);
            self.coordinator.signal.notify_all();
        }
        self.main_window_hwnd.store(0, Ordering::Release);
    }

    #[cfg(test)]
    fn start_cleaning_for_test(self: &Arc<Self>, attempt_epoch: u64) -> bool {
        self.start_cleaning_until(
            attempt_epoch,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        )
    }

    #[cfg(test)]
    fn return_running_for_test(self: &Arc<Self>, attempt_epoch: u64) -> bool {
        self.return_running(attempt_epoch)
    }

    #[cfg(test)]
    fn complete_pause_for_test(&self) -> bool {
        self.complete_pause_if_ready()
    }

    #[cfg(test)]
    fn terminal_for_test(&self, _attempt_epoch: u64) {
        self.enter_terminal();
    }

    #[cfg(test)]
    fn mode_for_test(&self) -> LifecycleMode {
        self.state.lock().expect("file index lock poisoned").mode
    }

    pub(crate) fn authorizes_publication(
        &self,
        expected_runtime_epoch: u64,
        expected_generation: u64,
    ) -> bool {
        let generation = self.publication_generation.load(Ordering::Acquire);
        self.runtime_epoch() == expected_runtime_epoch
            && generation != u64::MAX
            && generation == expected_generation
    }

    #[cfg(test)]
    fn prepare_worker(
        self: &Arc<Self>,
        volume: &FixedVolume,
    ) -> Result<WorkerPreparation, FileIndexError> {
        match self.reserve_and_prepare_worker(volume)? {
            WorkerStartDecision::Existing => Ok(WorkerPreparation::Existing),
            WorkerStartDecision::Start(start) => {
                let owner = start.owner;
                drop(start.reservation);
                Ok(WorkerPreparation::Start { owner })
            }
        }
    }

    fn reserve_and_prepare_worker(
        self: &Arc<Self>,
        volume: &FixedVolume,
    ) -> Result<WorkerStartDecision, FileIndexError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        let runtime_epoch = state.runtime_epoch;
        self.admit_locked(&state, AdmissionKind::DbWork, runtime_epoch)
            .map_err(|_| FileIndexError::Unavailable)?;
        let mut workers = self
            .workers
            .lock()
            .expect("file index worker lock poisoned");
        if workers
            .by_volume
            .get(&volume.identity)
            .is_some_and(|worker| {
                worker.runtime_epoch == runtime_epoch
                    && !worker.failed
                    && !worker.stop.load(Ordering::Acquire)
                    && worker.mount_point == volume.mount_point
                    && worker.join.as_ref().is_none_or(|join| !join.is_finished())
            })
        {
            return Ok(WorkerStartDecision::Existing);
        }
        let Some(owner) = workers.next_owner.checked_add(1) else {
            drop(workers);
            let newly_fatal = self.latch_exhaustion_locked(&mut state);
            drop(state);
            if newly_fatal {
                self.consume_fatal_effects();
            }
            return Err(FileIndexError::Unavailable);
        };
        let Some(next_db_work) = state.db_work.checked_add(1) else {
            drop(workers);
            let newly_fatal = self.latch_exhaustion_locked(&mut state);
            drop(state);
            if newly_fatal {
                self.consume_fatal_effects();
            }
            return Err(FileIndexError::Unavailable);
        };
        let stop = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let replaced = workers.by_volume.insert(
            volume.identity.clone(),
            WorkerRecord {
                owner,
                runtime_epoch,
                mount_point: volume.mount_point.clone(),
                stop: Arc::clone(&stop),
                generation: Arc::clone(&generation),
                join: None,
                failed: false,
            },
        );
        if let Some(replaced) = replaced.as_ref() {
            replaced.stop.store(true, Ordering::Release);
        }
        workers.next_owner = owner;
        state.db_work = next_db_work;
        let reservation = DbWorkReservation {
            state: Arc::clone(&self.state),
            coordinator: Arc::clone(&self.coordinator),
            runtime_epoch,
            released: false,
        };
        drop(workers);
        drop(state);

        if let Some(replaced) = replaced {
            let stale_volume = FixedVolume {
                identity: volume.identity.clone(),
                mount_point: replaced.mount_point.clone(),
            };
            stop_and_join_worker(replaced);
            if self
                .mark_fixed_volume_dirty(&stale_volume, &reservation)
                .is_err()
            {
                let _ = self.remove_worker_if_owner(&volume.identity, owner);
                drop(reservation);
                return Err(FileIndexError::Unavailable);
            }
        }
        Ok(WorkerStartDecision::Start(WorkerStart {
            owner,
            runtime_epoch,
            stop,
            generation,
            reservation,
        }))
    }

    fn attach_worker_join(
        &self,
        volume: &FixedVolume,
        owner: u64,
        runtime_epoch: u64,
        join: std::thread::JoinHandle<()>,
    ) -> Result<(), std::thread::JoinHandle<()>> {
        let mut workers = self
            .workers
            .lock()
            .expect("file index worker lock poisoned");
        let Some(worker) = workers.by_volume.get_mut(&volume.identity) else {
            return Err(join);
        };
        if worker.owner != owner
            || worker.runtime_epoch != runtime_epoch
            || worker.mount_point != volume.mount_point
            || worker.join.is_some()
        {
            return Err(join);
        }
        worker.join = Some(join);
        Ok(())
    }

    #[cfg(test)]
    fn install_worker(&self, volume: &FixedVolume, worker: WorkerRecord) {
        self.workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .insert(volume.identity.clone(), worker);
    }

    fn stop_detached_workers(
        self: &Arc<Self>,
        volumes: &[FixedVolume],
        reservation: &DbWorkReservation,
    ) -> Result<(), FileIndexError> {
        let removed = {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            let detached = workers
                .by_volume
                .keys()
                .filter(|identity| !volumes.iter().any(|volume| &volume.identity == *identity))
                .cloned()
                .collect::<Vec<_>>();
            detached
                .into_iter()
                .filter_map(|identity| {
                    workers
                        .by_volume
                        .remove(&identity)
                        .map(|worker| (identity, worker))
                })
                .collect::<Vec<_>>()
        };
        for (identity, worker) in removed {
            let volume = FixedVolume {
                identity,
                mount_point: worker.mount_point.clone(),
            };
            stop_and_join_worker(worker);
            self.mark_fixed_volume_dirty(&volume, reservation)?;
        }
        Ok(())
    }

    fn worker_is_current(&self, volume: &FixedVolume, owner: u64, generation: Option<u64>) -> bool {
        self.workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(&volume.identity)
            .is_some_and(|worker| {
                worker.owner == owner
                    && worker.runtime_epoch == self.runtime_epoch()
                    && worker.mount_point == volume.mount_point
                    && !worker.failed
                    && !worker.stop.load(Ordering::Acquire)
                    && generation.is_none_or(|generation| {
                        worker.generation.load(Ordering::Acquire) == generation
                    })
            })
    }

    fn worker_write_authorized_locked(
        &self,
        state: &IndexState,
        volume: &FixedVolume,
        owner: u64,
        generation: Option<u64>,
    ) -> bool {
        self.lifecycle.file_index_phase() == FileIndexPhase::Running
            && state.mode == LifecycleMode::Active
            && state.admission_open
            && !state.fatal_unavailable
            && state.runtime_epoch == self.runtime_epoch()
            && Self::authenticated_mount_matches(state, volume)
            && self.worker_is_current(volume, owner, generation)
    }

    fn worker_runtime_authorized(
        &self,
        volume: &FixedVolume,
        owner: u64,
        generation: Option<u64>,
        runtime_epoch: u64,
    ) -> bool {
        let state = self.state.lock().expect("file index lock poisoned");
        state.runtime_epoch == runtime_epoch
            && self.worker_write_authorized_locked(&state, volume, owner, generation)
    }

    fn worker_start_authorized(
        &self,
        volume: &FixedVolume,
        owner: u64,
        runtime_epoch: u64,
        reservation: &DbWorkReservation,
    ) -> bool {
        Arc::ptr_eq(&self.state, &reservation.state)
            && reservation.runtime_epoch == runtime_epoch
            && self.worker_runtime_authorized(volume, owner, None, runtime_epoch)
    }

    fn authenticated_mount_matches(state: &IndexState, volume: &FixedVolume) -> bool {
        volume.mount_point.to_str().is_some_and(|mount| {
            state
                .authenticated_mounts
                .iter()
                .any(|(identity, current)| identity == &volume.identity && current == mount)
        })
    }

    fn remove_worker_if_owner(&self, volume: &VolumeIdentity, owner: u64) -> Option<WorkerRecord> {
        let mut workers = self
            .workers
            .lock()
            .expect("file index worker lock poisoned");
        if workers
            .by_volume
            .get(volume)
            .is_some_and(|worker| worker.owner == owner)
        {
            workers.by_volume.remove(volume)
        } else {
            None
        }
    }

    fn mark_worker_stopped(&self, volume: &VolumeIdentity, owner: u64) -> bool {
        let mut workers = self
            .workers
            .lock()
            .expect("file index worker lock poisoned");
        let Some(worker) = workers.by_volume.get_mut(volume) else {
            return false;
        };
        if worker.owner != owner {
            return false;
        }
        worker.failed = true;
        worker.stop.store(true, Ordering::Release);
        true
    }

    fn clear_calibration_retries(&self) {
        self.coordinator.stop.store(true, Ordering::Release);
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        coordinator.pending_root = None;
        coordinator.pending_runtime_epoch = None;
        coordinator.active_root = None;
        coordinator.calibrated = true;
        coordinator.volumes.clear();
        self.coordinator.signal.notify_one();
    }

    fn latch_process_fatal(&self, state: &mut IndexState) -> bool {
        let newly_fatal = state.latch_unavailable(&self.publication_generation);
        {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            for worker in workers.by_volume.values_mut() {
                worker.stop.store(true, Ordering::Release);
            }
        }
        if let Some(worker) = self
            .integrity_worker
            .lock()
            .expect("file index integrity join lock poisoned")
            .as_ref()
        {
            worker.stop.store(true, Ordering::Release);
        }
        self.clear_calibration_retries();
        newly_fatal
    }

    fn latch_recovery_timeout(&self, state: &mut IndexState) -> bool {
        let newly_fatal = !state.fatal_unavailable;
        if newly_fatal
            && self
                .publication_generation
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                    generation.checked_add(1)
                })
                .is_err()
        {
            self.publication_generation
                .store(u64::MAX, Ordering::Release);
        }
        state.fatal_unavailable = true;
        state.availability = Availability::Unavailable;
        state.admission_open = false;
        state.inventory_previous_authenticated = None;
        state.pending_inventory_transitions.clear();
        state.quarantined_volumes.clear();
        {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            for worker in workers.by_volume.values_mut() {
                worker.stop.store(true, Ordering::Release);
            }
        }
        if let Some(worker) = self
            .integrity_worker
            .lock()
            .expect("file index integrity join lock poisoned")
            .as_ref()
        {
            worker.stop.store(true, Ordering::Release);
        }
        self.clear_calibration_retries();
        newly_fatal
    }

    fn finish_store_write<T>(
        &self,
        state: &mut IndexState,
        result: Result<T, StoreError>,
    ) -> Result<T, FileIndexError> {
        match result {
            Ok(value) => Ok(value),
            Err(StoreError::RevisionExhausted) => {
                self.latch_exhaustion_locked(state);
                Err(FileIndexError::Unavailable)
            }
            Err(StoreError::Corrupt) => Err(FileIndexError::RecoveryRequired),
            Err(_) => Err(FileIndexError::Unavailable),
        }
    }

    fn mark_fixed_volume_dirty(
        self: &Arc<Self>,
        volume: &FixedVolume,
        reservation: &DbWorkReservation,
    ) -> Result<(), FileIndexError> {
        if !Arc::ptr_eq(&self.state, &reservation.state) {
            return Err(FileIndexError::Unavailable);
        }
        let worker_mount = volume
            .mount_point
            .to_str()
            .ok_or(FileIndexError::Unavailable)?;
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != reservation.runtime_epoch
            || self
                .admit_locked(&state, AdmissionKind::DbWork, reservation.runtime_epoch)
                .is_err()
        {
            return Err(FileIndexError::Unavailable);
        }
        let mount = state
            .authenticated_mounts
            .iter()
            .find(|(identity, _)| identity == &volume.identity)
            .map(|(_, mount)| mount.clone())
            .unwrap_or_else(|| worker_mount.to_owned());
        let identities = state.authenticated_volumes.clone();
        let Some(store) = state.store.as_mut() else {
            return Err(FileIndexError::Unavailable);
        };
        let result = store.mark_volume_dirty(&volume.identity, &mount, &identities);
        let revision = match result {
            Ok(revision) => revision,
            Err(StoreError::Corrupt) => {
                drop(state);
                let _ = self.request_recovery(reservation);
                return Err(FileIndexError::RecoveryRequired);
            }
            Err(StoreError::RevisionExhausted) => {
                let newly_fatal = self.latch_exhaustion_locked(&mut state);
                drop(state);
                if newly_fatal {
                    self.consume_fatal_effects();
                }
                return Err(FileIndexError::Unavailable);
            }
            Err(StoreError::Sqlite | StoreError::InvalidData | StoreError::Platform) => {
                return Err(FileIndexError::Unavailable);
            }
        };
        state.index_revision_high_water = revision;
        Ok(())
    }

    fn finish_worker_start(
        self: &Arc<Self>,
        volume: &FixedVolume,
        owner: u64,
        completed_receiver: std::sync::mpsc::Receiver<bool>,
    ) -> Result<(), FileIndexError> {
        match completed_receiver.recv() {
            Ok(true) => Ok(()),
            Ok(false) => {
                if let Some(worker) = self.remove_worker_if_owner(&volume.identity, owner) {
                    stop_and_join_worker(worker);
                }
                Err(FileIndexError::Unavailable)
            }
            Err(_) => {
                if let Some(worker) = self.remove_worker_if_owner(&volume.identity, owner) {
                    stop_and_join_worker(worker);
                    let reservation = self
                        .reserve_db_work(self.runtime_epoch())
                        .map_err(|_| FileIndexError::Unavailable)?;
                    self.mark_fixed_volume_dirty(volume, &reservation)?;
                }
                Err(FileIndexError::Unavailable)
            }
        }
    }

    fn begin_worker_candidate(
        &self,
        volume: &FixedVolume,
        owner: u64,
        runtime_epoch: u64,
    ) -> Result<(u64, bool), FileIndexError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != runtime_epoch
            || !self.worker_write_authorized_locked(&state, volume, owner, None)
        {
            return Err(FileIndexError::Unavailable);
        }
        let before_authenticated = state.authenticated_volumes.clone();
        let mut provisional_authenticated = before_authenticated.clone();
        if !provisional_authenticated.contains(&volume.identity) {
            provisional_authenticated.push(volume.identity.clone());
        }
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let mount = volume
            .mount_point
            .to_str()
            .ok_or(FileIndexError::Unavailable)?;
        let result = store.begin_candidate(
            &volume.identity,
            mount,
            &before_authenticated,
            &provisional_authenticated,
        );
        let (generation, revision, has_committed) = self.finish_store_write(&mut state, result)?;
        if !has_committed {
            state.quarantined_volumes.remove(&volume.identity);
            state.authenticated_volumes = provisional_authenticated;
        }
        state.index_revision_high_water = revision;
        Ok((generation, has_committed))
    }

    fn commit_worker_candidate<F>(
        &self,
        volume: &FixedVolume,
        owner: u64,
        generation: u64,
        final_entries: Vec<IndexEntry>,
        denied_prefixes: &[String],
        materialize_replay: F,
    ) -> Result<u64, FileIndexError>
    where
        F: FnOnce(
            &mut dyn FnMut(IndexChangeBatch) -> Result<(), StoreError>,
        ) -> Result<(), StoreError>,
    {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_write_authorized_locked(&state, volume, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        let before_authenticated = state.authenticated_volumes.clone();
        let mut after_authenticated = before_authenticated.clone();
        if !after_authenticated.contains(&volume.identity) {
            after_authenticated.push(volume.identity.clone());
        }
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let result = store.commit_candidate_streaming(
            &volume.identity,
            generation,
            final_entries,
            denied_prefixes,
            (&before_authenticated, &after_authenticated),
            materialize_replay,
        );
        let revision = self.finish_store_write(&mut state, result)?;
        state.index_revision_high_water = revision;
        state.authenticated_volumes = after_authenticated;
        state.quarantined_volumes.remove(&volume.identity);
        Ok(revision)
    }

    pub(crate) fn search(
        self: &Arc<Self>,
        app_data_dir: &Path,
        spec: QuerySpec,
        expected_runtime_epoch: u64,
    ) -> Result<FileSearchBatch, FileIndexError> {
        let admission = match self.begin_search(expected_runtime_epoch)? {
            SearchAdmission::Immediate(batch) => return Ok(batch),
            admission @ SearchAdmission::Work { .. } => admission,
        };
        #[cfg(not(test))]
        self.refresh_query_volumes_with(|| {
            fixed_volumes().map_err(|_| FileIndexError::Unavailable)
        })?;
        let batch = self.search_admitted_with(
            app_data_dir,
            spec,
            admission,
            authenticate_app_data_root,
            open_store,
            |store, spec| store.query(spec, &[]),
        )?;
        #[cfg(not(test))]
        self.schedule_integrity();
        self.schedule_calibration();
        Ok(batch)
    }

    fn refresh_query_volumes_with<F>(&self, inventory: F) -> Result<(), FileIndexError>
    where
        F: FnOnce() -> Result<Vec<FixedVolume>, FileIndexError>,
    {
        let volumes = match inventory() {
            Ok(volumes) => volumes,
            Err(error) => {
                let mut state = self.state.lock().expect("file index lock poisoned");
                self.latch_process_fatal(&mut state);
                return Err(error);
            }
        };
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.fatal_unavailable {
            return Err(FileIndexError::Unavailable);
        }
        let changed = self.record_inventory_locked(&mut state, &volumes)?;
        drop(state);
        if changed {
            let mut coordinator = self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned");
            coordinator.calibrated = false;
        }
        Ok(())
    }

    fn record_inventory_locked(
        &self,
        state: &mut IndexState,
        volumes: &[FixedVolume],
    ) -> Result<bool, FileIndexError> {
        let mounts = match volumes
            .iter()
            .map(|volume| {
                volume
                    .mount_point
                    .to_str()
                    .map(|mount| (volume.identity.clone(), mount.to_owned()))
                    .ok_or(FileIndexError::Unavailable)
            })
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(mounts) => mounts,
            Err(error) => {
                self.latch_process_fatal(state);
                return Err(error);
            }
        };
        let Some(observation) = state.inventory_observation.checked_add(1) else {
            self.latch_process_fatal(state);
            return Err(FileIndexError::Unavailable);
        };
        let previous = state
            .authenticated_mounts
            .iter()
            .cloned()
            .collect::<HashMap<_, _>>();
        let current = mounts.iter().cloned().collect::<HashMap<_, _>>();
        let transitions = previous
            .keys()
            .chain(current.keys())
            .filter(|identity| previous.get(*identity) != current.get(*identity))
            .cloned()
            .collect::<HashSet<_>>();
        if !transitions.is_empty() && state.inventory_previous_authenticated.is_none() {
            state.inventory_previous_authenticated = Some(state.authenticated_volumes.clone());
        }
        state
            .pending_inventory_transitions
            .extend(transitions.iter().cloned());
        state
            .quarantined_volumes
            .extend(transitions.iter().cloned());
        if !transitions.is_empty() {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            for identity in &transitions {
                if let Some(worker) = workers.by_volume.get_mut(identity) {
                    worker.stop.store(true, Ordering::Release);
                }
            }
        }
        state
            .authenticated_volumes
            .retain(|identity| !transitions.contains(identity));
        let quarantined = &state.quarantined_volumes;
        state
            .authenticated_volumes
            .retain(|identity| !quarantined.contains(identity));
        state.authenticated_mounts = mounts;
        state.inventory_observation = observation;
        Ok(!state.pending_inventory_transitions.is_empty())
    }

    fn reconcile_inventory_locked(&self, state: &mut IndexState) -> Result<bool, FileIndexError> {
        let current_mounts = state.authenticated_mounts.clone();
        let previous_authenticated = state
            .inventory_previous_authenticated
            .clone()
            .unwrap_or_else(|| state.authenticated_volumes.clone());
        let transitions = state
            .pending_inventory_transitions
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let quarantined = state.quarantined_volumes.clone();
        let result = state
            .store
            .as_mut()
            .ok_or(FileIndexError::Unavailable)
            .map(|store| {
                store.reconcile_current_mounts(
                    &current_mounts,
                    &transitions,
                    &previous_authenticated,
                    &quarantined,
                )
            })?;
        let (identities, revision, changed) = self.finish_store_write(state, result)?;
        state.inventory_previous_authenticated = None;
        state.pending_inventory_transitions.clear();
        state.authenticated_volumes = identities
            .into_iter()
            .filter(|identity| !quarantined.contains(identity))
            .collect();
        state.index_revision_high_water = revision;
        Ok(changed)
    }

    fn reconcile_calibration_inventory(
        &self,
        expected_observation: u64,
        volumes: &[FixedVolume],
    ) -> Result<Option<bool>, FileIndexError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.inventory_observation != expected_observation {
            return Ok(None);
        }
        self.record_inventory_locked(&mut state, volumes)?;
        self.reconcile_inventory_locked(&mut state).map(Some)
    }

    #[cfg(test)]
    fn search_with<A, O, Q>(
        self: &Arc<Self>,
        app_data_dir: &Path,
        spec: QuerySpec,
        expected_runtime_epoch: u64,
        authenticate_root: A,
        open: O,
        query_store: Q,
    ) -> Result<FileSearchBatch, FileIndexError>
    where
        A: FnMut(&Path) -> Result<PathBuf, FileIndexError>,
        O: FnMut(&Path) -> Result<(Store, u64, Option<u64>), FileIndexError>,
        Q: FnMut(&mut Store, &QuerySpec) -> Result<StoreQueryResult, StoreError>,
    {
        let admission = match self.begin_search(expected_runtime_epoch)? {
            SearchAdmission::Immediate(batch) => return Ok(batch),
            admission @ SearchAdmission::Work { .. } => admission,
        };
        self.search_admitted_with(
            app_data_dir,
            spec,
            admission,
            authenticate_root,
            open,
            query_store,
        )
    }

    fn search_admitted_with<A, O, Q>(
        self: &Arc<Self>,
        app_data_dir: &Path,
        spec: QuerySpec,
        admission: SearchAdmission,
        mut authenticate_root: A,
        mut open: O,
        mut query_store: Q,
    ) -> Result<FileSearchBatch, FileIndexError>
    where
        A: FnMut(&Path) -> Result<PathBuf, FileIndexError>,
        O: FnMut(&Path) -> Result<(Store, u64, Option<u64>), FileIndexError>,
        Q: FnMut(&mut Store, &QuerySpec) -> Result<StoreQueryResult, StoreError>,
    {
        let SearchAdmission::Work { owner, reservation } = admission else {
            unreachable!("immediate search admission was already returned")
        };
        let expected_runtime_epoch = reservation.runtime_epoch;

        if let Some(owner) = owner {
            let authenticated = authenticate_root(app_data_dir).and_then(|path| {
                let root = path
                    .parent()
                    .ok_or(FileIndexError::Unavailable)?
                    .to_path_buf();
                {
                    let mut state = self.state.lock().expect("file index lock poisoned");
                    if state.mode != (LifecycleMode::Opening { owner })
                        || state.runtime_epoch != expected_runtime_epoch
                        || self.lifecycle.file_index_phase() != FileIndexPhase::Running
                    {
                        return Err(FileIndexError::Unavailable);
                    }
                    state.authenticated_app_data_root = Some(root.clone());
                }
                Ok((path, root))
            });
            let opened =
                authenticated.and_then(|(path, root)| open(&path).map(|opened| (opened, root)));
            let mut state = self.state.lock().expect("file index lock poisoned");
            if state.mode != (LifecycleMode::Opening { owner })
                || state.runtime_epoch != expected_runtime_epoch
            {
                if let Ok(((store, _, _), _)) = opened {
                    state.session_started = true;
                    if matches!(state.mode, LifecycleMode::Pausing { .. })
                        && state.retained_store.is_none()
                    {
                        state.prior_integrity = Some(store.prior_integrity_metadata());
                        state.retained_store = Some(store);
                    }
                }
                drop(state);
                drop(reservation);
                return Err(FileIndexError::Unavailable);
            }
            match opened {
                Ok(((store, revision, previous_revision), authenticated_root)) => {
                    state.session_started = true;
                    state.prior_integrity = Some(store.prior_integrity_metadata());
                    state.store = Some(store);
                    state.authenticated_app_data_root = Some(authenticated_root);
                    if let Some(previous) = previous_revision {
                        state.index_revision_high_water = previous;
                        let advanced = state.advance_revision_locked(&self.publication_generation);
                        if advanced.is_err() || advanced.ok() != Some(revision) {
                            self.latch_process_fatal(&mut state);
                            drop(state);
                            drop(reservation);
                            return Err(FileIndexError::Unavailable);
                        }
                    } else {
                        state.index_revision_high_water = revision;
                    }
                    state.mode = LifecycleMode::Active;
                    state.availability = Availability::Normal;
                    state.admission_open = true;
                }
                Err(FileIndexError::RecoveryRequired) => {
                    state.mode = LifecycleMode::Active;
                    state.admission_open = true;
                    drop(state);
                    let _ = self.request_recovery(&reservation);
                    drop(reservation);
                    return Err(FileIndexError::Unavailable);
                }
                Err(FileIndexError::Unavailable) => {
                    self.latch_process_fatal(&mut state);
                    drop(state);
                    drop(reservation);
                    return Err(FileIndexError::Unavailable);
                }
            }
        }

        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != expected_runtime_epoch || !state.admission_open {
            return Err(FileIndexError::Unavailable);
        }
        let mount_changed = match self.reconcile_inventory_locked(&mut state) {
            Ok(changed) => changed,
            Err(FileIndexError::RecoveryRequired) => {
                drop(state);
                let _ = self.request_recovery(&reservation);
                drop(reservation);
                return Err(FileIndexError::Unavailable);
            }
            Err(error) => {
                let consume_fatal = state.hide_requested;
                drop(state);
                drop(reservation);
                if consume_fatal {
                    self.consume_fatal_effects();
                }
                return Err(error);
            }
        };
        let identities = state.authenticated_volumes.clone();
        let result = match state.store.as_mut() {
            Some(store) if identities.is_empty() => query_store(store, &spec),
            Some(store) => store.query(&spec, &identities),
            None => Err(StoreError::InvalidData),
        };
        let result = match result {
            Ok(result) => result,
            Err(StoreError::Corrupt) => {
                drop(state);
                let _ = self.request_recovery(&reservation);
                drop(reservation);
                return Err(FileIndexError::Unavailable);
            }
            Err(StoreError::RevisionExhausted) => {
                let newly_fatal = self.latch_exhaustion_locked(&mut state);
                drop(state);
                drop(reservation);
                if newly_fatal {
                    self.consume_fatal_effects();
                }
                return Err(FileIndexError::Unavailable);
            }
            Err(StoreError::Sqlite | StoreError::InvalidData | StoreError::Platform) => {
                self.latch_process_fatal(&mut state);
                drop(state);
                drop(reservation);
                return Err(FileIndexError::Unavailable);
            }
        };
        if !state.integrity_started && !state.integrity_pending {
            let due = state
                .store
                .as_ref()
                .ok_or(FileIndexError::Unavailable)?
                .integrity_check_due_now();
            let due = match due {
                Ok(due) => due,
                Err(StoreError::Corrupt) => {
                    drop(state);
                    let _ = self.request_recovery(&reservation);
                    drop(reservation);
                    return Err(FileIndexError::Unavailable);
                }
                Err(StoreError::RevisionExhausted) => {
                    let newly_fatal = self.latch_exhaustion_locked(&mut state);
                    drop(state);
                    drop(reservation);
                    if newly_fatal {
                        self.consume_fatal_effects();
                    }
                    return Err(FileIndexError::Unavailable);
                }
                Err(StoreError::Sqlite | StoreError::InvalidData | StoreError::Platform) => false,
            };
            if due {
                state.integrity_pending = true;
            }
        }
        state.index_revision_high_water = result.index_revision;
        let batch = FileSearchBatch {
            runtime_epoch: expected_runtime_epoch,
            publication_generation: self.publication_generation.load(Ordering::Acquire),
            index_revision: result.index_revision,
            total: result.total,
            status: result.status,
            items: result
                .entries
                .into_iter()
                .map(|entry| FileResultDraft {
                    action: OpenIndexedPath {
                        runtime_epoch: expected_runtime_epoch,
                        row_id: entry.row_id,
                        volume_identity: entry.volume_identity,
                        relative_path: entry.relative_path,
                        kind: entry.kind,
                    },
                    name: entry.name,
                    kind: match entry.kind {
                        IndexedKind::File => FileResultKind::File,
                        IndexedKind::Directory => FileResultKind::Folder,
                    },
                    size_bytes: entry.size_bytes,
                    modified_utc: entry.modified_utc,
                    full_path: entry.display_path,
                })
                .collect(),
        };
        drop(state);
        drop(reservation);
        if mount_changed {
            let mut coordinator = self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned");
            coordinator.calibrated = false;
        }
        Ok(batch)
    }

    fn schedule_integrity(self: &Arc<Self>) -> bool {
        let state = self.state.lock().expect("file index lock poisoned");
        if !state.integrity_pending
            || self
                .admit_locked(&state, AdmissionKind::DbWork, state.runtime_epoch)
                .is_err()
            || !self.ensure_running_coordinator_thread_locked(&state)
        {
            return false;
        }
        drop(state);
        self.coordinator.signal.notify_all();
        true
    }

    fn drive_integrity(self: &Arc<Self>) -> bool {
        self.reap_finished_integrity();
        let stop = Arc::new(AtomicBool::new(false));
        let (root, database, reservation, runtime_epoch) = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if !state.integrity_pending
                || state.integrity_started
                || self
                    .admit_locked(&state, AdmissionKind::DbWork, state.runtime_epoch)
                    .is_err()
            {
                return false;
            }
            let mut slot = self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned");
            if slot.is_some() {
                return false;
            }
            let Some(root) = state.authenticated_app_data_root.clone() else {
                return false;
            };
            let runtime_epoch = state.runtime_epoch;
            let reservation = match self.reserve_db_work_locked(&mut state, runtime_epoch) {
                Ok(reservation) => reservation,
                Err(_) => return false,
            };
            state.integrity_pending = false;
            state.integrity_started = true;
            *slot = Some(IntegrityWorkerRecord {
                runtime_epoch,
                stop: Arc::clone(&stop),
                join: None,
            });
            (
                root.clone(),
                root.join("file-index.sqlite3"),
                reservation,
                runtime_epoch,
            )
        };
        let owner = Arc::clone(self);
        let worker_stop = Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            if worker_stop.load(Ordering::Acquire)
                || !owner.integrity_worker_authorized(runtime_epoch, &worker_stop)
                || authenticate_app_data_root(&root).ok().as_deref() != Some(database.as_path())
            {
                return;
            }
            let result = Store::run_integrity_check_at(&database);
            match result {
                Ok(true) => {
                    let authorized = {
                        let state = owner.state.lock().expect("file index lock poisoned");
                        owner
                            .admit_locked(&state, AdmissionKind::DbWork, runtime_epoch)
                            .is_ok()
                            && !worker_stop.load(Ordering::Acquire)
                            && owner.integrity_worker_matches(runtime_epoch, &worker_stop)
                    };
                    if authorized {
                        match owner.record_integrity_timestamp(
                            &root,
                            &database,
                            runtime_epoch,
                            &worker_stop,
                        ) {
                            Err(StoreError::Corrupt) => {
                                let _ = owner.request_recovery(&reservation);
                            }
                            Err(StoreError::RevisionExhausted) => owner.fail_closed_exhaustion(),
                            Ok(())
                            | Err(
                                StoreError::Sqlite | StoreError::InvalidData | StoreError::Platform,
                            ) => {}
                        }
                    }
                }
                Ok(false) | Err(StoreError::Corrupt) => {
                    let _ = owner.request_recovery(&reservation);
                }
                Err(StoreError::RevisionExhausted) => owner.fail_closed_exhaustion(),
                Err(StoreError::Sqlite | StoreError::InvalidData | StoreError::Platform) => {}
            }
            drop(reservation);
            owner.coordinator.signal.notify_all();
        });
        let mut slot = self
            .integrity_worker
            .lock()
            .expect("file index integrity join lock poisoned");
        let Some(worker) = slot.as_mut() else {
            drop(slot);
            stop.store(true, Ordering::Release);
            let _ = join.join();
            return false;
        };
        if worker.runtime_epoch != runtime_epoch
            || !Arc::ptr_eq(&worker.stop, &stop)
            || worker.join.is_some()
        {
            drop(slot);
            stop.store(true, Ordering::Release);
            let _ = join.join();
            return false;
        }
        worker.join = Some(join);
        true
    }

    fn integrity_worker_matches(&self, runtime_epoch: u64, stop: &Arc<AtomicBool>) -> bool {
        self.integrity_worker
            .lock()
            .expect("file index integrity join lock poisoned")
            .as_ref()
            .is_some_and(|worker| {
                worker.runtime_epoch == runtime_epoch && Arc::ptr_eq(&worker.stop, stop)
            })
    }

    fn integrity_worker_authorized(&self, runtime_epoch: u64, stop: &Arc<AtomicBool>) -> bool {
        let state = self.state.lock().expect("file index lock poisoned");
        self.admit_locked(&state, AdmissionKind::DbWork, runtime_epoch)
            .is_ok()
            && !stop.load(Ordering::Acquire)
            && self.integrity_worker_matches(runtime_epoch, stop)
    }

    fn record_integrity_timestamp(
        &self,
        root: &Path,
        database: &Path,
        runtime_epoch: u64,
        stop: &Arc<AtomicBool>,
    ) -> Result<(), StoreError> {
        Store::record_integrity_check_at_authorized(database, || {
            self.integrity_worker_authorized(runtime_epoch, stop)
                && authenticate_app_data_root(root).ok().as_deref() == Some(database)
        })
    }

    fn reap_finished_integrity(&self) {
        let worker = {
            let mut slot = self
                .integrity_worker
                .lock()
                .expect("file index integrity join lock poisoned");
            if slot
                .as_ref()
                .and_then(|worker| worker.join.as_ref())
                .is_some_and(std::thread::JoinHandle::is_finished)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(mut worker) = worker {
            if let Some(join) = worker.join.take() {
                if join.thread().id() != std::thread::current().id() {
                    let _ = join.join();
                }
            }
        }
    }

    fn schedule_calibration(self: &Arc<Self>) -> bool {
        let app_data_root = {
            let state = self.state.lock().expect("file index lock poisoned");
            let Some(root) = state.authenticated_app_data_root.clone() else {
                return false;
            };
            root
        };
        let finished_worker = self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .values()
            .any(|worker| worker.join.as_ref().is_some_and(|join| join.is_finished()));
        if finished_worker {
            self.coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned")
                .calibrated = false;
        }
        let (scheduled, start_thread) = self.mark_calibration_pending(app_data_root);
        if !scheduled {
            return false;
        }
        #[cfg(not(test))]
        if start_thread {
            let state = self.state.lock().expect("file index lock poisoned");
            let _ = self.ensure_running_coordinator_thread_locked(&state);
        }
        #[cfg(test)]
        let _ = start_thread;
        true
    }

    fn ensure_coordinator_thread(self: &Arc<Self>) {
        let finished = {
            let mut slot = self
                .coordinator
                .join
                .lock()
                .expect("file index coordinator join lock poisoned");
            if slot
                .as_ref()
                .is_some_and(std::thread::JoinHandle::is_finished)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(join) = finished {
            if join.thread().id() != std::thread::current().id() {
                let _ = join.join();
            }
        }
        let control_pending = self.control_pending();
        let mut slot = self
            .coordinator
            .join
            .lock()
            .expect("file index coordinator join lock poisoned");
        if slot.is_some() || (self.coordinator.stop.load(Ordering::Acquire) && !control_pending) {
            return;
        }
        self.coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned")
            .thread_started = true;
        let index = Arc::downgrade(self);
        let coordinator = Arc::clone(&self.coordinator);
        *slot = Some(std::thread::spawn({
            let coordinator = Arc::clone(&coordinator);
            move || Self::coordinator_loop(index, coordinator)
        }));
    }

    fn ensure_running_coordinator_thread_locked(self: &Arc<Self>, state: &IndexState) -> bool {
        if self
            .admit_locked(state, AdmissionKind::DbWork, state.runtime_epoch)
            .is_err()
            || self.coordinator.stop.load(Ordering::Acquire)
        {
            return false;
        }
        let mut slot = self
            .coordinator
            .join
            .lock()
            .expect("file index coordinator join lock poisoned");
        if slot.is_some() {
            return true;
        }
        let mut coordinator_state = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        coordinator_state.thread_started = true;
        let index = Arc::downgrade(self);
        let coordinator = Arc::clone(&self.coordinator);
        *slot = Some(std::thread::spawn({
            let coordinator = Arc::clone(&coordinator);
            move || Self::coordinator_loop(index, coordinator)
        }));
        true
    }

    fn control_pending(&self) -> bool {
        let state = self.state.lock().expect("file index lock poisoned");
        matches!(state.mode, LifecycleMode::Pausing { .. })
            || state.recovery_owner.is_some()
            || state.integrity_pending
            || state.hide_requested
    }

    fn mark_calibration_pending(&self, app_data_root: PathBuf) -> (bool, bool) {
        self.mark_calibration_pending_with(app_data_root, || {})
    }

    fn mark_calibration_pending_with<F>(
        &self,
        app_data_root: PathBuf,
        before_linearization: F,
    ) -> (bool, bool)
    where
        F: FnOnce(),
    {
        before_linearization();
        let mut state = self.state.lock().expect("file index lock poisoned");
        if self.lifecycle.file_index_phase() != FileIndexPhase::Running
            || state.mode != LifecycleMode::Active
            || state.fatal_unavailable
            || !state.admission_open
            || state.store.is_none()
            || self.runtime_epoch() != state.runtime_epoch
        {
            return (false, false);
        }
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        if self.coordinator.stop.load(Ordering::Acquire) {
            return (false, false);
        }
        coordinator.active_root = Some(app_data_root.clone());
        if coordinator.pending_root.is_some() || (coordinator.calibrated && !coordinator.running) {
            return (false, false);
        }
        let start_thread = !coordinator.thread_started;
        let Some(wakes) = coordinator.wakes.checked_add(1) else {
            drop(coordinator);
            let newly_fatal = self.latch_exhaustion_locked(&mut state);
            drop(state);
            if newly_fatal {
                self.consume_fatal_effects();
            }
            return (false, false);
        };
        coordinator.thread_started = true;
        coordinator.pending_root = Some(app_data_root);
        coordinator.pending_runtime_epoch = Some(state.runtime_epoch);
        coordinator.wakes = wakes;
        self.coordinator.signal.notify_one();
        (true, start_thread)
    }

    fn coordinator_loop(index: std::sync::Weak<Self>, coordinator: Arc<CoordinatorControl>) {
        'coordinator: loop {
            let Some(owner) = index.upgrade() else {
                coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned")
                    .thread_started = false;
                return;
            };
            owner.consume_fatal_effects();
            owner.reap_finished_workers();
            owner.reap_finished_integrity();
            if owner.complete_pause_if_ready() || owner.drive_recovery() || owner.drive_integrity()
            {
                drop(owner);
                continue;
            }
            if coordinator.stop.load(Ordering::Acquire) {
                if !owner.control_pending() {
                    coordinator
                        .state
                        .lock()
                        .expect("file index coordinator lock poisoned")
                        .thread_started = false;
                    return;
                }
                drop(owner);
                let state = coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned");
                let _ = coordinator
                    .signal
                    .wait_timeout(state, std::time::Duration::from_millis(50))
                    .expect("file index coordinator lock poisoned");
                continue;
            }
            drop(owner);
            {
                let mut state = coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned");
                while state.pending_root.is_none() {
                    if coordinator.stop.load(Ordering::Acquire) {
                        drop(state);
                        continue 'coordinator;
                    }
                    let now = std::time::Instant::now();
                    let pending = state
                        .volumes
                        .values()
                        .filter_map(|runtime| match runtime.calibration {
                            Calibration::Pending {
                                deadline,
                                runtime_epoch,
                            } => Some((deadline, runtime_epoch)),
                            Calibration::Idle | Calibration::Running { .. } => None,
                        })
                        .min_by_key(|(deadline, _)| *deadline);
                    if pending.is_some_and(|(deadline, _)| deadline <= now) {
                        state.pending_root = state.active_root.clone();
                        state.pending_runtime_epoch = state
                            .pending_root
                            .as_ref()
                            .and_then(|_| pending.map(|(_, runtime_epoch)| runtime_epoch));
                        if state.pending_root.is_some() {
                            break;
                        }
                    }
                    let timeout = pending
                        .map(|(deadline, _)| deadline.saturating_duration_since(now))
                        .unwrap_or(std::time::Duration::from_millis(250));
                    let waited = coordinator
                        .signal
                        .wait_timeout(state, timeout)
                        .expect("file index coordinator lock poisoned");
                    state = waited.0;
                    if coordinator.stop.load(Ordering::Acquire) || index.strong_count() == 0 {
                        drop(state);
                        continue 'coordinator;
                    }
                    if state.pending_root.is_none() {
                        drop(state);
                        continue 'coordinator;
                    }
                }
            }
            let Some(owner) = index.upgrade() else {
                coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned")
                    .thread_started = false;
                return;
            };
            let Some((_app_data_root, reservation)) = owner.claim_calibration_run_with(|| {})
            else {
                drop(owner);
                continue;
            };
            #[cfg(not(test))]
            let completed = owner.run_calibration(&_app_data_root, reservation);
            #[cfg(test)]
            let completed = {
                drop(reservation);
                false
            };
            drop(owner);
            Self::finish_calibration_run(&coordinator, completed);
        }
    }

    fn finish_calibration_run(coordinator: &CoordinatorControl, completed: bool) {
        let mut state = coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        state.running = false;
        let has_pending_volume = state
            .volumes
            .values()
            .any(|runtime| matches!(runtime.calibration, Calibration::Pending { .. }));
        state.calibrated = completed && state.pending_root.is_none() && !has_pending_volume;
        if state.pending_root.is_some() {
            coordinator.signal.notify_one();
        }
    }

    fn finish_volume_attempt(
        &self,
        identity: &VolumeIdentity,
        succeeded: bool,
        now: std::time::Instant,
        runtime_epoch: u64,
        failures_before: u32,
    ) -> bool {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.fatal_unavailable || !state.admission_open || state.store.is_none() {
            return false;
        }
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        let Some(runtime) = coordinator.volumes.get_mut(identity) else {
            return false;
        };
        if runtime
            .finish_start_attempt(succeeded, now, runtime_epoch, failures_before)
            .is_err()
        {
            drop(coordinator);
            self.latch_process_fatal(&mut state);
            return false;
        }
        if !succeeded {
            self.coordinator.signal.notify_one();
        }
        true
    }

    #[cfg(not(test))]
    fn run_calibration(
        self: &Arc<Self>,
        app_data_root: &Path,
        preflight_reservation: DbWorkReservation,
    ) -> bool {
        let expected_observation = self
            .state
            .lock()
            .expect("file index lock poisoned")
            .inventory_observation;
        let Ok((volumes, exclusions)) = self.calibration_inputs_with(
            || fixed_volumes().map_err(|_| FileIndexError::Unavailable),
            |volumes| {
                system_exclusions(app_data_root, volumes).map_err(|_| FileIndexError::Unavailable)
            },
        ) else {
            return false;
        };
        match self.reconcile_calibration_inventory(expected_observation, &volumes) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = self.mark_calibration_pending(app_data_root.to_path_buf());
                return false;
            }
            Err(FileIndexError::RecoveryRequired) => {
                let _ = self.request_recovery(&preflight_reservation);
                return false;
            }
            Err(FileIndexError::Unavailable) => return false,
        }
        if self
            .stop_detached_workers(&volumes, &preflight_reservation)
            .is_err()
        {
            return false;
        }
        let runtime_epoch = self.runtime_epoch();
        let now = std::time::Instant::now();
        {
            let mut coordinator = self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned");
            coordinator.active_root = Some(app_data_root.to_path_buf());
            for volume in &volumes {
                coordinator
                    .volumes
                    .entry(volume.identity.clone())
                    .or_default()
                    .request(now, runtime_epoch);
            }
            let current = volumes
                .iter()
                .map(|volume| volume.identity.clone())
                .collect::<HashSet<_>>();
            coordinator.volumes.retain(|identity, runtime| {
                if current.contains(identity) {
                    true
                } else if runtime.consecutive_failures == 0 {
                    false
                } else {
                    runtime.cancel_pending();
                    true
                }
            });
        }
        drop(preflight_reservation);
        let mut completed = true;
        for volume in volumes {
            let failures_before = {
                let mut coordinator = self
                    .coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned");
                coordinator
                    .volumes
                    .get_mut(&volume.identity)
                    .and_then(|runtime| {
                        runtime
                            .start_if_due(now, runtime_epoch)
                            .then_some(runtime.consecutive_failures)
                    })
            };
            let Some(failures_before) = failures_before else {
                continue;
            };
            let result = self.start_volume_worker(volume.clone(), exclusions.clone());
            if result.is_err() {
                completed = false;
            }
            if !self.finish_volume_attempt(
                &volume.identity,
                result.is_ok(),
                std::time::Instant::now(),
                runtime_epoch,
                failures_before,
            ) {
                return false;
            }
        }
        completed
    }

    fn calibration_inputs_with<V, E>(
        &self,
        inventory: V,
        exclusions: E,
    ) -> Result<(Vec<FixedVolume>, Vec<ExcludedPrefix>), FileIndexError>
    where
        V: FnOnce() -> Result<Vec<FixedVolume>, FileIndexError>,
        E: FnOnce(&[FixedVolume]) -> Result<Vec<ExcludedPrefix>, FileIndexError>,
    {
        let result = inventory()
            .and_then(|volumes| exclusions(&volumes).map(|excluded| (volumes, excluded)));
        if result.is_err() {
            let mut state = self.state.lock().expect("file index lock poisoned");
            self.latch_process_fatal(&mut state);
        }
        result
    }

    #[cfg(not(test))]
    fn start_volume_worker(
        self: &Arc<Self>,
        volume: FixedVolume,
        exclusions: Vec<ExcludedPrefix>,
    ) -> Result<(), FileIndexError> {
        let start = match self.reserve_and_prepare_worker(&volume)? {
            WorkerStartDecision::Existing => return Ok(()),
            WorkerStartDecision::Start(start) => start,
        };
        let WorkerStart {
            owner: owner_id,
            runtime_epoch,
            stop,
            generation: generation_owner,
            reservation,
        } = start;
        let owner = Arc::downgrade(self);
        let worker_stop = Arc::clone(&stop);
        let worker_generation = Arc::clone(&generation_owner);
        let (completed_sender, completed_receiver) = mpsc::sync_channel(1);
        let (start_sender, start_receiver) = mpsc::sync_channel(0);
        let worker_volume = volume.clone();
        let join = thread::spawn(move || {
            let reservation = reservation;
            if start_receiver.recv().is_err() {
                return;
            }
            let Some(index) = owner.upgrade() else {
                let _ = completed_sender.send(false);
                return;
            };
            if worker_stop.load(Ordering::Acquire)
                || !index.worker_start_authorized(
                    &worker_volume,
                    owner_id,
                    runtime_epoch,
                    &reservation,
                )
            {
                let _ = completed_sender.send(false);
                return;
            }
            drop(index);
            let result = (|| {
                let index = owner.upgrade().ok_or(FileIndexError::Unavailable)?;
                let mut watcher =
                    Watcher::arm(&worker_volume).map_err(|_| FileIndexError::Unavailable)?;
                let generation = index.calibrate_volume(
                    &worker_volume,
                    &exclusions,
                    &mut watcher,
                    owner_id,
                    &worker_stop,
                    runtime_epoch,
                )?;
                worker_generation.store(generation, Ordering::Release);
                index.complete_volume_calibration(&worker_volume, owner_id, runtime_epoch)?;
                drop(index);
                let _ = completed_sender.send(true);
                loop {
                    if worker_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let events = match watcher.wait_batch(250) {
                        Ok(Some(events)) => events,
                        Ok(None) => {
                            if owner.strong_count() == 0 {
                                return Ok(());
                            }
                            continue;
                        }
                        Err(_) => return Err(FileIndexError::Unavailable),
                    };
                    let index = owner.upgrade().ok_or(FileIndexError::Unavailable)?;
                    index.apply_watcher_events(
                        &worker_volume,
                        &exclusions,
                        generation,
                        owner_id,
                        runtime_epoch,
                        &events,
                    )?;
                }
            })();
            if result == Err(FileIndexError::RecoveryRequired) {
                if let Some(index) = owner.upgrade() {
                    let _ = index.request_recovery(&reservation);
                }
                let _ = completed_sender.send(false);
            } else if result.is_err() {
                if let Some(index) = owner.upgrade() {
                    index.consume_fatal_effects();
                    index.handle_worker_failure(&worker_volume, owner_id, &reservation);
                }
                let _ = completed_sender.send(false);
            }
        });
        if let Err(join) = self.attach_worker_join(&volume, owner_id, runtime_epoch, join) {
            drop(start_sender);
            let _ = join.join();
            return Err(FileIndexError::Unavailable);
        }
        if start_sender.send(()).is_err() {
            if let Some(worker) = self.remove_worker_if_owner(&volume.identity, owner_id) {
                stop_and_join_worker(worker);
            }
            return Err(FileIndexError::Unavailable);
        }
        self.finish_worker_start(&volume, owner_id, completed_receiver)
    }

    #[cfg(not(test))]
    fn calibrate_volume(
        &self,
        volume: &FixedVolume,
        exclusions: &[ExcludedPrefix],
        watcher: &mut Watcher,
        owner: u64,
        worker_stop: &AtomicBool,
        runtime_epoch: u64,
    ) -> Result<u64, FileIndexError> {
        if worker_stop.load(Ordering::Acquire)
            || !self.worker_runtime_authorized(volume, owner, None, runtime_epoch)
        {
            return Err(FileIndexError::Unavailable);
        }
        let (generation, has_committed) =
            self.begin_worker_candidate(volume, owner, runtime_epoch)?;
        let scanner_stop = Arc::new(AtomicBool::new(false));
        let (scan_sender, scan_receiver) = mpsc::sync_channel(1);
        let scan_volume_identity = volume.clone();
        let scan_exclusions = exclusions.to_vec();
        let scan_stop = Arc::clone(&scanner_stop);
        let scanner_join = thread::spawn(move || {
            let sender = scan_sender.clone();
            let result = scan_volume(
                &scan_volume_identity,
                &scan_exclusions,
                &scan_stop,
                |batch| send_scan_message(&sender, ScanMessage::Batch(batch), &scan_stop),
            );
            let _ = send_scan_message(&scan_sender, ScanMessage::Finished(result), &scan_stop);
        });
        let scanner = ScannerGuard {
            stop: scanner_stop,
            join: Some(scanner_join),
        };
        let mut replay = EventBuffer::new();
        let mut final_batch = None;
        let scan = loop {
            if worker_stop.load(Ordering::Acquire)
                || !self.worker_runtime_authorized(volume, owner, None, runtime_epoch)
            {
                return Err(FileIndexError::Unavailable);
            }
            match scan_receiver.try_recv() {
                Ok(ScanMessage::Batch(batch)) => {
                    if let Some(previous) = final_batch.replace(batch) {
                        let mut state = self.state.lock().expect("file index lock poisoned");
                        if state.runtime_epoch != runtime_epoch
                            || !self.worker_write_authorized_locked(&state, volume, owner, None)
                        {
                            return Err(FileIndexError::Unavailable);
                        }
                        let identities = state.authenticated_volumes.clone();
                        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
                        let result = store.append_candidate(
                            &volume.identity,
                            generation,
                            previous,
                            &identities,
                        );
                        let revision = self.finish_store_write(&mut state, result)?;
                        state.index_revision_high_water = revision;
                    }
                }
                Ok(ScanMessage::Finished(result)) => {
                    break result.map_err(|_| FileIndexError::Unavailable)?;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(FileIndexError::Unavailable);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if let Some(events) = watcher
                .wait_batch(10)
                .map_err(|_| FileIndexError::Unavailable)?
            {
                self.apply_scan_events(
                    ScanReplayContext {
                        volume,
                        exclusions,
                        generation,
                        has_committed,
                        owner,
                        runtime_epoch,
                    },
                    &mut replay,
                    &events,
                )?;
            }
        };
        scanner.finish()?;
        if let Some(events) = watcher
            .wait_batch(0)
            .map_err(|_| FileIndexError::Unavailable)?
        {
            self.apply_scan_events(
                ScanReplayContext {
                    volume,
                    exclusions,
                    generation,
                    has_committed,
                    owner,
                    runtime_epoch,
                },
                &mut replay,
                &events,
            )?;
        }
        let replay = replay
            .last_sequence()
            .map(|cutoff| replay.take_through(cutoff))
            .unwrap_or_default();
        if worker_stop.load(Ordering::Acquire)
            || !self.worker_runtime_authorized(volume, owner, None, runtime_epoch)
        {
            return Err(FileIndexError::Unavailable);
        }
        self.commit_worker_candidate(
            volume,
            owner,
            generation,
            final_batch.unwrap_or_default(),
            &scan.denied_prefixes,
            |apply| {
                materialize_events(
                    volume,
                    &replay,
                    exclusions,
                    || {
                        worker_stop.load(Ordering::Acquire)
                            || self.lifecycle.file_index_phase() != FileIndexPhase::Running
                            || !self.worker_is_current(volume, owner, None)
                    },
                    |batch| apply(batch).map_err(|_| BackendError::Platform),
                )
                .map_err(|_| StoreError::InvalidData)
            },
        )?;
        Ok(generation)
    }

    #[cfg(not(test))]
    fn apply_scan_events(
        &self,
        context: ScanReplayContext<'_>,
        replay: &mut EventBuffer,
        events: &[windows_backend::StructuredEvent],
    ) -> Result<(), FileIndexError> {
        if !self.worker_runtime_authorized(
            context.volume,
            context.owner,
            None,
            context.runtime_epoch,
        ) {
            return Err(FileIndexError::Unavailable);
        }
        let events =
            stage_replay_events(&context.volume.identity, events, context.exclusions, replay)?;
        if events.is_empty() {
            return Ok(());
        }
        if !context.has_committed {
            return Ok(());
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != context.runtime_epoch
            || !self.worker_write_authorized_locked(&state, context.volume, context.owner, None)
        {
            return Err(FileIndexError::Unavailable);
        }
        let identities = state.authenticated_volumes.clone();
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let result = store.apply_committed_streaming(
            &context.volume.identity,
            context.generation,
            &identities,
            |apply| {
                materialize_events(
                    context.volume,
                    &events,
                    context.exclusions,
                    || {
                        self.lifecycle.file_index_phase() != FileIndexPhase::Running
                            || !self.worker_is_current(context.volume, context.owner, None)
                    },
                    |batch| apply(batch).map_err(|_| BackendError::Platform),
                )
                .map_err(|_| StoreError::InvalidData)
            },
        );
        let revision = self.finish_store_write(&mut state, result)?;
        state.index_revision_high_water = revision;
        Ok(())
    }

    #[cfg(not(test))]
    fn apply_watcher_events(
        &self,
        volume: &FixedVolume,
        exclusions: &[ExcludedPrefix],
        generation: u64,
        owner: u64,
        runtime_epoch: u64,
        events: &[windows_backend::StructuredEvent],
    ) -> Result<(), FileIndexError> {
        if !self.worker_runtime_authorized(volume, owner, Some(generation), runtime_epoch) {
            return Err(FileIndexError::Unavailable);
        }
        let events = filter_replay_events(&volume.identity, events, exclusions);
        if events.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != runtime_epoch
            || !self.worker_write_authorized_locked(&state, volume, owner, Some(generation))
        {
            return Err(FileIndexError::Unavailable);
        }
        let identities = state.authenticated_volumes.clone();
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let result =
            store.apply_live_streaming(&volume.identity, generation, &identities, |apply| {
                materialize_events(
                    volume,
                    &events,
                    exclusions,
                    || {
                        self.lifecycle.file_index_phase() != FileIndexPhase::Running
                            || !self.worker_is_current(volume, owner, Some(generation))
                    },
                    |batch| apply(batch).map_err(|_| BackendError::Platform),
                )
                .map_err(|_| StoreError::InvalidData)
            });
        let revision = self.finish_store_write(&mut state, result)?;
        state.index_revision_high_water = revision;
        Ok(())
    }

    fn complete_volume_calibration(
        &self,
        volume: &FixedVolume,
        owner: u64,
        runtime_epoch: u64,
    ) -> Result<(), FileIndexError> {
        let generation = self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(&volume.identity)
            .filter(|worker| {
                worker.owner == owner
                    && worker.runtime_epoch == runtime_epoch
                    && worker.mount_point == volume.mount_point
                    && !worker.failed
                    && !worker.stop.load(Ordering::Acquire)
            })
            .map(|worker| worker.generation.load(Ordering::Acquire))
            .filter(|generation| *generation != 0)
            .ok_or(FileIndexError::Unavailable)?;
        if !self.worker_runtime_authorized(volume, owner, Some(generation), runtime_epoch) {
            return Err(FileIndexError::Unavailable);
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != runtime_epoch
            || !self.worker_write_authorized_locked(&state, volume, owner, Some(generation))
        {
            return Err(FileIndexError::Unavailable);
        }
        state.quarantined_volumes.remove(&volume.identity);
        Ok(())
    }

    fn handle_worker_failure(
        self: &Arc<Self>,
        volume: &FixedVolume,
        owner: u64,
        reservation: &DbWorkReservation,
    ) {
        let completed_generation = self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(&volume.identity)
            .filter(|worker| worker.owner == owner)
            .map(|worker| worker.generation.load(Ordering::Acquire))
            .unwrap_or(0);
        if !self.mark_worker_stopped(&volume.identity, owner) {
            return;
        }
        let _ = self.mark_fixed_volume_dirty(volume, reservation);
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.fatal_unavailable || !state.admission_open || state.store.is_none() {
            return;
        }
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        coordinator.calibrated = false;
        if completed_generation != 0 {
            let runtime_epoch = self.runtime_epoch();
            let runtime = coordinator
                .volumes
                .entry(volume.identity.clone())
                .or_default();
            runtime.calibration = Calibration::Running { runtime_epoch };
            if runtime
                .finish_failure(std::time::Instant::now(), runtime_epoch)
                .is_err()
            {
                drop(coordinator);
                self.latch_process_fatal(&mut state);
                return;
            }
            self.coordinator.signal.notify_one();
        }
    }

    #[cfg(test)]
    fn coordinator_snapshot_for_test(&self) -> CoordinatorSnapshot {
        let coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        CoordinatorSnapshot {
            pending_signals: usize::from(coordinator.pending_root.is_some()),
            wakes: coordinator.wakes,
            thread_starts: usize::from(coordinator.thread_started),
        }
    }

    fn claim_calibration_run_with<F>(&self, before_state: F) -> Option<(PathBuf, DbWorkReservation)>
    where
        F: FnOnce(),
    {
        before_state();
        let mut state = self.state.lock().expect("file index lock poisoned");
        self.admit_locked(&state, AdmissionKind::DbWork, state.runtime_epoch)
            .ok()?;
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        if coordinator.running || self.coordinator.stop.load(Ordering::Acquire) {
            return None;
        }
        if coordinator.pending_runtime_epoch != Some(state.runtime_epoch) {
            return None;
        }
        let app_data_root = coordinator.pending_root.clone()?;
        let Some(next_db_work) = state.db_work.checked_add(1) else {
            drop(coordinator);
            let newly_fatal = self.latch_exhaustion_locked(&mut state);
            drop(state);
            if newly_fatal {
                self.consume_fatal_effects();
            }
            return None;
        };
        let runtime_epoch = state.runtime_epoch;
        state.db_work = next_db_work;
        coordinator.running = true;
        coordinator.pending_root = None;
        coordinator.pending_runtime_epoch = None;
        Some((
            app_data_root,
            DbWorkReservation {
                state: Arc::clone(&self.state),
                coordinator: Arc::clone(&self.coordinator),
                runtime_epoch,
                released: false,
            },
        ))
    }

    #[cfg(test)]
    fn mark_fixed_volume_dirty_for_test(
        self: &Arc<Self>,
        volume: &FixedVolume,
    ) -> Result<(), FileIndexError> {
        let reservation = self
            .reserve_db_work(self.runtime_epoch())
            .map_err(|_| FileIndexError::Unavailable)?;
        self.mark_fixed_volume_dirty(volume, &reservation)
    }

    #[cfg(test)]
    fn stop_detached_workers_for_test(
        self: &Arc<Self>,
        volumes: &[FixedVolume],
    ) -> Result<(), FileIndexError> {
        let reservation = self
            .reserve_db_work(self.runtime_epoch())
            .map_err(|_| FileIndexError::Unavailable)?;
        self.stop_detached_workers(volumes, &reservation)
    }

    #[cfg(test)]
    fn handle_worker_failure_for_test(self: &Arc<Self>, volume: &FixedVolume, owner: u64) {
        if let Ok(reservation) = self.reserve_db_work(self.runtime_epoch()) {
            self.handle_worker_failure(volume, owner, &reservation);
        }
    }
}

fn stage_replay_events(
    identity: &VolumeIdentity,
    events: &[windows_backend::StructuredEvent],
    exclusions: &[ExcludedPrefix],
    replay: &mut EventBuffer,
) -> Result<Vec<windows_backend::StructuredEvent>, FileIndexError> {
    let last_sequence = events.last().map(|event| event.sequence);
    let events = windows_backend::filter_replay_events(identity, events, exclusions);
    if !events.is_empty() {
        replay
            .push_preserved_batch(events.iter().cloned())
            .map_err(|_| FileIndexError::Unavailable)?;
    }
    if let Some(last_sequence) = last_sequence {
        replay
            .observe_preserved_sequence(last_sequence)
            .map_err(|_| FileIndexError::Unavailable)?;
    }
    Ok(events)
}

fn stop_and_join_worker(mut worker: WorkerRecord) {
    worker.stop.store(true, Ordering::Release);
    if let Some(join) = worker.join.take() {
        if join.thread().id() != std::thread::current().id() {
            let _ = join.join();
        }
    }
}

impl Drop for FileIndex {
    fn drop(&mut self) {
        self.coordinator.stop.store(true, Ordering::Release);
        self.coordinator.signal.notify_all();
        if let Some(join) = self
            .coordinator
            .join
            .lock()
            .expect("file index coordinator join lock poisoned")
            .take()
        {
            if join.thread().id() != std::thread::current().id() {
                let _ = join.join();
            }
        }
        let workers = self
            .workers
            .get_mut()
            .expect("file index worker lock poisoned")
            .by_volume
            .drain()
            .map(|(_, worker)| worker)
            .collect::<Vec<_>>();
        for worker in workers {
            stop_and_join_worker(worker);
        }
        if let Some(mut worker) = self
            .integrity_worker
            .get_mut()
            .expect("file index integrity join lock poisoned")
            .take()
        {
            worker.stop.store(true, Ordering::Release);
            if let Some(join) = worker.join.take() {
                if join.thread().id() != std::thread::current().id() {
                    let _ = join.join();
                }
            }
        }
    }
}

#[cfg(test)]
struct CoordinatorSnapshot {
    pending_signals: usize,
    wakes: u64,
    thread_starts: usize,
}

fn empty_batch(
    runtime_epoch: u64,
    publication_generation: u64,
    index_revision: u64,
    status: FileIndexStatus,
) -> FileSearchBatch {
    FileSearchBatch {
        runtime_epoch,
        publication_generation,
        index_revision,
        total: 0,
        status,
        items: Vec::new(),
    }
}
