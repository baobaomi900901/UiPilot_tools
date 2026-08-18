use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::atomic_file::{commit_with_backup, read_optional, AtomicFileError, AtomicPaths};

const MESSAGE_SCHEMA: u32 = 1;
const MAX_MESSAGES: usize = 100;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MessageRecord {
    pub(super) id: String,
    pub(super) plugin_id: String,
    pub(super) plugin_name_snapshot: String,
    pub(super) created_at: String,
    pub(super) content: String,
    pub(super) read_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MessageSummary {
    pub(super) revision: String,
    pub(super) unread_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MessageSnapshot {
    pub(super) revision: String,
    pub(super) unread_count: usize,
    pub(super) messages: Vec<MessageRecord>,
    pub(super) changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PublishCommit {
    pub(super) record: MessageRecord,
    pub(super) summary: MessageSummary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PublishInput {
    pub(super) plugin_id: String,
    pub(super) plugin_name_snapshot: String,
    pub(super) content: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MessageStoreError {
    OperationFailed,
    BecameUnavailable,
    Unavailable,
}

pub(super) trait MessageClock: Send + Sync {
    fn now_utc_rfc3339(&self) -> Result<String, MessageStoreError>;
}

struct SystemMessageClock;

impl MessageClock for SystemMessageClock {
    fn now_utc_rfc3339(&self) -> Result<String, MessageStoreError> {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| MessageStoreError::OperationFailed)
    }
}

pub(super) trait MessageCommitter: Send + Sync {
    fn commit(
        &self,
        paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError>;
}

pub(super) struct AtomicMessageCommitter;

impl MessageCommitter for AtomicMessageCommitter {
    fn commit(
        &self,
        paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        commit_with_backup(paths, previous, candidate)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageStoreV1 {
    schema: u32,
    revision: String,
    next_message_id: String,
    messages: Vec<MessageRecord>,
}

impl Default for MessageStoreV1 {
    fn default() -> Self {
        Self {
            schema: MESSAGE_SCHEMA,
            revision: "0".into(),
            next_message_id: "1".into(),
            messages: Vec::new(),
        }
    }
}

struct ReadyStore {
    document: MessageStoreV1,
    raw: Option<Vec<u8>>,
}

enum StoreSession {
    Ready(ReadyStore),
    Unavailable,
}

enum Candidate {
    Missing,
    Valid(ReadyStore),
    Invalid,
}

pub(super) struct MessageStore {
    paths: AtomicPaths,
    state: Mutex<StoreSession>,
    committer: Arc<dyn MessageCommitter>,
    clock: Arc<dyn MessageClock>,
}

impl MessageStore {
    pub(super) fn load(root: &Path) -> Self {
        Self::load_with(
            root,
            Arc::new(AtomicMessageCommitter),
            Arc::new(SystemMessageClock),
        )
    }

    pub(super) fn load_with(
        root: &Path,
        committer: Arc<dyn MessageCommitter>,
        clock: Arc<dyn MessageClock>,
    ) -> Self {
        let paths = AtomicPaths::new(root, "messages.json");
        let state = if fs::create_dir_all(root).is_err() {
            StoreSession::Unavailable
        } else {
            load_session(&paths)
        };
        Self {
            paths,
            state: Mutex::new(state),
            committer,
            clock,
        }
    }

    pub(super) fn publish(&self, input: PublishInput) -> Result<PublishCommit, MessageStoreError> {
        let created_at = self.clock.now_utc_rfc3339()?;
        if !valid_publish_input(&input) || !valid_utc_rfc3339(&created_at) {
            return Err(MessageStoreError::OperationFailed);
        }

        let mut session = self.lock()?;
        let (message_id, next_message_id, next_revision) = match &*session {
            StoreSession::Unavailable => return Err(MessageStoreError::Unavailable),
            StoreSession::Ready(ready) => {
                let message_id = known_u64(&ready.document.next_message_id);
                let revision = known_u64(&ready.document.revision);
                let Some(next_message_id) = message_id.checked_add(1) else {
                    return transition_unavailable(&mut session);
                };
                let Some(next_revision) = revision.checked_add(1) else {
                    return transition_unavailable(&mut session);
                };
                (message_id, next_message_id, next_revision)
            }
        };

        let record = MessageRecord {
            id: message_id.to_string(),
            plugin_id: input.plugin_id,
            plugin_name_snapshot: input.plugin_name_snapshot,
            created_at,
            content: input.content,
            read_at: None,
        };
        let ready = ready_mut(&mut session)?;
        let mut candidate = ready.document.clone();
        candidate.revision = next_revision.to_string();
        candidate.next_message_id = next_message_id.to_string();
        candidate.messages.push(record.clone());
        if candidate.messages.len() > MAX_MESSAGES {
            candidate.messages.remove(0);
        }
        self.persist(ready, candidate)?;

        Ok(PublishCommit {
            record,
            summary: summary(&ready.document),
        })
    }

    pub(super) fn summary(&self) -> Result<MessageSummary, MessageStoreError> {
        let session = self.lock()?;
        match &*session {
            StoreSession::Ready(ready) => Ok(summary(&ready.document)),
            StoreSession::Unavailable => Err(MessageStoreError::Unavailable),
        }
    }

    pub(super) fn read_snapshot(&self) -> Result<MessageSnapshot, MessageStoreError> {
        let session = self.lock()?;
        match &*session {
            StoreSession::Ready(ready) => Ok(snapshot(&ready.document, false)),
            StoreSession::Unavailable => Err(MessageStoreError::Unavailable),
        }
    }

    pub(super) fn open_and_mark_read(&self) -> Result<MessageSnapshot, MessageStoreError> {
        let mut session = self.lock()?;
        let (cutoff, next_revision) = match &*session {
            StoreSession::Unavailable => return Err(MessageStoreError::Unavailable),
            StoreSession::Ready(ready) => {
                if ready
                    .document
                    .messages
                    .iter()
                    .all(|message| message.read_at.is_some())
                {
                    return Ok(snapshot(&ready.document, false));
                }
                let cutoff = ready
                    .document
                    .messages
                    .last()
                    .map(|message| known_u64(&message.id))
                    .unwrap_or(0);
                let Some(next_revision) = known_u64(&ready.document.revision).checked_add(1) else {
                    return transition_unavailable(&mut session);
                };
                (cutoff, next_revision)
            }
        };
        let read_at = self.clock.now_utc_rfc3339()?;
        if !valid_utc_rfc3339(&read_at) {
            return Err(MessageStoreError::OperationFailed);
        }

        let ready = ready_mut(&mut session)?;
        let mut candidate = ready.document.clone();
        candidate.revision = next_revision.to_string();
        for message in &mut candidate.messages {
            if message.read_at.is_none() && known_u64(&message.id) <= cutoff {
                message.read_at = Some(read_at.clone());
            }
        }
        self.persist(ready, candidate)?;
        Ok(snapshot(&ready.document, true))
    }

    pub(super) fn clear(&self) -> Result<MessageSnapshot, MessageStoreError> {
        let mut session = self.lock()?;
        let next_revision = match &*session {
            StoreSession::Unavailable => return Err(MessageStoreError::Unavailable),
            StoreSession::Ready(ready) if ready.document.messages.is_empty() => {
                return Ok(snapshot(&ready.document, false));
            }
            StoreSession::Ready(ready) => {
                let Some(next_revision) = known_u64(&ready.document.revision).checked_add(1) else {
                    return transition_unavailable(&mut session);
                };
                next_revision
            }
        };

        let ready = ready_mut(&mut session)?;
        let mut candidate = ready.document.clone();
        candidate.revision = next_revision.to_string();
        candidate.messages.clear();
        self.persist(ready, candidate)?;
        Ok(snapshot(&ready.document, true))
    }

    fn persist(
        &self,
        ready: &mut ReadyStore,
        candidate: MessageStoreV1,
    ) -> Result<(), MessageStoreError> {
        let bytes =
            serde_json::to_vec(&candidate).map_err(|_| MessageStoreError::OperationFailed)?;
        self.committer
            .commit(&self.paths, ready.raw.as_deref(), &bytes)
            .map_err(|_| MessageStoreError::OperationFailed)?;
        ready.document = candidate;
        ready.raw = Some(bytes);
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, StoreSession>, MessageStoreError> {
        self.state
            .lock()
            .map_err(|_| MessageStoreError::OperationFailed)
    }
}

fn load_session(paths: &AtomicPaths) -> StoreSession {
    let current = read_candidate(paths.current());
    if let Candidate::Valid(ready) = current {
        return StoreSession::Ready(ready);
    }

    let backup = read_candidate(paths.backup());
    if let Candidate::Valid(ready) = backup {
        return StoreSession::Ready(ready);
    }

    if matches!(current, Candidate::Missing) && matches!(backup, Candidate::Missing) {
        StoreSession::Ready(ReadyStore {
            document: MessageStoreV1::default(),
            raw: None,
        })
    } else {
        StoreSession::Unavailable
    }
}

fn read_candidate(path: &Path) -> Candidate {
    match read_optional(path) {
        Ok(None) => Candidate::Missing,
        Ok(Some(raw)) => match parse_document(&raw) {
            Some(document) => Candidate::Valid(ReadyStore {
                document,
                raw: Some(raw),
            }),
            None => Candidate::Invalid,
        },
        Err(_) => Candidate::Invalid,
    }
}

fn parse_document(bytes: &[u8]) -> Option<MessageStoreV1> {
    let document = serde_json::from_slice::<MessageStoreV1>(bytes).ok()?;
    let revision = parse_u64_decimal(&document.revision)?;
    let next_message_id = parse_u64_decimal(&document.next_message_id)?;
    if document.schema != MESSAGE_SCHEMA
        || next_message_id == 0
        || document.messages.len() > MAX_MESSAGES
        || revision.to_string() != document.revision
        || next_message_id.to_string() != document.next_message_id
    {
        return None;
    }

    let mut previous_id = 0;
    for message in &document.messages {
        let id = parse_u64_decimal(&message.id)?;
        if id == 0 || id <= previous_id || id >= next_message_id || !valid_message_record(message) {
            return None;
        }
        previous_id = id;
    }
    Some(document)
}

fn parse_u64_decimal(value: &str) -> Option<u64> {
    if value == "0" {
        return Some(0);
    }
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn known_u64(value: &str) -> u64 {
    parse_u64_decimal(value).expect("stored message counters are validated before use")
}

fn valid_publish_input(input: &PublishInput) -> bool {
    valid_identity_text(&input.plugin_id)
        && valid_identity_text(&input.plugin_name_snapshot)
        && valid_content(&input.content)
}

fn valid_message_record(message: &MessageRecord) -> bool {
    parse_u64_decimal(&message.id).is_some()
        && valid_identity_text(&message.plugin_id)
        && valid_identity_text(&message.plugin_name_snapshot)
        && valid_utc_rfc3339(&message.created_at)
        && message.read_at.as_deref().is_none_or(valid_utc_rfc3339)
        && valid_content(&message.content)
}

fn valid_identity_text(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn valid_content(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 500 && !value.chars().any(char::is_control)
}

fn valid_utc_rfc3339(value: &str) -> bool {
    let Some((date, time)) = value
        .strip_suffix('Z')
        .and_then(|value| value.split_once('T'))
    else {
        return false;
    };
    let date_parts = date.split('-').collect::<Vec<_>>();
    if date_parts.len() != 3
        || date_parts[0].len() != 4
        || date_parts[1].len() != 2
        || date_parts[2].len() != 2
    {
        return false;
    }
    let Ok(year) = date_parts[0].parse::<i32>() else {
        return false;
    };
    let Ok(month) = date_parts[1].parse::<u8>() else {
        return false;
    };
    let Ok(day) = date_parts[2].parse::<u8>() else {
        return false;
    };
    let Ok(month) = time::Month::try_from(month) else {
        return false;
    };
    if year < 1 || time::Date::from_calendar_date(year, month, day).is_err() {
        return false;
    }

    let (whole_time, fraction) = time
        .split_once('.')
        .map_or((time, None), |(whole, fraction)| (whole, Some(fraction)));
    let time_parts = whole_time.split(':').collect::<Vec<_>>();
    if time_parts.len() != 3 || time_parts.iter().any(|part| part.len() != 2) {
        return false;
    }
    let Ok(hour) = time_parts[0].parse::<u8>() else {
        return false;
    };
    let Ok(minute) = time_parts[1].parse::<u8>() else {
        return false;
    };
    let Ok(second) = time_parts[2].parse::<u8>() else {
        return false;
    };
    let nanosecond = match fraction {
        None => 0,
        Some(fraction)
            if !fraction.is_empty()
                && fraction.len() <= 9
                && fraction.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            let Ok(value) = fraction.parse::<u32>() else {
                return false;
            };
            value * 10_u32.pow((9 - fraction.len()) as u32)
        }
        Some(_) => return false,
    };
    time::Time::from_hms_nano(hour, minute, second, nanosecond).is_ok()
}

fn summary(document: &MessageStoreV1) -> MessageSummary {
    MessageSummary {
        revision: document.revision.clone(),
        unread_count: document
            .messages
            .iter()
            .filter(|message| message.read_at.is_none())
            .count(),
    }
}

fn snapshot(document: &MessageStoreV1, changed: bool) -> MessageSnapshot {
    let summary = summary(document);
    MessageSnapshot {
        revision: summary.revision,
        unread_count: summary.unread_count,
        messages: document.messages.clone(),
        changed,
    }
}

fn ready_mut(session: &mut StoreSession) -> Result<&mut ReadyStore, MessageStoreError> {
    match session {
        StoreSession::Ready(ready) => Ok(ready),
        StoreSession::Unavailable => Err(MessageStoreError::Unavailable),
    }
}

fn transition_unavailable<T>(session: &mut StoreSession) -> Result<T, MessageStoreError> {
    *session = StoreSession::Unavailable;
    Err(MessageStoreError::BecameUnavailable)
}
