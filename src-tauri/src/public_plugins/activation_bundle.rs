use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
};

use super::{
    activation::RuntimeSnapshot, EffectivePluginConfig, PreparedAlarmAsset, ValidatedAlarmAsset,
};

#[derive(Clone)]
pub(super) struct ActivationBundle {
    pub(super) config: EffectivePluginConfig,
    pub(super) runtime: Arc<RuntimeSnapshot>,
    pub(super) alarm: Option<ValidatedAlarmAsset>,
    pub(super) activation_id: u64,
    pub(super) admission_epoch: u64,
    pub(super) runtime_recovery_needed: bool,
}

impl ActivationBundle {
    pub(super) fn with_config(&self, config: EffectivePluginConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            runtime: Arc::clone(&self.runtime),
            alarm: self.alarm.clone(),
            activation_id: self.activation_id,
            admission_epoch: self.admission_epoch,
            runtime_recovery_needed: self.runtime_recovery_needed,
        })
    }

    pub(super) fn with_runtime_recovery(&self, runtime_recovery_needed: bool) -> Arc<Self> {
        Arc::new(Self {
            config: self.config.clone(),
            runtime: Arc::clone(&self.runtime),
            alarm: self.alarm.clone(),
            activation_id: self.activation_id,
            admission_epoch: self.admission_epoch,
            runtime_recovery_needed,
        })
    }
}

#[derive(Clone)]
pub(super) struct ActivationCandidate {
    pub(super) runtime: Arc<RuntimeSnapshot>,
    pub(super) alarm: Option<ValidatedAlarmAsset>,
    pub(super) activation_id: u64,
    pub(super) admission_epoch: u64,
    pub(super) runtime_recovery_needed: bool,
}

impl ActivationCandidate {
    pub(super) fn new(
        runtime: Arc<RuntimeSnapshot>,
        prepared_alarm: Option<&PreparedAlarmAsset>,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Self {
        let alarm = prepared_alarm.map(|alarm| {
            alarm.activate(
                &runtime.manifest.plugin_id,
                runtime.generation,
                activation_id,
                &runtime.digest,
            )
        });
        Self {
            runtime,
            alarm,
            activation_id,
            admission_epoch,
            runtime_recovery_needed: false,
        }
    }

    pub(super) fn bundle(&self, config: EffectivePluginConfig) -> Arc<ActivationBundle> {
        Arc::new(ActivationBundle {
            config,
            runtime: Arc::clone(&self.runtime),
            alarm: self.alarm.clone(),
            activation_id: self.activation_id,
            admission_epoch: self.admission_epoch,
            runtime_recovery_needed: self.runtime_recovery_needed,
        })
    }

    pub(super) fn reactivate(
        runtime: Arc<RuntimeSnapshot>,
        previous_alarm: Option<&ValidatedAlarmAsset>,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Self {
        let alarm = previous_alarm.map(|alarm| alarm.reactivate(runtime.generation, activation_id));
        Self {
            runtime,
            alarm,
            activation_id,
            admission_epoch,
            runtime_recovery_needed: false,
        }
    }

    pub(super) fn recovery(
        runtime: Arc<RuntimeSnapshot>,
        previous_alarm: Option<&ValidatedAlarmAsset>,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Self {
        let alarm = previous_alarm.map(|alarm| alarm.reactivate(runtime.generation, activation_id));
        Self {
            runtime,
            alarm,
            activation_id,
            admission_epoch,
            runtime_recovery_needed: true,
        }
    }
}

#[derive(Debug)]
pub(super) struct ActivationIdAllocator {
    next: AtomicU64,
}

#[derive(Debug)]
pub(super) struct AdmissionEpochAllocator {
    next: AtomicU64,
}

const RESERVED: u8 = 0;
const COMMITTING: u8 = 1;
const DURABLE: u8 = 2;
const PUBLISHED: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReservationError {
    Busy,
    Terminal,
    Exhausted,
    Unavailable,
}

pub(super) struct ActivationReservationBook {
    terminal: AtomicBool,
    next: AtomicU64,
    state: Mutex<ActivationStoreState>,
}

#[derive(Default)]
struct ActivationStoreState {
    slots: HashMap<String, ActivationSlot>,
}

#[derive(Default)]
struct ActivationSlot {
    reservation: Option<(u64, Arc<AtomicU8>)>,
    bundle: Option<Arc<ActivationBundle>>,
}

impl Default for ActivationReservationBook {
    fn default() -> Self {
        Self {
            terminal: AtomicBool::new(false),
            next: AtomicU64::new(1),
            state: Mutex::new(ActivationStoreState::default()),
        }
    }
}

impl ActivationReservationBook {
    pub(super) fn reserve(
        self: &Arc<Self>,
        plugin_id: &str,
    ) -> Result<ActivationReservation, ReservationError> {
        if self.is_terminal() {
            return Err(ReservationError::Terminal);
        }
        let id = self
            .next
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value != 0).then_some(value)?.checked_add(1)
            })
            .map_err(|_| ReservationError::Exhausted)?;
        let mut state = self.state.lock().map_err(|_| {
            self.terminal.store(true, Ordering::Release);
            ReservationError::Unavailable
        })?;
        if self.is_terminal() {
            return Err(ReservationError::Terminal);
        }
        let slot = state.slots.entry(plugin_id.to_owned()).or_default();
        if slot.reservation.is_some() {
            return Err(ReservationError::Busy);
        }
        let phase = Arc::new(AtomicU8::new(RESERVED));
        slot.reservation = Some((id, Arc::clone(&phase)));
        Ok(ActivationReservation {
            book: Arc::clone(self),
            plugin_id: plugin_id.to_owned(),
            id,
            phase,
        })
    }

