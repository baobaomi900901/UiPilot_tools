#![cfg(windows)]

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, FileTimes, OpenOptions};
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use everything_ipc::client::EverythingClient;
use everything_ipc::protocol::{
    EverythingQueryResult, EverythingQuerySpec, EverythingResultItem, EverythingSort,
};
use icu_casemap::CaseMapper;
use unicode_normalization::UnicodeNormalization;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const REQUEST_NAME: u32 = 0x0000_0001;
const REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
const REQUEST_ATTRIBUTES: u32 = 0x0000_0100;
const REQUEST_FLAGS: u32 =
    REQUEST_NAME | REQUEST_FULL_PATH_AND_NAME | REQUEST_DATE_MODIFIED | REQUEST_ATTRIBUTES;
const SORT_DATE_MODIFIED_DESCENDING: u32 = 14;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
const PAGE_SIZE: usize = 256;
const PAGE_OVERLAP: usize = 64;
const VISIBLE_LIMIT: usize = 200;
const MAX_PAGES: usize = 64;
const MAX_TIE_ENTRIES: usize = 16_384;
const MAX_ENTRY_MEMORY: usize = 32 * 1024 * 1024;
const QUERY_DEADLINE: Duration = Duration::from_secs(1);
const TRANSACTION_DEADLINE: Duration = Duration::from_secs(3);
const PROCESS_EXIT_DEADLINE: Duration = Duration::from_secs(5);
const INDEX_POLL_INTERVAL: Duration = Duration::from_millis(50);
const TIE_MTIME_SECONDS: u64 = 1_700_000_000;
const PRIORITY_MTIME_SECONDS: u64 = TIE_MTIME_SECONDS + 50;
const INSERT_MTIME_SECONDS: u64 = TIE_MTIME_SECONDS + 100;

static HARNESS_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct IsolatedEverything {
    root: PathBuf,
    indexed_root: PathBuf,
    config_path: PathBuf,
    database_path: PathBuf,
    executable: PathBuf,
    instance: String,
    child: Option<Child>,
    client: Option<EverythingClient>,
}

impl IsolatedEverything {
    fn prepare() -> Result<Self, Box<dyn Error>> {
        let sequence = next_harness_sequence();
        let instance = format!("UiPilotSemantic_{}_{}", std::process::id(), sequence);
        let temp_parent = fs::canonicalize(std::env::temp_dir())?;
        let root = temp_parent.join(format!("uipilot-everything-semantic-{instance}"));
        if root.exists() {
            return Err("isolated Everything temp root already exists".into());
        }
        fs::create_dir(&root)?;
        let indexed_root = root.join("indexed-tree");
        fs::create_dir(&indexed_root)?;
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let executable =
            fs::canonicalize(manifest_dir.join(r"..\..\third-party\everything\Everything.exe"))?;
        Ok(Self {
            config_path: root.join("Everything-test.ini"),
            database_path: root.join("Everything-test.db"),
            root,
            indexed_root,
            executable,
            instance,
            child: None,
            client: None,
        })
    }

    fn start(&mut self, deadline: Instant) -> Result<(), Box<dyn Error>> {
        self.write_isolated_config()?;
        let child = self.base_command().arg("-startup").spawn()?;
        self.child = Some(child);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Everything startup deadline elapsed".into());
        }
        self.client = Some(EverythingClient::connect(
            &self.instance,
            remaining.min(Duration::from_secs(10)),
        )?);
        Ok(())
    }

    fn client(&self) -> &EverythingClient {
        self.client
            .as_ref()
            .expect("Everything harness not started")
    }

    fn query(
        &self,
        search: &str,
        offset: u32,
        max_results: u32,
        deadline: Instant,
    ) -> Result<EverythingQueryResult, Box<dyn Error>> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("query transaction deadline elapsed".into());
        }
        Ok(self.client().query(EverythingQuerySpec {
            search: search.encode_utf16().collect(),
            offset,
            max_results,
            request_flags: REQUEST_FLAGS,
            sort: EverythingSort::DateModifiedDescending,
            deadline: Instant::now() + remaining.min(QUERY_DEADLINE),
        })?)
    }

    fn wait_for_exact_total(
        &self,
        search: &str,
        expected_total: u32,
        deadline: Instant,
    ) -> Result<(), Box<dyn Error>> {
        loop {
            if Instant::now() >= deadline {
                return Err(format!(
                    "isolated Everything index did not reach exact total {expected_total}"
                )
                .into());
            }
            match self.query(search, 0, 1, deadline) {
                Ok(result) if result.total == expected_total => return Ok(()),
                Ok(result) if result.total > expected_total => {
                    return Err("isolated Everything index contains unexpected entries".into());
                }
                Ok(_) | Err(_) => thread::sleep(INDEX_POLL_INTERVAL),
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        self.client.take();
        let mut first_error = None;
        match self.base_command().arg("-exit").spawn() {
            Ok(mut exit_process) => {
                if let Err(error) = wait_or_kill(&mut exit_process, Duration::from_secs(2)) {
                    first_error = Some(error);
                }
            }
            Err(error) => first_error = Some(error),
        }
        if let Some(mut child) = self.child.take() {
            if let Err(error) = wait_or_kill(&mut child, PROCESS_EXIT_DEADLINE) {
                first_error.get_or_insert(error);
            }
        }
        if let Err(error) = self.remove_owned_temp_root() {
            first_error.get_or_insert(error);
        }
        if let Some(error) = first_error {
            return Err(error.into());
        }
        Ok(())
    }

    fn base_command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .arg("-instance")
            .arg(&self.instance)
            .arg("-config")
            .arg(&self.config_path)
            .arg("-db")
            .arg(&self.database_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        command
    }

    fn write_isolated_config(&self) -> Result<(), Box<dyn Error>> {
        let folder = self
            .indexed_root
            .to_str()
            .ok_or("test folder path is not valid Unicode")?;
        if folder.contains(',') {
            return Err("test folder path cannot be represented by Everything folders INI".into());
        }
        let lines = [
            "[Everything]".to_owned(),
            "run_in_background=1".to_owned(),
            "show_tray_icon=0".to_owned(),
            "show_in_taskbar=0".to_owned(),
            "check_for_updates_on_startup=0".to_owned(),
            "beta_updates=0".to_owned(),
            "http_server_enabled=0".to_owned(),
            "http_server_logging_enabled=0".to_owned(),
            "etp_server_enabled=0".to_owned(),
            "etp_server_logging_enabled=0".to_owned(),
            "auto_include_fixed_volumes=0".to_owned(),
            "auto_include_removable_volumes=0".to_owned(),
            "auto_include_fixed_refs_volumes=0".to_owned(),
            "auto_include_removable_refs_volumes=0".to_owned(),
            "ntfs_volume_guids=".to_owned(),
            "ntfs_volume_paths=".to_owned(),
            "ntfs_volume_roots=".to_owned(),
            "ntfs_volume_includes=".to_owned(),
            "ntfs_volume_load_recent_changes=".to_owned(),
            "ntfs_volume_include_onlys=".to_owned(),
            "ntfs_volume_monitors=".to_owned(),
            "refs_volume_guids=".to_owned(),
            "refs_volume_paths=".to_owned(),
            "refs_volume_roots=".to_owned(),
            "refs_volume_includes=".to_owned(),
            "refs_volume_load_recent_changes=".to_owned(),
            "refs_volume_include_onlys=".to_owned(),
            "refs_volume_monitors=".to_owned(),
            "filelists=".to_owned(),
            format!("folders={folder}"),
            "folder_monitor_changes=1".to_owned(),
            "folder_buffer_size_list=65536".to_owned(),
            "folder_rescan_if_full_list=1".to_owned(),
            "folder_update_types=0".to_owned(),
            "exclude_folders=".to_owned(),
            "exclude_files=".to_owned(),
            "match_case=0".to_owned(),
            "match_whole_word=0".to_owned(),
            "match_path=0".to_owned(),
            "regex=0".to_owned(),
        ];
        let config = lines.join("\r\n") + "\r\n";
        for required in [
            "http_server_enabled=0",
            "etp_server_enabled=0",
            "auto_include_fixed_volumes=0",
            "auto_include_fixed_refs_volumes=0",
            "ntfs_volume_paths=",
            "refs_volume_paths=",
            "folder_monitor_changes=1",
        ] {
            assert!(config.contains(required));
        }
        fs::write(&self.config_path, config)?;
        Ok(())
    }

    fn remove_owned_temp_root(&self) -> io::Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        let temp_parent = fs::canonicalize(std::env::temp_dir())?;
        let parent = self
            .root
            .parent()
            .ok_or_else(|| io::Error::other("test temp root has no parent"))?;
        let leaf = self
            .root
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| io::Error::other("test temp root has invalid leaf"))?;
        if fs::canonicalize(parent)? != temp_parent
            || !leaf.starts_with("uipilot-everything-semantic-UiPilotSemantic_")
        {
            return Err(io::Error::other("refusing to remove unowned temp root"));
        }
        fs::remove_dir_all(&self.root)
    }
}

