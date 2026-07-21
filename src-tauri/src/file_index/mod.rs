use std::{
    collections::{HashMap, HashSet},
    fs, io,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
};

#[cfg(not(test))]
use std::{sync::mpsc, thread};

use icu_casemap::CaseMapper;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

mod store;
mod windows_backend;

use store::{ordinal_sort_identity, Store, StoreError, StoreQueryResult};
#[cfg(not(test))]
use windows_backend::{
    filter_replay_events, fixed_volumes, materialize_events, scan_volume, system_exclusions,
    BackendError, ScanSummary, Watcher,
};
use windows_backend::{EventBuffer, ExcludedPrefix, FixedVolume};

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
    };

    use super::{
        authenticate_app_data_root, begin_lazy_init_locked, fold_name, open_store,
        validate_index_path_shape, AdmissionError, FileCategory, FileIndex, FileIndexError,
        FileIndexStatus, FileSort, IndexState, LazyInitDecision, LifecycleMode, QuerySpec,
        StoreError, FOLD_ALGORITHM_ID,
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
            Err(AdmissionError::Unavailable)
        );
        assert!(state.fatal_unavailable);
        assert!(!state.admission_open);
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
        assert!(state.fatal_unavailable);
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
        let index = FileIndex::default();
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
        let index = FileIndex::default();
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
        let index = FileIndex::default();
        let dir = TestDir::new();
        let open_calls = Cell::new(0);
        let query_calls = Cell::new(0);
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

        let observer = FileIndex::default();
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
        let index = FileIndex::default();
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
        assert_eq!(
            index
                .reconcile_calibration_inventory(0, std::slice::from_ref(&remounted))
                .unwrap(),
            Some(true)
        );
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
                mount_point: fixed.mount_point.clone(),
                stop,
                generation: Arc::new(AtomicU64::new(0)),
                join: Some(join),
                failed: false,
            },
        );

        index.handle_worker_failure(&fixed, owner);
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
            index: &FileIndex,
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

        let index = FileIndex::default();
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
        let (started, old_owner) = install(&index, &c, &order);
        assert!(started);
        assert_eq!(install(&index, &c, &order), (false, old_owner));
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
        assert!(!index.worker_is_current(&identity, old_owner, Some(7)));
        assert!(index.worker_is_current(&identity, new_owner, Some(7)));

        index
            .state
            .lock()
            .unwrap()
            .quarantined_volumes
            .insert(identity.clone());
        assert!(index
            .complete_volume_calibration(&identity, old_owner)
            .is_err());
        assert!(index
            .state
            .lock()
            .unwrap()
            .quarantined_volumes
            .contains(&identity));
        index
            .complete_volume_calibration(&identity, new_owner)
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
        assert!(index.worker_is_current(&identity, replacement_owner, Some(7)));
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        store
            .seed_committed_for_test(&identity, [candidate_entry("find-kept.txt", 1)])
            .unwrap();
        index.state.lock().unwrap().store = Some(store);
        index.stop_detached_workers(&[]).unwrap();
        assert!(index.workers.lock().unwrap().by_volume.is_empty());
        assert!(index.begin_worker_candidate(&d, replacement_owner).is_err());
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
        let index = FileIndex::default();
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
        index.state.lock().unwrap().store = Some(store);

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
                mount_point: fixed.mount_point.clone(),
                stop,
                generation: Arc::new(AtomicU64::new(generation)),
                join: Some(join),
                failed: false,
            },
        );

        index.stop_detached_workers(&[]).unwrap();
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
        let index = FileIndex::default();
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
        reopened.recover_candidates_for_test().unwrap();
        assert!(reopened.candidate_rows_for_test(&volume).is_empty());
        assert_eq!(reopened.generation_state_for_test(&volume).1, None);
        let visible = reopened
            .query_for_test(&query(), std::slice::from_ref(&volume))
            .unwrap();
        assert_eq!(visible.entries.len(), 1);
        assert_eq!(visible.entries[0].name, "find-kept.txt");
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
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleMode {
    Uninitialized,
    Opening { owner: u64 },
    Active,
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
}

