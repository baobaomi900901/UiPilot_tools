use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Condvar, Mutex, MutexGuard},
};

use super::manifest::valid_plugin_id;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PluginDataCallIdentity {
    plugin_id: String,
    generation: u64,
    activation_id: u64,
    admission_epoch: u64,
}

impl PluginDataCallIdentity {
    pub(super) fn new(
        plugin_id: &str,
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> Result<Self, DataCallGateError> {
        if !valid_plugin_id(plugin_id)
            || generation == 0
            || activation_id == 0
            || admission_epoch == 0
        {
            return Err(DataCallGateError::InvalidIdentity);
        }
        Ok(Self {
            plugin_id: plugin_id.into(),
            generation,
            activation_id,
            admission_epoch,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DataCallGateError {
    InvalidIdentity,
    Expired,
    Unavailable,
}

#[derive(Default)]
pub(super) struct PluginDataCallGate {
    current: Mutex<HashMap<String, Arc<GateEpoch>>>,
}

struct GateEpoch {
    identity: PluginDataCallIdentity,
    state: Mutex<GateEpochState>,
    changed: Condvar,
}

struct GateEpochState {
    open: bool,
    in_flight: usize,
}

pub(super) struct PluginDataCallLease {
    epoch: Arc<GateEpoch>,
}

impl fmt::Debug for PluginDataCallLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginDataCallLease")
            .finish_non_exhaustive()
    }
}

pub(super) struct PluginDataCallDrain {
    epoch: Arc<GateEpoch>,
}

impl PluginDataCallGate {
    pub(super) fn activate(
        &self,
        identity: PluginDataCallIdentity,
    ) -> Result<(), DataCallGateError> {
        let mut current = self.lock_current()?;
        if current
            .get(&identity.plugin_id)
            .is_some_and(|epoch| epoch.identity == identity)
        {
            return Ok(());
        }
        current.insert(
            identity.plugin_id.clone(),
            Arc::new(GateEpoch {
                identity,
                state: Mutex::new(GateEpochState {
                    open: true,
                    in_flight: 0,
                }),
                changed: Condvar::new(),
            }),
        );
        Ok(())
    }

    pub(super) fn try_acquire(
        &self,
        identity: &PluginDataCallIdentity,
    ) -> Result<PluginDataCallLease, DataCallGateError> {
        let current = self.lock_current()?;
        let epoch = current
            .get(&identity.plugin_id)
            .filter(|epoch| epoch.identity == *identity)
            .cloned()
            .ok_or(DataCallGateError::Expired)?;
        let mut state = epoch.lock_state()?;
        if !state.open {
            return Err(DataCallGateError::Expired);
        }
        state.in_flight = state
            .in_flight
            .checked_add(1)
            .ok_or(DataCallGateError::Unavailable)?;
        drop(state);
        drop(current);
        Ok(PluginDataCallLease { epoch })
    }

    pub(super) fn close(
        &self,
        identity: &PluginDataCallIdentity,
    ) -> Result<PluginDataCallDrain, DataCallGateError> {
        let current = self.lock_current()?;
        let epoch = current
            .get(&identity.plugin_id)
            .filter(|epoch| epoch.identity == *identity)
            .cloned()
            .ok_or(DataCallGateError::Expired)?;
        epoch.lock_state()?.open = false;
        drop(current);
        Ok(PluginDataCallDrain { epoch })
    }

    pub(super) fn remove(
        &self,
        identity: &PluginDataCallIdentity,
    ) -> Result<bool, DataCallGateError> {
        let mut current = self.lock_current()?;
        let matches = current
            .get(&identity.plugin_id)
            .is_some_and(|epoch| epoch.identity == *identity);
        if matches {
            current.remove(&identity.plugin_id);
        }
        Ok(matches)
    }

    fn lock_current(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<String, Arc<GateEpoch>>>, DataCallGateError> {
        self.current
            .lock()
            .map_err(|_| DataCallGateError::Unavailable)
    }
}

impl GateEpoch {
    fn lock_state(&self) -> Result<MutexGuard<'_, GateEpochState>, DataCallGateError> {
        self.state
            .lock()
            .map_err(|_| DataCallGateError::Unavailable)
    }
}

impl PluginDataCallDrain {
    pub(super) fn wait(self) -> Result<(), DataCallGateError> {
        let mut state = self.epoch.lock_state()?;
        while state.in_flight != 0 {
            state = self
                .epoch
                .changed
                .wait(state)
                .map_err(|_| DataCallGateError::Unavailable)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn is_drained(&self) -> Result<bool, DataCallGateError> {
        Ok(self.epoch.lock_state()?.in_flight == 0)
    }
}

impl Drop for PluginDataCallLease {
    fn drop(&mut self) {
        let Ok(mut state) = self.epoch.state.lock() else {
            return;
        };
        if state.in_flight == 0 {
            return;
        }
        state.in_flight -= 1;
        if state.in_flight == 0 {
            self.epoch.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use super::{DataCallGateError, PluginDataCallGate, PluginDataCallIdentity};

    fn identity(
        generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> PluginDataCallIdentity {
        PluginDataCallIdentity::new(
            "com.example.storage",
            generation,
            activation_id,
            admission_epoch,
        )
        .unwrap()
    }

    #[test]
    fn exact_identity_is_required_and_closed_gate_rejects_new_leases() {
        let gate = PluginDataCallGate::default();
        let current = identity(2, 3, 4);
        gate.activate(current.clone()).unwrap();

        assert!(gate.try_acquire(&current).is_ok());
        for stale in [
            identity(1, 3, 4),
            identity(2, 2, 4),
            identity(2, 3, 3),
            PluginDataCallIdentity::new("com.other.storage", 2, 3, 4).unwrap(),
        ] {
            assert_eq!(
                gate.try_acquire(&stale).unwrap_err(),
                DataCallGateError::Expired
            );
        }

        let drain = gate.close(&current).unwrap();
        assert_eq!(
            gate.try_acquire(&current).unwrap_err(),
            DataCallGateError::Expired
        );
        drop(drain);
    }

    #[test]
    fn drain_waits_for_the_complete_admitted_call_without_holding_gate_admission() {
        let gate = PluginDataCallGate::default();
        let current = identity(1, 1, 1);
        gate.activate(current.clone()).unwrap();
        let lease = gate.try_acquire(&current).unwrap();
        let drain = gate.close(&current).unwrap();
        let (sender, receiver) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let _ = sender.send(drain.wait());
        });

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        drop(lease);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(Ok(())));
        waiter.join().unwrap();
    }

    #[test]
    fn config_preserves_the_gate_while_replacement_isolates_old_leases() {
        let gate = PluginDataCallGate::default();
        let first = identity(1, 1, 1);
        gate.activate(first.clone()).unwrap();
        let first_lease = gate.try_acquire(&first).unwrap();
        gate.activate(first.clone()).unwrap();
        let first_drain = gate.close(&first).unwrap();
        assert!(!first_drain.is_drained().unwrap());
        drop(first_lease);
        assert!(first_drain.is_drained().unwrap());

        let second = identity(2, 2, 2);
        gate.activate(second.clone()).unwrap();
        assert_eq!(
            gate.try_acquire(&first).unwrap_err(),
            DataCallGateError::Expired
        );
        assert!(gate.try_acquire(&second).is_ok());
    }

    #[test]
    fn malformed_or_zero_identity_is_rejected() {
        for identity in [
            PluginDataCallIdentity::new("", 1, 1, 1),
            PluginDataCallIdentity::new("com.example.storage", 0, 1, 1),
            PluginDataCallIdentity::new("com.example.storage", 1, 0, 1),
            PluginDataCallIdentity::new("com.example.storage", 1, 1, 0),
        ] {
            assert_eq!(identity, Err(DataCallGateError::InvalidIdentity));
        }
    }
}
