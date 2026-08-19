use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) const MIN_DELAY_MS: u64 = 1_000;
pub(crate) const MAX_DELAY_MS: u64 = 86_400_000;
pub(crate) const MAX_PENDING_PER_PLUGIN: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DelayedMessageRegistration {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) plugin_name_snapshot: String,
    pub(crate) request_id: String,
    pub(crate) content: String,
    pub(crate) delay_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScheduledPluginMessage {
    pub(crate) schedule_id: u64,
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) plugin_name_snapshot: String,
    pub(crate) request_id: String,
    pub(crate) content: String,
    pub(crate) due_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DelayedMessageScheduleError {
    InvalidRegistration,
    InvalidDelay,
    LimitExceeded,
    Unavailable,
}

#[derive(Debug)]
struct SchedulerState {
    next_schedule_id: u64,
    pending_by_due: BTreeMap<(Instant, u64), ScheduledPluginMessage>,
    pending_by_plugin: HashMap<String, usize>,
    terminal: bool,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            next_schedule_id: 1,
            pending_by_due: BTreeMap::new(),
            pending_by_plugin: HashMap::new(),
            terminal: false,
        }
    }
}

pub(crate) struct DelayedMessageScheduler {
    state: Mutex<SchedulerState>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Default for DelayedMessageScheduler {
    fn default() -> Self {
        Self {
            state: Mutex::new(SchedulerState::default()),
            wake: Condvar::new(),
            worker: Mutex::new(None),
        }
    }
}

impl DelayedMessageScheduler {
    pub(crate) fn schedule(
        &self,
        registration: DelayedMessageRegistration,
        now: Instant,
    ) -> Result<u64, DelayedMessageScheduleError> {
        if !valid_registration(&registration) {
            return Err(DelayedMessageScheduleError::InvalidRegistration);
        }
        if !(MIN_DELAY_MS..=MAX_DELAY_MS).contains(&registration.delay_ms) {
            return Err(DelayedMessageScheduleError::InvalidDelay);
        }
        let due_at = now
            .checked_add(Duration::from_millis(registration.delay_ms))
            .ok_or(DelayedMessageScheduleError::InvalidDelay)?;
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(DelayedMessageScheduleError::Unavailable);
        }
        if state
            .pending_by_plugin
            .get(&registration.plugin_id)
            .copied()
            .unwrap_or_default()
            >= MAX_PENDING_PER_PLUGIN
        {
            return Err(DelayedMessageScheduleError::LimitExceeded);
        }
        let schedule_id = state.next_schedule_id;
        state.next_schedule_id = schedule_id
            .checked_add(1)
            .ok_or(DelayedMessageScheduleError::Unavailable)?;
        let plugin_id = registration.plugin_id.clone();
        let message = ScheduledPluginMessage {
            schedule_id,
            plugin_id: registration.plugin_id,
            plugin_generation: registration.plugin_generation,
            plugin_name_snapshot: registration.plugin_name_snapshot,
            request_id: registration.request_id,
            content: registration.content,
            due_at,
        };
        if state
            .pending_by_due
            .insert((due_at, schedule_id), message)
            .is_some()
        {
            return Err(DelayedMessageScheduleError::Unavailable);
        }
        *state.pending_by_plugin.entry(plugin_id).or_default() += 1;
        drop(state);
        self.wake.notify_one();
        Ok(schedule_id)
    }