struct IndexState {
    mode: LifecycleMode,
    lazy_owner_high_water: u64,
    availability: Availability,
    admission_open: bool,
    fatal_unavailable: bool,
    runtime_epoch: u64,
    index_revision_high_water: u64,
    inventory_observation: u64,
    authenticated_volumes: Vec<VolumeIdentity>,
    authenticated_mounts: Vec<(VolumeIdentity, String)>,
    pending_inventory_transitions: HashSet<VolumeIdentity>,
    quarantined_volumes: HashSet<VolumeIdentity>,
    authenticated_app_data_root: Option<PathBuf>,
    store: Option<Store>,
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
            inventory_observation: 0,
            authenticated_volumes: Vec::new(),
            authenticated_mounts: Vec::new(),
            pending_inventory_transitions: HashSet::new(),
            quarantined_volumes: HashSet::new(),
            authenticated_app_data_root: None,
            store: None,
        }
    }
}

impl IndexState {
    fn advance_revision_locked(
        &mut self,
        publication_generation: &AtomicU64,
    ) -> Result<u64, AdmissionError> {
        let Some(next) = self.index_revision_high_water.checked_add(1) else {
            self.latch_unavailable(publication_generation);
            return Err(AdmissionError::Unavailable);
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

    fn latch_unavailable(&mut self, publication_generation: &AtomicU64) {
        if !self.fatal_unavailable {
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
        self.pending_inventory_transitions.clear();
        self.quarantined_volumes.clear();
        self.authenticated_app_data_root = None;
        self.store = None;
    }
}

fn begin_lazy_init_locked(
    state: &mut IndexState,
    expected_runtime_epoch: u64,
    publication_generation: &AtomicU64,
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
                state.latch_unavailable(publication_generation);
                return Err(AdmissionError::OwnerExhausted);
            };
            state.lazy_owner_high_water = owner;
            state.mode = LifecycleMode::Opening { owner };
            state.admission_open = false;
            Ok(LazyInitDecision::Start { owner })
        }
        LifecycleMode::Opening { .. } => Ok(LazyInitDecision::ObserveBuilding),
        LifecycleMode::Active => Err(AdmissionError::WrongMode),
    }
}

#[derive(Debug)]
pub(crate) enum FileIndexError {
    Unavailable,
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
    let mut store = Store::open(database, &identity).map_err(|_| FileIndexError::Unavailable)?;
    let identity_change = store
        .ensure_sort_identity(&identity)
        .map_err(|_| FileIndexError::Unavailable)?;
    let mut revision = match identity_change {
        Some((_, revision)) => revision,
        None => store
            .index_revision()
            .map_err(|_| FileIndexError::Unavailable)?,
    };
    let recovered = store
        .recover_candidates()
        .map_err(|_| FileIndexError::Unavailable)?;
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
    state: Mutex<IndexState>,
    coordinator: Arc<CoordinatorControl>,
    workers: Mutex<WorkerRegistry>,
    publication_runtime_epoch: AtomicU64,
    publication_generation: AtomicU64,
}

#[derive(Default)]
struct CoordinatorState {
    thread_started: bool,
    running: bool,
    calibrated: bool,
    pending_root: Option<PathBuf>,
    wakes: u64,
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
    mount_point: PathBuf,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    join: Option<std::thread::JoinHandle<()>>,
    failed: bool,
}

enum WorkerPreparation {
    Existing,
    Start { owner: u64 },
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
        Self {
            state: Mutex::new(IndexState::default()),
            coordinator: Arc::new(CoordinatorControl::default()),
            workers: Mutex::new(WorkerRegistry::default()),
            publication_runtime_epoch: AtomicU64::new(0),
            publication_generation: AtomicU64::new(0),
        }
    }
}

