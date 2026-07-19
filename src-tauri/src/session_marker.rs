use std::{fmt::Write as _, path::Path};

use serde::{Deserialize, Serialize};
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};

use crate::{
    atomic_file::{quarantine_invalid, read_optional, replace_without_backup},
    validation_data::{valid_date, valid_session_id, ValidationError, VALIDATION_SCHEMA_VERSION},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMarker {
    pub(crate) schema_version: u32,
    pub(crate) session_id: String,
    pub(crate) local_date: String,
}

pub(crate) fn new_session_id() -> Result<String, ValidationError> {
    let mut bytes = [0_u8; 16];
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_err() {
        return Err(ValidationError::SessionRandom);
    }

    let mut id = String::with_capacity(40);
    id.push_str("session-");
    for byte in bytes {
        write!(id, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(id)
}

pub(crate) fn load_marker_for_reconcile(
    path: &Path,
) -> Result<Option<SessionMarker>, ValidationError> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(None);
    };
    match parse_marker(&bytes) {
        Some(marker) => Ok(Some(marker)),
        None => {
            quarantine_invalid(path)?;
            Ok(None)
        }
    }
}

pub(crate) fn read_marker_for_clean(path: &Path) -> Result<SessionMarker, ValidationError> {
    let bytes = read_optional(path)?.ok_or(ValidationError::SessionOwnershipLost)?;
    parse_marker(&bytes).ok_or(ValidationError::SessionOwnershipLost)
}

pub(crate) fn replace_session_marker(
    path: &Path,
    marker: &SessionMarker,
) -> Result<(), ValidationError> {
    let bytes = serde_json::to_vec(marker).map_err(|_| ValidationError::Serialize)?;
    replace_without_backup(path, &bytes)?;
    Ok(())
}

fn parse_marker(bytes: &[u8]) -> Option<SessionMarker> {
    let marker = serde_json::from_slice::<SessionMarker>(bytes).ok()?;
    (marker.schema_version == VALIDATION_SCHEMA_VERSION
        && valid_session_id(&marker.session_id)
        && valid_date(&marker.local_date))
    .then_some(marker)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::validation_data::{ValidationError, VALIDATION_SCHEMA_VERSION};

    const SESSION_A: &str = "session-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-marker-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn marker(&self) -> PathBuf {
            self.0.join("open-session.json")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    #[test]
    fn generated_session_ids_are_opaque_lowercase_hex() {
        let first = new_session_id().unwrap();
        let second = new_session_id().unwrap();

        assert!(valid_session_id(&first));
        assert!(valid_session_id(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn marker_round_trip_uses_only_schema_id_and_date() {
        let dir = TestDir::new("round-trip");
        let marker = SessionMarker {
            schema_version: VALIDATION_SCHEMA_VERSION,
            session_id: SESSION_A.into(),
            local_date: "2026-07-18".into(),
        };

        replace_session_marker(&dir.marker(), &marker).unwrap();
        let loaded = load_marker_for_reconcile(&dir.marker()).unwrap().unwrap();

        assert_eq!(loaded, marker);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.marker()).unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 3);
    }

    #[test]
    fn malformed_reconcile_marker_is_quarantined() {
        let dir = TestDir::new("malformed-reconcile");
        fs::write(dir.marker(), b"not-json").unwrap();

        assert_eq!(load_marker_for_reconcile(&dir.marker()).unwrap(), None);

        assert!(!dir.marker().exists());
        assert!(fs::read_dir(dir.path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("open-session.json.invalid-")));
    }

    #[test]
    fn clean_reader_preserves_missing_or_malformed_evidence() {
        let dir = TestDir::new("clean-reader");

        assert_eq!(
            read_marker_for_clean(&dir.marker()),
            Err(ValidationError::SessionOwnershipLost)
        );
        fs::write(dir.marker(), b"not-json").unwrap();
        let before = fs::read(dir.marker()).unwrap();
        assert_eq!(
            read_marker_for_clean(&dir.marker()),
            Err(ValidationError::SessionOwnershipLost)
        );
        assert_eq!(fs::read(dir.marker()).unwrap(), before);
    }
}
