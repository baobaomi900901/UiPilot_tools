use std::{
    collections::BTreeMap,
    fmt, fs,
    path::Path,
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};

use crate::{
    apps::{AppCache, Application},
    atomic_file::{
        commit_with_backup, quarantine_invalid, read_optional, AtomicFileError, AtomicPaths,
    },
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    #[serde(default = "default_file_preview_enabled")]
    pub(crate) file_preview_enabled: bool,
    pub(crate) research_id: Option<String>,
    #[serde(default)]
    pub(crate) use_counts: BTreeMap<String, u64>,
}

pub(crate) struct SettingsUpdate {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
}

struct SettingsState {
    value: Settings,
    current_is_valid: bool,
}

pub(crate) struct SettingsStore {
    paths: AtomicPaths,
    state: Mutex<SettingsState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsError {
    Storage,
    Serialize,
    InvalidUpdate,
    UnknownApplication,
    CountOverflow,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".into(),
            autostart: false,
            file_preview_enabled: default_file_preview_enabled(),
            research_id: None,
            use_counts: BTreeMap::new(),
        }
    }
}

fn default_file_preview_enabled() -> bool {
    true
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Storage => "settings storage failed",
            Self::Serialize => "settings serialization failed",
            Self::InvalidUpdate => "settings update is invalid",
            Self::UnknownApplication => "settings application is unknown",
            Self::CountOverflow => "settings count overflow",
        })
    }
}

impl std::error::Error for SettingsError {}

impl From<AtomicFileError> for SettingsError {
    fn from(_: AtomicFileError) -> Self {
        Self::Storage
    }
}

fn validate_user_settings_update(update: &SettingsUpdate) -> Result<(), SettingsError> {
    if update
        .research_id
        .as_deref()
        .is_some_and(|value| !valid_research_id(value))
    {
        return Err(SettingsError::InvalidUpdate);
    }
    Ok(())
}

impl SettingsStore {
    pub(crate) fn validate_user_settings(update: &SettingsUpdate) -> Result<(), SettingsError> {
        validate_user_settings_update(update)
    }

    pub(crate) fn load(app_data_dir: &Path) -> Result<Self, SettingsError> {
        fs::create_dir_all(app_data_dir).map_err(|_| SettingsError::Storage)?;
        let paths = AtomicPaths::new(app_data_dir, "settings.json");

        if let Some(value) = load_candidate(paths.current())? {
            return Ok(Self {
                paths,
                state: Mutex::new(SettingsState {
                    value,
                    current_is_valid: true,
                }),
            });
        }
        if let Some(value) = load_candidate(paths.backup())? {
            return Ok(Self {
                paths,
                state: Mutex::new(SettingsState {
                    value,
                    current_is_valid: false,
                }),
            });
        }

        Ok(Self {
            paths,
            state: Mutex::new(SettingsState {
                value: Settings::default(),
                current_is_valid: false,
            }),
        })
    }

    pub(crate) fn decorate_applications(&self, applications: &mut [Application]) {
        let state = self.state.lock().expect("settings lock poisoned");
        for application in applications {
            application.use_count = state
                .value
                .use_counts
                .get(&application.app_id)
                .copied()
                .unwrap_or_default();
        }
    }

    pub(crate) fn update_user_settings(&self, update: SettingsUpdate) -> Result<(), SettingsError> {
        validate_user_settings_update(&update)?;
        let mut state = self.state.lock().expect("settings lock poisoned");

        let mut candidate = state.value.clone();
        candidate.hotkey = update.hotkey;
        candidate.autostart = update.autostart;
        candidate.research_id = update.research_id;
        self.persist(&mut state, candidate)
    }

    pub(crate) fn update_hotkey(&self, hotkey: String) -> Result<(), SettingsError> {
        let mut state = self.state.lock().expect("settings lock poisoned");
        let mut candidate = state.value.clone();
        candidate.hotkey = hotkey;
        self.persist(&mut state, candidate)
    }

    pub(crate) fn increment_use_count(
        &self,
        app_id: &str,
        cache: &AppCache,
    ) -> Result<(), SettingsError> {
        let mut state = self.state.lock().expect("settings lock poisoned");
        if !cache.contains(app_id) {
            return Err(SettingsError::UnknownApplication);
        }

        let mut candidate = state.value.clone();
        let count = candidate.use_counts.entry(app_id.into()).or_default();
        *count = count.checked_add(1).ok_or(SettingsError::CountOverflow)?;
        self.persist(&mut state, candidate)
    }