    pub(super) fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::Acquire)
    }

    pub(super) fn install_initial(
        &self,
        plugin_id: &str,
        bundle: Arc<ActivationBundle>,
    ) -> Result<(), ReservationError> {
        let mut state = self.state.lock().map_err(|_| {
            self.terminal.store(true, Ordering::Release);
            ReservationError::Unavailable
        })?;
        let slot = state.slots.entry(plugin_id.to_owned()).or_default();
        if slot.reservation.is_some() || slot.bundle.is_some() {
            return Err(ReservationError::Busy);
        }
        slot.bundle = Some(bundle);
        Ok(())
    }

    pub(super) fn bundle(
        &self,
        plugin_id: &str,
    ) -> Result<Option<Arc<ActivationBundle>>, ReservationError> {
        if self.is_terminal() {
            return Err(ReservationError::Terminal);
        }
        self.state
            .lock()
            .map_err(|_| ReservationError::Unavailable)
            .map(|state| {
                state
                    .slots
                    .get(plugin_id)
                    .and_then(|slot| slot.bundle.clone())
            })
    }

    pub(super) fn bundles(&self) -> Result<Vec<Arc<ActivationBundle>>, ReservationError> {
        if self.is_terminal() {
            return Err(ReservationError::Terminal);
        }
        self.state
            .lock()
            .map_err(|_| ReservationError::Unavailable)
            .map(|state| {
                state
                    .slots
                    .values()
                    .filter_map(|slot| slot.bundle.clone())
                    .collect()
            })
    }
}

pub(super) struct ActivationReservation {
    book: Arc<ActivationReservationBook>,
    plugin_id: String,
    id: u64,
    phase: Arc<AtomicU8>,
}

impl ActivationReservation {
    pub(super) fn begin_committing(&self) -> Result<(), ReservationError> {
        transition(&self.phase, RESERVED, COMMITTING)
    }

    pub(super) fn mark_durable(&self) -> Result<(), ReservationError> {
        transition(&self.phase, COMMITTING, DURABLE)
    }

    #[cfg(test)]
    pub(super) fn mark_published(&self) -> Result<(), ReservationError> {
        self.publish(None)
    }

    pub(super) fn rollback_not_committed(&self) -> Result<(), ReservationError> {
        transition(&self.phase, COMMITTING, RESERVED)
    }

    pub(super) fn publish(
        &self,
        bundle: Option<Arc<ActivationBundle>>,
    ) -> Result<(), ReservationError> {
        if self.phase.load(Ordering::Acquire) != DURABLE {
            return Err(ReservationError::Unavailable);
        }
        let mut state = self.book.state.lock().map_err(|_| {
            self.book.terminal.store(true, Ordering::Release);
            ReservationError::Unavailable
        })?;
        let slot = state
            .slots
            .get_mut(&self.plugin_id)
            .ok_or(ReservationError::Unavailable)?;
        if !slot
            .reservation
            .as_ref()
            .is_some_and(|(id, current)| *id == self.id && Arc::ptr_eq(current, &self.phase))
        {
            return Err(ReservationError::Unavailable);
        }
        slot.bundle = bundle;
        self.phase.store(PUBLISHED, Ordering::Release);
        slot.reservation = None;
        Ok(())
    }
}