    pub(crate) fn cancel_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<usize, DelayedMessageScheduleError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(DelayedMessageScheduleError::Unavailable);
        }
        let removed = state
            .pending_by_plugin
            .remove(plugin_id)
            .unwrap_or_default();
        if removed != 0 {
            state
                .pending_by_due
                .retain(|_, message| message.plugin_id != plugin_id);
        }
        drop(state);
        self.wake.notify_one();
        Ok(removed)
    }

    #[cfg(test)]
    pub(crate) fn claim_due(
        &self,
        now: Instant,
    ) -> Result<Option<ScheduledPluginMessage>, DelayedMessageScheduleError> {
        let mut state = self.lock_state()?;
        if state.terminal {
            return Err(DelayedMessageScheduleError::Unavailable);
        }
        Ok(pop_due(&mut state, now))
    }

    pub(crate) fn start<F>(self: &Arc<Self>, deliver: F) -> Result<(), DelayedMessageScheduleError>
    where
        F: Fn(ScheduledPluginMessage) + Send + Sync + 'static,
    {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| DelayedMessageScheduleError::Unavailable)?;
        if worker.is_some() || self.lock_state()?.terminal {
            return Err(DelayedMessageScheduleError::Unavailable);
        }
        let scheduler = Arc::clone(self);
        let deliver = Arc::new(deliver);
        let handle = thread::Builder::new()
            .name("uipilot-delayed-messages".into())
            .spawn(move || worker_loop(&scheduler, deliver))
            .map_err(|_| DelayedMessageScheduleError::Unavailable)?;
        *worker = Some(handle);
        Ok(())
    }

    pub(crate) fn shutdown(&self) {
        let Ok(mut worker) = self.worker.lock() else {
            return;
        };
        if let Ok(mut state) = self.state.lock() {
            state.terminal = true;
            state.pending_by_due.clear();
            state.pending_by_plugin.clear();
        }
        self.wake.notify_all();
        if let Some(handle) = worker.take() {
            let _ = handle.join();
        }
    }

    fn lock_state(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, SchedulerState>, DelayedMessageScheduleError> {
        self.state
            .lock()
            .map_err(|_| DelayedMessageScheduleError::Unavailable)
    }
}

fn valid_registration(registration: &DelayedMessageRegistration) -> bool {
    !registration.plugin_id.is_empty()
        && registration.plugin_generation != 0
        && !registration.plugin_name_snapshot.is_empty()
        && !registration.request_id.is_empty()
        && !registration.content.is_empty()
}

fn pop_due(state: &mut SchedulerState, now: Instant) -> Option<ScheduledPluginMessage> {
    let key = state
        .pending_by_due
        .first_key_value()
        .and_then(|(key, _)| (key.0 <= now).then_some(*key))?;
    let message = state.pending_by_due.remove(&key)?;
    decrement_plugin_count(state, &message.plugin_id);
    Some(message)
}

fn decrement_plugin_count(state: &mut SchedulerState, plugin_id: &str) {
    let Some(count) = state.pending_by_plugin.get_mut(plugin_id) else {
        return;
    };
    *count -= 1;
    if *count == 0 {
        state.pending_by_plugin.remove(plugin_id);
    }
}

