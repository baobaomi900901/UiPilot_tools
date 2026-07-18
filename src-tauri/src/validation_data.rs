use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::atomic_file::{
    commit_with_backup, quarantine_invalid, read_optional, AtomicFileError, AtomicPaths,
};

pub(crate) const VALIDATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyCounts {
    pub(crate) launcher_invocations: u64,
    pub(crate) application_launch_requests: u64,
    pub(crate) activation_successes: u64,
    pub(crate) activation_refusals: u64,
    pub(crate) unclean_sessions: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationState {
    schema_version: u32,
    daily_counts: BTreeMap<String, DailyCounts>,
    last_reconciled_session_id: Option<String>,
}

struct ValidationStoreState {
    value: ValidationState,
    current_is_valid: bool,
    current_session_id: Option<String>,
}

pub(crate) struct ValidationStore {
    paths: ValidationPaths,
    state: Mutex<ValidationStoreState>,
}

struct ValidationPaths {
    data: AtomicPaths,
    marker: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationCountsSnapshot {
    pub(crate) schema_version: u32,
    pub(crate) daily_counts: BTreeMap<String, DailyCounts>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidationEvent {
    LauncherInvoked,
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidationError {
    Storage,
    Serialize,
    InvalidDate,
    CounterOverflow,
    SessionNotOpen,
    SessionAlreadyOpen,
    SessionOwnershipLost,
    SessionRandom,
}

impl Default for ValidationState {
    fn default() -> Self {
        Self {
            schema_version: VALIDATION_SCHEMA_VERSION,
            daily_counts: BTreeMap::new(),
            last_reconciled_session_id: None,
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Storage => "validation storage failed",
            Self::Serialize => "validation serialization failed",
            Self::InvalidDate => "validation date is invalid",
            Self::CounterOverflow => "validation counter overflow",
            Self::SessionNotOpen => "validation session is not open",
            Self::SessionAlreadyOpen => "validation session is already open",
            Self::SessionOwnershipLost => "validation session ownership lost",
            Self::SessionRandom => "validation session random failed",
        })
    }
}

impl std::error::Error for ValidationError {}

impl From<AtomicFileError> for ValidationError {
    fn from(_: AtomicFileError) -> Self {
        Self::Storage
    }
}

impl ValidationStore {
    pub(crate) fn load(app_data_dir: &Path) -> Result<Self, ValidationError> {
        fs::create_dir_all(app_data_dir).map_err(|_| ValidationError::Storage)?;
        let paths = ValidationPaths {
            data: AtomicPaths::new(app_data_dir, "validation-data.json"),
            marker: app_data_dir.join("open-session.json"),
        };

        if let Some(value) = load_candidate(paths.data.current())? {
            return Ok(Self {
                paths,
                state: Mutex::new(ValidationStoreState {
                    value,
                    current_is_valid: true,
                    current_session_id: None,
                }),
            });
        }
        if let Some(value) = load_candidate(paths.data.backup())? {
            return Ok(Self {
                paths,
                state: Mutex::new(ValidationStoreState {
                    value,
                    current_is_valid: false,
                    current_session_id: None,
                }),
            });
        }

        Ok(Self {
            paths,
            state: Mutex::new(ValidationStoreState {
                value: ValidationState::default(),
                current_is_valid: false,
                current_session_id: None,
            }),
        })
    }

    pub(crate) fn record(&self, event: ValidationEvent) -> Result<(), ValidationError> {
        self.record_with(event, local_date, commit_with_backup)
    }

    pub(crate) fn clear_daily_counts(&self) -> Result<(), ValidationError> {
        self.clear_with(commit_with_backup)
    }

    pub(crate) fn export_snapshot(&self) -> ValidationCountsSnapshot {
        let state = self.state.lock().expect("validation lock poisoned");
        ValidationCountsSnapshot {
            schema_version: state.value.schema_version,
            daily_counts: state.value.daily_counts.clone(),
        }
    }

    fn record_with<D, P>(
        &self,
        event: ValidationEvent,
        date_provider: D,
        persist: P,
    ) -> Result<(), ValidationError>
    where
        D: FnOnce() -> Result<String, ValidationError>,
        P: FnOnce(&AtomicPaths, Option<&[u8]>, &[u8]) -> Result<(), AtomicFileError>,
    {
        let mut state = self.state.lock().expect("validation lock poisoned");
        if state.current_session_id.is_none() {
            return Err(ValidationError::SessionNotOpen);
        }
        let date = date_provider()?;
        if !valid_date(&date) {
            return Err(ValidationError::InvalidDate);
        }

        let mut candidate = state.value.clone();
        let counts = candidate.daily_counts.entry(date).or_default();
        match event {
            ValidationEvent::LauncherInvoked => {
                counts.launcher_invocations = checked_increment(counts.launcher_invocations)?;
            }
            ValidationEvent::LaunchRequested => {
                counts.application_launch_requests =
                    checked_increment(counts.application_launch_requests)?;
            }
            ValidationEvent::ActivationRequested => {
                counts.activation_successes = checked_increment(counts.activation_successes)?;
            }
            ValidationEvent::ActivationRefusedLaunchRequested => {
                counts.activation_refusals = checked_increment(counts.activation_refusals)?;
                counts.application_launch_requests =
                    checked_increment(counts.application_launch_requests)?;
            }
        }
        self.persist_with(&mut state, candidate, persist)
    }

    fn clear_with<P>(&self, persist: P) -> Result<(), ValidationError>
    where
        P: FnOnce(&AtomicPaths, Option<&[u8]>, &[u8]) -> Result<(), AtomicFileError>,
    {
        let mut state = self.state.lock().expect("validation lock poisoned");
        let mut candidate = state.value.clone();
        candidate.daily_counts.clear();
        self.persist_with(&mut state, candidate, persist)
    }

    fn persist_with<P>(
        &self,
        state: &mut MutexGuard<'_, ValidationStoreState>,
        candidate: ValidationState,
        persist: P,
    ) -> Result<(), ValidationError>
    where
        P: FnOnce(&AtomicPaths, Option<&[u8]>, &[u8]) -> Result<(), AtomicFileError>,
    {
        let previous_bytes =
            serde_json::to_vec(&state.value).map_err(|_| ValidationError::Serialize)?;
        let candidate_bytes =
            serde_json::to_vec(&candidate).map_err(|_| ValidationError::Serialize)?;
        let previous = state.current_is_valid.then_some(previous_bytes.as_slice());
        persist(&self.paths.data, previous, &candidate_bytes)?;
        let current_session_id = state.current_session_id.clone();
        **state = ValidationStoreState {
            value: candidate,
            current_is_valid: true,
            current_session_id,
        };
        Ok(())
    }
}

fn load_candidate(path: &Path) -> Result<Option<ValidationState>, ValidationError> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(None);
    };
    match serde_json::from_slice::<ValidationState>(&bytes) {
        Ok(value) if valid_state(&value) => Ok(Some(value)),
        _ => {
            quarantine_invalid(path)?;
            Ok(None)
        }
    }
}

fn valid_state(state: &ValidationState) -> bool {
    state.schema_version == VALIDATION_SCHEMA_VERSION
        && state.daily_counts.keys().all(|date| valid_date(date))
        && state
            .last_reconciled_session_id
            .as_deref()
            .map(valid_session_id)
            .unwrap_or(true)
}

fn valid_session_id(value: &str) -> bool {
    value.len() == 40
        && value.starts_with("session-")
        && value[8..]
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return false;
    }

    let year = decimal(&bytes[0..4]);
    let month = decimal(&bytes[5..7]);
    let day = decimal(&bytes[8..10]);
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let days_in_month = match month {
        2 if leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days_in_month).contains(&day)
}