impl Drop for IsolatedEverything {
    fn drop(&mut self) {
        if self.root.exists() {
            let _ = self.shutdown();
        }
    }
}

fn wait_or_kill(child: &mut Child, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait()?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn next_harness_sequence() -> u64 {
    let mut current = HARNESS_SEQUENCE.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .expect("Everything semantic harness sequence exhausted");
        match HARNESS_SEQUENCE.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

fn fold_name(value: &str) -> String {
    let first_nfc: String = value.nfc().collect();
    let folded = CaseMapper::new().fold_string(&first_nfc);
    folded.nfc().collect()
}

fn canonical_ordinal(path: &Path) -> Result<Vec<u16>, PaginationError> {
    if !path.is_absolute() {
        return Err(PaginationError::InvalidPage);
    }
    let canonical = fs::canonicalize(path).map_err(|_| PaginationError::ConcurrentMutation)?;
    let text = canonical
        .to_str()
        .ok_or(PaginationError::InvalidPage)?
        .replace('/', "\\");
    let normalized = text
        .strip_prefix(r"\\?\UNC\")
        .map(|suffix| format!(r"\\{suffix}"))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_owned))
        .unwrap_or(text);
    Ok(normalized.encode_utf16().collect())
}

fn canonical_path_set(
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<HashSet<Vec<u16>>, Box<dyn Error>> {
    paths
        .into_iter()
        .map(|path| canonical_ordinal(&path).map_err(|error| format!("{error:?}").into()))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct EntryKey {
    canonical_path: Vec<u16>,
    kind: u8,
    attributes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntryRecord {
    key: EntryKey,
    modified_filetime: Option<u64>,
}

#[derive(Clone)]
struct QueryPage {
    offset: u32,
    total: u32,
    request_flags: u32,
    sort_type: u32,
    items: Vec<EverythingResultItem>,
}

trait PageSource {
    fn set_trace_context(&mut self, _attempt: usize, _pass: usize) {}

    fn fetch(
        &mut self,
        offset: u32,
        max_results: u32,
        deadline: Instant,
    ) -> Result<QueryPage, PaginationError>;
}

struct RealPageSource<'a> {
    harness: &'a IsolatedEverything,
    search: &'a str,
}

impl PageSource for RealPageSource<'_> {
    fn fetch(
        &mut self,
        offset: u32,
        max_results: u32,
        deadline: Instant,
    ) -> Result<QueryPage, PaginationError> {
        let result = self
            .harness
            .query(self.search, offset, max_results, deadline)
            .map_err(|_| PaginationError::QueryFailed)?;
        Ok(QueryPage {
            offset,
            total: result.total,
            request_flags: result.request_flags,
            sort_type: result.sort_type,
            items: result.items,
        })
    }
}

#[derive(Clone, Debug)]
enum RealMutation {
    Insert {
        path: PathBuf,
        marker: String,
    },
    Delete {
        path: PathBuf,
        marker: String,
    },
    RenameHardlink {
        source: PathBuf,
        linked_sibling: PathBuf,
        source_marker: String,
        destination: PathBuf,
        destination_marker: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RealMutationEvidence {
    first_page_captured: bool,
    verified_first_page_targets: usize,
    applied: usize,
    observed: usize,
}

enum RealMutationState {
    Pending(RealMutation),
    Applying,
    Applied,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutationStateTag {
    Pending,
    Applying,
    Applied,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceOutcome {
    NotRun,
    Passed,
    ConcurrentMutation,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FetchTraceEvent {
    attempt: usize,
    pass: usize,
    offset: u32,
    state_before: MutationStateTag,
    state_after: MutationStateTag,
    preflight: TraceOutcome,
    apply: TraceOutcome,
    visibility: TraceOutcome,
    page_total: Option<u32>,
    item_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutationExecutionFailure {
    Apply,
    Visibility,
}

struct RealMutatingPageSource<'a> {
    harness: &'a IsolatedEverything,
    search: &'a str,
    baseline_total: u32,
    mutation_state: RealMutationState,
    first_page: Option<QueryPage>,
    post_mutation_first_page: Option<QueryPage>,
    evidence: RealMutationEvidence,
    trace_context: (usize, usize),
    fetch_trace: Vec<FetchTraceEvent>,
}

impl RealMutationState {
    fn tag(&self) -> MutationStateTag {
        match self {
            Self::Pending(_) => MutationStateTag::Pending,
            Self::Applying => MutationStateTag::Applying,
            Self::Applied => MutationStateTag::Applied,
            Self::Failed => MutationStateTag::Failed,
        }
    }
}

impl PageSource for RealMutatingPageSource<'_> {
    fn set_trace_context(&mut self, attempt: usize, pass: usize) {
        self.trace_context = (attempt, pass);
    }

    fn fetch(
        &mut self,
        offset: u32,
        max_results: u32,
        deadline: Instant,
    ) -> Result<QueryPage, PaginationError> {
        let state_before = self.mutation_state.tag();
        let mut preflight = TraceOutcome::NotRun;
        let mut apply = TraceOutcome::NotRun;
        let mut visibility = TraceOutcome::NotRun;
        if matches!(self.mutation_state, RealMutationState::Failed) {
            self.record_fetch(offset, state_before, preflight, apply, visibility, None);
            return Err(PaginationError::MutationFailed);
        }
        if matches!(self.mutation_state, RealMutationState::Applying) {
            self.record_fetch(offset, state_before, preflight, apply, visibility, None);
            return Err(PaginationError::MutationFailed);
        }
        if offset != 0 {
            if let RealMutationState::Pending(mutation) = &self.mutation_state {
                let first_page = match self.first_page.as_ref() {
                    Some(page) => page,
                    None => {
                        preflight = TraceOutcome::ConcurrentMutation;
                        self.record_fetch(offset, state_before, preflight, apply, visibility, None);
                        return Err(PaginationError::ConcurrentMutation);
                    }
                };
                let verified_targets = match mutation.preflight(first_page) {
                    Ok(targets) => {
                        preflight = TraceOutcome::Passed;
                        targets
                    }
                    Err(PaginationError::ConcurrentMutation) => {
                        preflight = TraceOutcome::ConcurrentMutation;
                        self.record_fetch(offset, state_before, preflight, apply, visibility, None);
                        return Err(PaginationError::ConcurrentMutation);
                    }
                    Err(error) => {
                        preflight = TraceOutcome::Failed;
                        self.record_fetch(offset, state_before, preflight, apply, visibility, None);
                        return Err(error);
                    }
                };
                let mutation = match std::mem::replace(
                    &mut self.mutation_state,
                    RealMutationState::Applying,
                ) {
                    RealMutationState::Pending(mutation) => mutation,
                    _ => return Err(PaginationError::MutationFailed),
                };
                match mutation.apply(self.harness, self.search, self.baseline_total, deadline) {
                    Ok(()) => {
                        apply = TraceOutcome::Passed;
                        visibility = TraceOutcome::Passed;
                    }
                    Err(MutationExecutionFailure::Apply) => {
                        apply = TraceOutcome::Failed;
                        self.mutation_state = RealMutationState::Failed;
                        self.record_fetch(offset, state_before, preflight, apply, visibility, None);
                        return Err(PaginationError::MutationFailed);
                    }
                    Err(MutationExecutionFailure::Visibility) => {
                        apply = TraceOutcome::Passed;
                        visibility = TraceOutcome::Failed;
                        self.mutation_state = RealMutationState::Failed;
                        self.record_fetch(offset, state_before, preflight, apply, visibility, None);
                        return Err(PaginationError::MutationFailed);
                    }
                }
                self.mutation_state = RealMutationState::Applied;
                self.evidence.verified_first_page_targets = verified_targets;
                self.evidence.applied += 1;
                self.evidence.observed += 1;
            }
        }

        let result = match self
            .harness
            .query(self.search, offset, max_results, deadline)
        {
            Ok(result) => result,
            Err(_) => {
                self.record_fetch(offset, state_before, preflight, apply, visibility, None);
                return Err(PaginationError::QueryFailed);
            }
        };
        let page = QueryPage {
            offset,
            total: result.total,
            request_flags: result.request_flags,
            sort_type: result.sort_type,
            items: result.items,
        };
        if offset == 0 {
            match self.mutation_state {
                RealMutationState::Pending(_) => {
                    self.first_page = Some(page.clone());
                    self.evidence.first_page_captured = true;
                }
                RealMutationState::Applied if self.post_mutation_first_page.is_none() => {
                    self.post_mutation_first_page = Some(page.clone());
                }
                RealMutationState::Applying | RealMutationState::Failed => {
                    self.record_fetch(
                        offset,
                        state_before,
                        preflight,
                        apply,
                        visibility,
                        Some(&page),
                    );
                    return Err(PaginationError::MutationFailed);
                }
                RealMutationState::Applied => {}
            }
        }
        self.record_fetch(
            offset,
            state_before,
            preflight,
            apply,
            visibility,
            Some(&page),
        );
        Ok(page)
    }
}

impl RealMutatingPageSource<'_> {
    fn record_fetch(
        &mut self,
        offset: u32,
        state_before: MutationStateTag,
        preflight: TraceOutcome,
        apply: TraceOutcome,
        visibility: TraceOutcome,
        page: Option<&QueryPage>,
    ) {
        self.fetch_trace.push(FetchTraceEvent {
            attempt: self.trace_context.0,
            pass: self.trace_context.1,
            offset,
            state_before,
            state_after: self.mutation_state.tag(),
            preflight,
            apply,
            visibility,
            page_total: page.map(|page| page.total),
            item_count: page.map(|page| page.items.len()),
        });
    }
}

impl RealMutation {
    fn preflight(&self, first_page: &QueryPage) -> Result<usize, PaginationError> {
        match self {
            Self::Insert { .. } => Ok(0),
            Self::Delete { path, .. } => {
                require_path_in_first_page(path, first_page)?;
                Ok(1)
            }
            Self::RenameHardlink {
                source,
                linked_sibling,
                ..
            } => {
                require_path_in_first_page(source, first_page)?;
                require_path_in_first_page(linked_sibling, first_page)?;
                Ok(2)
            }
        }
    }

    fn apply(
        &self,
        harness: &IsolatedEverything,
        common_search: &str,
        baseline_total: u32,
        deadline: Instant,
    ) -> Result<(), MutationExecutionFailure> {
        match self {
            Self::Insert { path, marker } => {
                fs::write(path, b"inserted pagination mutation")
                    .map_err(|_| MutationExecutionFailure::Apply)?;
                set_modified(path, UNIX_EPOCH + Duration::from_secs(INSERT_MTIME_SECONDS))
                    .map_err(|_| MutationExecutionFailure::Apply)?;
                wait_for_mutation_visibility(
                    harness,
                    common_search,
                    baseline_total
                        .checked_add(1)
                        .ok_or(MutationExecutionFailure::Apply)?,
                    &[(marker.as_str(), 1)],
                    deadline,
                )
                .map_err(|_| MutationExecutionFailure::Visibility)?;
                let snapshot = wait_for_stable_ordered_snapshot(
                    harness,
                    common_search,
                    deadline,
                    "insert_post_visibility",
                )
                .map_err(|_| MutationExecutionFailure::Visibility)?;
                let inserted_key =
                    canonical_ordinal(path).map_err(|_| MutationExecutionFailure::Visibility)?;
                if !snapshot_contains_path(&snapshot.ordered_keys, &inserted_key)
                    || !snapshot_contains_path(&snapshot.first_page_keys, &inserted_key)
                {
                    return Err(MutationExecutionFailure::Visibility);
                }
                Ok(())
            }
            Self::Delete { path, marker } => {
                let deleted_key =
                    canonical_ordinal(path).map_err(|_| MutationExecutionFailure::Apply)?;
                fs::remove_file(path).map_err(|_| MutationExecutionFailure::Apply)?;
                wait_for_mutation_visibility(
                    harness,
                    common_search,
                    baseline_total
                        .checked_sub(1)
                        .ok_or(MutationExecutionFailure::Apply)?,
                    &[(marker.as_str(), 0)],
                    deadline,
                )
                .map_err(|_| MutationExecutionFailure::Visibility)?;
                let snapshot = wait_for_stable_ordered_snapshot(
                    harness,
                    common_search,
                    deadline,
                    "delete_post_visibility",
                )
                .map_err(|_| MutationExecutionFailure::Visibility)?;
                if snapshot_contains_path(&snapshot.ordered_keys, &deleted_key) {
                    return Err(MutationExecutionFailure::Visibility);
                }
                Ok(())
            }
            Self::RenameHardlink {
                source,
                linked_sibling,
                source_marker,
                destination,
                destination_marker,
            } => {
                let old_source_key =
                    canonical_ordinal(source).map_err(|_| MutationExecutionFailure::Apply)?;
                let sibling_key = canonical_ordinal(linked_sibling)
                    .map_err(|_| MutationExecutionFailure::Apply)?;
                fs::rename(source, destination).map_err(|_| MutationExecutionFailure::Apply)?;
                wait_for_mutation_visibility(
                    harness,
                    common_search,
                    baseline_total,
                    &[
                        (source_marker.as_str(), 0),
                        (destination_marker.as_str(), 1),
                    ],
                    deadline,
                )
                .map_err(|_| MutationExecutionFailure::Visibility)?;
                let snapshot = wait_for_stable_ordered_snapshot(
                    harness,
                    common_search,
                    deadline,
                    "rename_post_visibility",
                )
                .map_err(|_| MutationExecutionFailure::Visibility)?;
                let destination_key = canonical_ordinal(destination)
                    .map_err(|_| MutationExecutionFailure::Visibility)?;
                if snapshot_contains_path(&snapshot.ordered_keys, &old_source_key)
                    || !snapshot_contains_path(&snapshot.ordered_keys, &destination_key)
                    || !snapshot_contains_path(&snapshot.ordered_keys, &sibling_key)
                {
                    return Err(MutationExecutionFailure::Visibility);
                }
                Ok(())
            }
        }
    }
}

fn require_path_in_first_page(
    expected_path: &Path,
    first_page: &QueryPage,
) -> Result<(), PaginationError> {
    let expected = canonical_ordinal(expected_path)?;
    let found = first_page.items.iter().any(|item| {
        canonical_ordinal(Path::new(&item.full_path))
            .map(|actual| actual == expected)
            .unwrap_or(false)
    });
    found
        .then_some(())
        .ok_or(PaginationError::ConcurrentMutation)
}

fn wait_for_mutation_visibility(
    harness: &IsolatedEverything,
    common_search: &str,
    expected_total: u32,
    marker_totals: &[(&str, u32)],
    deadline: Instant,
) -> Result<(), PaginationError> {
    wait_until_exact_total(harness, common_search, expected_total, deadline)?;
    for (marker, marker_total) in marker_totals {
        wait_until_exact_total(harness, marker, *marker_total, deadline)?;
    }
    Ok(())
}

fn wait_until_exact_total(
    harness: &IsolatedEverything,
    search: &str,
    expected_total: u32,
    deadline: Instant,
) -> Result<(), PaginationError> {
    loop {
        if Instant::now() >= deadline {
            return Err(PaginationError::QueryFailed);
        }
        match harness.query(search, 0, 1, deadline) {
            Ok(result) if result.total == expected_total => return Ok(()),
            Ok(_) | Err(_) => thread::sleep(INDEX_POLL_INTERVAL),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OrderedSnapshot {
    total: u32,
    request_flags: u32,
    sort_type: u32,
    ordered_keys: Vec<EntryKey>,
    first_page_keys: Vec<EntryKey>,
}

fn wait_for_stable_ordered_snapshot(
    harness: &IsolatedEverything,
    search: &str,
    deadline: Instant,
    stage: &'static str,
) -> Result<OrderedSnapshot, PaginationError> {
    let mut previous = None;
    let mut captures = 0usize;
    let mut mismatches = 0usize;
    loop {
        if Instant::now() >= deadline {
            eprintln!(
                "ORDERED_SNAPSHOT_TRACE stage={stage} captures={captures} mismatches={mismatches} outcome=Deadline"
            );
            return Err(PaginationError::DeadlineExceeded);
        }
        let current = match capture_ordered_snapshot(harness, search, deadline) {
            Ok(snapshot) => snapshot,
            Err(PaginationError::ConcurrentMutation) => {
                mismatches += 1;
                previous = None;
                continue;
            }
            Err(error) => return Err(error),
        };
        captures += 1;
        if previous.as_ref() == Some(&current) {
            eprintln!(
                "ORDERED_SNAPSHOT_TRACE stage={stage} captures={captures} mismatches={mismatches} outcome=Stable total={} entries={} first_page={}",
                current.total,
                current.ordered_keys.len(),
                current.first_page_keys.len()
            );
            return Ok(current);
        }
        if previous.is_some() {
            mismatches += 1;
        }
        previous = Some(current);
    }
}

fn capture_ordered_snapshot(
    harness: &IsolatedEverything,
    search: &str,
    deadline: Instant,
) -> Result<OrderedSnapshot, PaginationError> {
    let mut expected_total = None;
    let mut expected_flags = None;
    let mut expected_sort = None;
    let mut ordered_keys = Vec::new();
    let mut first_page_keys = Vec::new();
    let mut seen = HashSet::new();
    let mut offset = 0usize;
    let mut pages = 0usize;
    let mut memory_used = 0usize;

    loop {
        if Instant::now() >= deadline {
            return Err(PaginationError::DeadlineExceeded);
        }
        pages = pages
            .checked_add(1)
            .ok_or(PaginationError::TieGroupTooLarge)?;
        if pages > MAX_PAGES {
            return Err(PaginationError::TieGroupTooLarge);
        }
        let page = harness
            .query(
                search,
                u32::try_from(offset).map_err(|_| PaginationError::TieGroupTooLarge)?,
                PAGE_SIZE as u32,
                deadline,
            )
            .map_err(|_| PaginationError::QueryFailed)?;
        if page.request_flags != REQUEST_FLAGS
            || page.sort_type != SORT_DATE_MODIFIED_DESCENDING
            || page.items.len() > PAGE_SIZE
        {
            return Err(PaginationError::ConcurrentMutation);
        }
        match expected_total {
            Some(total) if total != page.total => {
                return Err(PaginationError::ConcurrentMutation);
            }
            None => {
                expected_total = Some(page.total);
                expected_flags = Some(page.request_flags);
                expected_sort = Some(page.sort_type);
            }
            _ => {}
        }
        let page_keys = page
            .items
            .iter()
            .map(entry_record)
            .map(|record| record.map(|record| record.key))
            .collect::<Result<Vec<_>, _>>()?;
        if offset == 0 {
            first_page_keys = page_keys.clone();
            append_snapshot_keys(&mut ordered_keys, &mut seen, &mut memory_used, page_keys)?;
        } else {
            if page_keys.len() < PAGE_OVERLAP || ordered_keys.len() < PAGE_OVERLAP {
                return Err(PaginationError::ConcurrentMutation);
            }
            if ordered_keys[ordered_keys.len() - PAGE_OVERLAP..] != page_keys[..PAGE_OVERLAP] {
                return Err(PaginationError::ConcurrentMutation);
            }
            append_snapshot_keys(
                &mut ordered_keys,
                &mut seen,
                &mut memory_used,
                page_keys.into_iter().skip(PAGE_OVERLAP),
            )?;
        }
        if ordered_keys.len() > MAX_TIE_ENTRIES || memory_used > MAX_ENTRY_MEMORY {
            return Err(PaginationError::TieGroupTooLarge);
        }
        let total = expected_total.ok_or(PaginationError::InvalidPage)? as usize;
        if ordered_keys.len() == total {
            return Ok(OrderedSnapshot {
                total: total as u32,
                request_flags: expected_flags.ok_or(PaginationError::InvalidPage)?,
                sort_type: expected_sort.ok_or(PaginationError::InvalidPage)?,
                ordered_keys,
                first_page_keys,
            });
        }
        if ordered_keys.len() > total || page.items.is_empty() {
            return Err(PaginationError::ConcurrentMutation);
        }
        offset = ordered_keys
            .len()
            .checked_sub(PAGE_OVERLAP)
            .ok_or(PaginationError::ConcurrentMutation)?;
    }
}

fn append_snapshot_keys<I>(
    ordered_keys: &mut Vec<EntryKey>,
    seen: &mut HashSet<EntryKey>,
    memory_used: &mut usize,
    keys: I,
) -> Result<(), PaginationError>
where
    I: IntoIterator<Item = EntryKey>,
{
    for key in keys {
        if !seen.insert(key.clone()) {
            return Err(PaginationError::ConcurrentMutation);
        }
        *memory_used = memory_used
            .checked_add(key.canonical_path.len().saturating_mul(2))
            .and_then(|value| value.checked_add(8))
            .ok_or(PaginationError::TieGroupTooLarge)?;
        ordered_keys.push(key);
    }
    Ok(())
}

fn snapshot_contains_path(keys: &[EntryKey], canonical_path: &[u16]) -> bool {
    keys.iter().any(|key| key.canonical_path == canonical_path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaginationError {
    ConcurrentMutation,
    TieGroupTooLarge,
    QueryFailed,
    InvalidPage,
    DeadlineExceeded,
    MutationFailed,
}

impl fmt::Display for PaginationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ConcurrentMutation => "concurrent Everything pagination mutation",
            Self::TieGroupTooLarge => "Everything cutoff tie group exceeded resource limits",
            Self::QueryFailed => "Everything page query failed",
            Self::InvalidPage => "Everything page was invalid",
            Self::DeadlineExceeded => "Everything pagination deadline exceeded",
            Self::MutationFailed => "Everything pagination mutation failed",
        })
    }
}

impl Error for PaginationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PassResult {
    total: u32,
    ordered_tie_entries: Vec<EntryRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StableCutoff {
    total: u32,
    ordered_tie_entries: Vec<EntryRecord>,
    visible: Vec<EntryKey>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PaginationTrace {
    attempts: usize,
    rejected_passes: usize,
    completed_passes: usize,
    events: Vec<PassTraceEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PassExitReason {
    CompleteAtTieBoundary,
    CompleteAtTotal,
    TotalMismatch,
    OverlapTooShort,
    OverlapMismatch,
    DuplicateEntry,
    Canonicalization,
    Deadline,
    PageContract,
    FetchConcurrentMutation,
    FetchError,
    ResourceLimit,
    InvalidPage,
    EmptyOrExcessEntries,
    OffsetUnderflow,
    DoublePassMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PassTraceEvent {
    attempt: usize,
    pass: usize,
    reason: PassExitReason,
    expected_total: Option<u32>,
    entries: usize,
    pages: usize,
}

fn read_stable_cutoff<S: PageSource>(
    source: &mut S,
    transaction_deadline: Instant,
) -> Result<StableCutoff, PaginationError> {
    read_stable_cutoff_with_trace(source, transaction_deadline).map(|(stable, _)| stable)
}

fn read_stable_cutoff_with_trace<S: PageSource>(
    source: &mut S,
    transaction_deadline: Instant,
) -> Result<(StableCutoff, PaginationTrace), PaginationError> {
    let mut trace = PaginationTrace::default();
    for attempt in 1..=2 {
        trace.attempts += 1;
        let first = match collect_cutoff_pass(source, transaction_deadline, &mut trace, attempt, 1)
        {
            Ok(pass) => {
                trace.completed_passes += 1;
                pass
            }
            Err(PaginationError::ConcurrentMutation) => {
                trace.rejected_passes += 1;
                continue;
            }
            Err(error) => {
                eprintln!("PAGINATION_TRACE {trace:?}");
                return Err(error);
            }
        };
        let second = match collect_cutoff_pass(source, transaction_deadline, &mut trace, attempt, 2)
        {
            Ok(pass) => {
                trace.completed_passes += 1;
                pass
            }
            Err(PaginationError::ConcurrentMutation) => {
                trace.rejected_passes += 1;
                continue;
            }
            Err(error) => {
                eprintln!("PAGINATION_TRACE {trace:?}");
                return Err(error);
            }
        };
        if first != second {
            trace.rejected_passes += 1;
            trace.events.push(PassTraceEvent {
                attempt,
                pass: 0,
                reason: PassExitReason::DoublePassMismatch,
                expected_total: Some(first.total),
                entries: first.ordered_tie_entries.len(),
                pages: 0,
            });
            continue;
        }
        let mut stable_order = first.ordered_tie_entries.clone();
        stable_order.sort_by(|left, right| {
            right
                .modified_filetime
                .cmp(&left.modified_filetime)
                .then_with(|| left.key.cmp(&right.key))
        });
        return Ok((
            StableCutoff {
                total: first.total,
                ordered_tie_entries: first.ordered_tie_entries,
                visible: stable_order
                    .into_iter()
                    .take(VISIBLE_LIMIT)
                    .map(|entry| entry.key)
                    .collect(),
            },
            trace,
        ));
    }
    eprintln!("PAGINATION_TRACE {trace:?}");
    Err(PaginationError::ConcurrentMutation)
}

fn collect_cutoff_pass<S: PageSource>(
    source: &mut S,
    transaction_deadline: Instant,
    trace: &mut PaginationTrace,
    attempt: usize,
    pass: usize,
) -> Result<PassResult, PaginationError> {
    let mut expected_total = None;
    let mut entries = Vec::<EntryRecord>::new();
    let mut seen = HashSet::<EntryKey>::new();
    let mut offset = 0usize;
    let mut page_count = 0usize;
    let mut memory_used = 0usize;

    macro_rules! fail {
        ($error:expr, $reason:expr) => {{
            trace.events.push(PassTraceEvent {
                attempt,
                pass,
                reason: $reason,
                expected_total,
                entries: entries.len(),
                pages: page_count,
            });
            return Err($error);
        }};
    }

    loop {
        if Instant::now() >= transaction_deadline {
            fail!(PaginationError::DeadlineExceeded, PassExitReason::Deadline);
        }
        page_count = match page_count.checked_add(1) {
            Some(count) => count,
            None => fail!(
                PaginationError::TieGroupTooLarge,
                PassExitReason::ResourceLimit
            ),
        };
        if page_count > MAX_PAGES {
            fail!(
                PaginationError::TieGroupTooLarge,
                PassExitReason::ResourceLimit
            );
        }
        let wire_offset = match u32::try_from(offset) {
            Ok(offset) => offset,
            Err(_) => fail!(
                PaginationError::TieGroupTooLarge,
                PassExitReason::ResourceLimit
            ),
        };
        source.set_trace_context(attempt, pass);
        let page = match source.fetch(wire_offset, PAGE_SIZE as u32, transaction_deadline) {
            Ok(page) => page,
            Err(PaginationError::ConcurrentMutation) => fail!(
                PaginationError::ConcurrentMutation,
                PassExitReason::FetchConcurrentMutation
            ),
            Err(error) => fail!(error, PassExitReason::FetchError),
        };
        if page.offset as usize != offset
            || page.request_flags != REQUEST_FLAGS
            || page.sort_type != SORT_DATE_MODIFIED_DESCENDING
            || page.items.len() > PAGE_SIZE
        {
            fail!(
                PaginationError::ConcurrentMutation,
                PassExitReason::PageContract
            );
        }
        match expected_total {
            Some(total) if total != page.total => {
                fail!(
                    PaginationError::ConcurrentMutation,
                    PassExitReason::TotalMismatch
                );
            }
            None => expected_total = Some(page.total),
            _ => {}
        }
        let page_records = match page
            .items
            .iter()
            .map(entry_record)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(records) => records,
            Err(PaginationError::ConcurrentMutation) => fail!(
                PaginationError::ConcurrentMutation,
                PassExitReason::Canonicalization
            ),
            Err(error) => fail!(error, PassExitReason::InvalidPage),
        };
        if offset == 0 {
            if let Err(error) =
                append_unique(&mut entries, &mut seen, &mut memory_used, page_records)
            {
                let reason = if error == PaginationError::ConcurrentMutation {
                    PassExitReason::DuplicateEntry
                } else {
                    PassExitReason::ResourceLimit
                };
                fail!(error, reason);
            }
        } else {
            if page_records.len() < PAGE_OVERLAP || entries.len() < PAGE_OVERLAP {
                fail!(
                    PaginationError::ConcurrentMutation,
                    PassExitReason::OverlapTooShort
                );
            }
            let expected_overlap = &entries[entries.len() - PAGE_OVERLAP..];
            if expected_overlap != &page_records[..PAGE_OVERLAP] {
                fail!(
                    PaginationError::ConcurrentMutation,
                    PassExitReason::OverlapMismatch
                );
            }
            if let Err(error) = append_unique(
                &mut entries,
                &mut seen,
                &mut memory_used,
                page_records.into_iter().skip(PAGE_OVERLAP),
            ) {
                let reason = if error == PaginationError::ConcurrentMutation {
                    PassExitReason::DuplicateEntry
                } else {
                    PassExitReason::ResourceLimit
                };
                fail!(error, reason);
            }
        }
        if entries.len() > MAX_TIE_ENTRIES || memory_used > MAX_ENTRY_MEMORY {
            fail!(
                PaginationError::TieGroupTooLarge,
                PassExitReason::ResourceLimit
            );
        }

        let total = match expected_total {
            Some(total) => total as usize,
            None => fail!(PaginationError::InvalidPage, PassExitReason::InvalidPage),
        };
        let tie_end = match cutoff_tie_end(&entries) {
            Ok(tie_end) => tie_end,
            Err(error) => fail!(error, PassExitReason::InvalidPage),
        };
        if let Some(tie_end) = tie_end {
            entries.truncate(tie_end);
            trace.events.push(PassTraceEvent {
                attempt,
                pass,
                reason: PassExitReason::CompleteAtTieBoundary,
                expected_total,
                entries: entries.len(),
                pages: page_count,
            });
            return Ok(PassResult {
                total: total as u32,
                ordered_tie_entries: entries,
            });
        }
        if entries.len() == total {
            trace.events.push(PassTraceEvent {
                attempt,
                pass,
                reason: PassExitReason::CompleteAtTotal,
                expected_total,
                entries: entries.len(),
                pages: page_count,
            });
            return Ok(PassResult {
                total: total as u32,
                ordered_tie_entries: entries,
            });
        }
        if entries.len() > total || page.items.is_empty() {
            fail!(
                PaginationError::ConcurrentMutation,
                PassExitReason::EmptyOrExcessEntries
            );
        }
        offset = match entries.len().checked_sub(PAGE_OVERLAP) {
            Some(offset) => offset,
            None => fail!(
                PaginationError::ConcurrentMutation,
                PassExitReason::OffsetUnderflow
            ),
        };
    }
}

fn append_unique<I>(
    entries: &mut Vec<EntryRecord>,
    seen: &mut HashSet<EntryKey>,
    memory_used: &mut usize,
    records: I,
) -> Result<(), PaginationError>
where
    I: IntoIterator<Item = EntryRecord>,
{
    for record in records {
        if !seen.insert(record.key.clone()) {
            return Err(PaginationError::ConcurrentMutation);
        }
        *memory_used = memory_used
            .checked_add(record.key.canonical_path.len().saturating_mul(2))
            .and_then(|value| value.checked_add(8))
            .ok_or(PaginationError::TieGroupTooLarge)?;
        entries.push(record);
    }
    Ok(())
}

fn cutoff_tie_end(entries: &[EntryRecord]) -> Result<Option<usize>, PaginationError> {
    if entries.len() <= VISIBLE_LIMIT {
        return Ok(None);
    }
    let cutoff = entries[VISIBLE_LIMIT - 1]
        .modified_filetime
        .ok_or(PaginationError::InvalidPage)?;
    for (index, entry) in entries.iter().enumerate().skip(VISIBLE_LIMIT) {
        if entry.modified_filetime != Some(cutoff) {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn entry_record(item: &EverythingResultItem) -> Result<EntryRecord, PaginationError> {
    let path = PathBuf::from(&item.full_path);
    Ok(EntryRecord {
        key: EntryKey {
            canonical_path: canonical_ordinal(&path)?,
            kind: u8::from(item.attributes & FILE_ATTRIBUTE_DIRECTORY != 0),
            attributes: item.attributes,
        },
        modified_filetime: item.modified_filetime,
    })
}

fn capture_real_pages<S: PageSource>(
    source: &mut S,
    deadline: Instant,
) -> Result<HashMap<u32, QueryPage>, PaginationError> {
    let mut pages = HashMap::new();
    let mut offset = 0usize;
    loop {
        let page = source.fetch(offset as u32, PAGE_SIZE as u32, deadline)?;
        let total = page.total as usize;
        let item_count = page.items.len();
        pages.insert(offset as u32, page);
        if offset + item_count >= total {
            return Ok(pages);
        }
        if item_count < PAGE_OVERLAP {
            return Err(PaginationError::ConcurrentMutation);
        }
        offset = offset
            .checked_add(item_count - PAGE_OVERLAP)
            .ok_or(PaginationError::TieGroupTooLarge)?;
    }
}

#[derive(Clone, Copy)]
enum InjectedDriftMode {
    DuplicatePage,
    MissingSentinel,
    NonOverlapDuplicate,
    ContinuousChange,
}

struct InjectedPageSource {
    pages: HashMap<u32, QueryPage>,
    first_page: QueryPage,
    mode: InjectedDriftMode,
    calls: usize,
}

impl PageSource for InjectedPageSource {
    fn fetch(
        &mut self,
        offset: u32,
        _max_results: u32,
        _deadline: Instant,
    ) -> Result<QueryPage, PaginationError> {
        self.calls += 1;
        let mut page = self
            .pages
            .get(&offset)
            .cloned()
            .ok_or(PaginationError::ConcurrentMutation)?;
        if offset == 0 {
            if matches!(self.mode, InjectedDriftMode::ContinuousChange)
                && self.calls.is_multiple_of(2)
            {
                page.items.swap(0, 1);
            }
            return Ok(page);
        }
        match self.mode {
            InjectedDriftMode::DuplicatePage => {
                page.items = self.first_page.items.clone();
            }
            InjectedDriftMode::MissingSentinel => {
                page.items.remove(0);
            }
            InjectedDriftMode::NonOverlapDuplicate => {
                page.items[PAGE_OVERLAP] = self.first_page.items[0].clone();
            }
            InjectedDriftMode::ContinuousChange => page.items.swap(0, 1),
        }
        Ok(page)
    }
}

fn create_semantic_tree(root: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut entries = Vec::new();
    let cases = [
        ("uipilotsem0001", "uipilotsem0001_中文资料.txt"),
        ("uipilotsem0002", "uipilotsem0002_Cafe\u{301}.txt"),
        ("uipilotsem0003", "uipilotsem0003_space name.txt"),
        ("uipilotsem0004", "uipilotsem0004_single'quote.txt"),
        ("uipilotsem0005", "uipilotsem0005_bang!paren();.txt"),
        ("uipilotsem0006", "uipilotsem0006_UPPERCASE.txt"),
        ("uipilotsem0007", "uipilotsem0007_fixed.uipilotext"),
    ];
    for (marker, name) in cases {
        let path = root.join(name);
        fs::write(&path, marker.as_bytes())?;
        entries.push((marker.to_owned(), path));
    }
    let nested_parent = root.join("nested-backslash-container");
    fs::create_dir(&nested_parent)?;
    let nested = nested_parent.join("uipilotsem0008_nested.txt");
    fs::write(&nested, b"nested")?;
    entries.push(("uipilotsem0008".to_owned(), nested));
    let folder = root.join("uipilotsem0009_folder");
    fs::create_dir(&folder)?;
    entries.push(("uipilotsem0009".to_owned(), folder));

    let illegal = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    assert!(illegal.iter().all(|character| {
        !Path::new(&format!("invalid{character}name")).is_absolute()
            && is_illegal_windows_component_character(*character)
    }));
    for legal in ['!', '(', ')', ';'] {
        assert!(!is_illegal_windows_component_character(legal));
    }
    Ok(entries)
}

struct LiteralSyntaxCase {
    literal: String,
    expected_path: Option<PathBuf>,
}

fn create_literal_syntax_tree(root: &Path) -> io::Result<Vec<LiteralSyntaxCase>> {
    let cases = [
        ("uipilotlitgate-ascii", "uipilotlitgate-ascii.txt", true),
        (
            "uipilotlitgate-space name",
            "uipilotlitgate-space name.txt",
            true,
        ),
        (
            "uipilotlitgate-中文资料",
            "uipilotlitgate-中文资料.txt",
            true,
        ),
        (
            "uipilotlitgate-bang!name",
            "uipilotlitgate-bang!name.txt",
            true,
        ),
        (
            "uipilotlitgate-pipe|decoy",
            "uipilotlitgate-pipe.txt",
            false,
        ),
        ("<uipilotlitgate-angle>", "uipilotlitgate-angle.txt", false),
        (
            "\"uipilotlitgate-quote phrase\"",
            "uipilotlitgate-quote phrase.txt",
            false,
        ),
        (
            "uipilotlitgate-question?tail.txt",
            "uipilotlitgate-questionXtail.txt",
            false,
        ),
        (
            "uipilotlitgate-star*tail*",
            "uipilotlitgate-star-middle-tail.txt",
            false,
        ),
        ("ext:txt", "uipilotlitgate-macro-extension.txt", false),
        ("regex:.*", "uipilotlitgate-macro-regex.dat", false),
        ("#x2A:", "uipilotlitgate-recursive-entity.dat", false),
        (
            "uipilotlitgate-mixed 文档!2026",
            "uipilotlitgate-mixed 文档!2026.txt",
            true,
        ),
    ];

    cases
        .into_iter()
        .map(|(literal, name, matches)| {
            let path = root.join(name);
            fs::write(&path, literal.as_bytes())?;
            Ok(LiteralSyntaxCase {
                literal: literal.to_owned(),
                expected_path: matches.then_some(path),
            })
        })
        .collect()
}

fn is_illegal_windows_component_character(character: char) -> bool {
    matches!(
        character,
        '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
    ) || character <= '\u{1f}'
}

fn create_tie_tree(root: &Path, count: usize) -> io::Result<(PathBuf, PathBuf)> {
    if count < 2 {
        return Err(io::Error::other("tie tree requires at least two entries"));
    }
    let fixed_time = UNIX_EPOCH + Duration::from_secs(TIE_MTIME_SECONDS);
    let first = root.join("uipilottie_00000.dat");
    fs::write(&first, b"tie")?;
    set_modified(&first, fixed_time)?;
    for index in 1..count - 1 {
        let path = root.join(format!("uipilottie_{index:05}.dat"));
        fs::write(&path, b"tie")?;
        set_modified(&path, fixed_time)?;
    }
    let hardlink = root.join(format!("uipilottie_{:05}_hardlink.dat", count - 1));
    fs::hard_link(&first, &hardlink)?;
    Ok((first, hardlink))
}

fn path_file_marker(path: &Path) -> Result<String, PaginationError> {
    path.file_name()
        .and_then(OsStr::to_str)
        .filter(|marker| !marker.is_empty())
        .map(str::to_owned)
        .ok_or(PaginationError::InvalidPage)
}

fn current_tie_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let matches = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|name| fold_name(name).contains("uipilottie_"))
            .unwrap_or(false);
        if matches {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn assert_stable_matches_current_tree(
    stable: &StableCutoff,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    let expected_paths = current_tie_paths(root)?;
    let expected = canonical_path_set(expected_paths.clone())?;
    let actual = stable
        .ordered_tie_entries
        .iter()
        .map(|entry| entry.key.canonical_path.clone())
        .collect::<HashSet<_>>();
    assert_eq!(stable.total as usize, expected_paths.len());
    assert_eq!(stable.ordered_tie_entries.len(), expected_paths.len());
    assert_eq!(actual, expected);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum RealMutationCase {
    Insert,
    Delete,
    RenameHardlink,
}

fn run_real_mutation_case(case: RealMutationCase) -> Result<(), Box<dyn Error>> {
    let harness_deadline = Instant::now() + Duration::from_secs(60);
    let mut harness = IsolatedEverything::prepare()?;
    let (first, hardlink) = create_tie_tree(&harness.indexed_root, 320)?;
    let original_first_key = canonical_ordinal(&first)?;
    let original_hardlink_key = canonical_ordinal(&hardlink)?;
    let (mutation, changed_path, expected_total, expected_first_page_targets) = match case {
        RealMutationCase::Insert => {
            let path = harness
                .indexed_root
                .join("uipilottie_inserted_before_cutoff.dat");
            (
                RealMutation::Insert {
                    path: path.clone(),
                    marker: path_file_marker(&path)?,
                },
                path,
                321,
                0,
            )
        }
        RealMutationCase::Delete => {
            let path = harness.indexed_root.join("uipilottie_00001.dat");
            set_modified(
                &path,
                UNIX_EPOCH + Duration::from_secs(PRIORITY_MTIME_SECONDS),
            )?;
            (
                RealMutation::Delete {
                    path: path.clone(),
                    marker: path_file_marker(&path)?,
                },
                path,
                319,
                1,
            )
        }
        RealMutationCase::RenameHardlink => {
            set_modified(
                &hardlink,
                UNIX_EPOCH + Duration::from_secs(PRIORITY_MTIME_SECONDS),
            )?;
            let destination = harness.indexed_root.join("uipilottie_renamed_hardlink.dat");
            (
                RealMutation::RenameHardlink {
                    source: hardlink.clone(),
                    linked_sibling: first.clone(),
                    source_marker: path_file_marker(&hardlink)?,
                    destination: destination.clone(),
                    destination_marker: path_file_marker(&destination)?,
                },
                destination,
                320,
                2,
            )
        }
    };
    harness.start(harness_deadline)?;
    harness.wait_for_exact_total("uipilottie_", 320, harness_deadline)?;
    let baseline_stage = match case {
        RealMutationCase::Insert => "insert_pre_transaction",
        RealMutationCase::Delete => "delete_pre_transaction",
        RealMutationCase::RenameHardlink => "rename_pre_transaction",
    };
    let baseline_snapshot = wait_for_stable_ordered_snapshot(
        &harness,
        "uipilottie_",
        Instant::now() + TRANSACTION_DEADLINE,
        baseline_stage,
    )?;
    match case {
        RealMutationCase::Insert => {}
        RealMutationCase::Delete => {
            let deleted_key = canonical_ordinal(&changed_path)?;
            if !snapshot_contains_path(&baseline_snapshot.first_page_keys, &deleted_key) {
                return Err("case=Delete stage=pre_transaction_stable_first_page".into());
            }
        }
        RealMutationCase::RenameHardlink => {
            if !snapshot_contains_path(&baseline_snapshot.first_page_keys, &original_first_key)
                || !snapshot_contains_path(
                    &baseline_snapshot.first_page_keys,
                    &original_hardlink_key,
                )
            {
                return Err("case=RenameHardlink stage=pre_transaction_stable_first_page".into());
            }
        }
    }

    let mut source = RealMutatingPageSource {
        harness: &harness,
        search: "uipilottie_",
        baseline_total: 320,
        mutation_state: RealMutationState::Pending(mutation),
        first_page: None,
        post_mutation_first_page: None,
        evidence: RealMutationEvidence::default(),
        trace_context: (0, 0),
        fetch_trace: Vec::new(),
    };
    let stable_result =
        read_stable_cutoff_with_trace(&mut source, Instant::now() + TRANSACTION_DEADLINE);
    eprintln!(
        "MUTATION_FETCH_TRACE case={case:?} events={:?}",
        source.fetch_trace
    );
    let (stable, trace) = stable_result
        .map_err(|error| format!("case={case:?} stage=read_stable_cutoff error={error:?}"))?;
    eprintln!("MUTATION_PASS_TRACE case={case:?} trace={trace:?}");

    assert!(source.evidence.first_page_captured);
    assert_eq!(
        source.evidence.verified_first_page_targets,
        expected_first_page_targets
    );
    assert_eq!(source.evidence.applied, 1);
    assert_eq!(source.evidence.observed, 1);
    assert_eq!(trace.attempts, 2);
    assert!(trace.rejected_passes >= 1);
    assert!(trace.completed_passes >= 2);
    assert_eq!(stable.total, expected_total);
    assert_stable_matches_current_tree(&stable, &harness.indexed_root)?;

    let stable_paths = stable
        .ordered_tie_entries
        .iter()
        .map(|entry| entry.key.canonical_path.clone())
        .collect::<HashSet<_>>();
    match case {
        RealMutationCase::Insert => {
            assert!(stable_paths.contains(&canonical_ordinal(&changed_path)?));
            require_path_in_first_page(
                &changed_path,
                source
                    .post_mutation_first_page
                    .as_ref()
                    .ok_or("retry did not capture a post-mutation offset 0 page")?,
            )?;
        }
        RealMutationCase::Delete => {
            assert!(!changed_path.exists());
        }
        RealMutationCase::RenameHardlink => {
            let renamed_hardlink_key = canonical_ordinal(&changed_path)?;
            assert_ne!(original_first_key, original_hardlink_key);
            assert_ne!(original_first_key, renamed_hardlink_key);
            assert!(!stable_paths.contains(&original_hardlink_key));
            assert!(stable_paths.contains(&original_first_key));
            assert!(stable_paths.contains(&renamed_hardlink_key));
        }
    }

    harness.shutdown()?;
    Ok(())
}

fn set_modified(path: &Path, time: SystemTime) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_times(FileTimes::new().set_modified(time))
}

#[test]
#[ignore = "Task 3 classification gate: uses a real vanished absolute path without Everything"]
fn vanished_query_entry_is_concurrent_mutation_but_relative_path_is_invalid_page(
) -> Result<(), Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "uipilot-vanished-entry-{}-{}",
        std::process::id(),
        next_harness_sequence()
    ));
    fs::write(&path, b"vanish")?;
    fs::remove_file(&path)?;
    assert_eq!(
        canonical_ordinal(&path),
        Err(PaginationError::ConcurrentMutation)
    );
    assert_eq!(
        canonical_ordinal(Path::new("relative-entry.dat")),
        Err(PaginationError::InvalidPage)
    );
    Ok(())
}

#[test]
#[ignore = "live gate: launches an isolated frozen Everything folder-index instance"]
fn real_literal_entity_queries_match_plain_filenames() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut harness = IsolatedEverything::prepare()?;
    let entries = create_literal_syntax_tree(&harness.indexed_root)?;
    for required_literal in ["ext:txt", "regex:.*", "#x2A:"] {
        assert!(
            entries
                .iter()
                .any(|entry| entry.literal == required_literal),
            "missing literal syntax fixture {required_literal:?}"
        );
    }
    harness.start(deadline)?;
    harness.wait_for_exact_total("uipilotlitgate-", entries.len() as u32, deadline)?;

    for entry in entries {
        let encoded_entities = entry
            .literal
            .chars()
            .map(|scalar| format!("#x{:X}:", u32::from(scalar)))
            .collect::<String>();
        let encoded = format!("nowildcards:{encoded_entities}");
        let result = harness.query(&encoded, 0, 200, deadline)?;
        let expected_total = if entry.expected_path.is_some() { 1 } else { 0 };
        assert_eq!(result.total, expected_total, "literal {:?}", entry.literal);
        assert_eq!(
            canonical_path_set(
                result
                    .items
                    .iter()
                    .map(|item| PathBuf::from(&item.full_path))
            )?,
            canonical_path_set(entry.expected_path.into_iter())?,
            "literal {:?}",
            entry.literal
        );
    }

    harness.shutdown()?;
    Ok(())
}

#[test]
#[ignore = "Task 3 live gate: launches an isolated frozen Everything folder-index instance"]
fn real_tree_matches_uipilot_folded_name_contains_semantics() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut harness = IsolatedEverything::prepare()?;
    let entries = create_semantic_tree(&harness.indexed_root)?;
    harness.start(deadline)?;

    for (marker, _) in &entries {
        let folded_query = fold_name(&marker.to_uppercase());
        let expected_paths = entries
            .iter()
            .filter_map(|(_, path)| {
                let name = path.file_name()?.to_str()?;
                fold_name(name)
                    .contains(&folded_query)
                    .then_some(path.clone())
            })
            .collect::<Vec<_>>();
        harness.wait_for_exact_total(marker, expected_paths.len() as u32, deadline)?;
        let result = harness.query(marker, 0, VISIBLE_LIMIT as u32, deadline)?;
        assert_eq!(result.total, expected_paths.len() as u32);
        assert_eq!(result.items.len(), expected_paths.len());
        let expected = canonical_path_set(expected_paths)?;
        let actual = canonical_path_set(
            result
                .items
                .iter()
                .map(|item| PathBuf::from(&item.full_path)),
        )?;
        assert_eq!(actual, expected);
    }

    harness.shutdown()?;
    Ok(())
}

#[test]
#[ignore = "Task 3 live gate: validates 320-item real tie pagination and hardlink path identity"]
fn real_tie_group_uses_path_entry_identity_and_stable_double_pass() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut harness = IsolatedEverything::prepare()?;
    let (first, hardlink) = create_tie_tree(&harness.indexed_root, 320)?;
    harness.start(deadline)?;
    harness.wait_for_exact_total("uipilottie_", 320, deadline)?;

    let mut source = RealPageSource {
        harness: &harness,
        search: "uipilottie_",
    };
    let stable = read_stable_cutoff(&mut source, Instant::now() + TRANSACTION_DEADLINE)?;
    assert_eq!(stable.total, 320);
    assert_eq!(stable.visible.len(), VISIBLE_LIMIT);
    assert_eq!(stable.ordered_tie_entries.len(), 320);
    let first_key = canonical_ordinal(&first)?;
    let hardlink_key = canonical_ordinal(&hardlink)?;
    assert_ne!(first_key, hardlink_key);
    assert!(stable
        .ordered_tie_entries
        .iter()
        .any(|entry| entry.key.canonical_path == first_key));
    assert!(stable
        .ordered_tie_entries
        .iter()
        .any(|entry| entry.key.canonical_path == hardlink_key));

    harness.shutdown()?;
    Ok(())
}

#[test]
#[ignore = "Task 3 diagnostic gate: real insert between isolated Query2 pages"]
fn real_pagination_insert_case() -> Result<(), Box<dyn Error>> {
    run_real_mutation_case(RealMutationCase::Insert)
}

#[test]
#[ignore = "Task 3 diagnostic gate: real delete between isolated Query2 pages"]
fn real_pagination_delete_case() -> Result<(), Box<dyn Error>> {
    run_real_mutation_case(RealMutationCase::Delete)
}

#[test]
#[ignore = "Task 3 diagnostic gate: real hardlink rename between isolated Query2 pages"]
fn real_pagination_rename_hardlink_case() -> Result<(), Box<dyn Error>> {
    run_real_mutation_case(RealMutationCase::RenameHardlink)
}

#[test]
#[ignore = "Task 3 live gate: injects deterministic page anomalies over real Everything captures"]
fn captured_real_pages_reject_wire_and_continuous_drift() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut harness = IsolatedEverything::prepare()?;
    create_tie_tree(&harness.indexed_root, 320)?;
    harness.start(deadline)?;
    harness.wait_for_exact_total("uipilottie_", 320, deadline)?;

    let mut real_source = RealPageSource {
        harness: &harness,
        search: "uipilottie_",
    };
    let pages = capture_real_pages(&mut real_source, deadline)?;
    let first_page = pages
        .get(&0)
        .cloned()
        .ok_or("real Everything did not return the first page")?;
    for mode in [
        InjectedDriftMode::DuplicatePage,
        InjectedDriftMode::MissingSentinel,
        InjectedDriftMode::NonOverlapDuplicate,
        InjectedDriftMode::ContinuousChange,
    ] {
        let mut injected = InjectedPageSource {
            pages: pages.clone(),
            first_page: first_page.clone(),
            mode,
            calls: 0,
        };
        assert_eq!(
            read_stable_cutoff(&mut injected, Instant::now() + TRANSACTION_DEADLINE),
            Err(PaginationError::ConcurrentMutation)
        );
    }

    harness.shutdown()?;
    Ok(())
}

#[test]
#[ignore = "Task 3 resource gate: creates more than 16,384 same-mtime real folder-index entries"]
fn real_tie_group_above_resource_limit_fails_closed() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut harness = IsolatedEverything::prepare()?;
    create_tie_tree(&harness.indexed_root, MAX_TIE_ENTRIES + 1)?;
    harness.start(deadline)?;
    harness.wait_for_exact_total("uipilottie_", (MAX_TIE_ENTRIES + 1) as u32, deadline)?;
    let mut source = RealPageSource {
        harness: &harness,
        search: "uipilottie_",
    };
    assert_eq!(
        read_stable_cutoff(&mut source, deadline),
        Err(PaginationError::TieGroupTooLarge)
    );
    harness.shutdown()?;
    Ok(())
}
