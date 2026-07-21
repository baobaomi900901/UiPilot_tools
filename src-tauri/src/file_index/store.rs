use std::{
    ffi::{c_int, c_void, CString},
    path::Path,
    slice, str,
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc,
    },
};

use rusqlite::{
    ffi, params, params_from_iter, types::Value, Connection, OpenFlags, OptionalExtension,
};
use windows::Win32::{
    Globalization::{
        CompareStringOrdinal, GetNLSVersionEx, COMPARE_STRING, LOCALE_NAME_INVARIANT,
        NLSVERSIONINFOEX,
    },
    System::SystemInformation::OSVERSIONINFOW,
};

use super::{
    FileIndexStatus, FileSort, IndexEntry, IndexedKind, QuerySpec, VolumeIdentity,
    FOLD_ALGORITHM_ID,
};

const APPLICATION_ID: i64 = 1_430_868_038;
const USER_VERSION: i64 = 1;

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA application_id=1430868038;
PRAGMA user_version=1;

CREATE TABLE IF NOT EXISTS metadata (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  fold_algorithm_id TEXT NOT NULL,
  ordinal_sort_identity TEXT NOT NULL,
  index_revision TEXT NOT NULL,
  clean_close INTEGER NOT NULL CHECK(clean_close IN (0,1)),
  last_integrity_check_utc TEXT
);
CREATE TABLE IF NOT EXISTS volumes (
  volume_guid_path TEXT NOT NULL,
  volume_serial INTEGER NOT NULL,
  filesystem_name TEXT NOT NULL,
  mount_point TEXT NOT NULL,
  committed_generation TEXT,
  candidate_generation TEXT,
  next_generation TEXT NOT NULL,
  scan_state TEXT NOT NULL CHECK(scan_state IN ('idle','scanning','dirty','partial')),
  PRIMARY KEY(volume_guid_path, volume_serial, filesystem_name)
);
CREATE TABLE IF NOT EXISTS entries (
  row_id INTEGER PRIMARY KEY,
  volume_guid_path TEXT NOT NULL,
  volume_serial INTEGER NOT NULL,
  filesystem_name TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  display_path TEXT NOT NULL,
  name TEXT NOT NULL,
  folded_name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('file','directory')),
  category TEXT NOT NULL,
  size_bytes TEXT,
  modified_utc_ms INTEGER NOT NULL,
  generation TEXT NOT NULL,
  UNIQUE(volume_guid_path, volume_serial, filesystem_name, relative_path)
);
CREATE TABLE IF NOT EXISTS candidate_entries (
  row_id INTEGER PRIMARY KEY,
  volume_guid_path TEXT NOT NULL,
  volume_serial INTEGER NOT NULL,
  filesystem_name TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  display_path TEXT NOT NULL,
  name TEXT NOT NULL,
  folded_name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('file','directory')),
  category TEXT NOT NULL,
  size_bytes TEXT,
  modified_utc_ms INTEGER NOT NULL,
  generation TEXT NOT NULL,
  UNIQUE(volume_guid_path, volume_serial, filesystem_name, relative_path)
);
CREATE VIRTUAL TABLE IF NOT EXISTS entry_names USING fts5(folded_name, content='entries', content_rowid='row_id', tokenize='trigram case_sensitive 1');
CREATE VIRTUAL TABLE IF NOT EXISTS candidate_names USING fts5(folded_name, content='candidate_entries', content_rowid='row_id', tokenize='trigram case_sensitive 1');

CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
  INSERT INTO entry_names(rowid, folded_name) VALUES (new.row_id, new.folded_name);
END;
CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
  INSERT INTO entry_names(entry_names, rowid, folded_name) VALUES ('delete', old.row_id, old.folded_name);
END;
CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE OF folded_name ON entries BEGIN
  INSERT INTO entry_names(entry_names, rowid, folded_name) VALUES ('delete', old.row_id, old.folded_name);
  INSERT INTO entry_names(rowid, folded_name) VALUES (new.row_id, new.folded_name);
END;
CREATE TRIGGER IF NOT EXISTS candidate_ai AFTER INSERT ON candidate_entries BEGIN
  INSERT INTO candidate_names(rowid, folded_name) VALUES (new.row_id, new.folded_name);
END;
CREATE TRIGGER IF NOT EXISTS candidate_ad AFTER DELETE ON candidate_entries BEGIN
  INSERT INTO candidate_names(candidate_names, rowid, folded_name) VALUES ('delete', old.row_id, old.folded_name);
END;
CREATE TRIGGER IF NOT EXISTS candidate_au AFTER UPDATE OF folded_name ON candidate_entries BEGIN
  INSERT INTO candidate_names(candidate_names, rowid, folded_name) VALUES ('delete', old.row_id, old.folded_name);
  INSERT INTO candidate_names(rowid, folded_name) VALUES (new.row_id, new.folded_name);
END;

CREATE INDEX IF NOT EXISTS entries_sort_desc ON entries(modified_utc_ms DESC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS entries_sort_asc ON entries(modified_utc_ms ASC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS entries_category_sort_desc ON entries(category, modified_utc_ms DESC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS entries_category_sort_asc ON entries(category, modified_utc_ms ASC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS candidate_entries_sort_desc ON candidate_entries(modified_utc_ms DESC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS candidate_entries_sort_asc ON candidate_entries(modified_utc_ms ASC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS candidate_entries_category_sort_desc ON candidate_entries(category, modified_utc_ms DESC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
CREATE INDEX IF NOT EXISTS candidate_entries_category_sort_asc ON candidate_entries(category, modified_utc_ms ASC, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC);
"#;

#[derive(Debug)]
pub(super) enum StoreError {
    Sqlite,
    InvalidData,
    Platform,
    RevisionExhausted,
}

impl From<rusqlite::Error> for StoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Sqlite
    }
}

struct CollationContext {
    ignore_case: bool,
    invalid: Arc<AtomicBool>,
}

unsafe extern "C" fn compare_collation(
    context: *mut c_void,
    left_len: c_int,
    left: *const c_void,
    right_len: c_int,
    right: *const c_void,
) -> c_int {
    let context = unsafe { &*(context.cast::<CollationContext>()) };
    let compared = (|| {
        let left_len = usize::try_from(left_len).ok()?;
        let right_len = usize::try_from(right_len).ok()?;
        if (left_len != 0 && left.is_null()) || (right_len != 0 && right.is_null()) {
            return None;
        }
        let left = unsafe { slice::from_raw_parts(left.cast::<u8>(), left_len) };
        let right = unsafe { slice::from_raw_parts(right.cast::<u8>(), right_len) };
        let left: Vec<u16> = str::from_utf8(left).ok()?.encode_utf16().collect();
        let right: Vec<u16> = str::from_utf8(right).ok()?.encode_utf16().collect();
        i32::try_from(left.len()).ok()?;
        i32::try_from(right.len()).ok()?;
        let result = unsafe { CompareStringOrdinal(&left, &right, context.ignore_case) };
        Some(match result.0 {
            1 => -1,
            2 => 0,
            3 => 1,
            _ => return None,
        })
    })();
    compared.unwrap_or_else(|| {
        context.invalid.store(true, AtomicOrdering::Release);
        0
    })
}

unsafe extern "C" fn destroy_collation(context: *mut c_void) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context.cast::<CollationContext>()) });
    }
}

