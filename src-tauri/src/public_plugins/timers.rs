use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::{Deserialize, Serialize};

const MIN_DURATION_MS: u64 = 1_000;
const MAX_DURATION_MS: u64 = 86_400_000;
const SLEEP_RECHECK_MS: u64 = 100;

pub(crate) trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

#[cfg(windows)]
#[derive(Default)]
pub(crate) struct SystemClock;

#[cfg(windows)]
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
    }
}

#[cfg(not(windows))]
pub(crate) struct SystemClock(std::time::Instant);

#[cfg(not(windows))]
impl Default for SystemClock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

#[cfg(not(windows))]
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TimerKey {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
}

impl TimerKey {
    pub(crate) fn new(plugin_id: &str, plugin_generation: u64) -> Option<Self> {
        (plugin_generation > 0 && super::manifest::valid_plugin_id(plugin_id)).then(|| Self {
            plugin_id: plugin_id.to_owned(),
            plugin_generation,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PluginTimerPhase {
    Idle,
    Running,
    Paused,
    Fired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginTimerStartInput {
    pub(crate) duration_ms: u64,
    pub(crate) completion_message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginTimerState {
    pub(crate) timer_revision: String,
    pub(crate) phase: PluginTimerPhase,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) remaining_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimerError {
    InvalidCaller,
    PermissionDenied,
    ExpiredWindowSessionError,
    InvalidTimerInput,
    TimerInputRequired,
    TimerInputNotAllowed,
    MessageStoreUnavailable,
    TimerUnavailable,
}

impl TimerError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::InvalidCaller => "InvalidCaller",
            Self::PermissionDenied => "PermissionDenied",
            Self::ExpiredWindowSessionError => "ExpiredWindowSessionError",
            Self::InvalidTimerInput => "InvalidTimerInput",
            Self::TimerInputRequired => "TimerInputRequired",
            Self::TimerInputNotAllowed => "TimerInputNotAllowed",
            Self::MessageStoreUnavailable => "MessageStoreUnavailable",
            Self::TimerUnavailable => "TimerUnavailable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrozenCompletion {
    pub(crate) completion_message: String,
    pub(crate) plugin_name_snapshot: String,
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) duration_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaimTicket {
    key: TimerKey,
    round_id: u64,
    claim_id: u64,
    claim_revision: u64,
    pub(crate) frozen_completion: FrozenCompletion,
}

impl ClaimTicket {
    pub(crate) fn key(&self) -> &TimerKey {
        &self.key
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct AudioTicket {
    pub(crate) key: TimerKey,
    pub(crate) round_id: u64,
    pub(crate) audio_id: u64,
    pub(crate) fired_revision: u64,
}

impl AudioTicket {
    pub(crate) fn key(&self) -> &TimerKey {
        &self.key
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimerMutation {
    pub(crate) state: PluginTimerState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TimerPostLockEffect {
    AudioCancelled(AudioTicket),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimerOperation<T> {
    pub(crate) result: Result<T, TimerError>,
    pub(crate) post_lock_effects: Vec<TimerPostLockEffect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaimCompletion {
    pub(crate) state: PluginTimerState,
    pub(crate) audio_ticket: Option<AudioTicket>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InternalPhase {
    Idle,
    Running,
    Paused,
    Claiming,
    Fired,
}

#[derive(Clone, Debug)]
struct StoredClaim {
    ticket: ClaimTicket,
    delivery_admitted: bool,
}

#[derive(Clone, Debug)]
enum TimerAudioState {
    Issued(AudioTicket),
    Admitted(AudioTicket),
    Confirmed(AudioTicket),
}

#[derive(Clone, Debug)]
struct TimerRecord {
    phase: InternalPhase,
    revision: u64,
    round_id: u64,
    claim_id: u64,
    audio_id: u64,
    duration_ms: Option<u64>,
    remaining_ms: Option<u64>,
    due_at_ms: Option<u64>,
    frozen_completion: Option<FrozenCompletion>,
    claim: Option<StoredClaim>,
    audio: Option<TimerAudioState>,
    unavailable: bool,
}

impl Default for TimerRecord {
    fn default() -> Self {
        Self {
            phase: InternalPhase::Idle,
            revision: 0,
            round_id: 0,
            claim_id: 0,
            audio_id: 0,
            duration_ms: None,
            remaining_ms: None,
            due_at_ms: None,
            frozen_completion: None,
            claim: None,
            audio: None,
            unavailable: false,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct QueueItem {
    due_at_ms: u64,
    key: TimerKey,
    round_id: u64,
    due_revision: u64,
}

#[derive(Default)]
struct ServiceState {
    records: BTreeMap<TimerKey, TimerRecord>,
    queue: BinaryHeap<Reverse<QueueItem>>,
    terminal: bool,
    worker_started: bool,
}

pub(crate) struct PluginTimerService {
    clock: Arc<dyn Clock>,
    state: Mutex<ServiceState>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
}

pub(crate) struct TimerAudioFocusAuthority<'a> {
    state: &'a mut ServiceState,
}

impl TimerAudioFocusAuthority<'_> {
    pub(crate) fn confirm_all_current(&mut self) {
        for record in self.state.records.values_mut() {
            confirm_current_audio(record);
        }
    }
}

impl PluginTimerService {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            state: Mutex::new(ServiceState::default()),
            wake: Condvar::new(),
            worker: Mutex::new(None),
        }
    }

    pub(crate) fn start_worker(
        self: &Arc<Self>,
        handler: Arc<dyn Fn(ClaimTicket) + Send + Sync>,
    ) -> Result<(), TimerError> {
        {
            let mut state = self.lock_state()?;
            if state.terminal || state.worker_started {
                return Err(TimerError::TimerUnavailable);
            }
            state.worker_started = true;
        }
        let service = Arc::clone(self);
        let worker = thread::Builder::new()
            .name("uipilot-plugin-timers".into())
            .spawn(move || service.worker_loop(handler))
            .map_err(|_| {
                if let Ok(mut state) = self.state.lock() {
                    state.worker_started = false;
                }
                TimerError::TimerUnavailable
            })?;
        *self
            .worker
            .lock()
            .map_err(|_| TimerError::TimerUnavailable)? = Some(worker);
        Ok(())
    }

    pub(crate) fn get_state(&self, key: &TimerKey) -> Result<PluginTimerState, TimerError> {
        let state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        match state.records.get(key) {
            Some(record) if record.unavailable => Err(TimerError::TimerUnavailable),
            Some(record) => Ok(project(record, self.clock.now_ms())),
            None => Ok(initial_state()),
        }
    }

    pub(crate) fn start(
        &self,
        key: &TimerKey,
        plugin_name_snapshot: &str,
        input: Option<PluginTimerStartInput>,
        message_store_available: bool,
    ) -> TimerOperation<TimerMutation> {
        let mut post_lock_effects = Vec::new();
        let result = self.start_result(
            key,
            plugin_name_snapshot,
            input,
            message_store_available,
            &mut post_lock_effects,
        );
        TimerOperation {
            result,
            post_lock_effects,
        }
    }

    fn start_result(
        &self,
        key: &TimerKey,
        plugin_name_snapshot: &str,
        input: Option<PluginTimerStartInput>,
        message_store_available: bool,
        post_lock_effects: &mut Vec<TimerPostLockEffect>,
    ) -> Result<TimerMutation, TimerError> {
        let now = self.clock.now_ms();
        let mut state = self.lock_state()?;
        ensure_available(&state, key)?;
        let phase = state
            .records
            .get(key)
            .map_or(InternalPhase::Idle, |record| record.phase);

        match phase {
            InternalPhase::Idle | InternalPhase::Fired => {
                let input = input.ok_or(TimerError::TimerInputRequired)?;
                validate_input(&input)?;
                if !message_store_available {
                    return Err(TimerError::MessageStoreUnavailable);
                }
                remove_queued(&mut state, key);
                let (queue_item, result) = {
                    let record = state.records.entry(key.clone()).or_default();
                    take_audio_for_operation(record, post_lock_effects);
                    let due_at_ms = now
                        .checked_add(input.duration_ms)
                        .ok_or_else(|| make_unavailable(record))?;
                    record.round_id = record
                        .round_id
                        .checked_add(1)
                        .ok_or_else(|| make_unavailable(record))?;
                    advance_revision(record)?;
                    record.phase = InternalPhase::Running;
                    record.duration_ms = Some(input.duration_ms);
                    record.remaining_ms = Some(input.duration_ms);
                    record.due_at_ms = Some(due_at_ms);
                    record.claim = None;
                    record.frozen_completion = Some(FrozenCompletion {
                        completion_message: input.completion_message,
                        plugin_name_snapshot: plugin_name_snapshot.to_owned(),
                        plugin_id: key.plugin_id.clone(),
                        plugin_generation: key.plugin_generation,
                        duration_ms: input.duration_ms,
                    });
                    (
                        QueueItem {
                            due_at_ms,
                            key: key.clone(),
                            round_id: record.round_id,
                            due_revision: record.revision,
                        },
                        TimerMutation {
                            state: project(record, now),
                        },
                    )
                };
                state.queue.push(Reverse(queue_item));
                self.wake.notify_all();
                Ok(result)
            }
            InternalPhase::Paused => {
                if input.is_some() {
                    return Err(TimerError::TimerInputNotAllowed);
                }
                if !message_store_available {
                    return Err(TimerError::MessageStoreUnavailable);
                }
                remove_queued(&mut state, key);
                let (queue_item, result) = {
                    let record = state.records.get_mut(key).expect("paused record missing");
                    let remaining_ms = record.remaining_ms.ok_or(TimerError::TimerUnavailable)?;
                    let due_at_ms = now
                        .checked_add(remaining_ms)
                        .ok_or_else(|| make_unavailable(record))?;
                    advance_revision(record)?;
                    record.phase = InternalPhase::Running;
                    record.due_at_ms = Some(due_at_ms);
                    (
                        QueueItem {
                            due_at_ms,
                            key: key.clone(),
                            round_id: record.round_id,
                            due_revision: record.revision,
                        },
                        TimerMutation {
                            state: project(record, now),
                        },
                    )
                };
                state.queue.push(Reverse(queue_item));
                self.wake.notify_all();
                Ok(result)
            }
            InternalPhase::Running | InternalPhase::Claiming => {
                if input.is_some() {
                    return Err(TimerError::TimerInputNotAllowed);
                }
                let record = state.records.get(key).expect("running record missing");
                Ok(TimerMutation {
                    state: project(record, now),
                })
            }
        }
    }

    pub(crate) fn stop(&self, key: &TimerKey) -> TimerOperation<TimerMutation> {
        TimerOperation {
            result: self.stop_result(key),
            post_lock_effects: Vec::new(),
        }
    }

    fn stop_result(&self, key: &TimerKey) -> Result<TimerMutation, TimerError> {
        let now = self.clock.now_ms();
        let mut state = self.lock_state()?;
        ensure_available(&state, key)?;
        let phase = state
            .records
            .get(key)
            .map_or(InternalPhase::Idle, |record| record.phase);
        if phase == InternalPhase::Running {
            remove_queued(&mut state, key);
            let record = state.records.get_mut(key).expect("running record missing");
            let remaining_ms = record
                .due_at_ms
                .unwrap_or(now)
                .saturating_sub(now)
                .min(record.duration_ms.unwrap_or_default());
            advance_revision(record)?;
            record.phase = InternalPhase::Paused;
            record.remaining_ms = Some(remaining_ms);
            record.due_at_ms = None;
        }
        let state_view = state
            .records
            .get(key)
            .map_or_else(initial_state, |record| project(record, now));
        Ok(TimerMutation { state: state_view })
    }

    pub(crate) fn reset(&self, key: &TimerKey) -> TimerOperation<TimerMutation> {
        let mut post_lock_effects = Vec::new();
        let result = self.reset_result(key, &mut post_lock_effects);
        TimerOperation {
            result,
            post_lock_effects,
        }
    }

    fn reset_result(
        &self,
        key: &TimerKey,
        post_lock_effects: &mut Vec<TimerPostLockEffect>,
    ) -> Result<TimerMutation, TimerError> {
        let now = self.clock.now_ms();
        let mut state = self.lock_state()?;
        ensure_available(&state, key)?;
        let phase = state
            .records
            .get(key)
            .map_or(InternalPhase::Idle, |record| record.phase);
        if phase == InternalPhase::Idle {
            let state_view = state
                .records
                .get(key)
                .map_or_else(initial_state, |record| project(record, now));
            return Ok(TimerMutation { state: state_view });
        }
        remove_queued(&mut state, key);
        let record = state.records.get_mut(key).expect("timer record missing");
        take_audio_for_operation(record, post_lock_effects);
        advance_revision(record)?;
        record.phase = InternalPhase::Idle;
        record.remaining_ms = record.duration_ms;
        record.due_at_ms = None;
        record.frozen_completion = None;
        record.claim = None;
        Ok(TimerMutation {
            state: project(record, now),
        })
    }

    pub(crate) fn admit_claim(&self, ticket: &ClaimTicket) -> Result<bool, TimerError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        let Some(record) = state.records.get_mut(&ticket.key) else {
            return Ok(false);
        };
        if !claim_matches(record, ticket) {
            return Ok(false);
        }
        record
            .claim
            .as_mut()
            .expect("claim missing")
            .delivery_admitted = true;
        Ok(true)
    }

    pub(crate) fn complete_claim(
        &self,
        ticket: &ClaimTicket,
        persisted: bool,
    ) -> Result<Option<ClaimCompletion>, TimerError> {
        let now = self.clock.now_ms();
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        let Some(record) = state.records.get_mut(&ticket.key) else {
            return Ok(None);
        };
        if !claim_matches(record, ticket) {
            return Ok(None);
        }
        if persisted
            && !record
                .claim
                .as_ref()
                .is_some_and(|claim| claim.delivery_admitted)
        {
            return Ok(None);
        }
        record.claim = None;
        record.frozen_completion = None;
        if persisted {
            record.audio_id = record
                .audio_id
                .checked_add(1)
                .ok_or_else(|| make_unavailable(record))?;
            advance_revision(record)?;
            record.phase = InternalPhase::Fired;
            record.remaining_ms = Some(0);
            let audio_ticket = AudioTicket {
                key: ticket.key.clone(),
                round_id: ticket.round_id,
                audio_id: record.audio_id,
                fired_revision: record.revision,
            };
            record.audio = Some(TimerAudioState::Issued(audio_ticket.clone()));
            Ok(Some(ClaimCompletion {
                state: project(record, now),
                audio_ticket: Some(audio_ticket),
            }))
        } else {
            advance_revision(record)?;
            record.phase = InternalPhase::Idle;
            record.remaining_ms = record.duration_ms;
            record.due_at_ms = None;
            Ok(Some(ClaimCompletion {
                state: project(record, now),
                audio_ticket: None,
            }))
        }
    }

    pub(crate) fn admit_audio_start(&self, ticket: &AudioTicket) -> Result<bool, TimerError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        let Some(record) = state.records.get_mut(&ticket.key) else {
            return Ok(false);
        };
        if !matches!(record.audio.as_ref(), Some(TimerAudioState::Issued(current)) if current == ticket)
        {
            return Ok(false);
        }
        record.audio = Some(TimerAudioState::Admitted(ticket.clone()));
        Ok(true)
    }

    pub(crate) fn confirm_audio_after_play_failure(
        &self,
        ticket: &AudioTicket,
    ) -> Result<bool, TimerError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        let Some(record) = state.records.get_mut(&ticket.key) else {
            return Ok(false);
        };
        if !matches!(record.audio.as_ref(), Some(TimerAudioState::Admitted(current)) if current == ticket)
        {
            return Ok(false);
        }
        record.audio = Some(TimerAudioState::Confirmed(ticket.clone()));
        Ok(true)
    }

    pub(crate) fn confirm_audio_without_start(
        &self,
        ticket: &AudioTicket,
    ) -> Result<bool, TimerError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        let Some(record) = state.records.get_mut(&ticket.key) else {
            return Ok(false);
        };
        if !matches!(record.audio.as_ref(), Some(TimerAudioState::Issued(current)) if current == ticket)
        {
            return Ok(false);
        }
        record.audio = Some(TimerAudioState::Confirmed(ticket.clone()));
        Ok(true)
    }

    pub(crate) fn with_audio_focus_authority<T>(
        &self,
        action: impl FnOnce(&mut TimerAudioFocusAuthority<'_>) -> T,
    ) -> Result<T, TimerError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        Ok(action(&mut TimerAudioFocusAuthority { state: &mut state }))
    }

    pub(crate) fn terminate_all_audio(&self) -> Result<(), TimerError> {
        let mut state = self.lock_state()?;
        for record in state.records.values_mut() {
            confirm_current_audio(record);
        }
        Ok(())
    }

    pub(crate) fn cancel_generation(&self, key: &TimerKey) -> TimerOperation<()> {
        let mut post_lock_effects = Vec::new();
        let result = (|| {
            let mut state = self.lock_state()?;
            if state.terminal {
                return Err(TimerError::TimerUnavailable);
            }
            remove_queued(&mut state, key);
            if let Some(mut record) = state.records.remove(key) {
                take_audio_for_operation(&mut record, &mut post_lock_effects);
            }
            self.wake.notify_all();
            Ok(())
        })();
        TimerOperation {
            result,
            post_lock_effects,
        }
    }

    pub(crate) fn shutdown(&self) -> TimerOperation<()> {
        let mut post_lock_effects = Vec::new();
        let mut result = (|| {
            let mut state = self.lock_state()?;
            if !state.terminal {
                state.terminal = true;
                state.queue.clear();
                for record in state.records.values_mut() {
                    take_audio_for_operation(record, &mut post_lock_effects);
                }
            }
            Ok(())
        })();
        self.wake.notify_all();
        if result.is_ok() {
            match self.worker.lock() {
                Ok(mut worker) => {
                    if let Some(worker) = worker.take() {
                        let _ = worker.join();
                    }
                }
                Err(_) => {
                    result = Err(TimerError::TimerUnavailable);
                }
            }
        }
        TimerOperation {
            result,
            post_lock_effects,
        }
    }

    pub(super) fn claim_next_due(&self) -> Result<Option<ClaimTicket>, TimerError> {
        let now = self.clock.now_ms();
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(TimerError::TimerUnavailable);
        }
        loop {
            let Some(Reverse(item)) = state.queue.peek().cloned() else {
                return Ok(None);
            };
            let current = queue_item_current(&state, &item);
            if !current {
                state.queue.pop();
                continue;
            }
            if item.due_at_ms > now {
                return Ok(None);
            }
            state.queue.pop();
            let record = state
                .records
                .get_mut(&item.key)
                .expect("current queue record missing");
            record.claim_id = record
                .claim_id
                .checked_add(1)
                .ok_or_else(|| make_unavailable(record))?;
            advance_revision(record)?;
            record.phase = InternalPhase::Claiming;
            record.remaining_ms = Some(0);
            record.due_at_ms = None;
            let ticket = ClaimTicket {
                key: item.key,
                round_id: item.round_id,
                claim_id: record.claim_id,
                claim_revision: record.revision,
                frozen_completion: record
                    .frozen_completion
                    .clone()
                    .ok_or_else(|| make_unavailable(record))?,
            };
            record.claim = Some(StoredClaim {
                ticket: ticket.clone(),
                delivery_admitted: false,
            });
            return Ok(Some(ticket));
        }
    }

    fn worker_loop(&self, handler: Arc<dyn Fn(ClaimTicket) + Send + Sync>) {
        loop {
            match self.claim_next_due() {
                Ok(Some(ticket)) => {
                    handler(ticket);
                    continue;
                }
                Err(TimerError::TimerUnavailable) => return,
                Err(_) => return,
                Ok(None) => {}
            }
            let Ok(state) = self.state.lock() else {
                return;
            };
            if state.terminal {
                return;
            }
            let Some(item) = state.queue.peek() else {
                if self.wake.wait(state).is_err() {
                    return;
                }
                continue;
            };
            let wait_ms = item
                .0
                .due_at_ms
                .saturating_sub(self.clock.now_ms())
                .clamp(1, SLEEP_RECHECK_MS);
            if self
                .wake
                .wait_timeout(state, Duration::from_millis(wait_ms))
                .is_err()
            {
                return;
            }
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, ServiceState>, TimerError> {
        self.state.lock().map_err(|_| TimerError::TimerUnavailable)
    }
}

fn validate_input(input: &PluginTimerStartInput) -> Result<(), TimerError> {
    let message = &input.completion_message;
    if !(MIN_DURATION_MS..=MAX_DURATION_MS).contains(&input.duration_ms)
        || message.trim().is_empty()
        || message.chars().count() > 500
        || message.chars().any(char::is_control)
    {
        return Err(TimerError::InvalidTimerInput);
    }
    Ok(())
}

fn ensure_available(state: &ServiceState, key: &TimerKey) -> Result<(), TimerError> {
    if state.terminal
        || state
            .records
            .get(key)
            .is_some_and(|record| record.unavailable)
    {
        Err(TimerError::TimerUnavailable)
    } else {
        Ok(())
    }
}

fn advance_revision(record: &mut TimerRecord) -> Result<u64, TimerError> {
    let Some(revision) = record.revision.checked_add(1) else {
        return Err(make_unavailable(record));
    };
    record.revision = revision;
    Ok(revision)
}

fn make_unavailable(record: &mut TimerRecord) -> TimerError {
    record.unavailable = true;
    record.phase = InternalPhase::Idle;
    record.due_at_ms = None;
    record.frozen_completion = None;
    record.claim = None;
    record.audio = None;
    TimerError::TimerUnavailable
}

fn remove_queued(state: &mut ServiceState, key: &TimerKey) {
    state.queue = state
        .queue
        .drain()
        .filter(|item| item.0.key != *key)
        .collect();
}

fn queue_item_current(state: &ServiceState, item: &QueueItem) -> bool {
    state.records.get(&item.key).is_some_and(|record| {
        !record.unavailable
            && record.phase == InternalPhase::Running
            && record.round_id == item.round_id
            && record.revision == item.due_revision
            && record.due_at_ms == Some(item.due_at_ms)
    })
}

fn claim_matches(record: &TimerRecord, ticket: &ClaimTicket) -> bool {
    !record.unavailable
        && record.phase == InternalPhase::Claiming
        && record.round_id == ticket.round_id
        && record.revision == ticket.claim_revision
        && record
            .claim
            .as_ref()
            .is_some_and(|claim| claim.ticket == *ticket)
}

fn initial_state() -> PluginTimerState {
    PluginTimerState {
        timer_revision: "0".into(),
        phase: PluginTimerPhase::Idle,
        duration_ms: None,
        remaining_ms: None,
    }
}

fn take_audio_for_operation(
    record: &mut TimerRecord,
    post_lock_effects: &mut Vec<TimerPostLockEffect>,
) {
    match record.audio.take() {
        Some(TimerAudioState::Admitted(ticket)) => {
            post_lock_effects.push(TimerPostLockEffect::AudioCancelled(ticket));
        }
        Some(TimerAudioState::Issued(_ticket) | TimerAudioState::Confirmed(_ticket)) => {}
        None => {}
    }
}

fn confirm_current_audio(record: &mut TimerRecord) {
    let ticket = match record.audio.as_ref() {
        Some(TimerAudioState::Issued(ticket) | TimerAudioState::Admitted(ticket)) => {
            Some(ticket.clone())
        }
        Some(TimerAudioState::Confirmed(_)) | None => None,
    };
    if let Some(ticket) = ticket {
        record.audio = Some(TimerAudioState::Confirmed(ticket));
    }
}

fn project(record: &TimerRecord, now_ms: u64) -> PluginTimerState {
    let phase = match record.phase {
        InternalPhase::Idle => PluginTimerPhase::Idle,
        InternalPhase::Running | InternalPhase::Claiming => PluginTimerPhase::Running,
        InternalPhase::Paused => PluginTimerPhase::Paused,
        InternalPhase::Fired => PluginTimerPhase::Fired,
    };
    let remaining_ms = match record.phase {
        InternalPhase::Running => record.due_at_ms.map(|due| {
            due.saturating_sub(now_ms)
                .min(record.duration_ms.unwrap_or_default())
        }),
        InternalPhase::Claiming | InternalPhase::Fired => Some(0),
        InternalPhase::Idle | InternalPhase::Paused => record.remaining_ms,
    };
    PluginTimerState {
        timer_revision: record.revision.to_string(),
        phase,
        duration_ms: record.duration_ms,
        remaining_ms,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc, Arc,
        },
        time::Duration,
    };

    use super::{
        Clock, PluginTimerPhase, PluginTimerService, PluginTimerStartInput, TimerError, TimerKey,
        TimerOperation, TimerPostLockEffect,
    };

    #[derive(Default)]
    struct TestClock(AtomicU64);

    impl TestClock {
        fn advance(&self, millis: u64) {
            self.0.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl Clock for TestClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn fixture() -> (Arc<PluginTimerService>, Arc<TestClock>, TimerKey) {
        let clock = Arc::new(TestClock::default());
        let service = Arc::new(PluginTimerService::new(clock.clone()));
        let key = TimerKey::new("com.example.timer", 7).unwrap();
        (service, clock, key)
    }

    fn input(duration_ms: u64) -> PluginTimerStartInput {
        PluginTimerStartInput {
            duration_ms,
            completion_message: " finished ".into(),
        }
    }

    trait TimerOperationTestExt<T> {
        fn unwrap(self) -> T;
    }

    impl<T> TimerOperationTestExt<T> for TimerOperation<T> {
        fn unwrap(self) -> T {
            self.result.unwrap()
        }
    }

    fn complete_fired_round(
        service: &PluginTimerService,
        clock: &TestClock,
        key: &TimerKey,
    ) -> super::AudioTicket {
        service
            .start(key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        let claim = service.claim_next_due().unwrap().unwrap();
        assert!(service.admit_claim(&claim).unwrap());
        service
            .complete_claim(&claim, true)
            .unwrap()
            .unwrap()
            .audio_ticket
            .unwrap()
    }

    #[test]
    fn audio_authority_transitions_keep_the_fired_revision() {
        let (service, clock, key) = fixture();
        let ticket = complete_fired_round(&service, &clock, &key);
        let fired_revision = service.get_state(&key).unwrap().timer_revision;

        assert!(service.admit_audio_start(&ticket).unwrap());
        assert_eq!(
            service.get_state(&key).unwrap().timer_revision,
            fired_revision
        );
        assert!(service.confirm_audio_after_play_failure(&ticket).unwrap());
        assert_eq!(
            service.get_state(&key).unwrap().timer_revision,
            fired_revision
        );
        assert!(!service.admit_audio_start(&ticket).unwrap());
    }

    #[test]
    fn reset_returns_admitted_audio_as_a_post_lock_cancellation() {
        let (service, clock, key) = fixture();
        let ticket = complete_fired_round(&service, &clock, &key);
        assert!(service.admit_audio_start(&ticket).unwrap());

        let operation = service.reset(&key);

        assert_eq!(
            operation.post_lock_effects,
            [TimerPostLockEffect::AudioCancelled(ticket)]
        );
        assert_eq!(
            operation.result.unwrap().state.phase,
            PluginTimerPhase::Idle
        );
    }

    #[test]
    fn new_round_exhaustion_still_returns_admitted_audio_cancellation() {
        let (service, clock, key) = fixture();
        let ticket = complete_fired_round(&service, &clock, &key);
        assert!(service.admit_audio_start(&ticket).unwrap());
        service
            .state
            .lock()
            .unwrap()
            .records
            .get_mut(&key)
            .unwrap()
            .round_id = u64::MAX;

        let operation = service.start(&key, "Timer", Some(input(1_000)), true);

        assert_eq!(operation.result, Err(TimerError::TimerUnavailable));
        assert_eq!(
            operation.post_lock_effects,
            [TimerPostLockEffect::AudioCancelled(ticket)]
        );
    }

    #[test]
    fn focus_authority_confirms_only_current_audio_without_revision_changes() {
        let (service, clock, first_key) = fixture();
        let first = complete_fired_round(&service, &clock, &first_key);
        assert!(service.admit_audio_start(&first).unwrap());
        let second_key = TimerKey::new("com.example.second", 1).unwrap();
        let second = complete_fired_round(&service, &clock, &second_key);
        let first_revision = service.get_state(&first_key).unwrap().timer_revision;
        let second_revision = service.get_state(&second_key).unwrap().timer_revision;

        service
            .with_audio_focus_authority(|authority| authority.confirm_all_current())
            .unwrap();

        assert!(!service.admit_audio_start(&first).unwrap());
        assert!(!service.admit_audio_start(&second).unwrap());
        assert_eq!(
            service.get_state(&first_key).unwrap().timer_revision,
            first_revision
        );
        assert_eq!(
            service.get_state(&second_key).unwrap().timer_revision,
            second_revision
        );
    }

    #[test]
    fn terminal_audio_cleanup_absorbs_issued_and_admitted_tickets() {
        let (service, clock, first_key) = fixture();
        let first = complete_fired_round(&service, &clock, &first_key);
        assert!(service.admit_audio_start(&first).unwrap());
        let second_key = TimerKey::new("com.example.second", 1).unwrap();
        let second = complete_fired_round(&service, &clock, &second_key);

        service.terminate_all_audio().unwrap();

        assert!(!service.admit_audio_start(&first).unwrap());
        assert!(!service.admit_audio_start(&second).unwrap());
    }

    #[test]
    fn focused_no_start_confirmation_absorbs_an_issued_ticket() {
        let (service, clock, key) = fixture();
        let ticket = complete_fired_round(&service, &clock, &key);
        let fired_revision = service.get_state(&key).unwrap().timer_revision;

        assert!(service.confirm_audio_without_start(&ticket).unwrap());

        assert!(!service.confirm_audio_without_start(&ticket).unwrap());
        assert!(!service.admit_audio_start(&ticket).unwrap());
        assert_eq!(
            service.get_state(&key).unwrap().timer_revision,
            fired_revision
        );
    }

    #[test]
    fn state_machine_starts_pauses_resumes_resets_and_fires() {
        let (service, clock, key) = fixture();
        let initial = service.get_state(&key).unwrap();
        assert_eq!(initial.phase, PluginTimerPhase::Idle);
        assert_eq!((initial.duration_ms, initial.remaining_ms), (None, None));
        assert_eq!(initial.timer_revision, "0");

        assert_eq!(
            service.start(&key, "Timer", None, true).result,
            Err(TimerError::TimerInputRequired)
        );
        let running = service
            .start(&key, "Timer", Some(input(10_000)), true)
            .unwrap();
        assert_eq!(running.state.phase, PluginTimerPhase::Running);
        assert_eq!(running.state.remaining_ms, Some(10_000));
        assert_eq!(running.state.timer_revision, "1");

        clock.advance(2_500);
        let paused = service.stop(&key).unwrap();
        assert_eq!(paused.state.phase, PluginTimerPhase::Paused);
        assert_eq!(paused.state.remaining_ms, Some(7_500));
        assert_eq!(paused.state.timer_revision, "2");
        clock.advance(20_000);
        assert!(service.claim_next_due().unwrap().is_none());

        let resumed = service.start(&key, "ignored", None, true).unwrap();
        assert_eq!(resumed.state.phase, PluginTimerPhase::Running);
        assert_eq!(resumed.state.timer_revision, "3");
        clock.advance(7_500);
        let ticket = service.claim_next_due().unwrap().unwrap();
        assert_eq!(service.stop(&key).unwrap().state.remaining_ms, Some(0));
        assert!(service.admit_claim(&ticket).unwrap());
        let fired = service.complete_claim(&ticket, true).unwrap().unwrap();
        assert_eq!(fired.state.phase, PluginTimerPhase::Fired);
        assert_eq!(fired.state.timer_revision, "5");
        assert!(fired.audio_ticket.is_some());

        let reset_operation = service.reset(&key);
        assert!(reset_operation.post_lock_effects.is_empty());
        let reset = reset_operation.unwrap();
        assert_eq!(reset.state.phase, PluginTimerPhase::Idle);
        assert_eq!(
            (reset.state.duration_ms, reset.state.remaining_ms),
            (Some(10_000), Some(10_000))
        );
        assert_eq!(
            service.start(&key, "Timer", None, true).result,
            Err(TimerError::TimerInputRequired)
        );
    }

    #[test]
    fn stop_and_claim_share_one_linearization_order() {
        let (service, clock, key) = fixture();
        service
            .start(&key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        let paused = service.stop(&key).unwrap();
        assert_eq!(paused.state.phase, PluginTimerPhase::Paused);
        assert!(service.claim_next_due().unwrap().is_none());

        service.reset(&key).unwrap();
        service
            .start(&key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        let ticket = service.claim_next_due().unwrap().unwrap();
        let too_late = service.stop(&key).unwrap();
        assert_eq!(too_late.state.phase, PluginTimerPhase::Running);
        assert_eq!(too_late.state.remaining_ms, Some(0));
        assert!(service.admit_claim(&ticket).unwrap());
        assert_eq!(
            service
                .complete_claim(&ticket, true)
                .unwrap()
                .unwrap()
                .state
                .phase,
            PluginTimerPhase::Fired
        );
    }

    #[test]
    fn reset_and_new_round_reject_stale_claim_and_audio_tickets() {
        let (service, clock, key) = fixture();
        service
            .start(&key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        let old_claim = service.claim_next_due().unwrap().unwrap();
        service.reset(&key).unwrap();
        assert!(!service.admit_claim(&old_claim).unwrap());
        assert!(service.complete_claim(&old_claim, true).unwrap().is_none());

        service
            .start(&key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        let claim = service.claim_next_due().unwrap().unwrap();
        assert!(service.admit_claim(&claim).unwrap());
        let audio = service
            .complete_claim(&claim, true)
            .unwrap()
            .unwrap()
            .audio_ticket
            .unwrap();
        assert!(service.admit_audio_start(&audio).unwrap());
        let next = service.start(&key, "Timer", Some(input(2_000)), true);
        assert_eq!(
            next.post_lock_effects,
            [TimerPostLockEffect::AudioCancelled(audio.clone())]
        );
        next.unwrap();
        assert!(!service.admit_audio_start(&audio).unwrap());
    }

    #[test]
    fn invalid_input_and_message_store_failure_do_not_change_state() {
        let (service, _, key) = fixture();
        for candidate in [
            PluginTimerStartInput {
                duration_ms: 999,
                completion_message: "ok".into(),
            },
            PluginTimerStartInput {
                duration_ms: 86_400_001,
                completion_message: "ok".into(),
            },
            PluginTimerStartInput {
                duration_ms: 1_000,
                completion_message: "   ".into(),
            },
            PluginTimerStartInput {
                duration_ms: 1_000,
                completion_message: "line\nbreak".into(),
            },
            PluginTimerStartInput {
                duration_ms: 1_000,
                completion_message: "x".repeat(501),
            },
        ] {
            assert_eq!(
                service.start(&key, "Timer", Some(candidate), true).result,
                Err(TimerError::InvalidTimerInput)
            );
        }
        assert_eq!(
            service
                .start(&key, "Timer", Some(input(1_000)), false)
                .result,
            Err(TimerError::MessageStoreUnavailable)
        );
        assert_eq!(service.get_state(&key).unwrap().timer_revision, "0");
    }

    #[test]
    fn disallowed_input_takes_precedence_over_input_content_validation() {
        let (service, _, key) = fixture();
        service
            .start(&key, "Timer", Some(input(10_000)), true)
            .unwrap();
        assert_eq!(
            service
                .start(
                    &key,
                    "Timer",
                    Some(PluginTimerStartInput {
                        duration_ms: 1,
                        completion_message: String::new(),
                    }),
                    true,
                )
                .result,
            Err(TimerError::TimerInputNotAllowed)
        );
        service.stop(&key).unwrap();
        assert_eq!(
            service
                .start(
                    &key,
                    "Timer",
                    Some(PluginTimerStartInput {
                        duration_ms: 1,
                        completion_message: String::new(),
                    }),
                    true,
                )
                .result,
            Err(TimerError::TimerInputNotAllowed)
        );
    }

    #[test]
    fn message_failure_returns_idle_and_shutdown_is_terminal() {
        let (service, clock, key) = fixture();
        service
            .start(&key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        let ticket = service.claim_next_due().unwrap().unwrap();
        assert!(service.admit_claim(&ticket).unwrap());
        let failed = service.complete_claim(&ticket, false).unwrap().unwrap();
        assert_eq!(failed.state.phase, PluginTimerPhase::Idle);
        assert_eq!(
            (failed.state.duration_ms, failed.state.remaining_ms),
            (Some(1_000), Some(1_000))
        );
        assert!(failed.audio_ticket.is_none());

        service.shutdown().unwrap();
        assert_eq!(service.get_state(&key), Err(TimerError::TimerUnavailable));
        assert_eq!(
            service
                .start(&key, "Timer", Some(input(1_000)), true)
                .result,
            Err(TimerError::TimerUnavailable)
        );
    }

    #[test]
    fn shared_worker_claims_each_round_once() {
        let (service, clock, key) = fixture();
        let (sender, receiver) = mpsc::channel();
        service
            .start_worker(Arc::new(move |ticket| {
                let _ = sender.send(ticket);
            }))
            .unwrap();
        service
            .start(&key, "Timer", Some(input(1_000)), true)
            .unwrap();
        clock.advance(1_000);
        service.wake.notify_all();

        let ticket = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(ticket.key(), &key);
        assert!(receiver.recv_timeout(Duration::from_millis(150)).is_err());
        assert_eq!(
            service.start_worker(Arc::new(|_| {})),
            Err(TimerError::TimerUnavailable)
        );
        service.shutdown().unwrap();
    }

    #[test]
    fn revision_exhaustion_is_failure_closed_for_only_that_timer() {
        let (service, _, key) = fixture();
        service
            .start(&key, "Timer", Some(input(10_000)), true)
            .unwrap();
        service
            .state
            .lock()
            .unwrap()
            .records
            .get_mut(&key)
            .unwrap()
            .revision = u64::MAX;

        assert_eq!(service.stop(&key).result, Err(TimerError::TimerUnavailable));
        assert_eq!(service.get_state(&key), Err(TimerError::TimerUnavailable));

        let other = TimerKey::new("com.example.other", 1).unwrap();
        assert_eq!(service.get_state(&other).unwrap().timer_revision, "0");
    }
}