    pub(crate) fn set_file_preview_enabled(&self, enabled: bool) -> Result<(), SettingsError> {
        let mut state = self.state.lock().expect("settings lock poisoned");
        let mut candidate = state.value.clone();
        candidate.file_preview_enabled = enabled;
        self.persist(&mut state, candidate)
    }

    pub(crate) fn research_id(&self) -> Option<String> {
        self.state
            .lock()
            .expect("settings lock poisoned")
            .value
            .research_id
            .clone()
    }

    pub(crate) fn snapshot(&self) -> Settings {
        self.state
            .lock()
            .expect("settings lock poisoned")
            .value
            .clone()
    }

    fn persist(
        &self,
        state: &mut MutexGuard<'_, SettingsState>,
        candidate: Settings,
    ) -> Result<(), SettingsError> {
        let previous_bytes =
            serde_json::to_vec(&state.value).map_err(|_| SettingsError::Serialize)?;
        let candidate_bytes =
            serde_json::to_vec(&candidate).map_err(|_| SettingsError::Serialize)?;
        let previous = state.current_is_valid.then_some(previous_bytes.as_slice());
        commit_with_backup(&self.paths, previous, &candidate_bytes)?;
        **state = SettingsState {
            value: candidate,
            current_is_valid: true,
        };
        Ok(())
    }
}

fn load_candidate(path: &Path) -> Result<Option<Settings>, SettingsError> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(None);
    };
    match serde_json::from_slice::<Settings>(&bytes) {
        Ok(settings) if valid_settings(&settings) => Ok(Some(settings)),
        _ => {
            quarantine_invalid(path)?;
            Ok(None)
        }
    }
}

fn valid_settings(settings: &Settings) -> bool {
    !matches!(
        settings.research_id.as_deref(),
        Some(value) if !valid_research_id(value)
    ) && settings
        .use_counts
        .keys()
        .all(|app_id| valid_app_id(app_id))
}