fn worker_loop<F>(scheduler: &DelayedMessageScheduler, deliver: Arc<F>)
where
    F: Fn(ScheduledPluginMessage) + Send + Sync + 'static,
{
    loop {
        let message = {
            let Ok(mut state) = scheduler.state.lock() else {
                return;
            };
            loop {
                if state.terminal {
                    return;
                }
                let now = Instant::now();
                if let Some(message) = pop_due(&mut state, now) {
                    break message;
                }
                if let Some((due_at, _)) = state.pending_by_due.first_key_value() {
                    let wait = due_at.0.saturating_duration_since(now);
                    let Ok((next, _)) = scheduler.wake.wait_timeout(state, wait) else {
                        return;
                    };
                    state = next;
                } else {
                    let Ok(next) = scheduler.wake.wait(state) else {
                        return;
                    };
                    state = next;
                }
            }
        };
        deliver(message);
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::*;

    fn registration(
        plugin_id: &str,
        request_id: &str,
        delay_ms: u64,
    ) -> DelayedMessageRegistration {
        DelayedMessageRegistration {
            plugin_id: plugin_id.into(),
            plugin_generation: 7,
            plugin_name_snapshot: format!("{plugin_id} name"),
            request_id: request_id.into(),
            content: format!("message {request_id}"),
            delay_ms,
        }
    }

    #[test]
    fn delay_bounds_are_inclusive_and_outside_values_allocate_nothing() {
        let now = Instant::now();
        for (delay_ms, expected) in [
            (
                MIN_DELAY_MS - 1,
                Err(DelayedMessageScheduleError::InvalidDelay),
            ),
            (MIN_DELAY_MS, Ok(1)),
            (MAX_DELAY_MS, Ok(2)),
            (
                MAX_DELAY_MS + 1,
                Err(DelayedMessageScheduleError::InvalidDelay),
            ),
        ] {
            let scheduler = DelayedMessageScheduler::default();
            let result =
                scheduler.schedule(registration("com.example.bounds", "request", delay_ms), now);
            let expected = expected.map(|_| 1);
            assert_eq!(result, expected, "delay_ms={delay_ms}");
        }
    }

    #[test]
    fn quota_is_plugin_scoped_and_rejection_does_not_consume_an_id() {
        let scheduler = DelayedMessageScheduler::default();
        let now = Instant::now();
        for index in 0..MAX_PENDING_PER_PLUGIN {
            assert_eq!(
                scheduler.schedule(
                    registration(
                        "com.example.full",
                        &format!("request-{index}"),
                        MIN_DELAY_MS
                    ),
                    now,
                ),
                Ok(index as u64 + 1)
            );
        }
        assert_eq!(
            scheduler.schedule(
                registration("com.example.full", "overflow", MIN_DELAY_MS),
                now,
            ),
            Err(DelayedMessageScheduleError::LimitExceeded)
        );
        assert_eq!(
            scheduler.schedule(
                registration("com.example.other", "other", MIN_DELAY_MS),
                now,
            ),
            Ok(MAX_PENDING_PER_PLUGIN as u64 + 1)
        );
        assert_eq!(
            scheduler.cancel_plugin("com.example.full"),
            Ok(MAX_PENDING_PER_PLUGIN)
        );
        assert_eq!(
            scheduler.schedule(
                registration("com.example.full", "after-cancel", MIN_DELAY_MS),
                now,
            ),
            Ok(MAX_PENDING_PER_PLUGIN as u64 + 2)
        );
    }

    #[test]
    fn due_messages_are_claimed_in_deadline_then_id_order_exactly_once() {
        let scheduler = DelayedMessageScheduler::default();
        let now = Instant::now();
        scheduler
            .schedule(registration("com.example.order", "late", 2_000), now)
            .unwrap();
        scheduler
            .schedule(registration("com.example.order", "first", 1_000), now)
            .unwrap();
        scheduler
            .schedule(registration("com.example.order", "second", 1_000), now)
            .unwrap();

        assert_eq!(
            scheduler.claim_due(now + Duration::from_millis(999)),
            Ok(None)
        );
        assert_eq!(
            scheduler
                .claim_due(now + Duration::from_millis(1_000))
                .unwrap()
                .unwrap()
                .request_id,
            "first"
        );
        assert_eq!(
            scheduler
                .claim_due(now + Duration::from_millis(1_000))
                .unwrap()
                .unwrap()
                .request_id,
            "second"
        );
        assert_eq!(
            scheduler.claim_due(now + Duration::from_millis(1_000)),
            Ok(None)
        );
        assert_eq!(
            scheduler
                .claim_due(now + Duration::from_millis(2_000))
                .unwrap()
                .unwrap()
                .request_id,
            "late"
        );
        assert_eq!(
            scheduler.claim_due(now + Duration::from_millis(2_000)),
            Ok(None)
        );
    }

    #[test]
    fn cancellation_removes_only_the_named_plugin() {
        let scheduler = DelayedMessageScheduler::default();
        let now = Instant::now();
        scheduler
            .schedule(registration("com.example.cancel", "one", MIN_DELAY_MS), now)
            .unwrap();
        scheduler
            .schedule(registration("com.example.keep", "two", MIN_DELAY_MS), now)
            .unwrap();

        assert_eq!(scheduler.cancel_plugin("com.example.cancel"), Ok(1));
        assert_eq!(
            scheduler
                .claim_due(now + Duration::from_millis(MIN_DELAY_MS))
                .unwrap()
                .unwrap()
                .plugin_id,
            "com.example.keep"
        );
        assert_eq!(
            scheduler.claim_due(now + Duration::from_millis(MIN_DELAY_MS)),
            Ok(None)
        );
    }

    #[test]
    fn worker_wakes_for_an_earlier_deadline_and_shutdown_is_terminal() {
        let scheduler = Arc::new(DelayedMessageScheduler::default());
        let (sender, receiver) = mpsc::channel();
        scheduler
            .start(move |message| {
                let _ = sender.send(message.request_id);
            })
            .unwrap();
        scheduler
            .schedule(
                registration("com.example.worker", "far", MAX_DELAY_MS),
                Instant::now(),
            )
            .unwrap();
        scheduler
            .schedule(
                registration("com.example.worker", "due", MIN_DELAY_MS),
                Instant::now() - Duration::from_millis(MIN_DELAY_MS + 1),
            )
            .unwrap();

        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            "due"
        );
        scheduler.shutdown();
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            scheduler.schedule(
                registration("com.example.worker", "after", MIN_DELAY_MS),
                Instant::now(),
            ),
            Err(DelayedMessageScheduleError::Unavailable)
        );
    }
}