fn register_collation(
    connection: &Connection,
    name: &str,
    ignore_case: bool,
) -> Result<Arc<AtomicBool>, StoreError> {
    let invalid = Arc::new(AtomicBool::new(false));
    let context = Box::into_raw(Box::new(CollationContext {
        ignore_case,
        invalid: Arc::clone(&invalid),
    }));
    let name = CString::new(name).map_err(|_| StoreError::InvalidData)?;
    let result = unsafe {
        ffi::sqlite3_create_collation_v2(
            connection.handle(),
            name.as_ptr(),
            ffi::SQLITE_UTF8,
            context.cast(),
            Some(compare_collation),
            Some(destroy_collation),
        )
    };
    if result != ffi::SQLITE_OK {
        unsafe { destroy_collation(context.cast()) };
        return Err(StoreError::Sqlite);
    }
    Ok(invalid)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueryStrategy {
    Empty,
    Instr,
    Trigram,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoredEntry {
    pub(super) display_path: String,
    pub(super) name: String,
    pub(super) kind: IndexedKind,
    pub(super) size_bytes: Option<u64>,
    pub(super) modified_utc: String,
}

pub(super) struct StoreQueryResult {
    pub(super) index_revision: u64,
    pub(super) total: u64,
    pub(super) status: FileIndexStatus,
    pub(super) entries: Vec<StoredEntry>,
    #[cfg(test)]
    pub(super) strategy: QueryStrategy,
}

pub(super) struct Store {
    connection: Connection,
    invalid_collations: [Arc<AtomicBool>; 2],
    #[cfg(test)]
    reindex_statement_count: usize,
}

impl Store {
    pub(super) fn open(path: &Path, ordinal_identity: &str) -> Result<Self, StoreError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        Self::initialize(connection, ordinal_identity)
    }

    fn initialize(connection: Connection, ordinal_identity: &str) -> Result<Self, StoreError> {
        let invalid_collations = [
            register_collation(&connection, "uipilot_name_ordinal_ci", true)?,
            register_collation(&connection, "uipilot_path_ordinal_cs", false)?,
        ];
        let page_count: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        if page_count == 0 {
            connection.execute_batch(SCHEMA)?;
            connection.execute(
                "INSERT INTO metadata(singleton, fold_algorithm_id, ordinal_sort_identity, index_revision, clean_close, last_integrity_check_utc) VALUES(1, ?1, ?2, '0', 0, NULL)",
                params![FOLD_ALGORITHM_ID, ordinal_identity],
            )?;
        } else {
            validate_existing_schema(&connection)?;
        }
        Ok(Self {
            connection,
            invalid_collations,
            #[cfg(test)]
            reindex_statement_count: 0,
        })
    }

    pub(super) fn ensure_sort_identity(
        &mut self,
        ordinal_identity: &str,
    ) -> Result<Option<(u64, u64)>, StoreError> {
        let current: String = self.connection.query_row(
            "SELECT ordinal_sort_identity FROM metadata WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if current == ordinal_identity {
            return Ok(None);
        }
        let previous = self.index_revision()?;
        let revision = previous
            .checked_add(1)
            .ok_or(StoreError::RevisionExhausted)?;
        let transaction = self.connection.transaction()?;
        transaction
            .execute_batch("REINDEX uipilot_name_ordinal_ci; REINDEX uipilot_path_ordinal_cs;")?;
        transaction.execute(
            "UPDATE metadata SET ordinal_sort_identity=?1, index_revision=?2 WHERE singleton=1",
            params![ordinal_identity, revision.to_string()],
        )?;
        transaction.commit()?;
        #[cfg(test)]
        {
            self.reindex_statement_count += 2;
        }
        Ok(Some((previous, revision)))
    }

    pub(super) fn index_revision(&self) -> Result<u64, StoreError> {
        let value: String = self.connection.query_row(
            "SELECT index_revision FROM metadata WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        parse_canonical_u64(&value)
    }

    pub(super) fn persist_index_revision(&mut self, revision: u64) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        if transaction.execute(
            "UPDATE metadata SET index_revision=?1 WHERE singleton=1",
            [revision.to_string()],
        )? != 1
        {
            return Err(StoreError::InvalidData);
        }
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn recover_candidates(&mut self) -> Result<Option<u64>, StoreError> {
        let transaction = self.connection.transaction()?;
        let pending: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM volumes WHERE candidate_generation IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        if pending == 0 {
            transaction.commit()?;
            return Ok(None);
        }
        transaction.execute("DELETE FROM candidate_entries", [])?;
        let mut statement = transaction.prepare(
            "SELECT volume_guid_path, volume_serial, filesystem_name, next_generation FROM volumes WHERE candidate_generation IS NOT NULL",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (guid, serial, filesystem, next) in rows {
            let next = parse_canonical_u64(&next)?
                .checked_add(1)
                .ok_or(StoreError::RevisionExhausted)?;
            transaction.execute(
                "UPDATE volumes SET candidate_generation=NULL, next_generation=?1, scan_state='dirty' WHERE volume_guid_path=?2 AND volume_serial=?3 AND filesystem_name=?4",
                params![next.to_string(), guid, serial, filesystem],
            )?;
        }
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok(Some(revision))
    }

    pub(super) fn begin_candidate(
        &mut self,
        volume: &VolumeIdentity,
        mount_point: &str,
    ) -> Result<(u64, u64, bool), StoreError> {
        let transaction = self.connection.transaction()?;
        let existing = transaction
            .query_row(
                "SELECT committed_generation, next_generation FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let (committed, generation) = match existing {
            Some((committed, next)) => (committed, parse_canonical_u64(&next)?),
            None => (None, 1),
        };
        let next = generation
            .checked_add(1)
            .ok_or(StoreError::RevisionExhausted)?;
        transaction.execute(
            "DELETE FROM candidate_entries WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
            params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        )?;
        transaction.execute(
            "INSERT INTO volumes(volume_guid_path,volume_serial,filesystem_name,mount_point,committed_generation,candidate_generation,next_generation,scan_state) VALUES(?1,?2,?3,?4,?5,?6,?7,'scanning') ON CONFLICT(volume_guid_path,volume_serial,filesystem_name) DO UPDATE SET mount_point=excluded.mount_point,candidate_generation=excluded.candidate_generation,next_generation=excluded.next_generation,scan_state='scanning'",
            params![
                volume.volume_guid_path,
                volume.volume_serial,
                volume.filesystem_name,
                mount_point,
                committed,
                generation.to_string(),
                next.to_string(),
            ],
        )?;
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok((generation, revision, committed.is_some()))
    }

    pub(super) fn reconcile_current_mounts(
        &mut self,
        current: &[(VolumeIdentity, String)],
        transitions: &[VolumeIdentity],
    ) -> Result<(Vec<VolumeIdentity>, u64, bool), StoreError> {
        let transaction = self.connection.transaction()?;
        let mut authenticated = Vec::new();
        let mut changed = !transitions.is_empty();
        for identity in transitions {
            if let Some((_, mount)) = current.iter().find(|(current, _)| current == identity) {
                transaction.execute(
                    "UPDATE volumes SET mount_point=?1,scan_state='dirty' WHERE volume_guid_path=?2 AND volume_serial=?3 AND filesystem_name=?4",
                    params![mount, identity.volume_guid_path, identity.volume_serial, identity.filesystem_name],
                )?;
            } else {
                transaction.execute(
                    "UPDATE volumes SET scan_state='dirty' WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                    params![identity.volume_guid_path, identity.volume_serial, identity.filesystem_name],
                )?;
            }
        }
        for (identity, mount) in current {
            let stored = transaction
                .query_row(
                    "SELECT mount_point FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                    params![identity.volume_guid_path, identity.volume_serial, identity.filesystem_name],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            match stored {
                Some(stored) if stored != *mount => {
                    if transaction.execute(
                        "UPDATE volumes SET mount_point=?1,scan_state='dirty' WHERE volume_guid_path=?2 AND volume_serial=?3 AND filesystem_name=?4",
                        params![mount, identity.volume_guid_path, identity.volume_serial, identity.filesystem_name],
                    )? != 1
                    {
                        return Err(StoreError::InvalidData);
                    }
                    changed = true;
                }
                Some(_) | None if !transitions.contains(identity) => {
                    authenticated.push(identity.clone());
                }
                Some(_) | None => {}
            }
        }
        let revision = if changed {
            advance_revision(&transaction)?
        } else {
            read_revision(&transaction)?
        };
        transaction.commit()?;
        Ok((authenticated, revision, changed))
    }

    pub(super) fn append_candidate(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        entries: impl IntoIterator<Item = IndexEntry>,
    ) -> Result<u64, StoreError> {
        let transaction = self.connection.transaction()?;
        require_candidate(&transaction, volume, generation)?;
        for entry in entries {
            upsert_entry(
                &transaction,
                "candidate_entries",
                volume,
                generation,
                &entry,
            )?;
        }
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok(revision)
    }

    #[cfg(not(test))]
    pub(super) fn apply_live_changes<'a>(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        deleted_prefixes: impl IntoIterator<Item = &'a str>,
        entries: impl IntoIterator<Item = IndexEntry>,
    ) -> Result<u64, StoreError> {
        let transaction = self.connection.transaction()?;
        let (candidate, committed) = require_generation(&transaction, volume, generation)?;
        for prefix in deleted_prefixes {
            if candidate.is_some() {
                delete_prefix(&transaction, "candidate_entries", volume, prefix)?;
            }
            if committed.is_some() {
                delete_prefix(&transaction, "entries", volume, prefix)?;
            }
        }
        for entry in entries {
            if let Some(candidate) = candidate {
                upsert_entry(&transaction, "candidate_entries", volume, candidate, &entry)?;
            }
            if let Some(committed) = committed {
                upsert_entry(&transaction, "entries", volume, committed, &entry)?;
            }
        }
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok(revision)
    }

    pub(super) fn apply_committed_changes_during_scan<'a>(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        deleted_prefixes: impl IntoIterator<Item = &'a str>,
        entries: impl IntoIterator<Item = IndexEntry>,
    ) -> Result<u64, StoreError> {
        let transaction = self.connection.transaction()?;
        let (_, committed) = require_generation(&transaction, volume, generation)?;
        let Some(committed) = committed else {
            let revision = read_revision(&transaction)?;
            transaction.commit()?;
            return Ok(revision);
        };
        for prefix in deleted_prefixes {
            delete_prefix(&transaction, "entries", volume, prefix)?;
        }
        for entry in entries {
            upsert_entry(&transaction, "entries", volume, committed, &entry)?;
        }
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok(revision)
    }

    pub(super) fn commit_candidate(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        final_entries: Vec<IndexEntry>,
        replay_deleted_prefixes: &[String],
        replay_entries: Vec<IndexEntry>,
        denied_prefixes: &[String],
    ) -> Result<u64, StoreError> {
        let transaction = self.connection.transaction()?;
        require_candidate(&transaction, volume, generation)?;
        for entry in final_entries {
            upsert_entry(
                &transaction,
                "candidate_entries",
                volume,
                generation,
                &entry,
            )?;
        }
        for prefix in denied_prefixes {
            copy_denied_prefix(&transaction, volume, generation, prefix)?;
        }
        for prefix in replay_deleted_prefixes {
            delete_prefix(&transaction, "candidate_entries", volume, prefix)?;
        }
        for entry in replay_entries {
            upsert_entry(
                &transaction,
                "candidate_entries",
                volume,
                generation,
                &entry,
            )?;
        }
        transaction.execute(
            "DELETE FROM entries WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
            params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        )?;
        transaction.execute(
            "INSERT INTO entries(volume_guid_path,volume_serial,filesystem_name,relative_path,display_path,name,folded_name,kind,category,size_bytes,modified_utc_ms,generation) SELECT volume_guid_path,volume_serial,filesystem_name,relative_path,display_path,name,folded_name,kind,category,size_bytes,modified_utc_ms,generation FROM candidate_entries WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3 AND generation=?4",
            params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name, generation.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM candidate_entries WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
            params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        )?;
        if transaction.execute(
            "UPDATE volumes SET committed_generation=?1,candidate_generation=NULL,scan_state=?2 WHERE volume_guid_path=?3 AND volume_serial=?4 AND filesystem_name=?5",
            params![generation.to_string(), if denied_prefixes.is_empty() { "idle" } else { "partial" }, volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        )? != 1 {
            return Err(StoreError::InvalidData);
        }
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok(revision)
    }

    #[cfg(test)]
    pub(super) fn fail_candidate(&mut self, volume: &VolumeIdentity) -> Result<u64, StoreError> {
        let mount_point = self
            .connection
            .query_row(
                "SELECT mount_point FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(StoreError::InvalidData)?;
        self.mark_volume_dirty(volume, &mount_point)
    }

    pub(super) fn mark_volume_dirty(
        &mut self,
        volume: &VolumeIdentity,
        mount_point: &str,
    ) -> Result<u64, StoreError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM candidate_entries WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
            params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        )?;
        transaction.execute(
            "INSERT INTO volumes(volume_guid_path,volume_serial,filesystem_name,mount_point,committed_generation,candidate_generation,next_generation,scan_state) VALUES(?1,?2,?3,?4,NULL,NULL,'1','dirty') ON CONFLICT(volume_guid_path,volume_serial,filesystem_name) DO UPDATE SET mount_point=excluded.mount_point,candidate_generation=NULL,scan_state='dirty'",
            params![
                volume.volume_guid_path,
                volume.volume_serial,
                volume.filesystem_name,
                mount_point,
            ],
        )?;
        let revision = advance_revision(&transaction)?;
        transaction.commit()?;
        Ok(revision)
    }

    pub(super) fn query(
        &mut self,
        spec: &QuerySpec,
        identities: &[VolumeIdentity],
    ) -> Result<StoreQueryResult, StoreError> {
        self.query_with_hook(spec, identities, || {})
    }

    fn query_with_hook<F>(
        &mut self,
        spec: &QuerySpec,
        identities: &[VolumeIdentity],
        after_snapshot: F,
    ) -> Result<StoreQueryResult, StoreError>
    where
        F: FnOnce(),
    {
        for invalid in &self.invalid_collations {
            invalid.store(false, AtomicOrdering::Release);
        }
        let strategy = match spec.folded_query.chars().count() {
            0 => QueryStrategy::Empty,
            1 | 2 => QueryStrategy::Instr,
            _ => QueryStrategy::Trigram,
        };
        let transaction = self.connection.transaction()?;
        let index_revision = read_revision(&transaction)?;
        let status = read_status(&transaction, identities)?;
        after_snapshot();
        if strategy == QueryStrategy::Empty || identities.is_empty() {
            transaction.commit()?;
            return Ok(StoreQueryResult {
                index_revision,
                total: 0,
                status,
                entries: Vec::new(),
                #[cfg(test)]
                strategy,
            });
        }

        let (from, predicate, values) = query_parts(spec, identities, strategy);
        let count_sql = format!("SELECT COUNT(*) {from} WHERE {predicate}");
        let total_i64: i64 =
            transaction.query_row(&count_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })?;
        let order = match spec.sort {
            FileSort::ModifiedDesc => "DESC",
            FileSort::ModifiedAsc => "ASC",
        };
        let item_sql = format!(
            "SELECT e.display_path, e.name, e.kind, e.size_bytes, strftime('%Y-%m-%dT%H:%M:%fZ', e.modified_utc_ms / 1000.0, 'unixepoch') {from} WHERE {predicate} ORDER BY e.modified_utc_ms {order}, e.name COLLATE uipilot_name_ordinal_ci ASC, e.display_path COLLATE uipilot_path_ordinal_cs ASC LIMIT 200"
        );
        let mut statement = transaction.prepare(&item_sql)?;
        let entries = statement
            .query_map(params_from_iter(values.iter()), |row| {
                let kind: String = row.get(2)?;
                let size: Option<String> = row.get(3)?;
                let modified_utc: Option<String> = row.get(4)?;
                Ok(StoredEntry {
                    display_path: row.get(0)?,
                    name: row.get(1)?,
                    kind: match kind.as_str() {
                        "file" => IndexedKind::File,
                        "directory" => IndexedKind::Directory,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    },
                    size_bytes: size.as_deref().map(parse_canonical_u64_sql).transpose()?,
                    modified_utc: modified_utc.ok_or(rusqlite::Error::InvalidQuery)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        transaction.commit()?;
        if self
            .invalid_collations
            .iter()
            .any(|invalid| invalid.load(AtomicOrdering::Acquire))
        {
            return Err(StoreError::InvalidData);
        }
        Ok(StoreQueryResult {
            index_revision,
            total: u64::try_from(total_i64).map_err(|_| StoreError::InvalidData)?,
            status,
            entries,
            #[cfg(test)]
            strategy,
        })
    }
}

fn validate_existing_schema(connection: &Connection) -> Result<(), StoreError> {
    let application_id: i64 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let journal_mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    if application_id != APPLICATION_ID
        || user_version != USER_VERSION
        || !journal_mode.eq_ignore_ascii_case("wal")
    {
        return Err(StoreError::InvalidData);
    }

    if schema_manifest(connection)? != canonical_schema_manifest()? {
        return Err(StoreError::InvalidData);
    }

    let metadata_rows: i64 =
        connection.query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))?;
    if metadata_rows != 1 {
        return Err(StoreError::InvalidData);
    }
    let (singleton, fold_algorithm_id, ordinal_identity, revision): (i64, String, String, String) =
        connection.query_row(
            "SELECT singleton, fold_algorithm_id, ordinal_sort_identity, index_revision FROM metadata",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    if singleton != 1 || fold_algorithm_id != FOLD_ALGORITHM_ID || ordinal_identity.is_empty() {
        return Err(StoreError::InvalidData);
    }
    parse_canonical_u64(&revision)?;
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct SchemaObject {
    kind: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

fn schema_manifest(connection: &Connection) -> Result<Vec<SchemaObject>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name, tbl_name",
    )?;
    let manifest = statement
        .query_map([], |row| {
            Ok(SchemaObject {
                kind: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row
                    .get::<_, Option<String>>(3)?
                    .map(|sql| sql.split_whitespace().collect::<Vec<_>>().join(" ")),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(manifest)
}

fn canonical_schema_manifest() -> Result<Vec<SchemaObject>, StoreError> {
    let connection = Connection::open_in_memory()?;
    register_collation(&connection, "uipilot_name_ordinal_ci", true)?;
    register_collation(&connection, "uipilot_path_ordinal_cs", false)?;
    connection.execute_batch(SCHEMA)?;
    schema_manifest(&connection)
}

fn parse_canonical_u64(value: &str) -> Result<u64, StoreError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(StoreError::InvalidData);
    }
    value.parse().map_err(|_| StoreError::InvalidData)
}

fn parse_canonical_u64_sql(value: &str) -> Result<u64, rusqlite::Error> {
    parse_canonical_u64(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn read_revision(transaction: &rusqlite::Transaction<'_>) -> Result<u64, StoreError> {
    let value: String = transaction.query_row(
        "SELECT index_revision FROM metadata WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    parse_canonical_u64(&value)
}

fn advance_revision(transaction: &rusqlite::Transaction<'_>) -> Result<u64, StoreError> {
    let next = read_revision(transaction)?
        .checked_add(1)
        .ok_or(StoreError::RevisionExhausted)?;
    if transaction.execute(
        "UPDATE metadata SET index_revision=?1 WHERE singleton=1",
        [next.to_string()],
    )? != 1
    {
        return Err(StoreError::InvalidData);
    }
    Ok(next)
}

fn require_candidate(
    transaction: &rusqlite::Transaction<'_>,
    volume: &VolumeIdentity,
    generation: u64,
) -> Result<Option<u64>, StoreError> {
    let (candidate, committed): (Option<String>, Option<String>) = transaction.query_row(
        "SELECT candidate_generation,committed_generation FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
        params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if candidate.as_deref() != Some(generation.to_string().as_str()) {
        return Err(StoreError::InvalidData);
    }
    committed.as_deref().map(parse_canonical_u64).transpose()
}

fn require_generation(
    transaction: &rusqlite::Transaction<'_>,
    volume: &VolumeIdentity,
    generation: u64,
) -> Result<(Option<u64>, Option<u64>), StoreError> {
    let (candidate, committed): (Option<String>, Option<String>) = transaction.query_row(
        "SELECT candidate_generation,committed_generation FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
        params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let candidate = candidate.as_deref().map(parse_canonical_u64).transpose()?;
    let committed = committed.as_deref().map(parse_canonical_u64).transpose()?;
    if candidate != Some(generation) && committed != Some(generation) {
        return Err(StoreError::InvalidData);
    }
    Ok((candidate, committed))
}

fn delete_prefix(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    volume: &VolumeIdentity,
    prefix: &str,
) -> Result<(), StoreError> {
    if table != "entries" && table != "candidate_entries" {
        return Err(StoreError::InvalidData);
    }
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let descendants = format!("{escaped}\\\\%");
    transaction.execute(
        &format!("DELETE FROM {table} WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3 AND (relative_path=?4 OR relative_path LIKE ?5 ESCAPE '\\')"),
        params![
            volume.volume_guid_path,
            volume.volume_serial,
            volume.filesystem_name,
            prefix,
            descendants,
        ],
    )?;
    Ok(())
}

fn copy_denied_prefix(
    transaction: &rusqlite::Transaction<'_>,
    volume: &VolumeIdentity,
    generation: u64,
    prefix: &str,
) -> Result<(), StoreError> {
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let descendants = format!("{escaped}\\\\%");
    transaction.execute(
        "INSERT OR IGNORE INTO candidate_entries(volume_guid_path,volume_serial,filesystem_name,relative_path,display_path,name,folded_name,kind,category,size_bytes,modified_utc_ms,generation) SELECT e.volume_guid_path,e.volume_serial,e.filesystem_name,e.relative_path,CASE WHEN substr(v.mount_point,-1,1)=char(92) OR substr(v.mount_point,-1,1)='/' THEN v.mount_point || e.relative_path ELSE v.mount_point || char(92) || e.relative_path END,e.name,e.folded_name,e.kind,e.category,e.size_bytes,e.modified_utc_ms,?1 FROM entries e JOIN volumes v ON v.volume_guid_path=e.volume_guid_path AND v.volume_serial=e.volume_serial AND v.filesystem_name=e.filesystem_name WHERE e.volume_guid_path=?2 AND e.volume_serial=?3 AND e.filesystem_name=?4 AND (?5='' OR e.relative_path=?5 OR e.relative_path LIKE ?6 ESCAPE '\\')",
        params![
            generation.to_string(),
            volume.volume_guid_path,
            volume.volume_serial,
            volume.filesystem_name,
            prefix,
            descendants,
        ],
    )?;
    Ok(())
}

fn upsert_entry(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    volume: &VolumeIdentity,
    generation: u64,
    entry: &IndexEntry,
) -> Result<(), StoreError> {
    if table != "entries" && table != "candidate_entries" {
        return Err(StoreError::InvalidData);
    }
    let sql = format!(
        "INSERT INTO {table}(volume_guid_path,volume_serial,filesystem_name,relative_path,display_path,name,folded_name,kind,category,size_bytes,modified_utc_ms,generation) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(volume_guid_path,volume_serial,filesystem_name,relative_path) DO UPDATE SET display_path=excluded.display_path,name=excluded.name,folded_name=excluded.folded_name,kind=excluded.kind,category=excluded.category,size_bytes=excluded.size_bytes,modified_utc_ms=excluded.modified_utc_ms,generation=excluded.generation"
    );
    transaction.execute(
        &sql,
        params![
            volume.volume_guid_path,
            volume.volume_serial,
            volume.filesystem_name,
            entry.relative_path,
            entry.display_path,
            entry.name,
            entry.folded_name,
            match entry.kind {
                IndexedKind::File => "file",
                IndexedKind::Directory => "directory",
            },
            entry.category,
            entry.size_bytes.map(|value| value.to_string()),
            entry.modified_utc_ms,
            generation.to_string(),
        ],
    )?;
    Ok(())
}

fn read_status(
    transaction: &rusqlite::Transaction<'_>,
    identities: &[VolumeIdentity],
) -> Result<FileIndexStatus, StoreError> {
    if identities.is_empty() {
        return Ok(FileIndexStatus::Building);
    }
    let (identity_sql, values) = identity_predicate("v", identities);
    let (volumes, building, partial): (i64, i64, i64) = transaction.query_row(
        &format!("SELECT COUNT(*), COALESCE(SUM(CASE WHEN committed_generation IS NULL OR scan_state IN ('scanning','dirty') THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN scan_state='partial' THEN 1 ELSE 0 END),0) FROM volumes v WHERE {identity_sql}"),
        params_from_iter(values.iter()),
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(if volumes == 0 || building != 0 {
        FileIndexStatus::Building
    } else if partial != 0 {
        FileIndexStatus::Partial
    } else {
        FileIndexStatus::Ready
    })
}

fn query_parts(
    spec: &QuerySpec,
    identities: &[VolumeIdentity],
    strategy: QueryStrategy,
) -> (String, String, Vec<Value>) {
    let from = "FROM (
        SELECT e.*,0 AS candidate FROM entries e
        JOIN volumes v ON v.volume_guid_path=e.volume_guid_path AND v.volume_serial=e.volume_serial AND v.filesystem_name=e.filesystem_name
        WHERE v.committed_generation IS NOT NULL
        UNION ALL
        SELECT c.*,1 AS candidate FROM candidate_entries c
        JOIN volumes v ON v.volume_guid_path=c.volume_guid_path AND v.volume_serial=c.volume_serial AND v.filesystem_name=c.filesystem_name
        WHERE v.committed_generation IS NULL AND v.candidate_generation IS NOT NULL
    ) e"
        .to_owned();
    let (identity_sql, mut values) = identity_predicate("e", identities);
    let mut predicates = vec![identity_sql];
    match strategy {
        QueryStrategy::Instr => {
            predicates.push("instr(e.folded_name, ?) > 0".to_owned());
            values.push(Value::Text(spec.folded_query.clone()));
        }
        QueryStrategy::Trigram => {
            let phrase = Value::Text(format!("\"{}\"", spec.folded_query.replace('"', "\"\"")));
            predicates.push("((e.candidate=0 AND e.row_id IN (SELECT rowid FROM entry_names WHERE folded_name MATCH ?)) OR (e.candidate=1 AND e.row_id IN (SELECT rowid FROM candidate_names WHERE folded_name MATCH ?)))".to_owned());
            values.push(phrase.clone());
            values.push(phrase);
        }
        QueryStrategy::Empty => {}
    }
    if let Some(category) = spec.category.store_value() {
        predicates.push("e.category=?".to_owned());
        values.push(Value::Text(category.to_owned()));
    }
    (from, predicates.join(" AND "), values)
}

fn identity_predicate(alias: &str, identities: &[VolumeIdentity]) -> (String, Vec<Value>) {
    let mut values = Vec::with_capacity(identities.len() * 3);
    let clauses = identities
        .iter()
        .map(|identity| {
            values.push(Value::Text(identity.volume_guid_path.clone()));
            values.push(Value::Integer(i64::from(identity.volume_serial)));
            values.push(Value::Text(identity.filesystem_name.clone()));
            format!("({alias}.volume_guid_path=? AND {alias}.volume_serial=? AND {alias}.filesystem_name=?)")
        })
        .collect::<Vec<_>>();
    (format!("({})", clauses.join(" OR ")), values)
}

pub(super) fn ordinal_sort_identity() -> Result<String, StoreError> {
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut OSVERSIONINFOW) -> i32;
    }

    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(std::mem::size_of::<OSVERSIONINFOW>())
            .map_err(|_| StoreError::Platform)?,
        ..Default::default()
    };
    if unsafe { RtlGetVersion(&mut version) } != 0 {
        return Err(StoreError::Platform);
    }
    let mut nls = NLSVERSIONINFOEX {
        dwNLSVersionInfoSize: u32::try_from(std::mem::size_of::<NLSVERSIONINFOEX>())
            .map_err(|_| StoreError::Platform)?,
        ..Default::default()
    };
    unsafe {
        GetNLSVersionEx(
            u32::try_from(COMPARE_STRING.0).map_err(|_| StoreError::Platform)?,
            LOCALE_NAME_INVARIANT,
            &mut nls,
        )
    }
    .map_err(|_| StoreError::Platform)?;
    let guid = nls.guidCustomVersion;
    Ok(format!(
        "{}.{}.{}:{}:{}:{}:{:08x}-{:04x}-{:04x}-{:02x?}",
        version.dwMajorVersion,
        version.dwMinorVersion,
        version.dwBuildNumber,
        nls.dwNLSVersion,
        nls.dwDefinedVersion,
        nls.dwEffectiveId,
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4,
    ))
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct TestEntry {
    pub(super) relative_path: String,
    pub(super) display_path: String,
    pub(super) name: String,
    pub(super) folded_name: String,
    pub(super) kind: IndexedKind,
    pub(super) category: String,
    pub(super) size_bytes: Option<u64>,
    pub(super) modified_utc_ms: i64,
    pub(super) generation: u64,
}

#[cfg(test)]
impl Store {
    pub(super) fn open_in_memory_for_test(identity: &str) -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?, identity)
    }

    fn pragma_i64(&self, name: &str) -> i64 {
        self.connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    }

    fn pragma_text(&self, name: &str) -> String {
        self.connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    }

    fn schema_objects(&self) -> Vec<String> {
        let mut statement = self
            .connection
            .prepare("SELECT name FROM sqlite_master ORDER BY name")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn metadata_integrity_marker(&self) -> (bool, Option<String>) {
        self.connection
            .query_row(
                "SELECT clean_close, last_integrity_check_utc FROM metadata WHERE singleton=1",
                [],
                |row| Ok((row.get::<_, i64>(0)? != 0, row.get(1)?)),
            )
            .unwrap()
    }

    pub(super) fn seed_committed_for_test(
        &mut self,
        volume: &VolumeIdentity,
        entries: impl IntoIterator<Item = TestEntry>,
    ) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT OR REPLACE INTO volumes(volume_guid_path, volume_serial, filesystem_name, mount_point, committed_generation, candidate_generation, next_generation, scan_state) VALUES(?1,?2,?3,'C:\\','1',NULL,'2','idle')",
            params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
        )?;
        for entry in entries {
            transaction.execute(
                "INSERT INTO entries(volume_guid_path, volume_serial, filesystem_name, relative_path, display_path, name, folded_name, kind, category, size_bytes, modified_utc_ms, generation) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    volume.volume_guid_path,
                    volume.volume_serial,
                    volume.filesystem_name,
                    entry.relative_path,
                    entry.display_path,
                    entry.name,
                    entry.folded_name,
                    match entry.kind {
                        IndexedKind::File => "file",
                        IndexedKind::Directory => "directory",
                    },
                    entry.category,
                    entry.size_bytes.map(|value| value.to_string()),
                    entry.modified_utc_ms,
                    entry.generation.to_string(),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn query_for_test(
        &mut self,
        spec: &QuerySpec,
        identities: &[VolumeIdentity],
    ) -> Result<StoreQueryResult, StoreError> {
        self.query(spec, identities)
    }

    fn query_with_hook_for_test<F>(
        &mut self,
        spec: &QuerySpec,
        identities: &[VolumeIdentity],
        after_snapshot: F,
    ) -> Result<StoreQueryResult, StoreError>
    where
        F: FnOnce(),
    {
        self.query_with_hook(spec, identities, after_snapshot)
    }

    fn ordered_names_for_test(&self, sort: FileSort) -> Vec<String> {
        let order = match sort {
            FileSort::ModifiedDesc => "DESC",
            FileSort::ModifiedAsc => "ASC",
        };
        let sql = format!("SELECT name FROM entries ORDER BY modified_utc_ms {order}, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC");
        let mut statement = self.connection.prepare(&sql).unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn query_plan_uses_sort_index_for_test(&self, sort: FileSort) -> bool {
        let (order, index) = match sort {
            FileSort::ModifiedDesc => ("DESC", "entries_sort_desc"),
            FileSort::ModifiedAsc => ("ASC", "entries_sort_asc"),
        };
        let sql = format!("EXPLAIN QUERY PLAN SELECT row_id FROM entries ORDER BY modified_utc_ms {order}, name COLLATE uipilot_name_ordinal_ci ASC, display_path COLLATE uipilot_path_ordinal_cs ASC LIMIT 200");
        let mut statement = self.connection.prepare(&sql).unwrap();
        let details = statement
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        details.iter().any(|detail| detail.contains(index))
            && details.iter().all(|detail| !detail.contains("TEMP B-TREE"))
    }

    fn ensure_sort_identity_for_test(&mut self, identity: &str) -> Result<bool, StoreError> {
        self.ensure_sort_identity(identity)
            .map(|revision| revision.is_some())
    }

    fn ordinal_identity_for_test(&self) -> String {
        self.connection
            .query_row(
                "SELECT ordinal_sort_identity FROM metadata WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    pub(super) fn index_revision_for_test(&self) -> u64 {
        self.index_revision().unwrap()
    }

    pub(super) fn remove_metadata_for_test(&self) {
        self.connection.execute("DELETE FROM metadata", []).unwrap();
    }

    fn reindex_statement_count_for_test(&self) -> usize {
        self.reindex_statement_count
    }

    pub(super) fn begin_candidate_for_test(
        &mut self,
        volume: &VolumeIdentity,
        mount_point: &str,
    ) -> Result<u64, StoreError> {
        self.begin_candidate(volume, mount_point)
            .map(|(generation, _, _)| generation)
    }

    pub(super) fn append_candidate_for_test(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        entries: impl IntoIterator<Item = TestEntry>,
    ) -> Result<u64, StoreError> {
        self.append_candidate(
            volume,
            generation,
            entries.into_iter().map(IndexEntry::from),
        )
    }

    pub(super) fn apply_committed_entry_for_test(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        entry: TestEntry,
    ) -> Result<u64, StoreError> {
        self.apply_committed_changes_during_scan(
            volume,
            generation,
            std::iter::empty::<&str>(),
            [IndexEntry::from(entry)],
        )
    }

    pub(super) fn commit_candidate_for_test(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        denied_prefixes: &[&str],
    ) -> Result<u64, StoreError> {
        self.commit_candidate(
            volume,
            generation,
            Vec::new(),
            &[],
            Vec::new(),
            &denied_prefixes
                .iter()
                .map(|prefix| (*prefix).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    pub(super) fn commit_candidate_with_replay_for_test(
        &mut self,
        volume: &VolumeIdentity,
        generation: u64,
        final_entries: impl IntoIterator<Item = TestEntry>,
        replay_entries: impl IntoIterator<Item = TestEntry>,
        denied_prefixes: &[&str],
    ) -> Result<u64, StoreError> {
        self.commit_candidate(
            volume,
            generation,
            final_entries.into_iter().map(IndexEntry::from).collect(),
            &[],
            replay_entries.into_iter().map(IndexEntry::from).collect(),
            &denied_prefixes
                .iter()
                .map(|prefix| (*prefix).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    pub(super) fn fail_candidate_for_test(
        &mut self,
        volume: &VolumeIdentity,
    ) -> Result<u64, StoreError> {
        self.fail_candidate(volume)
    }

    pub(super) fn recover_candidates_for_test(&mut self) -> Result<Option<u64>, StoreError> {
        self.recover_candidates()
    }

    pub(super) fn candidate_rows_for_test(&self, volume: &VolumeIdentity) -> Vec<String> {
        let mut statement = self
            .connection
            .prepare("SELECT name FROM candidate_entries WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3 ORDER BY name")
            .unwrap();
        statement
            .query_map(
                params![
                    volume.volume_guid_path,
                    volume.volume_serial,
                    volume.filesystem_name
                ],
                |row| row.get(0),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    pub(super) fn generation_state_for_test(
        &self,
        volume: &VolumeIdentity,
    ) -> (Option<u64>, Option<u64>, u64, String) {
        let (committed, candidate, next, state): (
            Option<String>,
            Option<String>,
            String,
            String,
        ) = self
            .connection
            .query_row(
                "SELECT committed_generation,candidate_generation,next_generation,scan_state FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        (
            committed
                .as_deref()
                .map(parse_canonical_u64)
                .transpose()
                .unwrap(),
            candidate
                .as_deref()
                .map(parse_canonical_u64)
                .transpose()
                .unwrap(),
            parse_canonical_u64(&next).unwrap(),
            state,
        )
    }

    pub(super) fn mount_point_for_test(&self, volume: &VolumeIdentity) -> String {
        self.connection
            .query_row(
                "SELECT mount_point FROM volumes WHERE volume_guid_path=?1 AND volume_serial=?2 AND filesystem_name=?3",
                params![volume.volume_guid_path, volume.volume_serial, volume.filesystem_name],
                |row| row.get(0),
            )
            .unwrap()
    }

    pub(super) fn fail_revision_updates_for_test(&self) {
        self.connection
            .execute_batch(
                "CREATE TEMP TRIGGER fail_revision_update BEFORE UPDATE OF index_revision ON metadata BEGIN SELECT RAISE(ABORT,'revision failed'); END;",
            )
            .unwrap();
    }
}

#[cfg(test)]
impl From<TestEntry> for IndexEntry {
    fn from(entry: TestEntry) -> Self {
        Self {
            relative_path: entry.relative_path,
            display_path: entry.display_path,
            name: entry.name,
            folded_name: entry.folded_name,
            kind: entry.kind,
            category: entry.category,
            size_bytes: entry.size_bytes,
            modified_utc_ms: entry.modified_utc_ms,
        }
    }
}

#[cfg(test)]
pub(super) fn compare_ordinal_for_test(
    left: &str,
    right: &str,
    ignore_case: bool,
) -> std::cmp::Ordering {
    compare_ordinal(left, right, ignore_case).unwrap()
}

#[cfg(test)]
fn compare_ordinal(
    left: &str,
    right: &str,
    ignore_case: bool,
) -> Result<std::cmp::Ordering, StoreError> {
    let left: Vec<u16> = left.encode_utf16().collect();
    let right: Vec<u16> = right.encode_utf16().collect();
    Ok(
        match unsafe { CompareStringOrdinal(&left, &right, ignore_case) }.0 {
            1 => std::cmp::Ordering::Less,
            2 => std::cmp::Ordering::Equal,
            3 => std::cmp::Ordering::Greater,
            _ => return Err(StoreError::Platform),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread,
    };

    use rusqlite::{params, Connection};

    use super::{register_collation, QueryStrategy, Store, TestEntry};
    use crate::file_index::{
        FileCategory, FileIndexStatus, FileSort, IndexedKind, QuerySpec, VolumeIdentity,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("uipilot-file-store-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
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

    fn volume() -> VolumeIdentity {
        VolumeIdentity {
            volume_guid_path: r"\\?\Volume{11111111-1111-1111-1111-111111111111}\".into(),
            volume_serial: 7,
            filesystem_name: "NTFS".into(),
        }
    }

    fn entry(name: &str, category: &str, modified: i64) -> TestEntry {
        TestEntry {
            relative_path: format!(r"Results\{name}"),
            display_path: format!(r"C:\Results\{name}"),
            name: name.into(),
            folded_name: crate::file_index::fold_name(name),
            kind: IndexedKind::File,
            category: category.into(),
            size_bytes: Some(10),
            modified_utc_ms: modified,
            generation: 1,
        }
    }

    fn query(text: &str, category: FileCategory, sort: FileSort) -> QuerySpec {
        QuerySpec {
            folded_query: crate::file_index::fold_name(text),
            category,
            sort,
        }
    }

    fn assert_rejected_without_writes(path: &Path) {
        let parent = path.parent().unwrap();
        let before = snapshot_files(parent);
        assert!(Store::open(path, "identity-a").is_err());
        assert_eq!(snapshot_files(parent), before);
    }

    fn snapshot_files(path: &Path) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect()
    }

    fn rewrite_schema_sql(path: &Path, name: &str, rewrite: impl FnOnce(String) -> String) {
        let connection = Connection::open(path).unwrap();
        let sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .unwrap();
        let rewritten = rewrite(sql.clone());
        assert_ne!(rewritten, sql);
        let schema_version: i64 = connection
            .query_row("PRAGMA schema_version", [], |row| row.get(0))
            .unwrap();
        connection
            .execute_batch("PRAGMA writable_schema=ON;")
            .unwrap();
        connection
            .execute(
                "UPDATE sqlite_schema SET sql=?1 WHERE name=?2",
                params![rewritten, name],
            )
            .unwrap();
        connection
            .pragma_update(None, "schema_version", schema_version + 1)
            .unwrap();
        connection
            .execute_batch("PRAGMA writable_schema=OFF;")
            .unwrap();
    }

    #[test]
    fn existing_foreign_future_and_missing_schemas_are_rejected_without_writes() {
        let dir = TestDir::new();

        let foreign = dir.path().join("foreign.sqlite3");
        Connection::open(&foreign)
            .unwrap()
            .execute_batch("CREATE TABLE foreign_data(value TEXT);")
            .unwrap();
        assert_rejected_without_writes(&foreign);

        let future = dir.path().join("future.sqlite3");
        drop(Store::open(&future, "identity-a").unwrap());
        Connection::open(&future)
            .unwrap()
            .execute_batch("PRAGMA user_version=2;")
            .unwrap();
        assert_rejected_without_writes(&future);

        let missing = dir.path().join("missing.sqlite3");
        drop(Store::open(&missing, "identity-a").unwrap());
        Connection::open(&missing)
            .unwrap()
            .execute_batch("DROP INDEX entries_sort_desc;")
            .unwrap();
        assert_rejected_without_writes(&missing);

        let wrong_shape = dir.path().join("wrong-shape.sqlite3");
        drop(Store::open(&wrong_shape, "identity-a").unwrap());
        Connection::open(&wrong_shape)
            .unwrap()
            .execute_batch(
                "DROP TRIGGER entries_ai;
                 CREATE TRIGGER entries_ai AFTER INSERT ON entries BEGIN SELECT 1; END;
                 DROP INDEX entries_sort_desc;
                 CREATE INDEX entries_sort_desc ON entries(name);",
            )
            .unwrap();
        assert_rejected_without_writes(&wrong_shape);
    }

    #[test]
    fn canonical_schema_manifest_rejects_supersets_and_missing_constraints_without_writes() {
        let dir = TestDir::new();

        let trigger_superset = dir.path().join("trigger-superset.sqlite3");
        drop(Store::open(&trigger_superset, "identity-a").unwrap());
        Connection::open(&trigger_superset)
            .unwrap()
            .execute_batch(
                "DROP TRIGGER entries_ai;
                 CREATE TRIGGER entries_ai AFTER INSERT ON entries BEGIN
                   INSERT INTO entry_names(rowid, folded_name) VALUES (new.row_id, new.folded_name);
                   SELECT RAISE(ABORT, 'blocked');
                 END;",
            )
            .unwrap();
        assert_rejected_without_writes(&trigger_superset);

        let extra_trigger = dir.path().join("extra-trigger.sqlite3");
        drop(Store::open(&extra_trigger, "identity-a").unwrap());
        Connection::open(&extra_trigger)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER unexpected_entries_trigger AFTER INSERT ON entries BEGIN
                   SELECT 1;
                 END;",
            )
            .unwrap();
        assert_rejected_without_writes(&extra_trigger);

        let partial_unique_index = dir.path().join("partial-unique-index.sqlite3");
        drop(Store::open(&partial_unique_index, "identity-a").unwrap());
        let connection = Connection::open(&partial_unique_index).unwrap();
        register_collation(&connection, "uipilot_name_ordinal_ci", true).unwrap();
        register_collation(&connection, "uipilot_path_ordinal_cs", false).unwrap();
        connection
            .execute_batch(
                "DROP INDEX entries_sort_desc;
                 CREATE UNIQUE INDEX entries_sort_desc ON entries(
                   modified_utc_ms DESC,
                   name COLLATE uipilot_name_ordinal_ci ASC,
                   display_path COLLATE uipilot_path_ordinal_cs ASC
                 ) WHERE category='documents';",
            )
            .unwrap();
        drop(connection);
        assert_rejected_without_writes(&partial_unique_index);

        let missing_check = dir.path().join("missing-check.sqlite3");
        drop(Store::open(&missing_check, "identity-a").unwrap());
        rewrite_schema_sql(&missing_check, "metadata", |sql| {
            sql.replace(" CHECK(singleton=1)", "")
        });
        assert_rejected_without_writes(&missing_check);

        let missing_unique = dir.path().join("missing-unique.sqlite3");
        drop(Store::open(&missing_unique, "identity-a").unwrap());
        rewrite_schema_sql(&missing_unique, "entries", |sql| {
            sql.replace(
                ",\n  UNIQUE(volume_guid_path, volume_serial, filesystem_name, relative_path)",
                "",
            )
        });
        assert_rejected_without_writes(&missing_unique);
    }

    #[test]
    fn canonical_schema_manifest_preserves_quoted_literal_case() {
        let dir = TestDir::new();

        let uppercase_check = dir.path().join("uppercase-check.sqlite3");
        drop(Store::open(&uppercase_check, "identity-a").unwrap());
        rewrite_schema_sql(&uppercase_check, "entries", |sql| {
            sql.replace(
                "CHECK(kind IN ('file','directory'))",
                "CHECK(kind IN ('FILE','DIRECTORY'))",
            )
        });
        assert_rejected_without_writes(&uppercase_check);

        let uppercase_trigger = dir.path().join("uppercase-trigger.sqlite3");
        drop(Store::open(&uppercase_trigger, "identity-a").unwrap());
        rewrite_schema_sql(&uppercase_trigger, "entries_ad", |sql| {
            sql.replace("'delete'", "'DELETE'")
        });
        assert_rejected_without_writes(&uppercase_trigger);
    }

    #[test]
    fn existing_fold_algorithm_mismatch_is_rejected_without_writes() {
        let dir = TestDir::new();
        let path = dir.path().join("fold.sqlite3");
        drop(Store::open(&path, "identity-a").unwrap());
        Connection::open(&path)
            .unwrap()
            .execute("UPDATE metadata SET fold_algorithm_id='other'", [])
            .unwrap();

        assert_rejected_without_writes(&path);
    }

    #[test]
    fn schema_v1_uses_wal_fts5_and_two_generations() {
        let store = Store::open_in_memory_for_test("identity-a").unwrap();
        assert_eq!(store.pragma_i64("application_id"), 1_430_868_038);
        assert_eq!(store.pragma_i64("user_version"), 1);
        assert_eq!(store.pragma_text("journal_mode"), "memory");

        let objects = store.schema_objects();
        for object in [
            "metadata",
            "volumes",
            "entries",
            "candidate_entries",
            "entry_names",
            "candidate_names",
            "entries_sort_desc",
            "entries_sort_asc",
            "entries_category_sort_desc",
            "entries_category_sort_asc",
            "candidate_entries_sort_desc",
            "candidate_entries_sort_asc",
            "candidate_entries_category_sort_desc",
            "candidate_entries_category_sort_asc",
        ] {
            assert!(objects.contains(&object.to_string()), "missing {object}");
        }
        assert_eq!(store.metadata_integrity_marker(), (false, None));
    }

    #[test]
    fn short_and_trigram_queries_use_one_read_snapshot() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let volume = volume();
        store
            .seed_committed_for_test(
                &volume,
                [
                    entry("UiPilot.xlsx", "excel", 2),
                    entry("UiPlan.docx", "word", 1),
                ],
            )
            .unwrap();

        let short = store
            .query_for_test(
                &query("ui", FileCategory::All, FileSort::ModifiedDesc),
                std::slice::from_ref(&volume),
            )
            .unwrap();
        assert_eq!(short.strategy, QueryStrategy::Instr);
        assert_eq!(short.total, 2);

        let trigram = store
            .query_for_test(
                &query("pilot", FileCategory::All, FileSort::ModifiedDesc),
                &[volume],
            )
            .unwrap();
        assert_eq!(trigram.strategy, QueryStrategy::Trigram);
        assert_eq!(trigram.total, 1);
        assert_eq!(trigram.entries[0].name, "UiPilot.xlsx");
    }

    #[test]
    fn ordinal_sort_matches_compare_string_ordinal() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let volume = volume();
        store
            .seed_committed_for_test(
                &volume,
                [
                    entry("alpha.txt", "other", 1),
                    entry("Alpha.txt", "other", 1),
                    entry("é.txt", "other", 1),
                    entry("e\u{301}.txt", "other", 1),
                    entry("𐐀.txt", "other", 1),
                ],
            )
            .unwrap();

        let result = store
            .query_for_test(
                &query("", FileCategory::All, FileSort::ModifiedDesc),
                &[volume],
            )
            .unwrap();
        assert_eq!(result.entries.len(), 0, "empty query never returns rows");
        assert!(store
            .ordered_names_for_test(FileSort::ModifiedDesc)
            .windows(2)
            .all(|pair| crate::file_index::store::compare_ordinal_for_test(
                &pair[0], &pair[1], true
            ) != std::cmp::Ordering::Greater));
        assert!(store.query_plan_uses_sort_index_for_test(FileSort::ModifiedDesc));
        assert!(store.query_plan_uses_sort_index_for_test(FileSort::ModifiedAsc));
    }

    #[test]
    fn ordinal_sort_identity_change_reindexes_before_query() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        assert_eq!(store.index_revision_for_test(), 0);
        assert!(store.ensure_sort_identity_for_test("identity-b").unwrap());
        assert_eq!(store.ordinal_identity_for_test(), "identity-b");
        assert_eq!(store.index_revision_for_test(), 1);
        assert_eq!(store.reindex_statement_count_for_test(), 2);
        assert!(!store.ensure_sort_identity_for_test("identity-b").unwrap());
    }

    #[test]
    fn category_sort_count_and_limit_are_exact() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let volume = volume();
        let mut entries = (0..201)
            .map(|index| entry(&format!("match-{index:03}.xlsx"), "excel", index))
            .collect::<Vec<_>>();
        let mut folder = entry("match-folder", "folder", 999);
        folder.kind = IndexedKind::Directory;
        folder.size_bytes = None;
        entries.push(folder);
        store.seed_committed_for_test(&volume, entries).unwrap();

        let result = store
            .query_for_test(
                &query("match", FileCategory::Excel, FileSort::ModifiedDesc),
                std::slice::from_ref(&volume),
            )
            .unwrap();
        assert_eq!(result.total, 201);
        assert_eq!(result.entries.len(), 200);
        assert_eq!(result.entries[0].name, "match-200.xlsx");
        assert_eq!(result.entries[199].name, "match-001.xlsx");

        let folders = store
            .query_for_test(
                &query("match", FileCategory::Folder, FileSort::ModifiedAsc),
                &[volume],
            )
            .unwrap();
        assert_eq!(folders.total, 1);
        assert_eq!(folders.entries[0].kind, IndexedKind::Directory);
    }

    #[test]
    fn detached_volume_state_does_not_change_authenticated_status() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let attached = volume();
        store
            .seed_committed_for_test(&attached, [entry("match.txt", "other", 1)])
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO volumes(volume_guid_path, volume_serial, filesystem_name, mount_point, committed_generation, candidate_generation, next_generation, scan_state) VALUES('detached',9,'NTFS','D:\\','1',NULL,'2','dirty')",
                [],
            )
            .unwrap();

        let result = store
            .query_for_test(
                &query("match", FileCategory::All, FileSort::ModifiedDesc),
                &[attached],
            )
            .unwrap();

        assert_eq!(result.status, FileIndexStatus::Ready);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn denied_volume_root_preserves_all_committed_rows_on_current_mount() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let attached = volume();
        store
            .seed_committed_for_test(
                &attached,
                [
                    entry("match-one.txt", "other", 1),
                    entry(r"folder\match-two.txt", "other", 2),
                ],
            )
            .unwrap();
        let generation = store.begin_candidate_for_test(&attached, r"D:\").unwrap();

        store
            .commit_candidate_for_test(&attached, generation, &[""])
            .unwrap();

        let result = store
            .query_for_test(
                &query("match", FileCategory::All, FileSort::ModifiedDesc),
                std::slice::from_ref(&attached),
            )
            .unwrap();
        assert_eq!(result.status, FileIndexStatus::Partial);
        assert_eq!(result.total, 2);
        assert!(result
            .entries
            .iter()
            .all(|entry| entry.display_path.starts_with(r"D:\")));
    }

    #[test]
    fn concurrent_writer_cannot_split_revision_status_count_and_items_snapshot() {
        let dir = TestDir::new();
        let path = dir.path().join("snapshot.sqlite3");
        let attached = volume();
        let mut reader = Store::open(&path, "identity-a").unwrap();
        reader
            .seed_committed_for_test(&attached, [entry("match-one.txt", "other", 1)])
            .unwrap();

        let worker_path = path.clone();
        let worker_volume = attached.clone();
        let (start_tx, start_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            start_rx.recv().unwrap();
            let mut writer = Store::open(&worker_path, "identity-a").unwrap();
            let transaction = writer.connection.transaction().unwrap();
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
                        worker_volume.volume_guid_path,
                        worker_volume.volume_serial,
                        worker_volume.filesystem_name,
                    ],
                )
                .unwrap();
            let second = entry("match-two.txt", "other", 2);
            transaction
                .execute(
                    "INSERT INTO entries(volume_guid_path, volume_serial, filesystem_name, relative_path, display_path, name, folded_name, kind, category, size_bytes, modified_utc_ms, generation) VALUES(?1,?2,?3,?4,?5,?6,?7,'file',?8,?9,?10,?11)",
                    rusqlite::params![
                        worker_volume.volume_guid_path,
                        worker_volume.volume_serial,
                        worker_volume.filesystem_name,
                        second.relative_path,
                        second.display_path,
                        second.name,
                        second.folded_name,
                        second.category,
                        second.size_bytes.map(|value| value.to_string()),
                        second.modified_utc_ms,
                        second.generation.to_string(),
                    ],
                )
                .unwrap();
            transaction.commit().unwrap();
            done_tx.send(()).unwrap();
        });

        let first = reader
            .query_with_hook_for_test(
                &query("match", FileCategory::All, FileSort::ModifiedDesc),
                std::slice::from_ref(&attached),
                || {
                    start_tx.send(()).unwrap();
                    done_rx.recv().unwrap();
                },
            )
            .unwrap();
        worker.join().unwrap();

        assert_eq!(first.index_revision, 0);
        assert_eq!(first.status, FileIndexStatus::Ready);
        assert_eq!(first.total, 1);
        assert_eq!(first.entries.len(), 1);

        let second = reader
            .query_for_test(
                &query("match", FileCategory::All, FileSort::ModifiedDesc),
                &[attached],
            )
            .unwrap();
        assert_eq!(second.index_revision, 1);
        assert_eq!(second.status, FileIndexStatus::Building);
        assert_eq!(second.total, 2);
        assert_eq!(second.entries.len(), 2);
    }

    #[test]
    fn empty_query_skips_instr_and_fts_but_returns_status_snapshot() {
        let mut store = Store::open_in_memory_for_test("identity-a").unwrap();
        let result = store
            .query_for_test(&query("", FileCategory::All, FileSort::ModifiedDesc), &[])
            .unwrap();
        assert_eq!(result.strategy, QueryStrategy::Empty);
        assert_eq!(result.total, 0);
        assert!(result.entries.is_empty());
        assert_eq!(result.status, FileIndexStatus::Building);
        assert_eq!(result.index_revision, 0);
    }

    #[test]
    fn new_schema_initializes_dirty_integrity_metadata() {
        let store = Store::open_in_memory_for_test("identity-a").unwrap();
        assert_eq!(store.metadata_integrity_marker(), (false, None));
    }
}