impl FileIndex {
    pub(crate) fn runtime_epoch(&self) -> u64 {
        self.publication_runtime_epoch.load(Ordering::Acquire)
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

    fn prepare_worker(&self, volume: &FixedVolume) -> Result<WorkerPreparation, FileIndexError> {
        let (replaced, owner) = {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            if workers
                .by_volume
                .get(&volume.identity)
                .is_some_and(|worker| {
                    !worker.failed
                        && !worker.stop.load(Ordering::Acquire)
                        && worker.mount_point == volume.mount_point
                        && worker.join.as_ref().is_some_and(|join| !join.is_finished())
                })
            {
                return Ok(WorkerPreparation::Existing);
            }
            let Some(owner) = workers.next_owner.checked_add(1) else {
                drop(workers);
                self.state
                    .lock()
                    .expect("file index lock poisoned")
                    .latch_unavailable(&self.publication_generation);
                return Err(FileIndexError::Unavailable);
            };
            let replaced = workers.by_volume.remove(&volume.identity);
            workers.next_owner = owner;
            (replaced, owner)
        };
        if let Some(replaced) = replaced {
            stop_and_join_worker(replaced);
            self.mark_fixed_volume_dirty(volume)?;
        }
        Ok(WorkerPreparation::Start { owner })
    }

    fn install_worker(&self, volume: &FixedVolume, worker: WorkerRecord) {
        self.workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .insert(volume.identity.clone(), worker);
    }

    fn stop_detached_workers(&self, volumes: &[FixedVolume]) -> Result<(), FileIndexError> {
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
            self.mark_fixed_volume_dirty(&volume)?;
        }
        Ok(())
    }

    fn worker_is_current(
        &self,
        volume: &VolumeIdentity,
        owner: u64,
        generation: Option<u64>,
    ) -> bool {
        self.workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(volume)
            .is_some_and(|worker| {
                worker.owner == owner
                    && !worker.failed
                    && !worker.stop.load(Ordering::Acquire)
                    && generation.is_none_or(|generation| {
                        worker.generation.load(Ordering::Acquire) == generation
                    })
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

    fn mark_fixed_volume_dirty(&self, volume: &FixedVolume) -> Result<(), FileIndexError> {
        let mount = volume
            .mount_point
            .to_str()
            .ok_or(FileIndexError::Unavailable)?;
        let mut state = self.state.lock().expect("file index lock poisoned");
        let Some(store) = state.store.as_mut() else {
            return Ok(());
        };
        match store.mark_volume_dirty(&volume.identity, mount) {
            Ok(revision) => {
                state.index_revision_high_water = revision;
                Ok(())
            }
            Err(_) => {
                state.latch_unavailable(&self.publication_generation);
                Err(FileIndexError::Unavailable)
            }
        }
    }

    fn finish_worker_start(
        &self,
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
                    self.mark_fixed_volume_dirty(volume)?;
                }
                Err(FileIndexError::Unavailable)
            }
        }
    }

    fn begin_worker_candidate(
        &self,
        volume: &FixedVolume,
        owner: u64,
    ) -> Result<(u64, bool), FileIndexError> {
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(&volume.identity, owner, None) {
            return Err(FileIndexError::Unavailable);
        }
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let mount = volume
            .mount_point
            .to_str()
            .ok_or(FileIndexError::Unavailable)?;
        let (generation, revision, has_committed) = store
            .begin_candidate(&volume.identity, mount)
            .map_err(|_| FileIndexError::Unavailable)?;
        state.index_revision_high_water = revision;
        Ok((generation, has_committed))
    }