fn valid_research_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_app_id(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("app-")
        && value[4..]
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
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
    use crate::apps::{AppCache, Application, ApplicationLaunchTarget};

    const APP_A: &str = "app-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const APP_B: &str = "app-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const APP_ABSENT: &str = "app-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-settings-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn current(&self) -> PathBuf {
            self.0.join("settings.json")
        }

        fn backup(&self) -> PathBuf {
            self.0.join("settings.json.backup")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn application(app_id: &str, display_name: &str) -> Application {
        Application {
            app_id: app_id.into(),
            display_name: display_name.into(),
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(r"C:\Menu\App.lnk"),
                executable: None,
            },
            icon: None,
            use_count: 0,
        }
    }

    fn cache(apps: &[(&str, &str)]) -> AppCache {
        AppCache::from_apps(
            apps.iter()
                .map(|(app_id, name)| application(app_id, name))
                .collect(),
        )
    }

    fn write_settings(path: &Path, settings: &Settings) {
        fs::write(path, serde_json::to_vec(settings).unwrap()).unwrap();
    }

    fn update(research_id: Option<&str>) -> SettingsUpdate {
        SettingsUpdate {
            hotkey: "Alt+Space".into(),
            autostart: false,
            research_id: research_id.map(Into::into),
        }
    }

    fn read_current(dir: &TestDir) -> Settings {
        serde_json::from_slice(&fs::read(dir.current()).unwrap()).unwrap()
    }

    fn read_backup(dir: &TestDir) -> Settings {
        serde_json::from_slice(&fs::read(dir.backup()).unwrap()).unwrap()
    }

    #[test]
    fn missing_files_load_defaults() {
        let dir = TestDir::new("defaults");

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.snapshot(), Settings::default());
    }

    #[test]
    fn valid_current_has_priority_over_backup() {
        let dir = TestDir::new("current-priority");
        let current = Settings {
            research_id: Some("current".into()),
            ..Settings::default()
        };
        let backup = Settings {
            research_id: Some("backup".into()),
            ..Settings::default()
        };
        write_settings(&dir.current(), &current);
        write_settings(&dir.backup(), &backup);

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.snapshot(), current);
    }

    #[test]
    fn invalid_current_is_quarantined_and_valid_backup_is_loaded() {
        let dir = TestDir::new("backup-recovery");
        fs::write(dir.current(), b"not-json").unwrap();
        let backup = Settings {
            research_id: Some("backup".into()),
            ..Settings::default()
        };
        write_settings(&dir.backup(), &backup);

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.snapshot(), backup);
        assert!(!dir.current().exists());
        assert!(fs::read_dir(dir.path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("settings.json.invalid-")));
    }

    #[test]
    fn invalid_current_and_backup_are_quarantined_before_defaults() {
        let dir = TestDir::new("invalid-both");
        fs::write(dir.current(), b"not-json").unwrap();
        fs::write(dir.backup(), b"also-not-json").unwrap();

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.snapshot(), Settings::default());
        assert!(!dir.current().exists());
        assert!(!dir.backup().exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn malformed_app_id_invalidates_the_whole_file() {
        let dir = TestDir::new("bad-app-id");
        fs::write(
            dir.current(),
            br#"{"hotkey":"Alt+Space","autostart":false,"researchId":null,"useCounts":{"app-BAD":1}}"#,
        )
        .unwrap();

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.snapshot(), Settings::default());
        assert!(!dir.current().exists());
    }

    #[test]
    fn valid_temporarily_absent_app_ids_are_preserved_on_load() {
        let dir = TestDir::new("absent-id");
        let persisted = Settings {
            use_counts: BTreeMap::from([(APP_ABSENT.into(), 4)]),
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.snapshot(), persisted);
    }

    #[test]
    fn missing_use_counts_loads_as_an_empty_map() {
        let dir = TestDir::new("missing-use-counts");
        fs::write(
            dir.current(),
            br#"{"hotkey":"Alt+Space","autostart":false,"researchId":"legacy"}"#,
        )
        .unwrap();

        let store = SettingsStore::load(dir.path()).unwrap();

        assert_eq!(store.research_id().as_deref(), Some("legacy"));
        assert!(store.snapshot().use_counts.is_empty());
    }

    #[test]
    fn legacy_aliases_are_dropped_on_next_write() {
        let dir = TestDir::new("drop-legacy-aliases");
        fs::write(
            dir.current(),
            format!(
                r#"{{"hotkey":"Alt+Space","autostart":false,"filePreviewEnabled":true,"researchId":"study_01","aliases":{{"{APP_A}":["legacy"]}},"useCounts":{{}}}}"#
            ),
        )
        .unwrap();

        let store = SettingsStore::load(dir.path()).unwrap();
        store.set_file_preview_enabled(false).unwrap();

        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.current()).unwrap()).unwrap();
        assert_eq!(persisted["researchId"], "study_01");
        assert_eq!(persisted["filePreviewEnabled"], false);
        assert!(persisted.get("aliases").is_none());
    }

    #[test]
    fn user_update_preserves_use_counts() {
        let dir = TestDir::new("preserve-fields");
        let persisted = Settings {
            use_counts: BTreeMap::from([(APP_A.into(), 7)]),
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);
        let store = SettingsStore::load(dir.path()).unwrap();

        store
            .update_user_settings(SettingsUpdate {
                hotkey: "Ctrl+Space".into(),
                autostart: true,
                research_id: Some("study_01".into()),
            })
            .unwrap();

        let value = store.snapshot();
        assert_eq!(value.use_counts[APP_A], 7);
        assert_eq!(value.hotkey, "Ctrl+Space");
        assert!(value.autostart);
        assert_eq!(value.research_id.as_deref(), Some("study_01"));
    }

    #[test]
    fn validate_user_settings_accepts_valid_input_without_changing_memory_or_disk() {
        let dir = TestDir::new("preflight-no-write");
        let persisted = Settings {
            research_id: Some("study_01".into()),
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);
        let current_bytes = fs::read(dir.current()).unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        let before = store.snapshot();
        let update = update(Some("study_02"));

        validate_user_settings_update(&update).unwrap();

        assert_eq!(store.snapshot(), before);
        assert_eq!(fs::read(dir.current()).unwrap(), current_bytes);
    }

    #[test]
    fn update_hotkey_only_preserves_other_settings() {
        let dir = TestDir::new("hotkey-only");
        let store = SettingsStore::load(dir.path()).unwrap();
        store
            .update_user_settings(update(Some("study_01")))
            .unwrap();

        store.update_hotkey("DoubleCtrl".into()).unwrap();

        let snapshot = store.snapshot();
        assert_eq!(snapshot.hotkey, "DoubleCtrl");
        assert_eq!(snapshot.research_id.as_deref(), Some("study_01"));
        assert_eq!(read_current(&dir), snapshot);
    }

    #[test]
    fn preflight_and_final_validation_reject_the_same_invalid_updates() {
        let dir = TestDir::new("preflight-final-validation");
        let persisted = Settings {
            research_id: Some("study_01".into()),
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);
        let current_bytes = fs::read(dir.current()).unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();

        for (preflight, final_update, expected) in [
            (
                update(Some(" ")),
                update(Some(" ")),
                SettingsError::InvalidUpdate,
            ),
            (
                update(Some(&"A".repeat(65))),
                update(Some(&"A".repeat(65))),
                SettingsError::InvalidUpdate,
            ),
        ] {
            assert_eq!(validate_user_settings_update(&preflight), Err(expected));
            assert_eq!(store.update_user_settings(final_update), Err(expected));
            assert_eq!(store.snapshot(), persisted);
            assert_eq!(fs::read(dir.current()).unwrap(), current_bytes);
        }
    }

    #[test]
    fn unknown_increment_and_overflow_leave_memory_and_disk_unchanged() {
        let dir = TestDir::new("bad-increment");
        let persisted = Settings {
            use_counts: BTreeMap::from([(APP_A.into(), u64::MAX)]),
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);
        let before = fs::read(dir.current()).unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        let cache = cache(&[(APP_A, "App")]);

        assert_eq!(
            store.increment_use_count(APP_B, &cache),
            Err(SettingsError::UnknownApplication)
        );
        assert_eq!(
            store.increment_use_count(APP_A, &cache),
            Err(SettingsError::CountOverflow)
        );
        assert_eq!(store.snapshot(), persisted);
        assert_eq!(fs::read(dir.current()).unwrap(), before);
    }

    #[test]
    fn decoration_uses_stable_id_when_display_names_are_duplicates() {
        let dir = TestDir::new("decorate");
        let persisted = Settings {
            use_counts: BTreeMap::from([(APP_A.into(), 3), (APP_B.into(), 8)]),
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);
        let store = SettingsStore::load(dir.path()).unwrap();
        let mut applications = cache(&[(APP_A, "Same"), (APP_B, "Same")]).snapshot();

        store.decorate_applications(&mut applications);

        assert_eq!(applications[0].use_count, 3);
        assert_eq!(applications[1].use_count, 8);
    }

    #[test]
    fn research_id_accepts_only_the_approved_boundaries_on_update() {
        let dir = TestDir::new("research-update");
        let store = SettingsStore::load(dir.path()).unwrap();
        let allowed_64 = "A".repeat(64);

        store.update_user_settings(update(Some("A"))).unwrap();
        assert_eq!(store.research_id().as_deref(), Some("A"));
        store
            .update_user_settings(update(Some(&allowed_64)))
            .unwrap();
        assert_eq!(store.research_id().as_deref(), Some(allowed_64.as_str()));
    }

    #[test]
    fn invalid_research_ids_are_rejected_on_update_without_state_changes() {
        let dir = TestDir::new("invalid-research-update");
        let store = SettingsStore::load(dir.path()).unwrap();

        for invalid in ["", " ", "é", &"A".repeat(65)] {
            assert_eq!(
                store.update_user_settings(update(Some(invalid))),
                Err(SettingsError::InvalidUpdate)
            );
            assert_eq!(store.snapshot(), Settings::default());
            assert!(!dir.current().exists());
        }
    }

    #[test]
    fn invalid_research_ids_in_current_are_quarantined() {
        for (index, invalid) in ["", " ", "é", &"A".repeat(65)].into_iter().enumerate() {
            let dir = TestDir::new(&format!("invalid-research-load-{index}"));
            let persisted = Settings {
                research_id: Some(invalid.into()),
                ..Settings::default()
            };
            write_settings(&dir.current(), &persisted);

            let store = SettingsStore::load(dir.path()).unwrap();

            assert_eq!(store.snapshot(), Settings::default());
            assert!(!dir.current().exists());
        }
    }

    #[test]
    fn concurrent_increments_are_serialized_in_memory_and_on_disk() {
        let dir = TestDir::new("concurrent-increments");
        let store = Arc::new(SettingsStore::load(dir.path()).unwrap());
        let cache = Arc::new(cache(&[(APP_A, "App")]));
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();

        for _ in 0..2 {
            let store = Arc::clone(&store);
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                store.increment_use_count(APP_A, &cache).unwrap();
            }));
        }
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(store.snapshot().use_counts[APP_A], 2);
        assert_eq!(read_current(&dir).use_counts[APP_A], 2);
    }

    #[test]
    fn second_save_after_defaults_creates_backup_from_first_save() {
        let dir = TestDir::new("defaults-then-two-saves");
        let store = SettingsStore::load(dir.path()).unwrap();
        let cache = cache(&[(APP_A, "App")]);

        store.increment_use_count(APP_A, &cache).unwrap();
        assert!(!dir.backup().exists());
        store.increment_use_count(APP_A, &cache).unwrap();

        assert_eq!(read_current(&dir).use_counts[APP_A], 2);
        assert_eq!(read_backup(&dir).use_counts[APP_A], 1);
    }

    #[test]
    fn second_save_after_backup_recovery_backs_up_first_new_current() {
        let dir = TestDir::new("backup-then-two-saves");
        let recovered = Settings {
            use_counts: BTreeMap::from([(APP_A.into(), 7)]),
            ..Settings::default()
        };
        write_settings(&dir.backup(), &recovered);
        let store = SettingsStore::load(dir.path()).unwrap();
        let cache = cache(&[(APP_A, "App")]);

        store.increment_use_count(APP_A, &cache).unwrap();
        assert_eq!(read_backup(&dir).use_counts[APP_A], 7);
        store.increment_use_count(APP_A, &cache).unwrap();

        assert_eq!(read_current(&dir).use_counts[APP_A], 9);
        assert_eq!(read_backup(&dir).use_counts[APP_A], 8);
    }

    #[test]
    fn file_preview_defaults_true_and_round_trips_legacy_settings() {
        let dir = TestDir::new("file-preview-legacy");
        fs::write(
            dir.current(),
            br#"{"hotkey":"Alt+Space","autostart":false,"researchId":null,"useCounts":{}}"#,
        )
        .unwrap();

        let store = SettingsStore::load(dir.path()).unwrap();

        assert!(store.snapshot().file_preview_enabled);
        store.set_file_preview_enabled(false).unwrap();
        assert!(!read_current(&dir).file_preview_enabled);
        assert!(
            !SettingsStore::load(dir.path())
                .unwrap()
                .snapshot()
                .file_preview_enabled
        );
    }

    #[test]
    fn user_settings_update_preserves_file_preview_preference() {
        let dir = TestDir::new("file-preview-user-update");
        let persisted = Settings {
            file_preview_enabled: false,
            ..Settings::default()
        };
        write_settings(&dir.current(), &persisted);
        let store = SettingsStore::load(dir.path()).unwrap();

        store
            .update_user_settings(update(Some("study_02")))
            .unwrap();

        assert!(!store.snapshot().file_preview_enabled);
        assert!(!read_current(&dir).file_preview_enabled);
    }

    #[test]
    fn file_preview_preference_updates_only_that_field() {
        let dir = TestDir::new("file-preview-only-field");
        let persisted = Settings {
            hotkey: "Ctrl+Space".into(),
            autostart: true,
            research_id: Some("study_01".into()),
            use_counts: BTreeMap::from([(APP_A.into(), 9)]),
            file_preview_enabled: true,
        };
        write_settings(&dir.current(), &persisted);
        let store = SettingsStore::load(dir.path()).unwrap();

        store.set_file_preview_enabled(false).unwrap();

        assert_eq!(
            store.snapshot(),
            Settings {
                file_preview_enabled: false,
                ..persisted
            }
        );
        assert_eq!(read_current(&dir), store.snapshot());
    }

    #[test]
    fn errors_have_fixed_path_free_messages() {
        fn assert_std_error<T: std::error::Error>() {}
        assert_std_error::<SettingsError>();

        let cases = [
            (SettingsError::Storage, "settings storage failed"),
            (SettingsError::Serialize, "settings serialization failed"),
            (SettingsError::InvalidUpdate, "settings update is invalid"),
            (
                SettingsError::UnknownApplication,
                "settings application is unknown",
            ),
            (SettingsError::CountOverflow, "settings count overflow"),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert!(!error.to_string().contains(':'));
        }
    }
}