impl Drop for ActivationReservation {
    fn drop(&mut self) {
        let phase = self.phase.load(Ordering::Acquire);
        if matches!(phase, COMMITTING | DURABLE) {
            self.book.terminal.store(true, Ordering::Release);
            return;
        }
        let Ok(mut state) = self.book.state.lock() else {
            self.book.terminal.store(true, Ordering::Release);
            return;
        };
        if let Some(slot) = state.slots.get_mut(&self.plugin_id) {
            if slot
                .reservation
                .as_ref()
                .is_some_and(|(id, current)| *id == self.id && Arc::ptr_eq(current, &self.phase))
            {
                slot.reservation = None;
            }
        }
    }
}

fn transition(phase: &AtomicU8, from: u8, to: u8) -> Result<(), ReservationError> {
    phase
        .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| ReservationError::Unavailable)
}

impl Default for ActivationIdAllocator {
    fn default() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }
}

impl ActivationIdAllocator {
    pub(super) fn allocate(&self) -> Option<u64> {
        self.next
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value != 0).then_some(value)?.checked_add(1)
            })
            .ok()
    }

    #[cfg(test)]
    fn with_next(next: u64) -> Self {
        Self {
            next: AtomicU64::new(next),
        }
    }
}

impl Default for AdmissionEpochAllocator {
    fn default() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }
}

impl AdmissionEpochAllocator {
    pub(super) fn allocate(&self) -> Option<u64> {
        self.next
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value != 0).then_some(value)?.checked_add(1)
            })
            .ok()
    }

    #[cfg(test)]
    fn with_next(next: u64) -> Self {
        Self {
            next: AtomicU64::new(next),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        ActivationIdAllocator, ActivationReservationBook, AdmissionEpochAllocator, ReservationError,
    };

    #[test]
    fn activation_ids_are_monotonic_allow_holes_and_never_wrap() {
        let allocator = ActivationIdAllocator::with_next(7);
        assert_eq!(allocator.allocate(), Some(7));
        assert_eq!(allocator.allocate(), Some(8));

        let exhausted = ActivationIdAllocator::with_next(u64::MAX);
        assert_eq!(exhausted.allocate(), None);
        assert_eq!(exhausted.allocate(), None);
    }

    #[test]
    fn admission_epochs_are_monotonic_and_never_wrap() {
        let allocator = AdmissionEpochAllocator::with_next(41);
        assert_eq!(allocator.allocate(), Some(41));
        assert_eq!(allocator.allocate(), Some(42));

        let exhausted = AdmissionEpochAllocator::with_next(u64::MAX);
        assert_eq!(exhausted.allocate(), None);
        assert_eq!(exhausted.allocate(), None);
    }

    #[test]
    fn reservation_drop_rolls_back_only_before_durable_commit() {
        let book = Arc::new(ActivationReservationBook::default());
        {
            let _reserved = book.reserve("com.example.timer").unwrap();
            assert_eq!(
                book.reserve("com.example.timer").err(),
                Some(ReservationError::Busy)
            );
        }
        assert!(book.reserve("com.example.timer").is_ok());
        assert!(!book.is_terminal());

        let book = Arc::new(ActivationReservationBook::default());
        let committing = book.reserve("com.example.timer").unwrap();
        committing.begin_committing().unwrap();
        drop(committing);
        assert!(book.is_terminal());
        assert_eq!(
            book.reserve("com.example.other").err(),
            Some(ReservationError::Terminal)
        );

        let book = Arc::new(ActivationReservationBook::default());
        let not_committed = book.reserve("com.example.timer").unwrap();
        not_committed.begin_committing().unwrap();
        not_committed.rollback_not_committed().unwrap();
        drop(not_committed);
        assert!(!book.is_terminal());
        assert!(book.reserve("com.example.timer").is_ok());
    }

    #[test]
    fn durable_reservation_must_publish_and_then_releases_the_plugin() {
        let book = Arc::new(ActivationReservationBook::default());
        let reservation = book.reserve("com.example.timer").unwrap();
        reservation.begin_committing().unwrap();
        reservation.mark_durable().unwrap();
        reservation.mark_published().unwrap();
        drop(reservation);

        assert!(!book.is_terminal());
        assert!(book.reserve("com.example.timer").is_ok());

        let book = Arc::new(ActivationReservationBook::default());
        let abandoned = book.reserve("com.example.timer").unwrap();
        abandoned.begin_committing().unwrap();
        abandoned.mark_durable().unwrap();
        drop(abandoned);
        assert!(book.is_terminal());
    }
}