    pub(crate) fn search(
        self: &Arc<Self>,
        app_data_dir: &Path,
        spec: QuerySpec,
        expected_runtime_epoch: u64,
    ) -> Result<FileSearchBatch, FileIndexError> {
        #[cfg(not(test))]
        self.refresh_query_volumes_with(|| {
            fixed_volumes().map_err(|_| FileIndexError::Unavailable)
        })?;
        let batch = self.search_with(
            app_data_dir,
            spec,
            expected_runtime_epoch,
            authenticate_app_data_root,
            open_store,
            |store, spec| store.query(spec, &[]),
        )?;
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
                self.state
                    .lock()
                    .expect("file index lock poisoned")
                    .latch_unavailable(&self.publication_generation);
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
                state.latch_unavailable(&self.publication_generation);
                return Err(error);
            }
        };
        let Some(observation) = state.inventory_observation.checked_add(1) else {
            state.latch_unavailable(&self.publication_generation);
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
        state
            .pending_inventory_transitions
            .extend(transitions.iter().cloned());
        state.quarantined_volumes.extend(transitions);
        state.authenticated_volumes = volumes
            .iter()
            .map(|volume| volume.identity.clone())
            .collect();
        state.authenticated_mounts = mounts;
        state.inventory_observation = observation;
        Ok(!state.pending_inventory_transitions.is_empty())
    }

    fn reconcile_inventory_locked(&self, state: &mut IndexState) -> Result<bool, FileIndexError> {
        let current_mounts = state.authenticated_mounts.clone();
        let transitions = state
            .pending_inventory_transitions
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let quarantined = state.quarantined_volumes.clone();
        let reconciled = state
            .store
            .as_mut()
            .ok_or(FileIndexError::Unavailable)
            .and_then(|store| {
                store
                    .reconcile_current_mounts(&current_mounts, &transitions)
                    .map_err(|_| FileIndexError::Unavailable)
            });
        let (identities, revision, changed) = match reconciled {
            Ok(reconciled) => reconciled,
            Err(error) => {
                state.latch_unavailable(&self.publication_generation);
                return Err(error);
            }
        };
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

    fn search_with<A, O, Q>(
        &self,
        app_data_dir: &Path,
        spec: QuerySpec,
        expected_runtime_epoch: u64,
        mut authenticate_root: A,
        mut open: O,
        mut query_store: Q,
    ) -> Result<FileSearchBatch, FileIndexError>
    where
        A: FnMut(&Path) -> Result<PathBuf, FileIndexError>,
        O: FnMut(&Path) -> Result<(Store, u64, Option<u64>), FileIndexError>,
        Q: FnMut(&mut Store, &QuerySpec) -> Result<StoreQueryResult, StoreError>,
    {
        let owner = {
            let mut state = self.state.lock().expect("file index lock poisoned");
            if state.runtime_epoch != expected_runtime_epoch {
                return Err(FileIndexError::Unavailable);
            }
            if state.fatal_unavailable {
                return Ok(empty_batch(
                    expected_runtime_epoch,
                    self.publication_generation.load(Ordering::Acquire),
                    state.index_revision_high_water,
                    FileIndexStatus::Unavailable,
                ));
            }
            match state.mode {
                LifecycleMode::Active if state.admission_open => None,
                LifecycleMode::Active => return Err(FileIndexError::Unavailable),
                _ => match begin_lazy_init_locked(
                    &mut state,
                    expected_runtime_epoch,
                    &self.publication_generation,
                )
                .map_err(|_| FileIndexError::Unavailable)?
                {
                    LazyInitDecision::Start { owner } => Some(owner),
                    LazyInitDecision::ObserveBuilding => {
                        return Ok(empty_batch(
                            expected_runtime_epoch,
                            self.publication_generation.load(Ordering::Acquire),
                            state.index_revision_high_water,
                            FileIndexStatus::Building,
                        ))
                    }
                },
            }
        };

        if let Some(owner) = owner {
            let opened = authenticate_root(app_data_dir).and_then(|path| {
                let root = path
                    .parent()
                    .ok_or(FileIndexError::Unavailable)?
                    .to_path_buf();
                open(&path).map(|opened| (opened, root))
            });
            let mut state = self.state.lock().expect("file index lock poisoned");
            if state.mode != (LifecycleMode::Opening { owner })
                || state.runtime_epoch != expected_runtime_epoch
            {
                return Err(FileIndexError::Unavailable);
            }
            match opened {
                Ok(((store, revision, previous_revision), authenticated_root)) => {
                    state.store = Some(store);
                    state.authenticated_app_data_root = Some(authenticated_root);
                    if let Some(previous) = previous_revision {
                        state.index_revision_high_water = previous;
                        if state
                            .advance_revision_locked(&self.publication_generation)
                            .map_err(|_| FileIndexError::Unavailable)?
                            != revision
                        {
                            state.latch_unavailable(&self.publication_generation);
                            return Err(FileIndexError::Unavailable);
                        }
                    } else {
                        state.index_revision_high_water = revision;
                    }
                    state.mode = LifecycleMode::Active;
                    state.availability = Availability::Normal;
                    state.admission_open = true;
                }
                Err(error) => {
                    state.latch_unavailable(&self.publication_generation);
                    return Err(error);
                }
            }
        }

        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.runtime_epoch != expected_runtime_epoch || !state.admission_open {
            return Err(FileIndexError::Unavailable);
        }
        let mount_changed = self.reconcile_inventory_locked(&mut state)?;
        let identities = state.authenticated_volumes.clone();
        let result = match state.store.as_mut() {
            Some(store) if identities.is_empty() => query_store(store, &spec),
            Some(store) => store.query(&spec, &identities),
            None => Err(StoreError::InvalidData),
        };
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                state.latch_unavailable(&self.publication_generation);
                return Err(FileIndexError::Unavailable);
            }
        };
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
            let index = Arc::downgrade(self);
            let coordinator = Arc::clone(&self.coordinator);
            let join = thread::spawn({
                let coordinator = Arc::clone(&coordinator);
                move || Self::coordinator_loop(index, coordinator)
            });
            *coordinator
                .join
                .lock()
                .expect("file index coordinator join lock poisoned") = Some(join);
        }
        #[cfg(test)]
        let _ = start_thread;
        true
    }

    fn mark_calibration_pending(&self, app_data_root: PathBuf) -> (bool, bool) {
        {
            let state = self.state.lock().expect("file index lock poisoned");
            if state.mode != LifecycleMode::Active
                || state.fatal_unavailable
                || !state.admission_open
                || state.store.is_none()
            {
                return (false, false);
            }
        }
        let mut coordinator = self
            .coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned");
        if coordinator.pending_root.is_some() || (coordinator.calibrated && !coordinator.running) {
            return (false, false);
        }
        let start_thread = !coordinator.thread_started;
        let Some(wakes) = coordinator.wakes.checked_add(1) else {
            drop(coordinator);
            let mut state = self.state.lock().expect("file index lock poisoned");
            state.latch_unavailable(&self.publication_generation);
            return (false, false);
        };
        coordinator.thread_started = true;
        coordinator.pending_root = Some(app_data_root);
        coordinator.wakes = wakes;
        self.coordinator.signal.notify_one();
        (true, start_thread)
    }

    #[cfg(not(test))]
    fn coordinator_loop(index: std::sync::Weak<Self>, coordinator: Arc<CoordinatorControl>) {
        loop {
            let app_data_root = {
                let mut state = coordinator
                    .state
                    .lock()
                    .expect("file index coordinator lock poisoned");
                while state.pending_root.is_none() {
                    if coordinator.stop.load(Ordering::Acquire) {
                        return;
                    }
                    let waited = coordinator
                        .signal
                        .wait_timeout(state, std::time::Duration::from_millis(250))
                        .expect("file index coordinator lock poisoned");
                    state = waited.0;
                    if coordinator.stop.load(Ordering::Acquire) || index.strong_count() == 0 {
                        return;
                    }
                }
                state.running = true;
                state.pending_root.take().expect("pending root disappeared")
            };
            let Some(owner) = index.upgrade() else {
                return;
            };
            let completed = owner.run_calibration(&app_data_root);
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
        state.calibrated = completed && state.pending_root.is_none();
        if state.pending_root.is_some() {
            coordinator.signal.notify_one();
        }
    }

    #[cfg(not(test))]
    fn run_calibration(self: &Arc<Self>, app_data_root: &Path) -> bool {
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
            Err(_) => return false,
        }
        if self.stop_detached_workers(&volumes).is_err() {
            return false;
        }
        let mut completed = true;
        for volume in volumes {
            if self
                .start_volume_worker(volume.clone(), exclusions.clone())
                .is_err()
            {
                completed = false;
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
            state.latch_unavailable(&self.publication_generation);
            let mut coordinator = self
                .coordinator
                .state
                .lock()
                .expect("file index coordinator lock poisoned");
            coordinator.pending_root = None;
            coordinator.running = false;
            coordinator.calibrated = true;
        }
        result
    }

    #[cfg(not(test))]
    fn start_volume_worker(
        self: &Arc<Self>,
        volume: FixedVolume,
        exclusions: Vec<ExcludedPrefix>,
    ) -> Result<(), FileIndexError> {
        let owner_id = match self.prepare_worker(&volume)? {
            WorkerPreparation::Existing => return Ok(()),
            WorkerPreparation::Start { owner } => owner,
        };
        let owner = Arc::downgrade(self);
        let stop = Arc::new(AtomicBool::new(false));
        let generation_owner = Arc::new(AtomicU64::new(0));
        let worker_stop = Arc::clone(&stop);
        let worker_generation = Arc::clone(&generation_owner);
        let (completed_sender, completed_receiver) = mpsc::sync_channel(1);
        let (start_sender, start_receiver) = mpsc::sync_channel(0);
        let worker_volume = volume.clone();
        let join = thread::spawn(move || {
            if start_receiver.recv().is_err() {
                return;
            }
            let result = (|| {
                let index = owner.upgrade().ok_or(FileIndexError::Unavailable)?;
                if !index.worker_is_current(&worker_volume.identity, owner_id, None) {
                    return Err(FileIndexError::Unavailable);
                }
                let mut watcher =
                    Watcher::arm(&worker_volume).map_err(|_| FileIndexError::Unavailable)?;
                let generation = index.calibrate_volume(
                    &worker_volume,
                    &exclusions,
                    &mut watcher,
                    owner_id,
                    &worker_stop,
                )?;
                worker_generation.store(generation, Ordering::Release);
                index.complete_volume_calibration(&worker_volume.identity, owner_id)?;
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
                        &events,
                    )?;
                }
            })();
            if result.is_err() {
                if let Some(index) = owner.upgrade() {
                    index.handle_worker_failure(&worker_volume, owner_id);
                }
                let _ = completed_sender.send(false);
            }
        });
        self.install_worker(
            &volume,
            WorkerRecord {
                owner: owner_id,
                mount_point: volume.mount_point.clone(),
                stop,
                generation: generation_owner,
                join: Some(join),
                failed: false,
            },
        );
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
    ) -> Result<u64, FileIndexError> {
        if worker_stop.load(Ordering::Acquire)
            || !self.worker_is_current(&volume.identity, owner, None)
        {
            return Err(FileIndexError::Unavailable);
        }
        let (generation, has_committed) = self.begin_worker_candidate(volume, owner)?;
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
                || !self.worker_is_current(&volume.identity, owner, None)
            {
                return Err(FileIndexError::Unavailable);
            }
            match scan_receiver.try_recv() {
                Ok(ScanMessage::Batch(batch)) => {
                    if let Some(previous) = final_batch.replace(batch) {
                        let mut state = self.state.lock().expect("file index lock poisoned");
                        if !self.worker_is_current(&volume.identity, owner, None) {
                            return Err(FileIndexError::Unavailable);
                        }
                        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
                        let revision = store
                            .append_candidate(&volume.identity, generation, previous)
                            .map_err(|_| FileIndexError::Unavailable)?;
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
                },
                &mut replay,
                &events,
            )?;
        }
        let replay = replay
            .last_sequence()
            .map(|cutoff| replay.take_through(cutoff))
            .unwrap_or_default();
        let replay = if replay.is_empty() {
            windows_backend::EventChanges {
                deleted_prefixes: Vec::new(),
                entries: Vec::new(),
            }
        } else {
            materialize_events(volume, &replay, exclusions)
                .map_err(|_| FileIndexError::Unavailable)?
        };
        if worker_stop.load(Ordering::Acquire)
            || !self.worker_is_current(&volume.identity, owner, None)
        {
            return Err(FileIndexError::Unavailable);
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(&volume.identity, owner, None) {
            return Err(FileIndexError::Unavailable);
        }
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let revision = store
            .commit_candidate(
                &volume.identity,
                generation,
                final_batch.unwrap_or_default(),
                &replay.deleted_prefixes,
                replay.entries,
                &scan.denied_prefixes,
            )
            .map_err(|_| FileIndexError::Unavailable)?;
        state.index_revision_high_water = revision;
        Ok(generation)
    }

    #[cfg(not(test))]
    fn apply_scan_events(
        &self,
        context: ScanReplayContext<'_>,
        replay: &mut EventBuffer,
        events: &[windows_backend::StructuredEvent],
    ) -> Result<(), FileIndexError> {
        if !self.worker_is_current(&context.volume.identity, context.owner, None) {
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
        let changes = materialize_events(context.volume, &events, context.exclusions)
            .map_err(|_| FileIndexError::Unavailable)?;
        if changes.deleted_prefixes.is_empty() && changes.entries.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(&context.volume.identity, context.owner, None) {
            return Err(FileIndexError::Unavailable);
        }
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let revision = store
            .apply_committed_changes_during_scan(
                &context.volume.identity,
                context.generation,
                changes.deleted_prefixes.iter().map(String::as_str),
                changes.entries,
            )
            .map_err(|_| FileIndexError::Unavailable)?;
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
        events: &[windows_backend::StructuredEvent],
    ) -> Result<(), FileIndexError> {
        if !self.worker_is_current(&volume.identity, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        let events = filter_replay_events(&volume.identity, events, exclusions);
        if events.is_empty() {
            return Ok(());
        }
        let changes = materialize_events(volume, &events, exclusions)
            .map_err(|_| FileIndexError::Unavailable)?;
        if changes.deleted_prefixes.is_empty() && changes.entries.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(&volume.identity, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        if state.fatal_unavailable || !state.admission_open {
            return Err(FileIndexError::Unavailable);
        }
        let store = state.store.as_mut().ok_or(FileIndexError::Unavailable)?;
        let revision = store
            .apply_live_changes(
                &volume.identity,
                generation,
                changes.deleted_prefixes.iter().map(String::as_str),
                changes.entries,
            )
            .map_err(|_| FileIndexError::Unavailable)?;
        state.index_revision_high_water = revision;
        Ok(())
    }

    fn complete_volume_calibration(
        &self,
        volume: &VolumeIdentity,
        owner: u64,
    ) -> Result<(), FileIndexError> {
        let generation = self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(volume)
            .filter(|worker| {
                worker.owner == owner && !worker.failed && !worker.stop.load(Ordering::Acquire)
            })
            .map(|worker| worker.generation.load(Ordering::Acquire))
            .filter(|generation| *generation != 0)
            .ok_or(FileIndexError::Unavailable)?;
        if !self.worker_is_current(volume, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(volume, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        state.quarantined_volumes.remove(volume);
        Ok(())
    }

    fn handle_worker_failure(&self, volume: &FixedVolume, owner: u64) {
        if !self.mark_worker_stopped(&volume.identity, owner) {
            return;
        }
        let _ = self.mark_fixed_volume_dirty(volume);
        self.coordinator
            .state
            .lock()
            .expect("file index coordinator lock poisoned")
            .calibrated = false;
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
