use std::{
    fs, io,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use icu_casemap::CaseMapper;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

mod store;

use store::{ordinal_sort_identity, Store, StoreError, StoreQueryResult};

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
            atomic::{AtomicU64, Ordering},
            Arc, Barrier,
        },
        thread,
    };

    use super::{
        authenticate_app_data_root, begin_lazy_init_locked, fold_name, open_store,
        validate_index_path_shape, AdmissionError, FileCategory, FileIndex, FileIndexError,
        FileIndexStatus, FileSort, IndexState, LazyInitDecision, LifecycleMode, QuerySpec,
        FOLD_ALGORITHM_ID,
    };
    use icu_casemap::CaseMapper;
    use rusqlite::Connection;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    use super::store::Store;
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
        let index = FileIndex::default();
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
                    let store = Store::open_in_memory_for_test("identity-a").unwrap();
                    store.remove_metadata_for_test();
                    Ok((store, 0, None))
                },
                |store, spec| {
                    query_calls.set(query_calls.get() + 1);
                    store.query(spec, &[])
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
    let revision = match identity_change {
        Some((_, revision)) => revision,
        None => store
            .index_revision()
            .map_err(|_| FileIndexError::Unavailable)?,
    };
    Ok((
        store,
        revision,
        identity_change.map(|(previous, _)| previous),
    ))
}

pub(crate) struct FileIndex {
    state: Mutex<IndexState>,
    publication_runtime_epoch: AtomicU64,
    publication_generation: AtomicU64,
}

impl Default for FileIndex {
    fn default() -> Self {
        Self {
            state: Mutex::new(IndexState::default()),
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

    pub(crate) fn search(
        &self,
        app_data_dir: &Path,
        spec: QuerySpec,
        expected_runtime_epoch: u64,
    ) -> Result<FileSearchBatch, FileIndexError> {
        self.search_with(
            app_data_dir,
            spec,
            expected_runtime_epoch,
            authenticate_app_data_root,
            open_store,
            |store, spec| store.query(spec, &[]),
        )
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
            let opened = authenticate_root(app_data_dir).and_then(|path| open(&path));
            let mut state = self.state.lock().expect("file index lock poisoned");
            if state.mode != (LifecycleMode::Opening { owner })
                || state.runtime_epoch != expected_runtime_epoch
            {
                return Err(FileIndexError::Unavailable);
            }
            match opened {
                Ok((store, revision, previous_revision)) => {
                    state.store = Some(store);
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
        let result = match state.store.as_mut() {
            Some(store) => query_store(store, &spec),
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
        Ok(FileSearchBatch {
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
        })
    }
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