fn decimal(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + u16::from(byte - b'0'))
}

fn leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn checked_increment(value: u64) -> Result<u64, ValidationError> {
    value.checked_add(1).ok_or(ValidationError::CounterOverflow)
}

fn local_date() -> Result<String, ValidationError> {
    let local = unsafe { GetLocalTime() };
    let value = format!("{:04}-{:02}-{:02}", local.wYear, local.wMonth, local.wDay);
    valid_date(&value)
        .then_some(value)
        .ok_or(ValidationError::InvalidDate)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Barrier,
        },
        thread,
    };

    use super::*;
    use crate::atomic_file::{commit_with_backup, AtomicFileError};

    const SESSION_A: &str = "session-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SESSION_B: &str = "session-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-validation-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn current(&self) -> PathBuf {
            self.0.join("validation-data.json")
        }

        fn backup(&self) -> PathBuf {
            self.0.join("validation-data.json.backup")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn day_counts(launcher_invocations: u64) -> DailyCounts {
        DailyCounts {
            launcher_invocations,
            ..DailyCounts::default()
        }
    }

    fn state_with(
        daily_counts: BTreeMap<String, DailyCounts>,
        last_reconciled_session_id: Option<&str>,
    ) -> ValidationState {
        ValidationState {
            schema_version: VALIDATION_SCHEMA_VERSION,
            daily_counts,
            last_reconciled_session_id: last_reconciled_session_id.map(Into::into),
        }
    }

    fn write_state(path: &Path, state: &ValidationState) {
        fs::write(path, serde_json::to_vec(state).unwrap()).unwrap();
    }

    fn read_state(path: &Path) -> ValidationState {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn open_store(label: &str) -> (TestDir, ValidationStore) {
        let dir = TestDir::new(label);
        let store = ValidationStore::load(dir.path()).unwrap();
        store.state.lock().unwrap().current_session_id = Some(SESSION_A.into());
        (dir, store)
    }

    fn load_open(dir: &TestDir) -> ValidationStore {
        let store = ValidationStore::load(dir.path()).unwrap();
        store.state.lock().unwrap().current_session_id = Some(SESSION_A.into());
        store
    }

    fn record_on(
        store: &ValidationStore,
        event: ValidationEvent,
        date: &str,
    ) -> Result<(), ValidationError> {
        store.record_with(event, || Ok(date.into()), commit_with_backup)
    }

    fn internal_state(store: &ValidationStore) -> (ValidationState, bool, Option<String>) {
        let state = store.state.lock().unwrap();
        (
            state.value.clone(),
            state.current_is_valid,
            state.current_session_id.clone(),
        )
    }

    #[test]
    fn all_events_use_the_approved_field_mapping() {
        let cases = [
            (ValidationEvent::LauncherInvoked, [1, 0, 0, 0]),
            (ValidationEvent::LaunchRequested, [0, 1, 0, 0]),
            (ValidationEvent::ActivationRequested, [0, 0, 1, 0]),
            (
                ValidationEvent::ActivationRefusedLaunchRequested,
                [0, 1, 0, 1],
            ),
        ];

        for (index, (event, expected)) in cases.into_iter().enumerate() {
            let (_dir, store) = open_store(&format!("event-{index}"));
            record_on(&store, event, "2026-07-18").unwrap();
            let snapshot = store.export_snapshot();
            let day = &snapshot.daily_counts["2026-07-18"];
            assert_eq!(
                [
                    day.launcher_invocations,
                    day.application_launch_requests,
                    day.activation_successes,
                    day.activation_refusals,
                ],
                expected
            );
            assert_eq!(day.unclean_sessions, 0);
        }
    }

    #[test]
    fn same_day_and_cross_day_events_aggregate_separately() {
        let (_dir, store) = open_store("daily-aggregation");

        record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
        record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
        record_on(&store, ValidationEvent::LaunchRequested, "2026-07-19").unwrap();

        let snapshot = store.export_snapshot();
        assert_eq!(snapshot.daily_counts["2026-07-18"].launcher_invocations, 2);
        assert_eq!(
            snapshot.daily_counts["2026-07-19"].application_launch_requests,
            1
        );
    }

    #[test]
    fn production_record_uses_one_valid_local_date() {
        let (_dir, store) = open_store("production-date");

        store.record(ValidationEvent::LauncherInvoked).unwrap();

        let snapshot = store.export_snapshot();
        assert_eq!(snapshot.daily_counts.len(), 1);
        let (date, counts) = snapshot.daily_counts.first_key_value().unwrap();
        assert!(valid_date(date));
        assert_eq!(counts.launcher_invocations, 1);
    }

    #[test]
    fn record_before_session_open_does_not_access_date_or_disk() {
        let dir = TestDir::new("session-not-open");
        let store = ValidationStore::load(dir.path()).unwrap();

        assert_eq!(
            store.record_with(
                ValidationEvent::LauncherInvoked,
                || panic!("date provider must not run"),
                |_paths, _previous, _candidate| panic!("persist must not run"),
            ),
            Err(ValidationError::SessionNotOpen)
        );
        assert!(!dir.current().exists());
    }

    #[test]
    fn checked_overflow_preserves_memory_and_current() {
        let dir = TestDir::new("overflow");
        let persisted = state_with(
            BTreeMap::from([("2026-07-18".into(), day_counts(u64::MAX))]),
            None,
        );
        write_state(&dir.current(), &persisted);
        let before = fs::read(dir.current()).unwrap();
        let store = load_open(&dir);

        assert_eq!(
            record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18"),
            Err(ValidationError::CounterOverflow)
        );
        assert_eq!(internal_state(&store).0, persisted);
        assert_eq!(fs::read(dir.current()).unwrap(), before);
    }

    #[test]
    fn clear_preserves_reconciled_and_current_session_ids() {
        let dir = TestDir::new("clear-preserves-session");
        let persisted = state_with(
            BTreeMap::from([("2026-07-18".into(), day_counts(3))]),
            Some(SESSION_B),
        );
        write_state(&dir.current(), &persisted);
        let store = load_open(&dir);

        store.clear_daily_counts().unwrap();

        let (value, current_is_valid, current_session_id) = internal_state(&store);
        assert!(value.daily_counts.is_empty());
        assert_eq!(value.last_reconciled_session_id.as_deref(), Some(SESSION_B));
        assert_eq!(current_session_id.as_deref(), Some(SESSION_A));
        assert!(current_is_valid);
    }

    #[test]
    fn strict_dates_accept_real_leap_days_and_reject_invalid_calendar_dates() {
        assert!(valid_date("2024-02-29"));
        assert!(valid_date("2026-12-31"));
        for invalid in [
            "2026/07/18",
            "2026-7-18",
            "026-07-18",
            "2026-00-10",
            "2026-13-10",
            "2026-01-00",
            "2026-01-32",
            "2026-04-31",
            "2026-02-29",
            "2100-02-29",
        ] {
            assert!(!valid_date(invalid), "accepted invalid date {invalid}");
        }
    }

    #[test]
    fn invalid_date_keys_are_quarantined_during_load() {
        for (index, invalid) in ["2026/07/18", "2026-02-29", "2026-04-31"]
            .into_iter()
            .enumerate()
        {
            let dir = TestDir::new(&format!("invalid-date-{index}"));
            write_state(
                &dir.current(),
                &state_with(BTreeMap::from([(invalid.into(), day_counts(1))]), None),
            );

            let store = ValidationStore::load(dir.path()).unwrap();

            assert!(store.export_snapshot().daily_counts.is_empty());
            assert!(!dir.current().exists());
        }
    }

    #[test]
    fn invalid_schema_or_reconciled_session_id_is_quarantined() {
        let cases = [
            ValidationState {
                schema_version: 2,
                ..ValidationState::default()
            },
            state_with(BTreeMap::new(), Some("session-BAD")),
        ];
        for (index, invalid) in cases.into_iter().enumerate() {
            let dir = TestDir::new(&format!("invalid-state-{index}"));
            write_state(&dir.current(), &invalid);

            let store = ValidationStore::load(dir.path()).unwrap();

            assert_eq!(internal_state(&store).0, ValidationState::default());
            assert!(!dir.current().exists());
        }
    }

    #[test]
    fn valid_current_wins_and_invalid_current_recovers_from_backup() {
        let current_dir = TestDir::new("current-priority");
        let current = state_with(BTreeMap::from([("2026-07-18".into(), day_counts(4))]), None);
        let backup = state_with(BTreeMap::from([("2026-07-18".into(), day_counts(2))]), None);
        write_state(&current_dir.current(), &current);
        write_state(&current_dir.backup(), &backup);
        assert_eq!(
            internal_state(&ValidationStore::load(current_dir.path()).unwrap()).0,
            current
        );

        let recovery_dir = TestDir::new("backup-recovery");
        fs::write(recovery_dir.current(), b"not-json").unwrap();
        write_state(&recovery_dir.backup(), &backup);
        let recovered = ValidationStore::load(recovery_dir.path()).unwrap();
        assert_eq!(internal_state(&recovered).0, backup);
        assert!(!internal_state(&recovered).1);
        assert!(!recovery_dir.current().exists());
    }

    #[test]
    fn invalid_current_and_backup_fall_back_to_defaults() {
        let dir = TestDir::new("invalid-both");
        fs::write(dir.current(), b"not-json").unwrap();
        fs::write(dir.backup(), b"also-not-json").unwrap();

        let store = ValidationStore::load(dir.path()).unwrap();

        assert_eq!(internal_state(&store).0, ValidationState::default());
        assert!(!dir.current().exists());
        assert!(!dir.backup().exists());
    }

    #[test]
    fn concurrent_records_survive_in_memory_and_current() {
        let (dir, store) = open_store("concurrent-records");
        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();

        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
            }));
        }
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(
            store.export_snapshot().daily_counts["2026-07-18"].launcher_invocations,
            2
        );
        assert_eq!(
            read_state(&dir.current()).daily_counts["2026-07-18"].launcher_invocations,
            2
        );
    }

    #[test]
    fn second_write_after_defaults_backs_up_first_write() {
        let (dir, store) = open_store("defaults-two-writes");

        record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
        assert!(!dir.backup().exists());
        record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();

        assert_eq!(
            read_state(&dir.current()).daily_counts["2026-07-18"].launcher_invocations,
            2
        );
        assert_eq!(
            read_state(&dir.backup()).daily_counts["2026-07-18"].launcher_invocations,
            1
        );
    }

    #[test]
    fn second_write_after_backup_recovery_backs_up_first_new_current() {
        let dir = TestDir::new("backup-two-writes");
        let recovered = state_with(BTreeMap::from([("2026-07-18".into(), day_counts(7))]), None);
        write_state(&dir.backup(), &recovered);
        let store = load_open(&dir);

        record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
        assert_eq!(
            read_state(&dir.backup()).daily_counts["2026-07-18"].launcher_invocations,
            7
        );
        record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();

        assert_eq!(
            read_state(&dir.current()).daily_counts["2026-07-18"].launcher_invocations,
            9
        );
        assert_eq!(
            read_state(&dir.backup()).daily_counts["2026-07-18"].launcher_invocations,
            8
        );
    }

    #[test]
    fn record_persistence_errors_preserve_old_state() {
        for (index, error) in atomic_errors().into_iter().enumerate() {
            let (dir, store) = open_store(&format!("record-error-{index}"));
            record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
            let before_state = internal_state(&store);
            let before_current = fs::read(dir.current()).unwrap();

            assert_eq!(
                store.record_with(
                    ValidationEvent::LaunchRequested,
                    || Ok("2026-07-18".into()),
                    |_paths, _previous, _candidate| Err(error),
                ),
                Err(ValidationError::Storage)
            );
            assert_eq!(internal_state(&store), before_state);
            assert_eq!(fs::read(dir.current()).unwrap(), before_current);
        }
    }

    #[test]
    fn clear_persistence_errors_preserve_old_state() {
        for (index, error) in atomic_errors().into_iter().enumerate() {
            let (dir, store) = open_store(&format!("clear-error-{index}"));
            record_on(&store, ValidationEvent::LauncherInvoked, "2026-07-18").unwrap();
            let before_state = internal_state(&store);
            let before_current = fs::read(dir.current()).unwrap();

            assert_eq!(
                store.clear_with(|_paths, _previous, _candidate| Err(error)),
                Err(ValidationError::Storage)
            );
            assert_eq!(internal_state(&store), before_state);
            assert_eq!(fs::read(dir.current()).unwrap(), before_current);
        }
    }

    fn atomic_errors() -> [AtomicFileError; 6] {
        [
            AtomicFileError::Read,
            AtomicFileError::CandidateWrite,
            AtomicFileError::BackupWrite,
            AtomicFileError::BackupReplace,
            AtomicFileError::CurrentReplace,
            AtomicFileError::InvalidQuarantine,
        ]
    }

    #[test]
    fn export_snapshot_contains_only_schema_and_daily_counts() {
        let dir = TestDir::new("export-shape");
        write_state(
            &dir.current(),
            &state_with(
                BTreeMap::from([("2026-07-18".into(), day_counts(1))]),
                Some(SESSION_B),
            ),
        );
        let store = load_open(&dir);

        let value = serde_json::to_value(store.export_snapshot()).unwrap();
        let object = value.as_object().unwrap();

        assert_eq!(object.len(), 2);
        assert!(object.contains_key("schemaVersion"));
        assert!(object.contains_key("dailyCounts"));
        let json = value.to_string();
        for forbidden in [
            "lastReconciledSessionId",
            "currentSessionId",
            "session-",
            "marker",
            "path",
            "raw",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn defaults_use_fixed_schema_and_paths() {
        let dir = TestDir::new("defaults");
        let store = ValidationStore::load(dir.path()).unwrap();

        assert_eq!(internal_state(&store).0, ValidationState::default());
        assert_eq!(store.export_snapshot().schema_version, 1);
        assert_eq!(store.paths.data.current(), dir.current());
        assert_eq!(store.paths.marker, dir.path().join("open-session.json"));
    }

    #[test]
    fn errors_have_fixed_path_free_messages() {
        fn assert_std_error<T: std::error::Error>() {}
        assert_std_error::<ValidationError>();

        let cases = [
            (ValidationError::Storage, "validation storage failed"),
            (
                ValidationError::Serialize,
                "validation serialization failed",
            ),
            (ValidationError::InvalidDate, "validation date is invalid"),
            (
                ValidationError::CounterOverflow,
                "validation counter overflow",
            ),
            (
                ValidationError::SessionNotOpen,
                "validation session is not open",
            ),
            (
                ValidationError::SessionAlreadyOpen,
                "validation session is already open",
            ),
            (
                ValidationError::SessionOwnershipLost,
                "validation session ownership lost",
            ),
            (
                ValidationError::SessionRandom,
                "validation session random failed",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert!(!error.to_string().contains(':'));
        }
    }
}
