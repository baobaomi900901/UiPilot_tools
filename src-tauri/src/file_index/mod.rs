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
        time::{Duration, Instant},
    };

    use super::{
        authenticate_app_data_root, begin_lazy_init_locked, fold_name, open_store,
        validate_index_path_shape, AdmissionError, FileCategory, FileIndex, FileIndexError,
        FileIndexStatus, FileSort, IndexState, LazyInitDecision, LifecycleMode, QuerySpec,
        StoreError, VolumeRuntime, FOLD_ALGORITHM_ID,
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
        let first_stop = Arc::new(AtomicBool::new(false));
        let second_stop = Arc::new(AtomicBool::new(false));
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
        assert!(index.coordinator.stop.load(Ordering::Acquire));
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
        assert!(!Arc::new(index).schedule_calibration());

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
        let index = FileIndex::default();
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
        fn install_worker(index: &FileIndex, volume: &super::FixedVolume) -> u64 {
            let owner = match index.prepare_worker(volume).unwrap() {
                super::WorkerPreparation::Start { owner } => owner,
                super::WorkerPreparation::Existing => panic!("worker must be new"),
            };
            index.install_worker(
                volume,
                super::WorkerRecord {
                    owner,
                    mount_point: volume.mount_point.clone(),
                    stop: Arc::new(AtomicBool::new(false)),
                    generation: Arc::new(AtomicU64::new(0)),
                    join: None,
                    failed: false,
                },
            );
            owner
        }

        let index = FileIndex::default();
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
        let (generation, has_committed) = index.begin_worker_candidate(&new_volume, owner).unwrap();
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
        index.mark_fixed_volume_dirty(&new_volume).unwrap();
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

        let first = FileIndex::default();
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
            .begin_worker_candidate(&new_volume, first_owner)
            .unwrap();
        assert_eq!(first.state.lock().unwrap().index_revision_high_water, 0);

        let remount = FileIndex::default();
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
                .begin_worker_candidate(&new_volume, remount_owner)
                .unwrap()
                .1
        );
        let state = remount.state.lock().unwrap();
        assert!(!state.authenticated_volumes.contains(&new_identity));
        assert!(state.quarantined_volumes.contains(&new_identity));
    }

    #[test]
    fn successful_remount_commit_restores_visibility_in_same_gate() {
        let index = FileIndex::default();
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
                mount_point: remounted.mount_point.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                generation: Arc::clone(&generation_owner),
                join: None,
                failed: false,
            },
        );
        let (generation, has_committed) = index.begin_worker_candidate(&remounted, owner).unwrap();
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
        assert!(index.complete_volume_calibration(&c, old_owner).is_err());
        assert!(index
            .state
            .lock()
            .unwrap()
            .quarantined_volumes
            .contains(&identity));
        index.complete_volume_calibration(&d, new_owner).unwrap();
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
    fn stale_remount_worker_failure_cleans_candidate_on_current_mount() {
        let index = FileIndex::default();
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

        index.mark_fixed_volume_dirty(&stale).unwrap();
        assert!(index.complete_volume_calibration(&stale, owner).is_err());

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
            Err(AdmissionError::Unavailable)
        );
        assert_eq!(state.index_revision_high_water, u64::MAX);
        assert_eq!(publication_generation.load(Ordering::Acquire), 8);
        assert!(state.fatal_unavailable);
        assert!(!state.admission_open);
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
            index.mark_fixed_volume_dirty(&fixed),
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
        let index = FileIndex::default();
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

        index.mark_fixed_volume_dirty(&fixed).unwrap();

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
        let index = FileIndex::default();
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
                mount_point: fixed.mount_point.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                generation: Arc::new(AtomicU64::new(1)),
                join: None,
                failed: false,
            },
        );

        index.handle_worker_failure(&fixed, owner);

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
        let (done_tx, done_rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            read_rx.recv().unwrap();
            let mut reader = Store::open(&reader_database, "identity-a").unwrap();
            let snapshot = reader.query_for_test(&query(), &[volume()]).unwrap();
            done_tx.send(snapshot).unwrap();
        });
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
    inventory_previous_authenticated: Option<Vec<VolumeIdentity>>,
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
            inventory_previous_authenticated: None,
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
        self.inventory_previous_authenticated = None;
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
                let mut state = self.state.lock().expect("file index lock poisoned");
                self.latch_process_fatal(&mut state);
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

    fn worker_is_current(&self, volume: &FixedVolume, owner: u64, generation: Option<u64>) -> bool {
        self.workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(&volume.identity)
            .is_some_and(|worker| {
                worker.owner == owner
                    && worker.mount_point == volume.mount_point
                    && !worker.failed
                    && !worker.stop.load(Ordering::Acquire)
                    && generation.is_none_or(|generation| {
                        worker.generation.load(Ordering::Acquire) == generation
                    })
            })
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
        coordinator.active_root = None;
        coordinator.calibrated = true;
        coordinator.volumes.clear();
        self.coordinator.signal.notify_one();
    }

    fn latch_process_fatal(&self, state: &mut IndexState) {
        state.latch_unavailable(&self.publication_generation);
        {
            let mut workers = self
                .workers
                .lock()
                .expect("file index worker lock poisoned");
            for worker in workers.by_volume.values_mut() {
                worker.stop.store(true, Ordering::Release);
            }
        }
        self.clear_calibration_retries();
    }

    fn finish_store_write<T>(
        &self,
        state: &mut IndexState,
        result: Result<T, StoreError>,
    ) -> Result<T, FileIndexError> {
        match result {
            Ok(value) => Ok(value),
            Err(StoreError::RevisionExhausted) => {
                self.latch_process_fatal(state);
                Err(FileIndexError::Unavailable)
            }
            Err(_) => Err(FileIndexError::Unavailable),
        }
    }

    fn mark_fixed_volume_dirty(&self, volume: &FixedVolume) -> Result<(), FileIndexError> {
        let worker_mount = volume
            .mount_point
            .to_str()
            .ok_or(FileIndexError::Unavailable)?;
        let mut state = self.state.lock().expect("file index lock poisoned");
        if state.fatal_unavailable || !state.admission_open {
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
        let revision = self.finish_store_write(&mut state, result)?;
        state.index_revision_high_water = revision;
        Ok(())
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
        if !self.worker_is_current(volume, owner, None)
            || !Self::authenticated_mount_matches(&state, volume)
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
        if !self.worker_is_current(volume, owner, Some(generation))
            || !Self::authenticated_mount_matches(&state, volume)
        {
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
                _ => match match begin_lazy_init_locked(
                    &mut state,
                    expected_runtime_epoch,
                    &self.publication_generation,
                ) {
                    Ok(decision) => decision,
                    Err(_) => {
                        if state.fatal_unavailable {
                            self.latch_process_fatal(&mut state);
                        }
                        return Err(FileIndexError::Unavailable);
                    }
                } {
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
                        let advanced = state.advance_revision_locked(&self.publication_generation);
                        if advanced.is_err() || advanced.ok() != Some(revision) {
                            self.latch_process_fatal(&mut state);
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
                    self.latch_process_fatal(&mut state);
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
                self.latch_process_fatal(&mut state);
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
        if state.mode != LifecycleMode::Active
            || state.fatal_unavailable
            || !state.admission_open
            || state.store.is_none()
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
            self.latch_process_fatal(&mut state);
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
                    let now = std::time::Instant::now();
                    let deadline = state
                        .volumes
                        .values()
                        .filter_map(|runtime| match runtime.calibration {
                            Calibration::Pending { deadline, .. } => Some(deadline),
                            Calibration::Idle | Calibration::Running { .. } => None,
                        })
                        .min();
                    if deadline.is_some_and(|deadline| deadline <= now) {
                        state.pending_root = state.active_root.clone();
                        if state.pending_root.is_some() {
                            break;
                        }
                    }
                    let timeout = deadline
                        .map(|deadline| deadline.saturating_duration_since(now))
                        .unwrap_or(std::time::Duration::from_millis(250));
                    let waited = coordinator
                        .signal
                        .wait_timeout(state, timeout)
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
                if !index.worker_is_current(&worker_volume, owner_id, None) {
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
                index.complete_volume_calibration(&worker_volume, owner_id)?;
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
        if worker_stop.load(Ordering::Acquire) || !self.worker_is_current(volume, owner, None) {
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
            if worker_stop.load(Ordering::Acquire) || !self.worker_is_current(volume, owner, None) {
                return Err(FileIndexError::Unavailable);
            }
            match scan_receiver.try_recv() {
                Ok(ScanMessage::Batch(batch)) => {
                    if let Some(previous) = final_batch.replace(batch) {
                        let mut state = self.state.lock().expect("file index lock poisoned");
                        if !self.worker_is_current(volume, owner, None)
                            || !Self::authenticated_mount_matches(&state, volume)
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
        if worker_stop.load(Ordering::Acquire) || !self.worker_is_current(volume, owner, None) {
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
        if !self.worker_is_current(context.volume, context.owner, None) {
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
        if !self.worker_is_current(context.volume, context.owner, None)
            || !Self::authenticated_mount_matches(&state, context.volume)
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
                    || !self.worker_is_current(context.volume, context.owner, None),
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
        events: &[windows_backend::StructuredEvent],
    ) -> Result<(), FileIndexError> {
        if !self.worker_is_current(volume, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        let events = filter_replay_events(&volume.identity, events, exclusions);
        if events.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(volume, owner, Some(generation))
            || !Self::authenticated_mount_matches(&state, volume)
            || state.fatal_unavailable
            || !state.admission_open
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
                    || !self.worker_is_current(volume, owner, Some(generation)),
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
    ) -> Result<(), FileIndexError> {
        let generation = self
            .workers
            .lock()
            .expect("file index worker lock poisoned")
            .by_volume
            .get(&volume.identity)
            .filter(|worker| {
                worker.owner == owner
                    && worker.mount_point == volume.mount_point
                    && !worker.failed
                    && !worker.stop.load(Ordering::Acquire)
            })
            .map(|worker| worker.generation.load(Ordering::Acquire))
            .filter(|generation| *generation != 0)
            .ok_or(FileIndexError::Unavailable)?;
        if !self.worker_is_current(volume, owner, Some(generation)) {
            return Err(FileIndexError::Unavailable);
        }
        let mut state = self.state.lock().expect("file index lock poisoned");
        if !self.worker_is_current(volume, owner, Some(generation))
            || !Self::authenticated_mount_matches(&state, volume)
        {
            return Err(FileIndexError::Unavailable);
        }
        state.quarantined_volumes.remove(&volume.identity);
        Ok(())
    }

    fn handle_worker_failure(&self, volume: &FixedVolume, owner: u64) {
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
        let _ = self.mark_fixed_volume_dirty(volume);
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
